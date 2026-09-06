//! One production interaction loop: input, revision-gated frames and owned external completions.

use std::{io, time::Instant};

use anyhow::Context as _;
use futures_util::{Stream, StreamExt};
use plexmaton_runtime::{LiveRuntime, RuntimeUpdate};
use plexmaton_tui::{Flow, Workspace};
use ratatui::{Terminal, backend::Backend};

use crate::{
    clipboard::TerminalClipboard, dispatch_live, restore_undelivered, retry, route_approval,
    route_interrupt, route_submission, session_picker, statusline, stream_frames,
};

#[cfg(test)]
mod tests;

/// Runs the interactive select separately so every error returns to the owner that joins runtime.
#[allow(
    clippy::too_many_arguments,
    reason = "the composition boundary names each independently owned effect and joins it on exit"
)]
pub(super) async fn drive_session<B: Backend>(
    terminal: &mut Terminal<B>,
    runtime: &mut LiveRuntime,
    clipboard: &mut TerminalClipboard<impl io::Write>,
    workspace: &mut Workspace,
    picker: &mut session_picker::SessionPicker,
    status_line: &mut Option<statusline::StatusLine>,
    terminal_events: &mut (impl Stream<Item = io::Result<crossterm::event::Event>> + Unpin),
    preparation: &mut plexmaton_cli::preparation::LivePreparation,
    mut native_output: impl FnMut(&mut B, plexmaton_tui::math::NativeStage<'_>) -> Result<(), B::Error>,
) -> anyhow::Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let mut frames = stream_frames::StreamFrames::new(Instant::now());
    loop {
        deliver_copy(workspace.take_copy(), clipboard)?;
        frames
            .draw_with_native(workspace, terminal, Instant::now(), &mut native_output)
            .context("draw TUI frame")?;
        if preparation.sync(workspace) {
            continue;
        }
        let note_deadline = workspace.note_deadline();
        let drag_deadline = workspace.drag_autoscroll_deadline();
        let frame_deadline = frames.deadline();

        tokio::select! {
            prepared = preparation.next() => preparation.apply(prepared, workspace),
            delivered = clipboard.next() => delivered.context("copy to the clipboard")?,
            update = picker.next() => {
                frames.flush(workspace);
                if picker.apply(update, runtime, workspace).await? && let Some(status) = status_line {
                    status.mark_dirty();
                }
            }
            update = next_status_update(status_line) => {
                if let Some(status) = status_line {
                    match update {
                        statusline::Update::Capture => {
                            let size = terminal.size().context("read terminal dimensions")?;
                            status.capture(runtime, statusline::Dimensions { columns: size.width, rows: size.height }, workspace);
                        }
                        statusline::Update::Output(output) => status.apply(output, workspace),
                    }
                }
            }
            () = wait_for_deadline(note_deadline) => {
                workspace.expire_note(Instant::now());
            }
            () = wait_for_deadline(drag_deadline) => {
                workspace.advance_drag_autoscroll(Instant::now());
            }
            () = wait_for_deadline(frame_deadline) => {}
            runtime_update = runtime.next_update() => {
                match runtime_update.context("receive live runtime update")? {
                    RuntimeUpdate::Event(event) => {
                        if matches!(event.event, plexmaton_core::SessionEvent::AgentCreated { .. }
                            | plexmaton_core::SessionEvent::AgentStatusChanged { .. }
                            | plexmaton_core::SessionEvent::TurnUsageUpdated { .. })
                            && let Some(status) = status_line { status.mark_dirty(); }
                        frames.receive(workspace, event);
                        retry::sync_actions(runtime, workspace);
                    }
                    RuntimeUpdate::Report(report) => {
                        frames.flush(workspace);
                        if let Some(status) = status_line { status.mark_dirty(); }
                        restore_undelivered(workspace, runtime.agent_id().clone(), report);
                        retry::sync_actions(runtime, workspace);
                    }
                    RuntimeUpdate::Finished => {
                        frames.flush(workspace);
                        frames.draw_with_native(workspace, terminal, Instant::now(), &mut native_output).context("draw final TUI frame")?;
                        break;
                    }
                }
            }
            terminal_event = terminal_events.next() => {
                match terminal_event {
                    Some(Ok(event)) => {
                        if matches!(event, crossterm::event::Event::Resize(..))
                            && let Some(status) = status_line { status.mark_dirty(); }
                        let outcome = frames.handle(workspace, &event);
                        picker.observe_closed(workspace);
                        if apply_workspace_outcome(outcome, runtime, workspace, clipboard, picker).await? {
                            break;
                        }
                    }
                    Some(Err(error)) => return Err(error).context("read terminal event"),
                    None => break,
                }
            }
        }
    }
    Ok(())
}

async fn apply_workspace_outcome(
    outcome: plexmaton_tui::Outcome,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    clipboard: &mut TerminalClipboard<impl io::Write>,
    picker: &mut session_picker::SessionPicker,
) -> anyhow::Result<bool> {
    if let Some(command) = outcome.command {
        picker.execute_command(workspace, runtime, command);
    }
    if let Some(id) = outcome.resume {
        picker.select(id, runtime, workspace);
    }
    if let Some(retry) = outcome.retry {
        retry::execute(runtime, workspace, retry).await?;
    }
    if let Some(submission) = outcome.submitted {
        dispatch_live(runtime, workspace, route_submission(submission)).await?;
        retry::sync_actions(runtime, workspace);
    }
    if let Some(agent_id) = outcome.interrupted {
        dispatch_live(runtime, workspace, route_interrupt(agent_id)).await?;
    }
    if let Some(approval) = outcome.approval {
        dispatch_live(runtime, workspace, route_approval(approval)).await?;
    }
    deliver_copy(outcome.copied, clipboard)?;
    Ok(outcome.flow == Flow::Quit)
}

fn deliver_copy(
    request: Option<plexmaton_tui::CopyRequest>,
    clipboard: &mut TerminalClipboard<impl io::Write>,
) -> anyhow::Result<()> {
    if let Some(request) = request {
        clipboard
            .submit(request.text)
            .context("copy to the clipboard")?;
    }
    Ok(())
}

async fn next_status_update(status: &mut Option<statusline::StatusLine>) -> statusline::Update {
    match status {
        Some(status) => status.next().await,
        None => std::future::pending().await,
    }
}

/// Waits for an owned deadline; an absent deadline adds no ambient clock or background task.
async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => std::future::pending().await,
    }
}
