//! Actual workspace math/copy/clipping review. SVG models terminal fonts; it is not the transport.
//! cargo run -p plexmaton-tui --example math_preview -- <output-directory>

use std::{fmt::Write as _, path::Path};

use plexmaton_core::{
    AgentId, AgentStatus, EventSequence, SessionEvent, SessionEventEnvelope, TranscriptItemId,
    TranscriptRole,
};
use plexmaton_tui::{
    MarkdownTheme, Palette, Workspace,
    math::{
        FontStyle, MathPaint, MathPresentation, MathUnavailable, NativeStage, NativeText,
        TextScale, VerticalAlign,
    },
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    style::{Color, Modifier},
};
use unicode_width::UnicodeWidthStr as _;

#[path = "support/frame_svg.rs"]
mod frame_svg;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Review {
    workspace: Workspace,
    terminal: Terminal<TestBackend>,
    native: Vec<NativeText>,
}

impl Review {
    fn new(width: u16, height: u16, source: &str, math: MathPresentation) -> Result<Self> {
        let mut workspace = Workspace::with_presentation(
            Palette::ansi().with_markdown_theme(MarkdownTheme::Pastel),
            math,
        );
        workspace.set_working_directory("~/plexmaton".into());
        let agent = AgentId::new("primary")?;
        let item = TranscriptItemId::new("math-reply")?;
        workspace.emit(
            [
                SessionEvent::AgentCreated {
                    agent_id: agent.clone(),
                    label: "Plexmaton".into(),
                    status: AgentStatus::Idle,
                },
                SessionEvent::TranscriptItemStarted {
                    agent_id: agent.clone(),
                    item_id: item.clone(),
                    role: TranscriptRole::Assistant,
                },
                SessionEvent::TranscriptDelta {
                    agent_id: agent,
                    item_id: item,
                    item_revision: 1,
                    text: source.into(),
                },
            ]
            .into_iter()
            .enumerate()
            .map(|(index, event)| SessionEventEnvelope {
                sequence: EventSequence::new(index as u64 + 1),
                event,
            })
            .collect(),
        );
        let mut review = Self {
            workspace,
            terminal: Terminal::new(TestBackend::new(width, height))?,
            native: Vec::new(),
        };
        review.draw()?;
        Ok(review)
    }

    fn draw(&mut self) -> Result<bool> {
        let mut changed = false;
        for _ in 0..1024 {
            changed |= self
                .workspace
                .draw_with_native(&mut self.terminal, |_, stage| {
                    if let NativeStage::End { current, .. } = stage {
                        self.native = current.to_vec();
                    }
                    Ok(())
                })?
                .is_some();
            let Some(work) = self.workspace.take_preparation() else {
                return Ok(changed);
            };
            // Explicit offline worker seam, never hidden inside interactive draw or input.
            let prepared = plexmaton_tui::preparation::prepare_batch(&work.requests)
                .map_err(|_| "review batch exceeded capacity")?;
            if !self.workspace.complete_preparation(work.token, prepared) {
                return Err("review reply was refused".into());
            }
        }
        Err("review preparation did not settle".into())
    }

    fn mouse(&mut self, kind: MouseEventKind, x: u16, y: u16) -> plexmaton_tui::Outcome {
        self.workspace.handle(&Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }))
    }

    fn top(&mut self) -> Result<()> {
        for _ in 0..600 {
            self.mouse(MouseEventKind::ScrollUp, 5, 6);
            if !self.draw()? {
                return Ok(());
            }
        }
        Err("review did not reach the top".into())
    }

    fn contains(&self, needle: &str) -> bool {
        let buffer = self.terminal.backend().buffer();
        (0..buffer.area.height).any(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .contains(needle)
        })
    }

    fn save(&self, directory: &Path, name: &str) -> Result<()> {
        let mut svg = frame_svg::svg(self.terminal.backend().buffer());
        svg.truncate(svg.len() - "</g></svg>".len());
        for native in &self.native {
            glyph(&mut svg, native);
        }
        svg.push_str("</g></svg>\n");
        std::fs::write(
            directory.join(format!(
                "{name}-{}.svg",
                self.terminal.backend().buffer().area.width
            )),
            svg,
        )?;
        Ok(())
    }
}

