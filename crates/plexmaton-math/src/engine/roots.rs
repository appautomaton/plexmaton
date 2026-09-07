//! Reserve the engine-owned root index as a group before the display list loses ownership.
use ratex_layout::{
    LayoutBox,
    layout_box::{BoxContent, VBoxChildKind},
};

use super::geometry;
use crate::{Limit, MAX_NODES, MathError};

pub(super) fn reserve_indices(root: &mut LayoutBox) -> Result<(), MathError> {
    let mut pending = vec![root];
    let mut visited = 0;
    while let Some(node) = pending.pop() {
        visited += 1;
        if visited + pending.len() > MAX_NODES {
            return Err(MathError::Limited(Limit::Nodes));
        }
        match &mut node.content {
            BoxContent::Radical {
                body,
                index,
                index_offset,
                index_scale,
                ..
            } => {
                if let Some(index) = index {
                    // RaTeX 0.1.14 emits the index at surd_x + 5/18 em, inside the surd ink.
                    // Cell reservations cannot overlap: place the complete owned subtree in the
                    // index_offset area already reserved by the engine, preserving internal layout.
                    let shift = geometry(-(*index_offset + 5.0 / 18.0) / *index_scale)?;
                    let content = std::mem::replace(&mut index.content, BoxContent::Empty);
                    let original = LayoutBox {
                        content,
                        ..(**index).clone()
                    };
                    let mut kern = LayoutBox::new_empty();
                    kern.width = shift;
                    kern.content = BoxContent::Kern;
                    let mut trailing = kern.clone();
                    trailing.width = -shift;
                    index.content = BoxContent::HBox(vec![kern, original, trailing]);
                    pending.push(index);
                }
                pending.push(body);
            }
            BoxContent::HBox(children) => pending.extend(children),
            BoxContent::VBox(children) => {
                for child in children {
                    if let VBoxChildKind::Box(child) = &mut child.kind {
                        pending.push(child);
                    }
                }
            }
            BoxContent::Fraction { numer, denom, .. } => {
                pending.extend([numer.as_mut(), denom.as_mut()])
            }
            BoxContent::SupSub { base, sup, sub, .. }
            | BoxContent::OpLimits { base, sup, sub, .. } => {
                pending.push(base);
                pending.extend(sup.iter_mut().chain(sub).map(Box::as_mut));
            }
            BoxContent::Accent { base, accent, .. } => {
                pending.extend([base.as_mut(), accent.as_mut()])
            }
            BoxContent::LeftRight { left, inner, right } => {
                pending.extend([left.as_mut(), inner.as_mut(), right.as_mut()])
            }
            BoxContent::Array {
                cells, row_tags, ..
            } => {
                pending.extend(cells.iter_mut().flatten());
                pending.extend(row_tags.iter_mut().flatten());
            }
            BoxContent::Framed { body, .. }
            | BoxContent::RaiseBox { body, .. }
            | BoxContent::Scaled { body, .. }
            | BoxContent::Angl { body, .. }
            | BoxContent::Overline { body, .. }
            | BoxContent::Underline { body, .. } => pending.push(body),
            BoxContent::ProofTree { children, .. } => {
                pending.extend(children.iter_mut().map(|child| &mut child.box_))
            }
            BoxContent::Glyph { .. }
            | BoxContent::GlyphRun { .. }
            | BoxContent::Rule { .. }
            | BoxContent::SvgPath { .. }
            | BoxContent::Kern
            | BoxContent::Empty => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MTH-4: layout-box traversal has its own bound before native projection.
    #[test]
    fn root_index_normalization_bounds_layout_nodes() {
        let mut root = LayoutBox::new_empty();
        root.content = BoxContent::HBox(vec![LayoutBox::new_empty(); MAX_NODES]);
        assert_eq!(
            reserve_indices(&mut root),
            Err(MathError::Limited(Limit::Nodes))
        );
    }
}
