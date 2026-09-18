use std::collections::HashSet;

use ratatui::style::Modifier;

use plexmaton_core::ToolCallStatus;

use super::{Palette, Role, Slots, invert, tool_role};

fn palettes() -> [(&'static str, Palette); 2] {
    [
        ("pastel", Palette::pastel()),
        ("inverted", Palette::inverted()),
    ]
}

#[test]
fn attention_levels_are_distinguishable_in_every_palette() {
    for (name, palette) in palettes() {
        let styles: HashSet<_> = Role::ATTENTION
            .iter()
            .map(|role| format!("{:?}", palette.style(*role)))
            .collect();
        assert_eq!(
            styles.len(),
            Role::ATTENTION.len(),
            "{name} collapses two attention levels onto one style"
        );
    }
}

#[test]
fn approval_tool_states_map_to_attention_without_treating_denial_as_failure() {
    assert_eq!(
        tool_role(ToolCallStatus::AwaitingApproval),
        Role::ActionRequired
    );
    assert_eq!(tool_role(ToolCallStatus::Denied), Role::Muted);
}

/// A border answers two questions at once: whose surface this is, and whether it is the one
/// being operated. Hue answers the first and intensity the second, so neither may collapse
/// into the other and no two surfaces may say the same thing at the same intensity.
#[test]
fn every_surface_border_says_whose_it_is_and_whether_it_has_focus() {
    let hues = [
        Role::SurfaceRoster,
        Role::SurfacePrimary,
        Role::SurfaceDelegate,
    ];
    for (name, palette) in palettes() {
        for hue in hues {
            assert_ne!(
                palette.surface_border(Some(hue), true),
                palette.surface_border(Some(hue), false),
                "{name}: {hue:?} cannot show whether it has focus"
            );
        }
        for focused in [true, false] {
            for (first, second) in [(0, 1), (0, 2), (1, 2)] {
                assert_ne!(
                    palette.surface_border(Some(hues[first]), focused),
                    palette.surface_border(Some(hues[second]), focused),
                    "{name}: {:?} and {:?} say the same thing at focus {focused}",
                    hues[first],
                    hues[second]
                );
            }
        }
        // A surface with no identity of its own still shows focus, on the neutral line.
        assert_ne!(
            palette.surface_border(None, true),
            palette.surface_border(None, false),
            "{name} cannot show which surface has focus"
        );
    }
}

#[test]
fn key_hints_are_reversed_without_a_named_colour() {
    for (name, palette) in palettes() {
        let style = palette.style(Role::KeyHint);
        assert!(
            style.add_modifier.contains(Modifier::REVERSED),
            "{name} key hints are not reversed"
        );
        assert!(
            style.fg.is_none() && style.bg.is_none(),
            "{name} key hints inject a named colour"
        );
        assert_ne!(
            style,
            palette.style(Role::Accent),
            "{name} paints keys as identity emphasis"
        );
    }
}

#[test]
fn a_palette_is_a_complete_assignment_of_roles() {
    let base = Palette::pastel();
    let rebuilt = Palette::from_roles(|role| base.style(role));
    for role in Role::ALL {
        assert_eq!(
            rebuilt.style(role),
            base.style(role),
            "{role:?} did not round-trip through from_roles"
        );
    }
}

/// A theme supplies the twelve slots and reaches no further.
///
/// Two halves, and the test fails on either. Every colour a role paints with came from a slot, so
/// reassigning the slots reassigns all of them and no role can be holding a colour of its own; and
/// no role's weight or italic moves when only colours were supplied, so no colourway can take the
/// bold off a failure or the quiet off work in progress.
#[test]
fn a_theme_reassigns_colour_and_never_meaning() {
    let designed = Palette::from_slots(Slots::designed());
    let swapped = Palette::from_slots(Slots::designed().map(invert));
    for role in Role::ALL {
        let (before, after) = (designed.style(role), swapped.style(role));
        assert_eq!(
            (before.add_modifier, before.sub_modifier),
            (after.add_modifier, after.sub_modifier),
            "{role:?} changed what it means when only its colours were reassigned"
        );
        for (channel, before, after) in [
            ("foreground", before.fg, after.fg),
            ("background", before.bg, after.bg),
        ] {
            assert_eq!(
                after,
                before.map(invert),
                "{role:?} has a {channel} the theme did not supply"
            );
        }
    }
}
