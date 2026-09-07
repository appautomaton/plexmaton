//! One production interaction loop: input, revision-gated frames and owned external completions.

mod model;
use model::apply_model;
use std::{io, time::Instant};

use anyhow::Context as _;
use futures_util::{Stream, StreamExt};
use plexmaton_runtime::{LiveRuntime, RuntimeUpdate};
use plexmaton_tui::{
    Command, CommandRun, CompactRefusal, CompactionNote, ConversationRequest, Flow, Page,
    PermissionRequest, Workspace,
};
use ratatui::{Terminal, backend::Backend};

use crate::{
    clipboard::TerminalClipboard, dispatch_live, permission_controls, restore_undelivered, retry,
    route_approval, route_interrupt, route_submission, session_picker, statusline, stream_frames,
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
    picker: &mut session_picker::ConversationPicker,
    status_line: &mut Option<statusline::StatusLine>,
    permissions: &mut permission_controls::PermissionControls,
    terminal_events: &mut (impl Stream<Item = io::Result<crossterm::event::Event>> + Unpin),
    preparation: &mut plexmaton_cli::preparation::LivePreparation,
    mut native_output: impl FnMut(&mut B, plexmaton_tui::math::NativeStage<'_>) -> Result<(), B::Error>,
) -> anyhow::Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let mut frames = stream_frames::StreamFrames::new(Instant::now());
    loop {
        let copy = workspace.take_copy();
        deliver_copy(copy, clipboard, workspace)?;
        frames
            .draw_with_native(workspace, terminal, Instant::now(), &mut native_output)
            .context("draw TUI frame")?;
        if preparation.sync(workspace) {
            continue;
        }
        let note_deadline = workspace.note_deadline();
        let drag_deadline = workspace.drag_autoscroll_deadline();
        let frame_deadline = frames.deadline();
        let effort_deadline = workspace.effort_animation_deadline(Instant::now());

        tokio::select! {
            prepared = preparation.next() => preparation.apply(prepared, workspace),
            delivered = clipboard.next() => {
                report_copy(delivered.context("copy to the clipboard")?, workspace);
            },
            update = permissions.next() => {
                frames.flush(workspace);
                permission_controls::PermissionControls::publish(update, workspace);
                let report = runtime.permissions_changed().await.context("apply current permissions to waiting calls")?;
                restore_undelivered(workspace, runtime.agent_id().clone(), report);
            }
            update = picker.next() => {
                frames.flush(workspace);
                if picker.apply(update, runtime, workspace).await? && let Some(status) = status_line {
                    status.mark_dirty();
                }
            }
            update = next_status_update(status_line) => {
                apply_status_update(update, status_line, terminal, runtime, workspace)?;
            }
            () = wait_for_deadline(note_deadline) => {
                workspace.expire_note(Instant::now());
            }
            () = wait_for_deadline(drag_deadline) => {
                workspace.advance_drag_autoscroll(Instant::now());
            }
            () = wait_for_deadline(frame_deadline) => {}
            () = wait_for_deadline(effort_deadline) => { workspace.advance_effort_animation(Instant::now()); }
            runtime_update = runtime.next_update() => {
                match runtime_update.context("receive live runtime update")? {
                    RuntimeUpdate::Event(event) => {
                        if let Some(status) = status_line { status.observe(&event.event); }
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
                        permissions.observe_closed(workspace);
                        if apply_workspace_outcome(outcome, runtime, workspace, clipboard, picker, permissions, status_line).await? {
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
    picker: &mut session_picker::ConversationPicker,
    permissions: &mut permission_controls::PermissionControls,
    status_line: &mut Option<statusline::StatusLine>,
) -> anyhow::Result<bool> {
    if let Some(change) = &outcome.model {
        let result = apply_model(change, runtime, picker);
        if let Ok(model) = &result {
            workspace.set_effort_choices(model.allowed_reasoning_efforts().map(<[_]>::to_vec));
            if let Some(status) = status_line {
                status.mark_dirty();
            }
        }
        workspace.report_model(result.map(|model| crate::configuration_summary(&model)));
    }
    if let Some(change) = &outcome.effort {
        let result = runtime
            .set_reasoning_effort(&change.agent, change.effort)
            .map_err(|refusal| refusal.to_string())
            .map(|model| crate::configuration_summary(&model));
        if result.is_ok()
            && let Some(status) = status_line
        {
            status.mark_dirty();
        }
        workspace.report_effort(result);
    }
    if let Some(page) = outcome.page {
        open_page(page, workspace, picker, permissions);
        if page == Page::Configuration
            && let Some(model) = runtime.configured_model()
        {
            workspace.show_configuration(crate::configuration_summary(model));
        }
    }
    match outcome.permission {
        Some(PermissionRequest::Refresh) => permissions.refresh(),
        Some(PermissionRequest::Change(intent)) => permissions.apply(intent),
        None => {}
    }
    match outcome.conversation {
        Some(ConversationRequest::List) => picker.open(workspace),
        Some(ConversationRequest::New) => picker.new_conversation(workspace, runtime),
        Some(ConversationRequest::Saved(id)) => picker.select(id, runtime, workspace),
        None => {}
    }
    if let Some(run) = outcome.command {
        run_command(run, runtime, workspace, picker, permissions).await?;
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
    deliver_copy(outcome.copied, clipboard, workspace)?;
    Ok(outcome.flow == Flow::Quit)
}

/// Runs a Command against the conversation it names; the runtime admits or refuses it (CMC-1).
async fn run_command(
    run: CommandRun,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    picker: &mut session_picker::ConversationPicker,
    permissions: &mut permission_controls::PermissionControls,
) -> anyhow::Result<()> {
    use plexmaton_runtime::{CompactionRequest, CompactionRequestRefusal as Refusal};
    match run.command {
        Command::New => picker.new_conversation(workspace, runtime),
        Command::Resume => picker.open(workspace),
        Command::Permissions => permissions.refresh(),
        Command::Effort | Command::Model => {}
        Command::Compact => {
            let note = match runtime
                .request_compaction(run.target.agent.clone())
                .await
                .context("request compaction")?
            {
                CompactionRequest::Started { .. } => CompactionNote::Started,
                CompactionRequest::Refused(refusal) => CompactionNote::Refused(match refusal {
                    Refusal::TurnActive => CompactRefusal::TurnActive,
                    Refusal::ApprovalPending => CompactRefusal::ApprovalPending,
                    Refusal::CompactionActive => CompactRefusal::CompactionActive,
                    Refusal::ShuttingDown => CompactRefusal::ShuttingDown,
                    Refusal::BudgetUnavailable => CompactRefusal::BudgetUnavailable,
                    Refusal::NothingToCompact => CompactRefusal::NothingToCompact,
                    Refusal::HistoryTooLarge => CompactRefusal::HistoryTooLarge,
                    Refusal::SourceUnavailable => CompactRefusal::SourceUnavailable,
                }),
            };
            workspace.report_compaction(&run.target.agent, note);
        }
    }
    Ok(())
}

/// Opens a Drawer page. What each one costs is owned here, never by the workspace (DRW-3).
pub(super) fn open_page(
    page: Page,
    workspace: &mut Workspace,
    picker: &mut session_picker::ConversationPicker,
    permissions: &mut permission_controls::PermissionControls,
) {
    match page {
        Page::Configuration => workspace.show_configuration(picker.configuration()),
        Page::Permissions => permissions.open(workspace),
    }
}

fn deliver_copy(
    request: Option<plexmaton_tui::CopyRequest>,
    clipboard: &mut TerminalClipboard<impl io::Write>,
    workspace: &mut Workspace,
) -> anyhow::Result<()> {
    if let Some(request) = request {
        let receipt = clipboard
            .submit(request.text)
            .context("copy to the clipboard")?;
        workspace.clear_copy_receipt();
        report_copy(receipt, workspace);
    }
    Ok(())
}

fn report_copy(receipt: Option<plexmaton_tui::CopyReceipt>, workspace: &mut Workspace) {
    if let Some(receipt) = receipt {
        workspace.report_copy(receipt, Instant::now());
    }
}

/// Apply the footer owner's completion with the current terminal/runtime snapshot (STL-1).
fn apply_status_update<B: Backend>(
    update: statusline::Update,
    status: &mut Option<statusline::StatusLine>,
    terminal: &Terminal<B>,
    runtime: &LiveRuntime,
    workspace: &mut Workspace,
) -> anyhow::Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    if let Some(status) = status {
        match update {
            statusline::Update::Capture => {
                let size = terminal.size().context("read terminal dimensions")?;
                status.capture(
                    runtime,
                    statusline::Dimensions {
                        columns: size.width,
                        rows: size.height,
                    },
                    workspace,
                );
            }
            statusline::Update::Output(output) => status.apply(output, workspace),
        }
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
