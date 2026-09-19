//! One production interaction loop: input, revision-gated frames and owned external completions.

mod child_input;
mod model;
mod stop;
use std::{io, time::Instant};
use stop::{apply_interrupt, apply_stop_settlement};

use anyhow::Context as _;
use futures_util::{Stream, StreamExt};
use plexmaton_runtime::{LiveRuntime, OwnedCollaborationActivity, RuntimeUpdate};
use plexmaton_tui::{
    Command, CommandRun, CompactRefusal, CompactionNote, ConversationRequest, Flow, Page,
    PermissionRequest, Workspace,
};
use ratatui::{Terminal, backend::Backend};

use crate::{
    clipboard::TerminalClipboard, collaboration::RootProjectionProgress, conversation_tree,
    dispatch_live, input::apply_report, input_queue, permission_controls, retry, route_approval,
    session_picker, statusline, stream_frames,
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
    collaboration: &mut Option<crate::collaboration::Collaboration>,
    mut native_output: impl FnMut(&mut B, plexmaton_tui::math::NativeStage<'_>) -> Result<(), B::Error>,
) -> anyhow::Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let mut frames = stream_frames::StreamFrames::new(Instant::now());
    loop {
        input_queue::sync(runtime, workspace);
        let copy = workspace.take_copy();
        deliver_copy(copy, clipboard, workspace)?;
        frames
            .draw_with_native(workspace, terminal, Instant::now(), &mut native_output)
            .context("draw TUI frame")?;
        if restart_after_owned_updates(
            preparation,
            collaboration.as_mut(),
            runtime,
            workspace,
            &mut frames,
        )
        .await?
        {
            continue;
        }
        let note_deadline = workspace.note_deadline();
        let drag_deadline = workspace.drag_autoscroll_deadline();
        let frame_deadline = frames.deadline();
        let effort_deadline = workspace.effort_animation_deadline(Instant::now());
        let collaboration_enabled = collaboration
            .as_ref()
            .is_some_and(|collaboration| collaboration.can_poll(runtime));

        tokio::select! {
            // A root with no collaboration never yields here, so the arm is inert rather than a
            // branch the loop has to skip.
            activity = next_collaboration(collaboration.as_mut()), if collaboration_enabled => {
                apply_collaboration(
                    activity,
                    collaboration.as_mut(),
                    runtime,
                    workspace,
                    &mut frames,
                ).await?;
            }
            prepared = preparation.next() => preparation.apply(prepared, workspace),
            delivered = clipboard.next() => {
                report_copy(delivered.context("copy to the clipboard")?, workspace);
            },
            update = permissions.next() => {
                frames.flush(workspace);
                permission_controls::PermissionControls::publish(update, workspace);
                let report = runtime.permissions_changed().await.context("apply current permissions to waiting calls")?;
                apply_report(runtime, workspace, runtime.agent_id().clone(), report);
            }
            update = picker.next() => {
                frames.flush(workspace);
                if picker.apply(update, runtime, collaboration, workspace).await?
                    && let Some(status) = status_line
                {
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
                let update = runtime_update.context("receive live runtime update")?;
                let finished = apply_runtime_progress(
                    update,
                    collaboration.as_mut(),
                    runtime,
                    workspace,
                    status_line,
                    &mut frames,
                ).await?;
                if finished {
                    frames.draw_with_native(workspace, terminal, Instant::now(), &mut native_output).context("draw final TUI frame")?;
                    break;
                }
            }
            terminal_event = terminal_events.next() => {
                let Some(event) = terminal_event.transpose().context("read terminal event")? else {
                    break;
                };
                if apply_terminal_event(
                    &event,
                    runtime,
                    workspace,
                    clipboard,
                    picker,
                    permissions,
                    status_line,
                    collaboration.as_mut(),
                    &mut frames,
                )
                .await?
                {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Applies one acknowledged root update before retrying rows whose placement it made durable.
async fn apply_runtime_progress(
    update: RuntimeUpdate,
    collaboration: Option<&mut crate::collaboration::Collaboration>,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    status_line: &mut Option<statusline::StatusLine>,
    frames: &mut stream_frames::StreamFrames,
) -> anyhow::Result<bool> {
    let finished = apply_runtime_update(update, runtime, workspace, status_line, frames);
    if let Some(collaboration) = collaboration {
        collaboration.refresh_root(runtime).await?;
        collaboration.deliver_pending(runtime).await?;
        collaboration.apply_child_controls(workspace)?;
    }
    Ok(finished)
}

/// Publishes preparation output first, then advances the root-owned projection slot once its
/// journal barrier has settled. Either applied output starts a fresh frame iteration.
async fn restart_after_owned_updates(
    preparation: &mut plexmaton_cli::preparation::LivePreparation,
    collaboration: Option<&mut crate::collaboration::Collaboration>,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    frames: &mut stream_frames::StreamFrames,
) -> anyhow::Result<bool> {
    if preparation.sync(workspace) {
        return Ok(true);
    }
    let Some(collaboration) = collaboration else {
        return Ok(false);
    };
    if !collaboration.has_pending_projection() {
        return Ok(false);
    }
    match collaboration.drive_pending(runtime).await? {
        RootProjectionProgress::Applied => {
            // Runtime events already removed from its queue reach the projection before the newly
            // numbered delegated envelope can be polled into the next frame.
            frames.flush(workspace);
            collaboration.deliver_pending(runtime).await?;
            collaboration.apply_child_controls(workspace)?;
            Ok(true)
        }
        RootProjectionProgress::Idle
        | RootProjectionProgress::PendingCommit
        | RootProjectionProgress::RequiresReopen
        | RootProjectionProgress::ConversationMismatch => Ok(false),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the interaction boundary names each independently owned effect"
)]
async fn apply_workspace_outcome(
    outcome: plexmaton_tui::Outcome,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    mut collaboration: Option<&mut crate::collaboration::Collaboration>,
    clipboard: &mut TerminalClipboard<impl io::Write>,
    picker: &mut session_picker::ConversationPicker,
    permissions: &mut permission_controls::PermissionControls,
    status_line: &mut Option<statusline::StatusLine>,
) -> anyhow::Result<bool> {
    model::apply_settings(&outcome, runtime, workspace, picker, status_line);
    if let Some(agent) = &outcome.withdrawn {
        // The queue belongs to the runtime; the workspace only asked. The text comes back through
        // the same `undelivered` path that already returns messages nothing claimed (IQU-4).
        match runtime.withdraw_queued(agent) {
            Ok(report) => apply_report(runtime, workspace, agent.clone(), report),
            Err(error) => return Err(error).context("take back a waiting message"),
        }
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
    if let Some(request) = outcome.tree {
        match request {
            plexmaton_tui::TreeRequest::Refresh(agent) => {
                conversation_tree::open(runtime, workspace, &agent);
            }
            plexmaton_tui::TreeRequest::Navigate(navigation) => {
                conversation_tree::navigate(runtime, workspace, navigation)?;
            }
            plexmaton_tui::TreeRequest::Edit(edit) => {
                conversation_tree::edit(runtime, workspace, edit)?;
            }
            plexmaton_tui::TreeRequest::Copy(request) => {
                let copy = conversation_tree::copy(runtime, workspace, &request);
                deliver_copy(copy, clipboard, workspace)?;
            }
        }
    }
    if let Some(submission) = outcome.submitted {
        // Sending a message is the plainest sign the user moved past an offered switch (SPK-2).
        picker.forget_offer(workspace);
        child_input::dispatch(submission, collaboration.as_deref_mut(), runtime, workspace).await?;
        retry::sync_actions(runtime, workspace);
    }
    if let Some(agent_id) = outcome.interrupted {
        apply_interrupt(agent_id, runtime, workspace, collaboration.as_deref_mut()).await?;
    }
    if let Some(approval) = outcome.approval {
        let child = collaboration
            .and_then(|collaboration| collaboration.dispatch_child_approval(approval.clone()));
        if let Some((to, report)) = child {
            apply_report(runtime, workspace, to, report);
        } else {
            dispatch_live(runtime, workspace, route_approval(approval)).await?;
        }
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
        Command::Tree => conversation_tree::open(runtime, workspace, &run.target.agent),
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

/// Applies one runtime update, reporting whether the session has finished.
fn apply_runtime_update(
    update: RuntimeUpdate,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    status_line: &mut Option<statusline::StatusLine>,
    frames: &mut stream_frames::StreamFrames,
) -> bool {
    match update {
        RuntimeUpdate::Event(event) => {
            if let Some(status) = status_line {
                status.observe(&event.event);
            }
            frames.receive(workspace, event);
            retry::sync_actions(runtime, workspace);
        }
        RuntimeUpdate::Report(report) => {
            frames.flush(workspace);
            if let Some(status) = status_line {
                status.mark_dirty();
            }
            apply_report(runtime, workspace, runtime.agent_id().clone(), report);
            retry::sync_actions(runtime, workspace);
        }
        RuntimeUpdate::Finished => {
            frames.flush(workspace);
            return true;
        }
    }
    false
}

/// Handles one terminal event, reporting whether the session should close.
#[allow(clippy::too_many_arguments)]
async fn apply_terminal_event(
    event: &crossterm::event::Event,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    clipboard: &mut TerminalClipboard<impl io::Write>,
    picker: &mut session_picker::ConversationPicker,
    permissions: &mut permission_controls::PermissionControls,
    status_line: &mut Option<statusline::StatusLine>,
    collaboration: Option<&mut crate::collaboration::Collaboration>,
    frames: &mut stream_frames::StreamFrames,
) -> anyhow::Result<bool> {
    if matches!(event, crossterm::event::Event::Resize(..))
        && let Some(status) = status_line
    {
        status.mark_dirty();
    }
    let outcome = frames.handle(workspace, event);
    picker.observe_closed(workspace);
    permissions.observe_closed(workspace);
    apply_workspace_outcome(
        outcome,
        runtime,
        workspace,
        collaboration,
        clipboard,
        picker,
        permissions,
        status_line,
    )
    .await
}

/// Transfers one selected activity into the root-owned slot and drives it when JRN-7 permits.
async fn apply_collaboration(
    activity: Option<OwnedCollaborationActivity>,
    collaboration: Option<&mut crate::collaboration::Collaboration>,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    frames: &mut stream_frames::StreamFrames,
) -> anyhow::Result<()> {
    let Some((activity, collaboration)) = activity.zip(collaboration) else {
        return Ok(());
    };
    #[cfg(debug_assertions)]
    let process_cut = crate::collaboration::PendingHandoffProcessCut::capture(&activity);
    frames.flush(workspace);
    let staged = collaboration.stage(activity)?;
    collaboration.apply_child_controls(workspace)?;
    #[cfg(debug_assertions)]
    process_cut.wait();
    if let Some((to, outcome)) = collaboration.take_user_input_settlement() {
        child_input::apply_settlement(outcome, to, runtime, workspace);
        return Ok(());
    }
    if let Some((to, outcome)) = collaboration.take_stop_settlement() {
        apply_stop_settlement(outcome, to, runtime, workspace);
        return Ok(());
    }
    let progress = if staged {
        collaboration.drive_pending(runtime).await?
    } else {
        RootProjectionProgress::Idle
    };
    collaboration.apply_child_controls(workspace)?;
    if matches!(
        progress,
        RootProjectionProgress::Applied | RootProjectionProgress::Idle
    ) {
        collaboration.deliver_pending(runtime).await?;
    }
    Ok(())
}

/// Waits for the root's collaboration owner, or never resolves when the root has none.
async fn next_collaboration(
    collaboration: Option<&mut crate::collaboration::Collaboration>,
) -> Option<OwnedCollaborationActivity> {
    match collaboration {
        Some(collaboration) => collaboration.next().await,
        None => std::future::pending().await,
    }
}
