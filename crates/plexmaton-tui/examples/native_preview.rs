//! Interactive, offline workspace fixture rendered directly by the terminal.
//! See .agents/spikes/kitty-native-preview/README.md for scope and controls.

use std::{
    io,
    time::{Duration, Instant},
};

use plexmaton_tui::{Flow, SurfaceId, Workspace};
use ratatui::{
    DefaultTerminal,
    crossterm::{
        event::{
            self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture,
            EnableBracketedPaste, EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent,
            KeyModifiers,
        },
        execute,
    },
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[path = "support/native_control.rs"]
mod native_control;
#[path = "support/native_effects.rs"]
mod native_effects;

fn restore() {
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture
    );
    ratatui::restore();
}

fn main() -> Result<()> {
    let (mut demo, mut workspace) = native_control::ControlDemo::start()?;

    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            restore();
            return Err(error.into());
        }
    };
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
    let result = run(&mut workspace, &mut terminal, &mut demo);
    restore();
    result
}

fn run(
    workspace: &mut Workspace,
    terminal: &mut DefaultTerminal,
    demo: &mut native_control::ControlDemo,
) -> Result<()> {
    execute!(
        io::stdout(),
        EnableBracketedPaste,
        EnableFocusChange,
        EnableMouseCapture
    )?;
    draw(workspace, terminal)?;
    focus(workspace, terminal, SurfaceId::Agents)?;
    workspace.handle(&Event::Key(KeyEvent::new(
        KeyCode::Down,
        KeyModifiers::NONE,
    )));
    draw(workspace, terminal)?;
    focus(workspace, terminal, SurfaceId::Composer)?;
    loop {
        draw(workspace, terminal)?;
        if event::poll(Duration::from_millis(50))? {
            let event = event::read()?;
            if !demo.advance(workspace, &event)?
                && native_effects::handle(workspace, &event) == Flow::Quit
            {
                return Ok(());
            }
        }
        workspace.expire_note(Instant::now());
    }
}

fn focus(
    workspace: &mut Workspace,
    terminal: &mut DefaultTerminal,
    target: SurfaceId,
) -> Result<()> {
    for _ in 0..=workspace.surfaces().len() {
        if workspace.state().focused(workspace.surfaces()) == Some(target) {
            return Ok(());
        }
        workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        draw(workspace, terminal)?;
    }
    Err("preview focus target is unavailable".into())
}

fn draw(workspace: &mut Workspace, terminal: &mut DefaultTerminal) -> Result<()> {
    // This finite fixture uses synchronous preparation, as the other offline examples do.
    // It demonstrates cells and routing, not production worker scheduling or native math.
    for _ in 0..1024 {
        workspace.draw(terminal)?;
        if let Some(work) = workspace.take_preparation() {
            match plexmaton_tui::preparation::prepare_batch(&work.requests) {
                Ok(prepared) => {
                    if !workspace.complete_preparation(work.token, prepared) {
                        return Err("preview preparation was rejected".into());
                    }
                }
                Err(plexmaton_tui::preparation::BatchRefusal::Capacity) => workspace
                    .fail_preparation(work.token, plexmaton_tui::preparation::Refusal::Capacity),
            }
        } else if !workspace.needs_draw() {
            return Ok(());
        }
    }
    Err("preview preparation did not settle".into())
}
