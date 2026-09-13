//! Rendered proposal for child conversation control; this does not wire new product actions.
//! cargo run -p plexmaton-tui --example ownership_preview -- <output-directory>

use std::path::Path;

use plexmaton_tui::{Palette, Role};
use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[path = "support/frame_svg.rs"]
mod frame_svg;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
enum Preview {
    MainRunning,
    MainIdle,
    UserControlled,
}

impl Preview {
    fn name(self) -> &'static str {
        match self {
            Self::MainRunning => "main-running",
            Self::MainIdle => "main-idle",
            Self::UserControlled => "user-controlled",
        }
    }
    fn state(self) -> &'static str {
        match self {
            Self::MainRunning => "Running",
            Self::MainIdle => "Idle",
            Self::UserControlled => "Ready",
        }
    }
}

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for width in [120, 88, 60] {
        for state in [
            Preview::MainRunning,
            Preview::MainIdle,
            Preview::UserControlled,
        ] {
            let mut terminal = Terminal::new(TestBackend::new(width, 26))?;
            terminal.draw(|frame| {
                let area = frame.area();
                let palette = Palette::pastel();
                frame.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(palette.style(Role::BorderFocused))
                        .title(Line::from(vec![
                            Span::styled(" Luna / max ", palette.style(Role::Accent)),
                            Span::styled(
                                format!(" {} ", state.state()),
                                palette.style(Role::Ambient),
                            ),
                        ])),
                    area,
                );
                let row = |y, height| Rect::new(3, y, area.width.saturating_sub(6), height);
                let controller = match state {
                    Preview::UserControlled => "Controller: User",
                    _ => "Controller: Main",
                };
                frame.render_widget(
                    Paragraph::new(controller).style(palette.style(Role::SectionHeading)),
                    row(2, 1),
                );
                frame.render_widget(
                    Paragraph::new("Read-only tools | No shell").style(palette.style(Role::Muted)),
                    row(3, 1),
                );
                let body = vec![
                    Line::from(Span::styled(
                        "Main -> Luna   task",
                        palette.style(Role::Accent),
                    )),
                    Line::from("Inspect the replay boundary. Read only."),
                    Line::from("Report findings without changing files."),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Luna -> Main   finding",
                        palette.style(Role::NewInformation),
                    )),
                    Line::from("The journal keeps accepted mail across restarts."),
                    Line::from(match state {
                        Preview::MainRunning => "I am checking the interrupted-turn cases.",
                        _ => "The findings are ready for Main.",
                    }),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Artifact   replay-review.md",
                        palette.style(Role::NewInformation),
                    )),
                ];
                frame.render_widget(
                    Paragraph::new(body)
                        .style(palette.style(Role::Body))
                        .wrap(Wrap { trim: false }),
                    row(6, 11),
                );
                let divider = "─".repeat(usize::from(area.width.saturating_sub(6)));
                frame.render_widget(
                    Paragraph::new(divider).style(palette.style(Role::Border)),
                    row(18, 1),
                );
                match state {
                    Preview::MainRunning => {
                        frame.render_widget(
                            Paragraph::new("No user composer.").style(palette.style(Role::Muted)),
                            row(20, 1),
                        );
                        frame.render_widget(
                            Paragraph::new("You can inspect, copy, or stop.")
                                .style(palette.style(Role::Muted)),
                            row(21, 1),
                        );
                        frame.render_widget(
                            Paragraph::new("[ Stop run ]")
                                .style(palette.style(Role::ActionRequired)),
                            row(23, 1),
                        );
                    }
                    Preview::MainIdle => {
                        frame.render_widget(
                            Paragraph::new("No user composer. Waiting for Main.")
                                .style(palette.style(Role::Muted)),
                            row(20, 1),
                        );
                        frame.render_widget(
                            Paragraph::new("Direct input requires acknowledged Handoff.")
                                .style(palette.style(Role::Muted)),
                            row(21, 1),
                        );
                    }
                    Preview::UserControlled => {
                        frame.render_widget(
                            Paragraph::new("Handoff acknowledged: Main -> User")
                                .style(palette.style(Role::NewInformation)),
                            row(20, 1),
                        );
                        frame.render_widget(
                            Paragraph::new("> Ask Luna a follow-up...")
                                .style(palette.style(Role::Body)),
                            row(22, 1),
                        );
                        frame.render_widget(
                            Paragraph::new("Tool capabilities are unchanged.")
                                .style(palette.style(Role::Muted)),
                            row(23, 1),
                        );
                    }
                }
            })?;
            std::fs::write(
                directory.join(format!("{}-{width}.svg", state.name())),
                frame_svg::svg(terminal.backend().buffer()),
            )?;
        }
    }
    Ok(())
}