fn glyph(svg: &mut String, native: &NativeText) {
    let run = &native.glyph;
    let scale = match run.scale {
        TextScale::Full => 1.0,
        TextScale::Script => 0.7,
        TextScale::ScriptScript => 0.5,
        TextScale::Large => 2.0,
    };
    let mut foreground = match run.paint {
        MathPaint::Inherit => frame_svg::color(
            native.style.fg.unwrap_or(Color::Reset),
            frame_svg::FOREGROUND,
        ),
        MathPaint::Rgb { red, green, blue } => format!("#{red:02x}{green:02x}{blue:02x}"),
    };
    let mut background = frame_svg::color(
        native.style.bg.unwrap_or(Color::Reset),
        frame_svg::BACKGROUND,
    );
    if native.style.add_modifier.contains(Modifier::REVERSED) {
        std::mem::swap(&mut foreground, &mut background);
    }
    let x = u32::from(run.x) * 9;
    let top = u32::from(run.y) * 20;
    if background != frame_svg::BACKGROUND {
        write!(
            svg,
            r#"<rect x="{x}" y="{top}" width="{}" height="{}" fill="{background}"/>"#,
            u32::from(run.columns) * 9,
            u32::from(run.rows) * 20
        )
        .expect("String write");
    }
    let extra = f64::from(run.rows) * 20.0 - 20.0 * scale;
    let shift = match run.align {
        VerticalAlign::Top => 0.0,
        VerticalAlign::Bottom => extra,
        VerticalAlign::Center => extra / 2.0,
    };
    let y = f64::from(top) + shift + 15.0 * scale;
    let italic = if matches!(run.style, FontStyle::Italic | FontStyle::BoldItalic)
        || native.style.add_modifier.contains(Modifier::ITALIC)
    {
        "italic"
    } else {
        "normal"
    };
    let weight = if matches!(run.style, FontStyle::Bold | FontStyle::BoldItalic)
        || native.style.add_modifier.contains(Modifier::BOLD)
    {
        "bold"
    } else {
        "normal"
    };
    write!(svg, r#"<text x="{x}" y="{y:.2}" font-size="{}" font-style="{italic}" font-weight="{weight}" fill="{foreground}" textLength="{:.2}" lengthAdjust="spacingAndGlyphs">{}</text>"#, 14.0 * scale, run.text.width() as f64 * 9.0 * scale, frame_svg::escape(&run.text)).expect("String write");
}

fn clipped_review(width: u16, height: u16) -> Result<Review> {
    // Wheel increments are three rows; vary prose padding to exercise each boundary residue.
    for padding in 0..3 {
        let content = format!(
            "{}\n\\[\\sum_{{i=1}}^{{n}} x_i\\]\n{}",
            "before\n".repeat(40),
            "after\n".repeat(40 + padding)
        );
        let mut clipped = Review::new(width, height, &content, MathPresentation::Native)?;
        for _ in 0..100 {
            if clipped.contains("Math clipped") {
                return Ok(clipped);
            }
            clipped.mouse(MouseEventKind::ScrollUp, 5, 6);
            if !clipped.draw()? {
                break;
            }
        }
    }
    Err("clipped multicell fixture did not reach its boundary".into())
}

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    let document: serde_json::Value = serde_json::from_str(include_str!(
        "../../plexmaton-math/fixtures/attention-derivatives.json"
    ))?;
    let source = document["text"].as_str().ok_or("reply text")?;
    let first = &document["math"][0];
    let formula = &source[first["start"].as_u64().ok_or("formula start")? as usize
        ..first["end"].as_u64().ok_or("formula end")? as usize];
    let table = "## Formula table\n\n| Parameter with a descriptive name | Equation | Interpretation |\n| --- | ---: | --- |\n| first | \\( \\frac{ab}{c} \\) | **ready** |\n| second | $x_{ij}^2$ | after |";
    for (width, height) in [(120, 40), (88, 42), (60, 46)] {
        let mut reply = Review::new(width, height, source, MathPresentation::Native)?;
        reply.top()?;
        reply.save(directory, "reply")?;
        let run = reply
            .native
            .iter()
            .find(|native| native.glyph.text.contains("Attention"))
            .ok_or("attention formula not visible")?;
        let (x, y) = (run.glyph.x, run.glyph.y);
        reply.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        reply.draw()?;
        let copied = reply
            .mouse(MouseEventKind::Up(MouseButton::Left), x, y)
            .copied
            .ok_or("formula click did not copy")?;
        if copied.text != formula {
            return Err("formula copy changed original source".into());
        }
        reply.draw()?;
        reply.save(directory, "selection")?;
        Review::new(
            width,
            height,
            formula,
            MathPresentation::Source(MathUnavailable::Multiplexer),
        )?
        .save(directory, "source")?;
        Review::new(width, height, table, MathPresentation::Native)?.save(directory, "table")?;
        clipped_review(width, height)?.save(directory, "clipped")?;
    }
    Ok(())
}
