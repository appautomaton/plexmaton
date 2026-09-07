//! One iteration of the workspace: events in, terminal events in, at most one frame out.
//!
//! This exists so that "a frame happens only when something changed" is a value a caller can read
//! rather than a side effect buried in an `async fn` around a real terminal. The executable and the
//! measurement harness drive the same object, which is what makes a measured frame the same frame
//! the user gets. The contract is
//! [`specs/frame-loop.md`](../../../.agents/specs/frame-loop.md).

use std::time::Instant;

use plexmaton_core::{AgentId, ConversationEventEnvelope};
#[cfg(test)]
use ratatui::Terminal;
use ratatui::crossterm::event::{Event, KeyEventKind};

use crate::{
    intent::{Direction, DrawerIntent, SelectionIntent, TextIntent, TuiIntent},
    router::{Routed, Router, RouterContext},
    state::{
        ApprovalSubmission, CleanupNotice, ConversationRequest, ConversationRestoration,
        CopyRequest, Page, PermissionRequest, PersistenceNotice, QuitPress, Submission,
        ViewRevision, ViewState,
    },
    surface::SurfaceTree,
    theme::Palette,
    transcript::TranscriptMetrics,
};

#[cfg(test)]
mod approval_interaction_tests;
mod approval_pointer;
#[cfg(test)]
mod approval_queue_tests;
mod composer_menu;
#[cfg(test)]
mod composer_menu_tests;
#[cfg(test)]
mod composer_tests;
mod copy;
mod drawer;
mod effort;
#[cfg(test)]
mod effort_tests;
mod hover;
#[cfg(test)]
mod hover_tests;
#[cfg(test)]
mod markdown_tests;
#[cfg(test)]
mod math_tests;
mod paint;
#[cfg(test)]
mod palette_tests;
#[cfg(test)]
mod permission_tests;
mod pointer;
mod preparation;
#[cfg(test)]
mod preparation_tests;
mod pressed;
#[cfg(test)]
mod pressed_tests;
mod retry;
#[cfg(test)]
mod skill_menu_tests;
mod status;
mod text_selection;
#[cfg(test)]
mod text_selection_tests;

use pointer::{DragAutoScroll, PressedEntry};

/// Whether the event loop continues after an intent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Flow {
    /// Keep running.
    #[default]
    Continue,
    /// The user asked to leave.
    Quit,
}

/// What one terminal event left for the executable to do.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    /// Whether the loop continues.
    pub flow: Flow,
    /// What the user submitted, and who to. Only the runtime may turn it into transcript events,
    /// so it leaves the workspace as a value rather than being written anywhere (COM-3).
    pub submitted: Option<Submission>,
    /// Which agent the user asked to interrupt. The target is resolved from the focused
    /// conversation before the command crosses the composition boundary (INV-7).
    pub interrupted: Option<AgentId>,
    /// An answer to the exact approval the user explicitly opened. The runtime routes it to the
    /// loop that owns the pending call (APV-4).
    pub approval: Option<ApprovalSubmission>,
    /// What the user asked to copy. Leaves as a value for the same reason: the clipboard is the
    /// host's, and nothing in this crate may reach for it (SEL-4).
    pub copied: Option<CopyRequest>,
    /// The page the user chose from the Drawer. What opening it costs belongs to the composition
    /// root, so it leaves as a value rather than being carried out here (DRW-3).
    pub page: Option<Page>,
    /// Explicit retry, separate from ordinary composer submission.
    pub retry: Option<crate::RetrySubmission>,
    /// A conversation to open, new or by identity; only the composition root can (SPK-2).
    pub conversation: Option<ConversationRequest>,
    /// What a permissions place asked of the retained owner, which only the root holds (PER-7).
    pub permission: Option<PermissionRequest>,
    /// A Command to run against the conversation it names; the runtime admits or refuses it
    /// (CMC-1, CPL-9).
    pub command: Option<CommandRun>,
    /// An explicit effort selected for the addressed conversation.
    pub effort: Option<EffortChange>,
}

/// Effort selection captured at acceptance; only the runtime can apply it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffortChange {
    pub agent: AgentId,
    pub effort: plexmaton_core::ReasoningEffort,
}

/// A Command accepted in the composer, with the target captured at that moment (CMC-1).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRun {
    pub command: crate::Command,
    pub target: CommandTarget,
}

/// The conversation a Command runs against. The runtime's admission is its revalidation: a
/// busy or stale target is refused there with a typed reason, never redirected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandTarget {
    pub agent: AgentId,
}

impl Outcome {
    const fn quit() -> Self {
        Self {
            flow: Flow::Quit,
            submitted: None,
            interrupted: None,
            approval: None,
            copied: None,
            page: None,
            retry: None,
            conversation: None,
            permission: None,
            command: None,
            effort: None,
        }
    }
}

/// What one frame cost, in work rather than in time.
///
/// Work is the half of a performance claim that is the same on every machine, so it is what tests
/// assert and what a report prints beside its timings (FR-4).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameWork {
    /// Entry heights first resolved or invalidated for this frame; expensive preparation is external.
    pub entries_wrapped: usize,
    /// Conversation lines this frame built.
    pub lines_built: usize,
}

/// The projection, its router, its retained layout cache, and the last frame's registry.
///
/// Held together because they are only correct together: routing hit-tests the surfaces the last
/// frame drew (SURF-1), the wheel resolves a reading position against the heights that frame
/// measured (TR-3), and the repaint gate compares against the revision that frame painted (FR-1).
/// Assembling these separately at each call site is three chances to wire one of them wrong.
#[derive(Debug, Default)]
pub struct Workspace {
    state: ViewState,
    router: Router,
    surfaces: SurfaceTree,
    metrics: TranscriptMetrics,
    palette: Palette,
    painted: Option<ViewRevision>,
    /// Decisions bind to the request and scope in the last successfully delivered frame.
    painted_approval: Option<approval_pointer::PaintedApproval>,
    frames: u64,
    /// Last reported mouse cell; identical reports cannot undo keyboard navigation.
    hover_point: Option<crate::Point>,
    /// Foldable entry pressed most recently; drag/cancel clears it before release can disclose it.
    pressed_entry: Option<PressedEntry>,
    /// The one row a button press landed on, on any surface with rows.
    pressed: Option<pressed::Pressed>,
    /// Timer-owned motion for a captured conversation drag held at a viewport edge.
    drag_autoscroll: Option<DragAutoScroll>,
    preparation: preparation::Preparation,
    copy: copy::CopyPreparation,
    native: crate::math::NativeFrame,
    effort_animation: Option<effort::EffortAnimation>,
}

impl Workspace {
    /// Delivers producer feedback to the exact pending approval card.
    pub fn report_approval_refusal(
        &mut self,
        agent: &AgentId,
        approval: &plexmaton_core::ApprovalId,
        feedback: crate::ApprovalFeedback,
        offer: Option<plexmaton_core::RememberPermissionOffer>,
    ) {
        self.state
            .report_approval_refusal(agent, approval, feedback, offer);
    }
    /// Builds a workspace that paints with `palette`.
    ///
    /// The default workspace uses [`Palette::ansi`]. Widgets resolve semantic roles through this
    /// value; replacing it with [`Self::set_palette`] changes presentation, not widget definitions.
    #[must_use]
    pub fn with_palette(palette: Palette) -> Self {
        Self::with_presentation(palette, crate::math::MathPresentation::default())
    }

    /// Construct one generation with the output owner's measured native-text capability.
    /// Capability is immutable for that generation; palette-only changes retain its geometry.
    #[must_use]
    pub fn with_presentation(palette: Palette, math: crate::math::MathPresentation) -> Self {
        Self {
            palette,
            metrics: TranscriptMetrics::with_math(math),
            ..Self::default()
        }
    }

    /// Replace presentation styles without changing semantic state, height geometry or selection.
    /// Reapplying the same palette is a no-op; a changed palette requests one repaint (FR-1/TR-1).
    pub fn set_palette(&mut self, palette: Palette) {
        if self.palette != palette {
            self.palette = palette;
            self.painted = None;
        }
    }

    /// The projection, for whatever the executable needs to read out of it.
    #[must_use]
    pub const fn state(&self) -> &ViewState {
        &self.state
    }

    /// Retained transcript layout work, for a harness to count.
    #[must_use]
    pub const fn metrics(&self) -> &TranscriptMetrics {
        &self.metrics
    }

    /// The surfaces the last frame drew, which is what a pointer event resolves against.
    ///
    /// Read-only, and read-only is the whole point: a caller that could register into this could
    /// give routing geometry no frame ever painted, which is the defect SURF-1 exists to prevent.
    #[must_use]
    pub const fn surfaces(&self) -> &SurfaceTree {
        &self.surfaces
    }

    /// Frames painted since this workspace started.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Applies producer events to the projection.
    ///
    /// A producer contract violation is a visible, typed notice inside the projection rather than a
    /// reason to tear down the user's terminal (`state::notices`), so nothing is returned to check
    /// here.
    pub fn emit(&mut self, events: Vec<ConversationEventEnvelope>) {
        let before = self.state.revision();
        for envelope in events {
            let _outcome = self.state.apply(envelope);
        }
        if self.state.revision() != before {
            self.validate_text_selection();
            self.reconcile_copy();
            // Producer changes can move rows under a stationary pointer. The next motion resolves
            // a fresh frame target; keeping the old identity would make the accent move with it.
            self.state.hover_entry(None);
        }
    }

    /// Restores user text a runtime returned instead of silently discarding its ownership.
    pub fn return_input(&mut self, to: AgentId, text: String) {
        self.state.return_input(to, text);
        // Composer growth can change the transcript viewport beneath a stationary pointer.
        self.state.hover_entry(None);
    }

    /// Restores returned text and its deliberate skill binding when the composer is otherwise empty.
    pub fn return_skill_input(&mut self, to: AgentId, text: String, skill: Option<String>) {
        self.state.return_skill_input(to, text, skill);
        self.state.hover_entry(None);
    }

    /// Shows a session-writer failure that cannot itself enter the failed durable stream.
    pub fn report_persistence_failure(&mut self, failure: PersistenceNotice) {
        self.state.report_persistence_failure(failure);
    }

    /// Shows an owner that could not be joined cleanly after session persistence failed.
    pub fn report_cleanup_failure(&mut self, failure: CleanupNotice) {
        self.state.report_cleanup_failure(failure);
    }

    /// Anchors a saved-grant receipt beside its tool without changing semantic copy or events.
    pub fn report_saved_project_permission(
        &mut self,
        to: &AgentId,
        receipt: plexmaton_core::SavedProjectPermission,
    ) {
        self.state.report_saved_project_permission(to, receipt);
    }

    /// Shows one bounded skill discovery, load, or activation diagnostic.
    pub fn report_skill_diagnostic(&mut self, message: String) {
        self.state.report_skill_diagnostic(message);
    }

    /// Confirms successful restoration, including any file-tail repair, outside the journal.
    pub fn report_conversation_recovery(&mut self, recovery: ConversationRestoration) {
        self.state.report_conversation_recovery(recovery);
    }

    /// Where a compaction the user asked for stands, from the composition root (CPL-9).
    pub fn report_compaction(&mut self, agent: &AgentId, note: crate::CompactionNote) {
        self.state.report_compaction(agent, note);
    }

    /// Translates one terminal event and applies whatever it asked for.
    pub fn handle(&mut self, event: &Event) -> Outcome {
        self.handle_at(event, Instant::now())
    }

    /// Time-explicit event reduction keeps the chord deterministic under tests and at its boundary.
    fn handle_at(&mut self, event: &Event, now: Instant) -> Outcome {
        let Self {
            state,
            router,
            surfaces,
            ..
        } = self;
        let context = RouterContext {
            effort_selector: state.effort_visible(),
            surfaces,
            // Derived from whichever surface holds focus, never asserted here (SURF-3).
            focus: state.keyboard_focus(surfaces),
            focused: state.focused(surfaces),
            // A fact about the frame that was drawn, not about intent: `Escape` resolves the
            // layer the user can see (FR-3).
            dismissible: surfaces.has_dismissible()
                || state.editing_retry()
                || state.focused(surfaces) == Some(crate::SurfaceId::Approval),
            selecting: state.selection().is_some() || state.copy_input(surfaces).is_some(),
        };
        let routed = router.translate(event, &context);
        if let Event::Key(key) = event
            && key.kind != KeyEventKind::Release
        {
            // Once the user switches to the keyboard, a pointer affordance no longer claims to be
            // the active target. Repeating `None` is free (FR-1).
            state.hover_entry(None);
        }
        let outcome = match routed {
            Routed::Intent(intent) => self.apply(intent, now),
            Routed::Ignored(_) => Outcome::default(),
        };
        if outcome.copied.is_some() {
            self.cancel_pending_copy();
        }
        self.reconcile_copy();
        outcome
    }

    /// Names where the process runs, for the status line. The composition root knows; this crate
    /// never asks the filesystem.
    pub fn set_working_directory(&mut self, path: String) {
        self.state.set_working_directory(path);
    }

    /// Opens the Configuration page with what the composition root projected (DRW-4).
    pub fn show_configuration(&mut self, summary: crate::ConfigurationSummary) {
        self.state.show_configuration(summary);
    }

    /// Names the resolved model for the composer's rule (ui-ux §input).
    pub fn set_model(&mut self, summary: crate::ConfigurationSummary) {
        self.state.set_model(summary);
    }

    /// Whether projection changes, resize or palette replacement need another frame (FR-1).
    ///
    /// A presentation batch can use this gate without owning another copy of the painted revision.
    #[must_use]
    pub fn needs_draw(&self) -> bool {
        self.painted != Some(self.state.revision())
    }

    /// Applies one intent to the workspace.
    ///
    /// Intents whose reducer arrives in a later delivery step are listed explicitly rather than
    /// caught by a wildcard, so a new intent cannot be added and silently do nothing.
    fn apply(&mut self, intent: TuiIntent, now: Instant) -> Outcome {
        if !matches!(intent, TuiIntent::Pointer(_)) {
            self.pressed = None;
        }
        match intent {
            TuiIntent::Menu(intent) => return self.apply_menu(intent),
            TuiIntent::Retry(action) => {
                return Outcome {
                    retry: self.perform_retry_action(action),
                    ..Outcome::default()
                };
            }
            TuiIntent::Quit => match self.state.press_quit(now) {
                QuitPress::Confirmed => return Outcome::quit(),
                QuitPress::Asked => {}
            },
            TuiIntent::Interrupt => {
                return Outcome {
                    interrupted: self.state.interrupt(&self.surfaces),
                    ..Outcome::default()
                };
            }
            TuiIntent::Drawer(DrawerIntent::Open) => {
                self.state.open_drawer(&self.surfaces);
            }
            TuiIntent::Drawer(DrawerIntent::Step(direction)) => {
                self.step_drawer(direction);
            }
            TuiIntent::Drawer(DrawerIntent::Choose) => {
                return self.choose_in_drawer();
            }
            TuiIntent::Text(edit) => {
                if matches!(edit, TextIntent::Submit)
                    && self.state.editing_retry()
                    && self.state.focused(&self.surfaces) == Some(crate::SurfaceId::Composer)
                {
                    return Outcome {
                        retry: self.state.retry_submission(),
                        ..Outcome::default()
                    };
                }
                // A whole draft that is a Command runs, menu or no menu (CMC-2).
                if matches!(edit, TextIntent::Submit)
                    && self.state.focused(&self.surfaces) == Some(crate::SurfaceId::Composer)
                    && let Some(outcome) = self.submit_command()
                {
                    return outcome;
                }
                let submitted = self.state.edit(&self.surfaces, edit);
                return Outcome {
                    submitted,
                    ..Outcome::default()
                };
            }
            TuiIntent::Selection(SelectionIntent::Extend(direction)) => {
                self.state.select(&self.surfaces, direction);
            }
            TuiIntent::Selection(SelectionIntent::ToggleOpen) => {
                self.state
                    .toggle_selected_entry(&self.surfaces, &self.metrics);
            }
            TuiIntent::Selection(SelectionIntent::Copy) => {
                return Outcome {
                    copied: self
                        .state
                        .copy_input(&self.surfaces)
                        .or_else(|| self.request_selection_copy()),
                    ..Outcome::default()
                };
            }
            TuiIntent::MoveSelection(direction) => self.state.move_selection(direction),
            TuiIntent::CycleFocus(direction) => self.state.cycle_focus(&self.surfaces, direction),
            TuiIntent::Pointer(pointer) => {
                if let Some(outcome) = self.button_pointer(pointer) {
                    return outcome;
                }
                return Outcome {
                    copied: self.pointer(pointer, now),
                    ..Outcome::default()
                };
            }
            TuiIntent::Inspector(inspector) => self.state.inspect(&self.surfaces, inspector),
            TuiIntent::Attention(attention) => self.state.attend(&self.surfaces, attention),
            TuiIntent::InspectCommand(action) => return self.inspect_command(action),
            TuiIntent::Approval(approval) => {
                return Outcome {
                    approval: self.decide_visible_approval(approval),
                    ..Outcome::default()
                };
            }
            TuiIntent::Dismiss => {
                self.state.dismiss(&self.surfaces);
            }
            // A resize leaves the projection unchanged, so the repaint gate has to be told that the
            // painted frame no longer describes the screen (FR-1).
            TuiIntent::TerminalResized { .. } => {
                self.painted_approval = None;
                self.pressed = None;
                self.state.hover_entry(None);
                self.cancel_pointer_click();
                self.painted = None;
            }
            // Hover routing: the wheel moves the viewport under the pointer and never touches focus
            // (INV-3). Which surface that is was already decided by viewport eligibility.
            TuiIntent::Scroll { surface, direction } => {
                // The row under a stationary pointer may change when its viewport moves. Clear the
                // old semantic target rather than highlighting it at its new location.
                self.state.hover_entry(None);
                self.state
                    .scroll(&self.surfaces, &self.metrics, surface, direction);
            }
            TuiIntent::Hover { surface, at } => self.hover(surface, at),
        }
        Outcome::default()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ratatui::{
        Terminal,
        backend::TestBackend,
        crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        },
        layout::Rect,
        style::{Color, Style},
    };

    use plexmaton_core::{
        AgentId, AgentStatus, ApprovalDecision, ApprovalId, ArtifactId, AttentionId,
        AttentionRequest, ConversationEvent, ToolCallId, ToolCallStatus, ToolCapability,
        ToolDetail, ToolPresentation, TranscriptItemId,
    };

    use super::{Flow, Outcome, Workspace};
    use crate::{
        Page, SubmissionKind,
        state::StatusNote,
        surface::{Point, SurfaceId},
        test_support::{Conversation, canonical_runtime},
        theme::{Palette, Role},
    };

    /// A workspace holding the canonical timeline, with one frame already drawn.
    fn drawn(width: u16, height: u16) -> (Workspace, Terminal<TestBackend>) {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(canonical_runtime().ready(u64::MAX));
        workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        (workspace, terminal)
    }

    struct FoldableTool {
        workspace: Workspace,
        terminal: Terminal<TestBackend>,
        conversation: Conversation,
        agent: AgentId,
        item: TranscriptItemId,
        call: ToolCallId,
        invocation: ToolPresentation,
    }

    /// A running tool with retained invocation detail, newest in the primary conversation.
    fn foldable_tool() -> FoldableTool {
        foldable_tool_on(SurfaceId::Transcript)
    }

    /// A running tool in the conversation drawn by `surface`.
    fn foldable_tool_on(surface: SurfaceId) -> FoldableTool {
        let mut conversation = Conversation::canonical();
        let agent = match surface {
            SurfaceId::Transcript => conversation.state.primary_agent(),
            SurfaceId::Inspector => conversation.state.sub_agents().next(),
            _ => None,
        }
        .map(|agent| agent.id.clone())
        .unwrap_or_else(|| panic!("the canonical timeline creates an agent for {surface:?}"));
        let item = TranscriptItemId::new("foldable-tool")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        let call =
            ToolCallId::new("foldable-tool").unwrap_or_else(|error| panic!("fixture: {error}"));
        conversation.emit(ConversationEvent::ToolCallChanged {
            agent_id: agent.clone(),
            item_id: item.clone(),
            item_revision: 0,
            call_id: call.clone(),
            label: "read_file".to_owned(),
            status: ToolCallStatus::Queued,
            presentation: ToolPresentation::default(),
        });
        let invocation = ToolPresentation {
            invocation: Some(ToolDetail::Text {
                source: "path: crates/plexmaton-tui/src/content.rs".to_owned(),
                omitted_bytes: 0,
            }),
            outcome: None,
        };
        conversation.emit(ConversationEvent::ToolCallChanged {
            agent_id: agent.clone(),
            item_id: item.clone(),
            item_revision: 1,
            call_id: call.clone(),
            label: "read_file".to_owned(),
            status: ToolCallStatus::Running,
            presentation: invocation.clone(),
        });
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);
        if surface == SurfaceId::Inspector {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Down, KeyModifiers::NONE),
            );
        }
        FoldableTool {
            workspace,
            terminal,
            conversation,
            agent,
            item,
            call,
            invocation,
        }
    }

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn bounds(workspace: &Workspace, surface_id: SurfaceId) -> Rect {
        workspace
            .surfaces
            .get(surface_id)
            .unwrap_or_else(|| panic!("{surface_id:?} must be registered"))
            .bounds
    }

    #[test]
    fn an_injected_palette_is_the_one_the_frame_paints() {
        let palette = Palette::from_roles(|role| match role {
            Role::Border => Style::new().fg(Color::Magenta),
            role => Palette::ansi().style(role),
        });
        let mut workspace = Workspace::with_palette(palette);
        let mut terminal = Terminal::new(TestBackend::new(120, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(canonical_runtime().ready(u64::MAX));
        workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));

        // The composer is not focused at start, so its top rule wears the plain border role.
        let composer = bounds(&workspace, SurfaceId::Composer);
        let corner = &terminal.backend().buffer()[(composer.x, composer.y)];
        assert_eq!(corner.symbol(), "─");
        assert_eq!(
            corner.style().fg,
            Some(Color::Magenta),
            "the frame must paint the injected assignment, not the ansi preset"
        );
    }

    /// FR-1: a frame is drawn when something changed and at no other time.
    ///
    /// Both halves matter and they fail differently. Without the gate the workspace repaints on
    /// every idle tick; without the resize arm it never repaints after a resize, because a resize
    /// changes what a frame means without changing anything the revision counts.
    #[test]
    fn a_frame_is_drawn_only_when_something_changed() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        let settled = workspace.frames();
        assert_eq!(
            settled, 2,
            "the initial pending frame and its preparation both paint"
        );

        let work = workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        assert_eq!(work, None, "an unchanged projection must not repaint");

        assert_eq!(
            workspace.handle(&press(KeyCode::Char('x'), KeyModifiers::NONE)),
            Outcome::default(),
            "an unbound key asks for nothing"
        );
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "and an unbound key must not force a redraw"
        );

        workspace.handle(&Event::Resize(100, 40));
        assert!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"))
                .is_some(),
            "a resize changes no projection state, and must still force the next frame"
        );
        assert_eq!(
            workspace.frames(),
            settled + 1,
            "only the resize cost another frame"
        );
    }

    /// ENT-4/TR-1/TR-3: disclosure addresses the moving end of a semantic range, preserves the
    /// reader's top anchor, and invalidates only that entry at every retained width.
    #[test]
    fn ctrl_o_opens_the_selections_focus_entry_in_place_at_each_drawn_width() {
        let FoldableTool {
            mut workspace,
            mut terminal,
            mut conversation,
            agent,
            item,
            ..
        } = foldable_tool();
        conversation.extend(1);
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);

        // Warm the second cache width before disclosure, then return to the first.
        terminal.backend_mut().resize(95, 40);
        workspace.handle(&Event::Resize(95, 40));
        frame(&mut workspace, &mut terminal);
        terminal.backend_mut().resize(120, 40);
        workspace.handle(&Event::Resize(120, 40));
        frame(&mut workspace, &mut terminal);
        tab_to(&mut workspace, &mut terminal, SurfaceId::Transcript);

        // The newest filler is the anchor; the second backward extension leaves a two-entry range
        // whose moving end is the preceding tool.
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        assert_eq!(
            workspace
                .state
                .selection()
                .map(|selection| selection.entries()),
            Some(2)
        );
        let viewport = workspace
            .surfaces
            .viewport(SurfaceId::Transcript)
            .unwrap_or_else(|| panic!("the transcript has a measured viewport"));
        let anchor = workspace
            .metrics
            .anchor_at(&agent, viewport.content_width, viewport.offset)
            .unwrap_or_else(|| panic!("the visible conversation has a top anchor"));

        workspace.handle(&press(KeyCode::Char('o'), KeyModifiers::CONTROL));
        let opened = frame(&mut workspace, &mut terminal);
        assert_eq!(opened.entries_wrapped, 1);
        assert!(workspace.state.disclosure().is_open(&item));
        assert_eq!(workspace.state.conversation_position(&agent), Some(&anchor));
        assert!(
            painted(&terminal, &workspace, SurfaceId::Transcript)
                .contains("path: crates/plexmaton-tui/src/content.rs")
        );

        terminal.backend_mut().resize(95, 40);
        workspace.handle(&Event::Resize(95, 40));
        assert_eq!(
            frame(&mut workspace, &mut terminal).entries_wrapped,
            1,
            "the already-cached second width invalidates only the disclosed item"
        );
    }

    /// INV-3/FR-1: bare pointer motion is presentation only, and repeating its resolved target is
    /// free. Selection, focus, scroll, and the semantic entry all remain byte-for-byte unchanged.
    #[test]
    fn hover_changes_only_the_foldable_rows_appearance_and_repeating_it_costs_nothing() {
        let FoldableTool {
            mut workspace,
            mut terminal,
            mut conversation,
            agent,
            item,
            ..
        } = foldable_tool();
        let at = point_on(&terminal, &workspace, SurfaceId::Transcript, "read_file");
        let focus = focused(&workspace);
        let selection = workspace.state.selection().cloned();
        let position = workspace.state.conversation_position(&agent).cloned();
        let semantic = workspace
            .state
            .agent(&agent)
            .cloned()
            .unwrap_or_else(|| panic!("the primary agent exists"));

        workspace.handle(&mouse(MouseEventKind::Moved, at.x, at.y));
        let hovered = frame(&mut workspace, &mut terminal);
        assert_eq!(hovered.entries_wrapped, 0, "style does not change height");
        assert_eq!(focused(&workspace), focus);
        assert_eq!(workspace.state.selection(), selection.as_ref());
        assert_eq!(
            workspace.state.conversation_position(&agent),
            position.as_ref()
        );
        assert_eq!(workspace.state.agent(&agent), Some(&semantic));
        assert_eq!(
            terminal.backend().buffer()[(at.x, at.y)].style().fg,
            workspace.palette.style(Role::Accent).fg,
            "the compact row advertises that it can be opened"
        );

        let wrapped = workspace.metrics.wrapped();
        workspace.handle(&mouse(MouseEventKind::Moved, at.x, at.y));
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "the same hover target is not another visible fact"
        );
        assert_eq!(workspace.metrics.wrapped(), wrapped);

        conversation.extend(1);
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);
        assert!(
            !workspace
                .state
                .entry_appearance(SurfaceId::Transcript, &agent, &item, false)
                .hovered,
            "producer-driven relayout invalidates the stale under-pointer identity"
        );
    }

    /// ENT-4/FR-3: one click opens the item from the frame pressed, even when focusing an
    /// inspector inserts its input strip before release. `Ctrl-O` requires an explicit selection;
    /// neither a disclosure click nor a hover manufactures that selection, and a drag is not a click.
    #[test]
    fn pointer_and_ctrl_o_toggle_the_same_item_while_drag_cancels_disclosure() {
        let FoldableTool {
            mut workspace,
            mut terminal,
            item,
            ..
        } = foldable_tool_on(SurfaceId::Inspector);
        assert_eq!(focused(&workspace), Some(SurfaceId::Agents));
        let at = point_on(&terminal, &workspace, SurfaceId::Inspector, "read_file");

        step(
            &mut workspace,
            &mut terminal,
            &mouse(MouseEventKind::Down(MouseButton::Left), at.x, at.y),
        );
        assert_eq!(focused(&workspace), Some(SurfaceId::Inspector));
        step(
            &mut workspace,
            &mut terminal,
            &mouse(MouseEventKind::Up(MouseButton::Left), at.x, at.y),
        );
        assert!(
            workspace.state.disclosure().is_open(&item),
            "the first click survives the focus-driven relayout"
        );
        assert_eq!(
            workspace
                .state
                .selection()
                .map(|selection| selection.entries()),
            None
        );
        workspace.handle(&press(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert!(workspace.state.disclosure().is_open(&item));
        assert!(
            workspace
                .settled_draw(&mut terminal)
                .expect("unselected Ctrl-O")
                .is_none()
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        let selected_at = point_on(&terminal, &workspace, SurfaceId::Inspector, "read_file");
        workspace.handle(&mouse(MouseEventKind::Moved, selected_at.x, selected_at.y));
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "hover masked by the selected style is not retained as an invisible frame"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Char('o'), KeyModifiers::CONTROL),
        );
        assert!(
            !workspace.state.disclosure().is_open(&item),
            "the keyboard addresses the explicit one-entry selection"
        );

        let at = point_on(&terminal, &workspace, SurfaceId::Inspector, "read_file");
        step(
            &mut workspace,
            &mut terminal,
            &mouse(MouseEventKind::Down(MouseButton::Left), at.x, at.y),
        );
        step(
            &mut workspace,
            &mut terminal,
            &mouse(
                MouseEventKind::Drag(MouseButton::Left),
                at.x.saturating_add(1),
                at.y,
            ),
        );
        step(
            &mut workspace,
            &mut terminal,
            &mouse(
                MouseEventKind::Up(MouseButton::Left),
                at.x.saturating_add(1),
                at.y,
            ),
        );
        assert!(!workspace.state.disclosure().is_open(&item));
    }

    /// ENT-4/SEL-1/SEL-2: reading a tool is not selecting it. Header/detail retain their roles,
    /// deliberate drag copies only its range, and keyboard selection still copies the whole source.
    #[test]
    fn tool_disclosure_never_creates_a_copy_range_or_reverses_detail_at_three_widths() {
        for surface in [SurfaceId::Transcript, SurfaceId::Inspector] {
            for width in [120, 88, 60] {
                let FoldableTool {
                    mut workspace,
                    mut terminal,
                    agent,
                    item,
                    invocation,
                    ..
                } = foldable_tool_on(surface);
                terminal.backend_mut().resize(width, 40);
                workspace.handle(&Event::Resize(width, 40));
                frame(&mut workspace, &mut terminal);
                let semantic = workspace.state.agent(&agent).cloned().expect("agent");
                let at = point_on(&terminal, &workspace, surface, "read_file");
                let pressed =
                    workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at.x, at.y));
                assert!(pressed.copied.is_none());
                frame(&mut workspace, &mut terminal);
                let release =
                    workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at.x, at.y));
                assert!(release.copied.is_none());
                assert_eq!(frame(&mut workspace, &mut terminal).entries_wrapped, 1);
                assert!(workspace.state.disclosure().is_open(&item));
                assert!(workspace.state.selection().is_none());
                assert!(
                    workspace
                        .handle(&press(KeyCode::Char('y'), KeyModifiers::CONTROL))
                        .copied
                        .is_none()
                );
                assert_eq!(workspace.state.agent(&agent), Some(&semantic));
                for needle in ["read_file", "invocation", "path:"] {
                    let row = point_on(&terminal, &workspace, surface, needle).y;
                    let bounds = bounds(&workspace, surface);
                    for x in bounds.x + 1..bounds.right() - 1 {
                        assert!(
                            !terminal.backend().buffer()[(x, row)]
                                .modifier
                                .contains(ratatui::style::Modifier::REVERSED),
                            "disclosure reversed {needle} at {surface:?}/{width}"
                        );
                    }
                }
                let source_row = point_on(&terminal, &workspace, surface, "path:");
                let start = Point {
                    x: source_row.x + 4,
                    ..source_row
                };
                assert_eq!(
                    terminal.backend().buffer()[(start.x, start.y)].symbol(),
                    "p"
                );
                workspace.handle(&mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    start.x,
                    start.y,
                ));
                frame(&mut workspace, &mut terminal);
                workspace.handle(&mouse(
                    MouseEventKind::Drag(MouseButton::Left),
                    start.x + 4,
                    start.y,
                ));
                frame(&mut workspace, &mut terminal);
                assert!(
                    terminal.backend().buffer()[(start.x, start.y)]
                        .modifier
                        .contains(ratatui::style::Modifier::REVERSED),
                    "{surface:?}/{width} at {start:?}: selection {:?}, drawn {}",
                    workspace.state.selection(),
                    painted(&terminal, &workspace, surface)
                );
                assert!(
                    !terminal.backend().buffer()[(start.x + 4, start.y)]
                        .modifier
                        .contains(ratatui::style::Modifier::REVERSED)
                );
                let copied = workspace
                    .handle(&mouse(
                        MouseEventKind::Up(MouseButton::Left),
                        start.x + 4,
                        start.y,
                    ))
                    .copied
                    .expect("drag copies");
                assert_eq!(copied.text, "path");
                assert!(
                    workspace.state.disclosure().is_open(&item),
                    "drag cannot collapse the tool"
                );
                assert!(
                    workspace
                        .settled_draw(&mut terminal)
                        .expect("unchanged released range")
                        .is_none()
                );
                step(
                    &mut workspace,
                    &mut terminal,
                    &press(KeyCode::Esc, KeyModifiers::NONE),
                );
                step(
                    &mut workspace,
                    &mut terminal,
                    &press(KeyCode::Up, KeyModifiers::SHIFT),
                );
                let copied = workspace
                    .handle(&press(KeyCode::Char('y'), KeyModifiers::CONTROL))
                    .copied
                    .expect("explicit whole selection");
                let Some(ToolDetail::Text { source, .. }) = invocation.invocation else {
                    panic!("retained invocation");
                };
                assert_eq!(copied.text, source);
                assert_eq!(copied.entries, 1);
            }
        }
    }

    /// TR-1/MD-4/MD-5/ENT-4: disclosed tool preparation is shared by measurement, paint and copy;
    /// palette/hover reuse it, while a lifecycle revision replaces precisely that prepared entry.
    #[test]
    fn open_tool_repaint_reuses_preparation_and_keeps_hover_local() {
        for surface in [SurfaceId::Transcript, SurfaceId::Inspector] {
            for width in [120, 88, 60] {
                let FoldableTool {
                    mut workspace,
                    mut terminal,
                    mut conversation,
                    agent,
                    item,
                    call,
                    invocation,
                } = foldable_tool_on(surface);
                terminal.backend_mut().resize(width, 44);
                workspace.handle(&Event::Resize(width, 44));
                frame(&mut workspace, &mut terminal);
                let at = point_on(&terminal, &workspace, surface, "read_file");
                workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), at.x, at.y));
                workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), at.x, at.y));
                let before = workspace.metrics.text_layouts();
                frame(&mut workspace, &mut terminal);
                assert_eq!(
                    workspace.metrics.text_layouts(),
                    before + 1,
                    "open prepares once"
                );
                let prepared = workspace.metrics.text_layouts();
                let source = workspace.state.agent(&agent).expect("agent").clone();
                for palette in [Palette::pastel(), Palette::monochrome(), Palette::ansi()] {
                    workspace.set_palette(palette);
                    assert_eq!(frame(&mut workspace, &mut terminal).entries_wrapped, 0);
                    let at = point_on(&terminal, &workspace, surface, "read_file");
                    workspace.handle(&mouse(MouseEventKind::Moved, at.x, at.y));
                    workspace
                        .settled_draw(&mut terminal)
                        .expect("hover may already be retained");
                    let detail = point_on(&terminal, &workspace, surface, "path:");
                    assert_eq!(
                        terminal.backend().buffer()[(detail.x + 4, detail.y)].symbol(),
                        "p"
                    );
                    assert_eq!(
                        terminal.backend().buffer()[(at.x, at.y)].fg,
                        palette.style(Role::Accent).fg.unwrap_or(Color::Reset)
                    );
                    assert_eq!(
                        terminal.backend().buffer()[(detail.x + 4, detail.y)].fg,
                        palette.style(Role::Body).fg.unwrap_or(Color::Reset)
                    );
                    assert_eq!(
                        workspace.metrics.text_layouts(),
                        prepared,
                        "paint/hover re-prepared detail"
                    );
                    assert_eq!(workspace.state.agent(&agent), Some(&source));
                }
                let mut completed = invocation;
                completed.outcome = Some(ToolDetail::Text {
                    source: "done".into(),
                    omitted_bytes: 0,
                });
                conversation.emit(ConversationEvent::ToolCallChanged {
                    agent_id: agent,
                    item_id: item.clone(),
                    item_revision: 2,
                    call_id: call,
                    label: "read_file".into(),
                    status: ToolCallStatus::Succeeded,
                    presentation: completed,
                });
                workspace.emit(conversation.drain());
                assert_eq!(frame(&mut workspace, &mut terminal).entries_wrapped, 1);
                assert_eq!(workspace.metrics.text_layouts(), prepared + 1);
                assert!(workspace.state.disclosure().is_open(&item));
                assert!(painted(&terminal, &workspace, surface).contains("done"));
            }
        }
    }

    /// ENT-4/MD-4/MD-5/SEL-2: actual conversation selection adds reversal without flattening
    /// canonical diff meaning; palette changes repaint the same prepared rows and exact source.
    #[test]
    fn selected_diff_keeps_semantic_colors_and_reuses_prepared_rows_at_three_widths() {
        const PATCH: &str =
            "*** Begin Patch\n*** Update File: config.toml\n-old\n+new\n*** End Patch";
        for surface in [SurfaceId::Transcript, SurfaceId::Inspector] {
            for width in [120, 88, 60] {
                let FoldableTool {
                    mut workspace,
                    mut terminal,
                    mut conversation,
                    agent,
                    item,
                    call,
                    mut invocation,
                } = foldable_tool_on(surface);
                invocation.outcome = Some(ToolDetail::Diff {
                    patch: PATCH.into(),
                });
                conversation.emit(ConversationEvent::ToolCallChanged {
                    agent_id: agent,
                    item_id: item,
                    item_revision: 2,
                    call_id: call,
                    label: "read_file".into(),
                    status: ToolCallStatus::Succeeded,
                    presentation: invocation.clone(),
                });
                workspace.emit(conversation.drain());
                terminal.backend_mut().resize(width, 44);
                workspace.handle(&Event::Resize(width, 44));
                frame(&mut workspace, &mut terminal);
                tab_to(&mut workspace, &mut terminal, surface);
                step(
                    &mut workspace,
                    &mut terminal,
                    &press(KeyCode::Up, KeyModifiers::SHIFT),
                );
                step(
                    &mut workspace,
                    &mut terminal,
                    &press(KeyCode::Char('o'), KeyModifiers::CONTROL),
                );
                // Disclosure parks the original reading position; reach the enlarged tail in
                // the inspector's shorter viewport before asserting its diff colors.
                let region = bounds(&workspace, surface);
                step(
                    &mut workspace,
                    &mut terminal,
                    &mouse(MouseEventKind::ScrollDown, region.x + 1, region.y + 1),
                );
                let prepared = workspace.metrics.text_layouts();
                for palette in [Palette::pastel(), Palette::monochrome(), Palette::ansi()] {
                    workspace.set_palette(palette);
                    assert_eq!(frame(&mut workspace, &mut terminal).entries_wrapped, 0);
                    assert_eq!(workspace.metrics.text_layouts(), prepared);
                    for (text, role) in [("-old", Role::Failure), ("+new", Role::NewInformation)] {
                        let at = point_on(&terminal, &workspace, surface, text);
                        let cell = &terminal.backend().buffer()[(at.x + 4, at.y)];
                        assert_eq!(cell.fg, palette.style(role).fg.unwrap_or(Color::Reset));
                        assert!(cell.modifier.contains(
                            ratatui::style::Modifier::REVERSED | palette.style(role).add_modifier
                        ));
                    }
                    let copied = workspace
                        .handle(&press(KeyCode::Char('y'), KeyModifiers::CONTROL))
                        .copied
                        .expect("whole diff source");
                    let Some(ToolDetail::Text { source, .. }) = &invocation.invocation else {
                        panic!("retained invocation");
                    };
                    assert_eq!(copied.text, format!("{source}\n{PATCH}"));
                    workspace
                        .settled_draw(&mut terminal)
                        .expect("copy feedback");
                }
            }
        }
    }

    /// ENT-4/SEL-4: disclosure never changes clipboard source, and neither width, scroll, nor a
    /// palette substitution can make copied tool text inherit terminal decoration.
    #[test]
    fn tool_copy_is_identical_when_compact_open_resized_scrolled_and_monochrome() {
        let FoldableTool {
            mut workspace,
            mut terminal,
            invocation,
            ..
        } = foldable_tool();
        tab_to(&mut workspace, &mut terminal, SurfaceId::Transcript);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        let copy = |workspace: &mut Workspace| {
            workspace
                .handle(&press(KeyCode::Char('y'), KeyModifiers::CONTROL))
                .copied
                .unwrap_or_else(|| panic!("the selected tool has retained source"))
        };
        let compact = copy(&mut workspace);

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Char('o'), KeyModifiers::CONTROL),
        );
        let open = copy(&mut workspace);
        terminal.backend_mut().resize(60, 24);
        workspace.handle(&Event::Resize(60, 24));
        frame(&mut workspace, &mut terminal);
        let transcript = bounds(&workspace, SurfaceId::Transcript);
        step(
            &mut workspace,
            &mut terminal,
            &mouse(
                MouseEventKind::ScrollUp,
                transcript.x.saturating_add(1),
                transcript.y.saturating_add(1),
            ),
        );
        let resized = copy(&mut workspace);
        workspace.palette = Palette::monochrome();
        workspace.painted = None;
        frame(&mut workspace, &mut terminal);
        let monochrome = copy(&mut workspace);

        assert_eq!(compact, open);
        assert_eq!(compact, resized);
        assert_eq!(compact, monochrome);
        let ToolPresentation {
            invocation: Some(ToolDetail::Text { source, .. }),
            ..
        } = invocation
        else {
            panic!("fixture keeps a text invocation");
        };
        assert_eq!(compact.text, source);
        assert!(!compact.text.contains("invocation"));
        assert!(!compact.text.contains('│'));
    }

    /// ENT-4: lifecycle replacement keeps view-owned disclosure on the stable transcript item.
    #[test]
    fn tool_completion_preserves_the_users_open_state() {
        let FoldableTool {
            mut workspace,
            mut terminal,
            mut conversation,
            agent,
            item,
            call,
            invocation,
        } = foldable_tool();
        tab_to(&mut workspace, &mut terminal, SurfaceId::Transcript);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Char('o'), KeyModifiers::CONTROL),
        );

        let presentation = ToolPresentation {
            invocation: invocation.invocation,
            outcome: Some(ToolDetail::Text {
                source: "status: exited\nexit_code: 0\nstdout:\n188 tests passed\nstderr:\n[empty]"
                    .to_owned(),
                omitted_bytes: 0,
            }),
        };
        conversation.emit(ConversationEvent::ToolCallChanged {
            agent_id: agent,
            item_id: item.clone(),
            item_revision: 2,
            call_id: call,
            label: "read_file".to_owned(),
            status: ToolCallStatus::Succeeded,
            presentation,
        });
        workspace.emit(conversation.drain());
        assert_eq!(frame(&mut workspace, &mut terminal).entries_wrapped, 1);

        assert!(workspace.state.disclosure().is_open(&item));
        let drawn = painted(&terminal, &workspace, SurfaceId::Transcript);
        assert!(drawn.contains("succeeded"));
        assert!(drawn.contains("188 tests passed"));
    }

    /// FR-1: the derived current-work fact occupies chrome, not another row or another revision.
    #[test]
    fn current_work_does_not_move_input_and_repeated_facts_cost_no_frame() {
        let mut conversation = Conversation::canonical();
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);

        let transcript = bounds(&workspace, SurfaceId::Transcript);
        let composer = bounds(&workspace, SurfaceId::Composer);
        let activity = |terminal: &Terminal<TestBackend>, workspace: &Workspace| {
            painted(terminal, workspace, SurfaceId::Transcript)
                .lines()
                .last()
                .unwrap_or_default()
                .to_owned()
        };
        assert!(
            activity(&terminal, &workspace).contains("Thinking"),
            "running work is named on the conversation's activity line"
        );
        assert!(
            !painted(&terminal, &workspace, SurfaceId::Composer).contains("Thinking"),
            "and never on the composer's rules"
        );

        let primary = workspace
            .state()
            .primary_agent()
            .map(|agent| agent.id.clone())
            .unwrap_or_else(|| panic!("the canonical scenario creates a primary agent"));
        conversation.emit(ConversationEvent::AgentStatusChanged {
            agent_id: primary.clone(),
            status: AgentStatus::Idle,
        });
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);

        assert_eq!(bounds(&workspace, SurfaceId::Transcript), transcript);
        assert_eq!(bounds(&workspace, SurfaceId::Composer), composer);
        assert!(
            !activity(&terminal, &workspace).contains("Thinking"),
            "idle draws nothing on the activity line"
        );

        conversation.emit(ConversationEvent::AgentStatusChanged {
            agent_id: primary,
            status: AgentStatus::Idle,
        });
        workspace.emit(conversation.drain());
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "repeating the fact painted in the boundary must not produce a frame"
        );
    }

    /// INV-7: the chord asks first and leaves only on a second press before its deadline. Other
    /// terminal input does not turn an explicit time window into an implicit input sequence.
    #[test]
    fn the_quit_chord_confirms_only_inside_its_one_second_window() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        workspace.set_working_directory("~/work".to_owned());
        let started = Instant::now();
        let redraw = |workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>| {
            workspace
                .settled_draw(terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"));
        };
        redraw(&mut workspace, &mut terminal);
        assert!(painted(&terminal, &workspace, SurfaceId::Status).contains("~/work"));

        // From inside a sub-agent's window, because the answer must not depend on which
        // conversation the key was pressed in.
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        redraw(&mut workspace, &mut terminal);
        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        redraw(&mut workspace, &mut terminal);
        assert_eq!(focused(&workspace), Some(SurfaceId::Inspector));
        assert_eq!(
            workspace.handle_at(&press(KeyCode::Char('d'), KeyModifiers::CONTROL), started,),
            Outcome::default(),
            "the first press asks"
        );
        redraw(&mut workspace, &mut terminal);
        let asked = painted(&terminal, &workspace, SurfaceId::Status);
        assert!(asked.contains("press Ctrl-D again to quit"), "{asked}");
        assert!(
            !asked.contains("~/work"),
            "the question replaces the directory"
        );

        assert_eq!(
            workspace
                .handle_at(
                    &press(KeyCode::Tab, KeyModifiers::NONE),
                    started + Duration::from_millis(100),
                )
                .flow,
            Flow::Continue
        );
        redraw(&mut workspace, &mut terminal);
        let still_armed = painted(&terminal, &workspace, SurfaceId::Status);
        assert!(
            still_armed.contains("press Ctrl-D again to quit"),
            "ordinary input does not withdraw a timed chord: {still_armed}"
        );
        workspace.handle_at(
            &Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column: 1,
                row: 1,
                modifiers: KeyModifiers::NONE,
            }),
            started + Duration::from_millis(200),
        );
        workspace.handle_at(
            &Event::Resize(121, 25),
            started + Duration::from_millis(300),
        );
        assert_eq!(
            workspace.note_deadline(),
            Some(started + Duration::from_secs(1)),
            "pointer motion and resize leave the explicit deadline alone"
        );

        assert_eq!(
            workspace.handle_at(
                &press(KeyCode::Char('d'), KeyModifiers::CONTROL),
                started + Duration::from_millis(999),
            ),
            Outcome::quit(),
            "the second press inside the one-second window leaves"
        );

        let (mut expired, _terminal) = drawn(120, 24);
        expired.handle_at(&press(KeyCode::Char('d'), KeyModifiers::CONTROL), started);
        assert_eq!(
            expired.handle_at(
                &press(KeyCode::Char('d'), KeyModifiers::CONTROL),
                started + Duration::from_secs(1),
            ),
            Outcome::default(),
            "at the deadline the old question has expired and this press starts a new window"
        );
        assert_eq!(
            expired.note_deadline(),
            Some(started + Duration::from_secs(2))
        );
    }

    /// FR-1: the one-shot deadline changes the projection once; stale wakes are free.
    #[test]
    fn the_quit_deadline_expires_once_and_costs_one_frame() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        workspace.set_working_directory("~/work".to_owned());
        frame(&mut workspace, &mut terminal);
        let started = Instant::now();

        workspace.handle_at(&press(KeyCode::Char('d'), KeyModifiers::CONTROL), started);
        frame(&mut workspace, &mut terminal);
        assert!(!workspace.expire_note(started + Duration::from_millis(999)));
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "waking before the deadline changes nothing"
        );

        assert!(workspace.expire_note(started + Duration::from_secs(1)));
        frame(&mut workspace, &mut terminal);
        let settled = painted(&terminal, &workspace, SurfaceId::Status);
        assert!(
            settled.contains("~/work"),
            "deadline restores rest: {settled}"
        );
        assert!(!workspace.expire_note(started + Duration::from_secs(2)));
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "a stale deadline costs no duplicate frame"
        );
    }

    /// INV-7: `Ctrl-C` clears a draft or interrupts its conversation, never both and never quits.
    #[test]
    fn ctrl_c_clears_a_draft_or_interrupts_but_never_does_both() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        workspace.set_working_directory("~/work".to_owned());
        let composer = bounds(&workspace, SurfaceId::Composer);
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: composer.x.saturating_add(1),
            row: composer.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        }));
        for character in "hi".chars() {
            workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(workspace.state.composer().text(), "hi");
        workspace.handle(&press(KeyCode::Tab, KeyModifiers::NONE));
        assert_ne!(
            focused(&workspace),
            Some(SurfaceId::Composer),
            "the regression requires the addressed draft to have lost its cursor"
        );

        let outcome = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(outcome.flow, Flow::Continue, "clearing is not quitting");
        assert_eq!(
            outcome.interrupted, None,
            "a cleared draft is not an interrupt"
        );
        assert_eq!(workspace.state.composer().text(), "");
        workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        assert!(
            painted(&terminal, &workspace, SurfaceId::Status).contains("~/work"),
            "taking the draft asks nothing"
        );

        let outcome = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            outcome.interrupted.as_ref().map(AgentId::as_str),
            Some("agent-a")
        );
        workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        let status = painted(&terminal, &workspace, SurfaceId::Status);
        assert!(
            status.contains("~/work"),
            "an ordinary interrupt leaves no quit prompt: {status}"
        );

        let revision = workspace.state.revision();
        let outcome = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            outcome.flow,
            Flow::Continue,
            "however many times: never a quit"
        );
        assert_eq!(
            workspace.state.revision(),
            revision,
            "repeating an ordinary interrupt is not a visible change (FR-1)"
        );
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "and an unchanged status costs no duplicate frame"
        );

        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: composer.x.saturating_add(1),
            row: composer.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        }));
        workspace.handle(&press(KeyCode::Char('x'), KeyModifiers::NONE));
        workspace.handle(&press(KeyCode::Char('d'), KeyModifiers::CONTROL));
        let revision = workspace.state.revision();
        let cleared = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            workspace.state.revision().get(),
            revision.get().saturating_add(1),
            "clearing the draft and withdrawing the quit question is one transition"
        );
        assert_eq!(workspace.state.status().note(), StatusNote::Quiet);
        assert_eq!(workspace.state.composer().text(), "");
        assert_eq!(
            cleared.interrupted, None,
            "clearing the draft consumes Ctrl-C without interrupting"
        );
        assert_eq!(
            workspace.handle(&press(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Outcome::default(),
            "Ctrl-C broke the quit chord, so the next Ctrl-D only asks again"
        );

        let revision = workspace.state.revision();
        let interrupted = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            interrupted.interrupted.as_ref().map(AgentId::as_str),
            Some("agent-a"),
            "an empty draft routes the interrupt"
        );
        assert_eq!(workspace.state.status().note(), StatusNote::Quiet);
        assert_eq!(
            workspace.state.revision().get(),
            revision.get().saturating_add(1),
            "the same interrupt also withdraws the armed quit question once"
        );
    }

    /// INV-7: the command is addressed by the conversation holding focus, not by selection alone.
    #[test]
    fn ctrl_c_names_the_conversation_it_interrupts() {
        let (mut workspace, mut terminal) = drawn(120, 40);

        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        let looking = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            looking.interrupted.as_ref().map(AgentId::as_str),
            Some("agent-a"),
            "looking at a worker does not retarget commands before the user enters its window"
        );

        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        let entered = workspace.handle(&press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(
            entered.interrupted.as_ref().map(AgentId::as_str),
            Some("agent-b"),
            "the entered conversation owns the interrupt"
        );
    }

    /// SURF-3 through the loop: `CycleFocus` and `Press` have consumers, not just tests.
    #[test]
    fn tab_walks_the_ring_and_a_click_focuses_the_region_it_landed_in() {
        let (mut workspace, _terminal) = drawn(120, 24);

        assert_eq!(focused(&workspace), Some(SurfaceId::Agents));

        workspace.handle(&press(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(focused(&workspace), Some(SurfaceId::Transcript));

        workspace.handle(&press(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(focused(&workspace), Some(SurfaceId::Agents));

        let transcript = bounds(&workspace, SurfaceId::Transcript);
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: transcript.x,
            row: transcript.y,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Transcript),
            "a press focuses the surface it hit"
        );
    }

    /// The wheel resolves against the frame that was drawn, and against nothing before one exists.
    ///
    /// A tree with no measured viewport is what the loop holds until the first paint. Guessing a
    /// target there would scroll a surface whose size nothing has established.
    #[test]
    fn the_wheel_moves_a_drawn_viewport_and_nothing_before_one_exists() {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(60, 20))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let mut conversation = Conversation::canonical();
        conversation.extend(8);
        workspace.state = conversation.state;

        // Before the first frame the pointer is over a workspace nothing has laid out.
        let before = workspace.state.revision();
        let blind = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 30,
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(workspace.handle(&blind), Outcome::default());
        assert_eq!(
            workspace.state.revision(),
            before,
            "an unlaid-out workspace has no viewport to move"
        );

        workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        let transcript = bounds(&workspace, SurfaceId::Transcript);
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: transcript.x.saturating_add(1),
            row: transcript.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        }));

        assert!(
            workspace.state.revision() > before,
            "a drawn viewport moves under the wheel"
        );
        assert!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"))
                .is_some(),
            "and the scroll is a visible change, so it repaints"
        );
    }

    /// FR-2, at a scale an argument cannot cover: what a frame builds does not grow with history.
    ///
    /// The two conversations differ by a factor of ten, and a steady frame has to cost the same in
    /// both. Running one size only would pass against a renderer that still builds everything, so
    /// the comparison is the test and the absolute numbers are not.
    ///
    /// The cold frame is the deliberate exception, asserted rather than hidden: knowing how tall a
    /// conversation is means wrapping every entry once, and that is what buys every later frame.
    #[test]
    fn frame_work_is_bounded_by_the_viewport_and_not_by_the_history() {
        let mut steady = Vec::new();
        for messages in [200_usize, 2000] {
            let mut workspace = Workspace::default();
            let mut terminal = Terminal::new(TestBackend::new(80, 24))
                .unwrap_or_else(|error| panic!("test terminal: {error}"));
            let mut conversation = Conversation::canonical();
            conversation.extend(messages);
            workspace.emit(conversation.drain());

            let cold = frame(&mut workspace, &mut terminal);
            assert_eq!(
                cold.entries_wrapped,
                messages.saturating_add(1),
                "a cold frame measures every entry exactly once, the canonical opener included"
            );

            // A streaming delta into the newest entry, which is the frame that has to stay cheap.
            conversation.append(" One more sentence of streamed text arrives.");
            workspace.emit(conversation.drain());
            let delta = frame(&mut workspace, &mut terminal);
            assert_eq!(
                delta.entries_wrapped, 1,
                "a delta re-measures the entry it changed and nothing behind it"
            );

            assert_eq!(
                workspace.metrics().retained(),
                messages.saturating_add(1),
                "the cache holds one height per entry and no more"
            );
            steady.push((delta.lines_built, cold.lines_built));
        }

        let [small, large] = steady
            .as_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("two sizes were measured"));
        assert_eq!(
            small, large,
            "a ten-fold longer conversation built a different number of lines, so the frame is \
             still paying for history: {small:?} then {large:?}"
        );
    }

    /// FR-2: moving the reader costs no measurement at all.
    ///
    /// Scrolling changes neither an entry's revision nor the panel's width, so every height it needs
    /// is already cached. A frame that re-wrapped here would make the wheel the most expensive
    /// thing in the workspace.
    #[test]
    fn scrolling_a_measured_conversation_wraps_nothing() {
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let mut conversation = Conversation::canonical();
        conversation.extend(400);
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);

        let transcript = bounds(&workspace, SurfaceId::Transcript);
        for _ in 0..8 {
            workspace.handle(&Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: transcript.x.saturating_add(1),
                row: transcript.y.saturating_add(1),
                modifiers: KeyModifiers::NONE,
            }));
            assert_eq!(
                frame(&mut workspace, &mut terminal).entries_wrapped,
                0,
                "the wheel must read cached heights, never recompute them"
            );
        }
    }

    fn painted(
        terminal: &Terminal<TestBackend>,
        workspace: &Workspace,
        surface_id: SurfaceId,
    ) -> String {
        crate::test_support::region_text(terminal.backend().buffer(), bounds(workspace, surface_id))
    }

    fn point_on(
        terminal: &Terminal<TestBackend>,
        workspace: &Workspace,
        surface: SurfaceId,
        text: &str,
    ) -> Point {
        let bounds = bounds(workspace, surface);
        let row = painted(terminal, workspace, surface)
            .lines()
            .position(|line| line.contains(text))
            .unwrap_or_else(|| {
                panic!(
                    "{text:?} must be painted in {surface:?}:\n{}",
                    painted(terminal, workspace, surface)
                )
            });
        Point {
            x: bounds.x.saturating_add(1),
            y: bounds
                .y
                .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
        }
    }

    /// Resolved against the frame that was drawn, which is the only focus a key can act on (FR-3).
    fn focused(workspace: &Workspace) -> Option<SurfaceId> {
        workspace.state.focused(&workspace.surfaces)
    }

    /// One event and the frame that follows it, the way the loop runs them.
    ///
    /// Batching events without a frame between them is a different thing to test: focus and hit
    /// testing resolve against the registry the *last frame* drew, so two events in a row would
    /// have the second one reading geometry the user never saw (FR-3).
    fn step(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>, event: &Event) {
        workspace.handle(event);
        let _frame = workspace
            .settled_draw(terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
    }

    /// Walks the focus ring to one surface rather than counting presses.
    ///
    /// Counting `Tab`s encodes the ring's current membership into every test that walks it, and the
    /// ring legitimately gains and loses stops. What is being asserted is that the surface is
    /// reachable by keyboard, which is what this asks.
    fn tab_to(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>, target: SurfaceId) {
        let tab = press(KeyCode::Tab, KeyModifiers::NONE);
        for _ in 0..=workspace.surfaces.len() {
            if focused(workspace) == Some(target) {
                return;
            }
            step(workspace, terminal, &tab);
        }
        panic!("{target:?} is not a stop on this frame's focus ring");
    }

    fn cursor(terminal: &Terminal<TestBackend>) -> Option<ratatui::layout::Position> {
        let backend = terminal.backend();
        backend.cursor_visible().then(|| backend.cursor_position())
    }

    /// Draws and insists the frame happened, for tests whose subject is what one cost.
    fn frame(workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>) -> super::FrameWork {
        workspace
            .settled_draw(terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"))
            .unwrap_or_else(|| panic!("this frame was expected to paint"))
    }

    /// INS-4 and INV-6 through the executable: opening focuses, and `Escape` gives focus back.
    ///
    /// The ladder has had no consumer since step 1, so this is the first time `Dismiss` resolves
    /// anything. `Escape` with nothing open must still not quit, which is the other half of INV-6
    /// and the reason the ladder exists at all.
    #[test]
    fn selecting_another_agent_opens_its_window_and_escape_returns_focus_to_the_conversation() {
        let (mut workspace, mut terminal) = drawn(120, 40);

        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_none(),
            "nothing is open until the user looks at someone else"
        );
        assert_eq!(
            workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE)),
            Outcome::default(),
            "and Escape with nothing to dismiss is not a quit"
        );

        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_some());
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Agents),
            "looking is not entering: the arrows keep working in the list"
        );

        workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Inspector),
            "entering is an explicit action, so the window is usable without a second step"
        );

        workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_none());
        assert_eq!(
            selected(&workspace),
            "none",
            "closing is looking at nobody else"
        );
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Transcript),
            "closing returns the keyboard to the conversation, not to the top of the ring"
        );
    }

    /// Rows of the conversation's interior left readable beneath the shelf.
    fn readable_rows(workspace: &Workspace) -> u16 {
        let conversation = bounds(workspace, SurfaceId::Transcript);
        let shelf = bounds(workspace, SurfaceId::Inspector);
        conversation
            .bottom()
            .saturating_sub(1)
            .saturating_sub(shelf.bottom())
    }

    /// The conversation's painted rows beneath the shelf: what the user can still read of it.
    fn painted_beneath(terminal: &Terminal<TestBackend>, workspace: &Workspace) -> String {
        let conversation = bounds(workspace, SurfaceId::Transcript);
        let shelf = bounds(workspace, SurfaceId::Inspector);
        painted(terminal, workspace, SurfaceId::Transcript)
            .lines()
            .skip(usize::from(shelf.bottom().saturating_sub(conversation.y)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// INS-1: the window shows the agent the user is looking at, floating over the primary's
    /// conversation, which keeps its rectangle and its title; `Escape` closes it. Nothing on
    /// screen is ever shown twice.
    #[test]
    fn the_window_floats_over_the_primary_and_escape_closes_it() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        let before = bounds(&workspace, SurfaceId::Transcript);

        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        let conversation = bounds(&workspace, SurfaceId::Transcript);
        let shelf = bounds(&workspace, SurfaceId::Inspector);
        assert_eq!(
            conversation, before,
            "the conversation keeps its whole rectangle"
        );
        assert_eq!(
            conversation.union(shelf),
            conversation,
            "and the window floats inside it"
        );
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Agent B"));
        assert!(
            painted(&terminal, &workspace, SurfaceId::Composer).contains("Message Agent A"),
            "the conversation's title stays readable above the window"
        );
        assert!(
            painted_beneath(&terminal, &workspace).contains("remains interactive"),
            "and the conversation is still readable beneath it"
        );

        workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_none(),
            "Escape is looking at nobody else"
        );
        assert_eq!(selected(&workspace), "none");
    }

    /// A click in the list is the pointer's way of looking at a sub-agent, and it opens the same
    /// window the arrows do (INS-1). Rows are the painted ones, so a wrapped label still hits.
    #[test]
    fn clicking_an_agent_in_the_list_selects_it_and_opens_its_window() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        let list = bounds(&workspace, SurfaceId::Agents);
        let row_of = |label: &str, terminal: &Terminal<TestBackend>, workspace: &Workspace| {
            let index = painted(terminal, workspace, SurfaceId::Agents)
                .lines()
                .position(|line| line.contains(label))
                .unwrap_or_else(|| panic!("{label} is in the list"));
            list.y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX))
        };
        let click = |workspace: &mut Workspace, row: u16| {
            workspace.handle(&mouse(
                MouseEventKind::Down(MouseButton::Left),
                list.x.saturating_add(2),
                row,
            ));
            workspace.handle(&mouse(
                MouseEventKind::Up(MouseButton::Left),
                list.x.saturating_add(2),
                row,
            ));
        };
        assert!(
            !painted(&terminal, &workspace, SurfaceId::Agents).contains("Agent A"),
            "the primary is the screen, not a row in the list"
        );

        let row = row_of("Agent B", &terminal, &workspace);
        click(&mut workspace, row);
        frame(&mut workspace, &mut terminal);
        assert_eq!(selected(&workspace), "agent-b");
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Agent B"));

        click(&mut workspace, row);
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "clicking the agent already looked at changes nothing, so it costs no frame (FR-1)"
        );

        click(&mut workspace, list.bottom().saturating_sub(2));
        assert_eq!(
            selected(&workspace),
            "agent-b",
            "a click on an empty row looks at nobody new"
        );

        workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_none());
    }

    /// COM-1 and INS-5: two inputs exist, one cursor does, and neither costs the
    /// conversation its rows.
    ///
    /// This is the first time "exactly one cursor" is a claim that could fail. Until now there was
    /// one text input in the workspace, so the invariant held by construction; now the inspector
    /// carries the inspected agent's steer input and focus is the only thing deciding which of the
    /// two has the caret.
    ///
    /// COM-1: the one cursor is where the text ends, at a width that made the draft wrap.
    ///
    /// Driven through the real frame rather than through the composer, because the defect this
    /// pins lived in the join: the composer measured itself in newlines, the panel wrapped what it
    /// was given, and the caret was placed against the unwrapped last line. At 60 columns a long
    /// draft painted its tail ending at column 34 while the caret sat at column 59 — on the right
    /// border, which is not a cell any character can be typed into.
    #[test]
    fn a_wrapped_draft_puts_the_caret_at_the_end_of_the_text_not_on_the_border() {
        let (mut workspace, mut terminal) = drawn(60, 24);
        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
        // No spaces, so the wrap point is arithmetic rather than a word boundary, and the row the
        // caret must land on is one this test can compute rather than guess.
        for character in "x".repeat(150).chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }

        let composer = bounds(&workspace, SurfaceId::Composer);
        let cursor = terminal
            .get_cursor_position()
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        let inside = composer.width.saturating_sub(2);
        // 150 characters at 58 columns is two full rows and a third holding the remainder.
        let tail = 150_u16 % inside;

        assert_eq!(composer.height, 5, "three wrapped rows and two borders");
        assert_eq!(
            cursor.x,
            composer.x + 1 + tail,
            "the caret sits after the last character, not against the border"
        );
        assert!(
            cursor.x < composer.right() - 1,
            "and the border is not a cell the caret may occupy"
        );
        assert_eq!(
            cursor.y,
            composer.bottom() - 2,
            "on the last of the three wrapped rows, which is the one being typed into"
        );
    }

    /// COM-1: height is measured at the composer's column, not at the terminal around it.
    #[test]
    fn a_draft_reserves_the_rows_it_needs_in_an_ultrawide_split() {
        let (mut workspace, mut terminal) = drawn(160, 24);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);

        let characters = 100_u16;
        for character in "x".repeat(usize::from(characters)).chars() {
            workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
        }
        frame(&mut workspace, &mut terminal);

        let composer = bounds(&workspace, SurfaceId::Composer);
        let inside = composer.width.saturating_sub(2);
        assert!(
            inside < characters,
            "the fixture must wrap in the composer column"
        );
        assert!(
            characters < 160_u16.saturating_sub(2),
            "the same draft must fit at terminal width or this proves nothing"
        );
        assert_eq!(composer.height, 4, "two painted rows and two frame rows");
    }

    /// Every event is followed by a frame, the way the loop runs them. Focus cycles against the
    /// tree the last frame drew (FR-3), so batching two focus changes without a frame between
    /// would be asking the ring about a surface that had not been registered yet.
    #[test]
    fn the_inspector_takes_the_cursor_and_the_composer_keeps_one_row() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        let tab = press(KeyCode::Tab, KeyModifiers::NONE);

        // Walk to the composer and leave a draft there.
        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
        for character in "to the primary".chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }
        let expanded = bounds(&workspace, SurfaceId::Composer).height;
        assert!(expanded > 1, "an uncollapsed composer is a bordered region");

        // Step off it before opening anything: under a cursor `Enter` submits, and opening an
        // inspector is not something typing can do by accident (INV-2). Then peek a *different*
        // agent — a draft is keyed by who it addresses, so an inspector pointed at the primary
        // agent would correctly be showing the very same draft as the composer.
        tab_to(&mut workspace, &mut terminal, SurfaceId::Agents);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert_eq!(
            bounds(&workspace, SurfaceId::Composer).height,
            2,
            "one row and the edge it closes the box with: one row of jump, not three (INS-5)"
        );
        assert!(
            painted(&terminal, &workspace, SurfaceId::Composer).contains("to return"),
            "the collapsed row still says where typing would go and how to get back"
        );
        let caret = cursor(&terminal).unwrap_or_else(|| panic!("a focused input owns the cursor"));
        assert!(
            bounds(&workspace, SurfaceId::Inspector).contains(caret),
            "the caret is at {caret:?}, and it belongs to the input that has focus"
        );
        let focused_conversation = bounds(&workspace, SurfaceId::Transcript).height;

        // Step out of the window: `Tab` lands on the primary composer, which is what the collapsed
        // row promised, so the window's input stops existing (INS-5) and the caret is now the
        // composer's.
        step(&mut workspace, &mut terminal, &tab);
        assert_eq!(focused(&workspace), Some(SurfaceId::Composer));
        assert_eq!(
            bounds(&workspace, SurfaceId::Composer).height,
            expanded,
            "and the primary composer comes back to full size"
        );
        let caret = cursor(&terminal).unwrap_or_else(|| panic!("the composer owns the cursor now"));
        assert!(
            bounds(&workspace, SurfaceId::Composer).contains(caret),
            "the caret is at {caret:?}, outside the composer"
        );
        assert!(
            focused_conversation >= bounds(&workspace, SurfaceId::Transcript).height,
            "the steer input costs the conversation nothing: its rows come out of the inspector's \
             own budget (INS-5), and the collapsing composer gives two more back"
        );

        // Back in, and type: the primary draft is untouched, because a draft belongs to the
        // conversation it addresses rather than to whichever input has focus.
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::BackTab, KeyModifiers::SHIFT),
        );
        for character in "hold on".chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }
        assert_eq!(workspace.state.composer().text(), "to the primary");
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("hold on"));

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert_eq!(
            cursor(&terminal),
            None,
            "the conversation is not a text input, so no caret is on screen"
        );
        assert_eq!(bounds(&workspace, SurfaceId::Composer).height, expanded);
    }

    /// The primary with plenty of history, and B beside it at ultrawide. The two columns are
    /// equals, so the terminal is one cell odd to make them differ by one: two conversations at
    /// two widths in one frame is what the cache has to get right.
    fn with_the_second_agent_beside_the_conversation()
    -> (Workspace, Terminal<TestBackend>, Conversation) {
        let mut conversation = Conversation::canonical();
        conversation.extend(20);

        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(141, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        (workspace, terminal, conversation)
    }

    fn measured(workspace: &Workspace, surface_id: SurfaceId) -> crate::Viewport {
        workspace
            .surfaces
            .viewport(surface_id)
            .unwrap_or_else(|| panic!("{surface_id:?} was drawn, so it has been measured"))
    }

    /// The rows one agent's whole conversation wraps to at `width`, measured the un-virtualized way.
    fn whole_conversation_rows(workspace: &Workspace, agent: &AgentId, width: u16) -> usize {
        let palette = Palette::default();
        let agent = workspace
            .state
            .agent(agent)
            .unwrap_or_else(|| panic!("the fixture created {agent}"));
        let lines: Vec<_> = agent
            .entries()
            .flat_map(|item| {
                crate::content::transcript_entry(
                    item,
                    &palette,
                    crate::state::EntryAppearance::compact(false),
                    width,
                )
            })
            .collect();
        ratatui::widgets::Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .line_count(width)
    }

    /// TR-1 through the executable: each panel's rows belong to the width that panel was drawn at.
    ///
    /// Heights were keyed by agent alone, with the width only deciding whether an entry was still
    /// valid — so a conversation drawn at a second width threw away the first's measurements. What
    /// that cost is asserted here rather than argued: a steady frame paints nothing, and a streaming
    /// delta wraps one entry at the one width its conversation is on screen at.
    #[test]
    fn a_conversation_drawn_at_two_widths_measures_correctly_at_both() {
        let agent_a = AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}"));
        let agent_b = AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}"));
        let (mut workspace, mut terminal, mut conversation) =
            with_the_second_agent_beside_the_conversation();
        let primary = measured(&workspace, SurfaceId::Transcript);
        let inspected = measured(&workspace, SurfaceId::Inspector);

        assert_ne!(
            primary.content_width, inspected.content_width,
            "the two panels have to be different widths or this proves nothing"
        );
        for (agent, viewport) in [(&agent_a, primary), (&agent_b, inspected)] {
            assert_eq!(
                viewport.content_rows,
                whole_conversation_rows(&workspace, agent, viewport.content_width),
                "a panel {} columns wide reported the height of some other width",
                viewport.content_width
            );
        }

        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "and having measured both, an unchanged frame has nothing to repaint"
        );

        conversation.append(" and more streamed text.");
        workspace.emit(conversation.drain());
        let work = frame(&mut workspace, &mut terminal);
        assert_eq!(
            work.entries_wrapped, 1,
            "a delta costs one wrap at the width its conversation is drawn at; a whole history is the defect"
        );
    }

    /// TR-3 through the executable: the reader moves by the width of the panel under the wheel.
    ///
    /// The control is the same terminal with nothing open. An inspector showing the same
    /// conversation must not change how far one notch takes its reader — and it did, because the
    /// anchor was resolved against whichever width the cache had measured last, which was the
    /// narrow column's.
    #[test]
    fn a_wheel_notch_moves_the_conversation_the_same_distance_with_an_inspector_open() {
        let notch = |workspace: &mut Workspace, terminal: &mut Terminal<TestBackend>| {
            let over = bounds(workspace, SurfaceId::Transcript);
            let before = measured(workspace, SurfaceId::Transcript).offset;
            step(
                workspace,
                terminal,
                &mouse(MouseEventKind::ScrollUp, over.x + 2, over.y + 2),
            );
            before.saturating_sub(measured(workspace, SurfaceId::Transcript).offset)
        };

        let (mut alone, mut alone_terminal, _events) =
            with_the_second_agent_beside_the_conversation();
        step(
            &mut alone,
            &mut alone_terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(
            alone.surfaces.get(SurfaceId::Inspector).is_none(),
            "the control has to have closed the inspector"
        );
        let control = notch(&mut alone, &mut alone_terminal);
        assert!(control > 0, "the fixture has to be scrollable");

        let (mut inspecting, mut inspecting_terminal, _events) =
            with_the_second_agent_beside_the_conversation();
        // The second column comes out of the conversation's width (ui-ux §layout classes), so
        // the conversation is
        // narrower with it open; a notch must still move the reader the same number of rows.
        assert!(
            measured(&inspecting, SurfaceId::Transcript).content_width
                < measured(&alone, SurfaceId::Transcript).content_width,
            "the fixture has to change the conversation's width, or this proves nothing"
        );
        assert_eq!(
            notch(&mut inspecting, &mut inspecting_terminal),
            control,
            "one notch moved the reader a different distance because a second panel was open"
        );
    }

    /// INS-7 through the executable: an inspector with no room for its input holds no cursor.
    ///
    /// INS-5 already said a rectangle that cannot hold both keeps the conversation and shows no
    /// input. What it did not say is what focus becomes, and the surface went on reporting text
    /// focus regardless — so the caret was placed at the end of the conversation's last line, as
    /// though a transcript item were an editor, and every keystroke landed in a draft with nothing
    /// on screen to show it. The three now agree: no input, no cursor, no draft.
    #[test]
    fn an_inspector_too_short_for_its_input_takes_no_typing_and_no_cursor() {
        let agent_b = AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}"));
        let (mut workspace, mut terminal) = drawn(120, 40);
        let shrink = press(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let grow = press(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let draft = |workspace: &Workspace| workspace.state.draft(&agent_b).text().to_owned();

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        for character in "steer".chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }
        assert_eq!(
            draft(&workspace),
            "steer",
            "there is a draft to be typed into"
        );
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Message Agent B"));

        // Down to the height the guarantee clamps at, where a conversation and an input no longer
        // both fit. The loop is bounded by the ring rather than counted: what is asserted is where
        // dragging stops, not how many presses it takes to get there.
        for _ in 0..bounds(&workspace, SurfaceId::Inspector).height {
            step(&mut workspace, &mut terminal, &shrink);
        }
        let squeezed = bounds(&workspace, SurfaceId::Inspector);
        assert_eq!(
            squeezed.height, 3,
            "the shelf is at the smallest height layout will draw"
        );
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Inspector),
            "it still holds focus; what changed is what holding focus means"
        );
        assert_eq!(
            workspace.state.keyboard_focus(&workspace.surfaces),
            crate::KeyboardFocus::Navigation,
            "a surface with no input on screen is a navigation surface"
        );
        assert_eq!(
            cursor(&terminal),
            None,
            "and nothing owns a cursor, least of all the conversation"
        );
        assert!(
            !painted(&terminal, &workspace, SurfaceId::Inspector).contains("Message Agent B"),
            "there is no input drawn, which is what INS-5 asks for"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Char('x'), KeyModifiers::NONE),
        );
        assert_eq!(
            draft(&workspace),
            "steer",
            "typing must not reach a draft the user cannot see"
        );
        // The consequence of becoming a navigation surface, asserted rather than left to be
        // discovered: with no cursor on screen a letter is a command again, and no bare letter is
        // the quit (INV-7), so this transition does not put an exit under a keypress the user
        // thought was text — `Escape` remains the way out.
        assert_eq!(
            workspace
                .handle(&press(KeyCode::Char('q'), KeyModifiers::NONE))
                .flow,
            Flow::Continue,
            "a bare letter never ends the session"
        );

        // Give the rows back, and the input comes back with the draft that was waiting for it.
        for _ in 0..3 {
            step(&mut workspace, &mut terminal, &grow);
        }
        assert!(painted(&terminal, &workspace, SurfaceId::Inspector).contains("Message Agent B"));
        let caret = cursor(&terminal).unwrap_or_else(|| panic!("the input is back and drawn"));
        assert!(
            bounds(&workspace, SurfaceId::Inspector).contains(caret),
            "the caret is at {caret:?}, outside the inspector"
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Char('!'), KeyModifiers::NONE),
        );
        assert_eq!(
            draft(&workspace),
            "steer!",
            "and typing lands where the caret is"
        );
    }

    fn two_conversations() -> (Workspace, Terminal<TestBackend>) {
        let mut conversation = Conversation::canonical();
        let agent_b = AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}"));
        conversation.extend(20).extend_agent(&agent_b, 20);

        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(100, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        workspace
            .settled_draw(&mut terminal)
            .unwrap_or_else(|error| panic!("test render: {error}"));

        // Look at B: its conversation opens over A's, which stays where it is (INS-1).
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        assert_eq!(selected(&workspace), "agent-b");
        (workspace, terminal)
    }

    /// The canonical journey's step 5: two agents streaming into independent conversations.
    ///
    /// This is the phase's hardest declared requirement and it had never been exercised — the
    /// second window once drew a regrouped detail list instead of the selected agent's conversation,
    /// so there was only ever one conversation on screen. Independence is the part
    /// worth asserting: the reading position belongs to the conversation rather than to the panel
    /// (TR-5), and with two panels that stops being a distinction without a difference.
    #[test]
    fn two_conversations_scroll_independently_and_neither_moves_the_other() {
        let (mut workspace, mut terminal) = two_conversations();

        let conversation = painted_beneath(&terminal, &workspace);
        let inspected = painted(&terminal, &workspace, SurfaceId::Inspector);
        assert_ne!(
            conversation, inspected,
            "two panels showing the same text would prove nothing about independence"
        );

        // Scroll the inspector only. Hover routing puts the wheel where the pointer is, without
        // touching focus, so this is one reader moving and the other staying put (ui-ux §nested
        // scrolling).
        let over_inspector = bounds(&workspace, SurfaceId::Inspector);
        for _ in 0..3 {
            step(
                &mut workspace,
                &mut terminal,
                &mouse(
                    MouseEventKind::ScrollUp,
                    over_inspector.x + 2,
                    over_inspector.y + 2,
                ),
            );
        }

        assert_ne!(
            painted(&terminal, &workspace, SurfaceId::Inspector),
            inspected,
            "the inspected conversation moved"
        );
        assert_eq!(
            painted_beneath(&terminal, &workspace),
            conversation,
            "and the one beneath it did not"
        );

        // Now the other way round, from where each of them is standing.
        let inspected = painted(&terminal, &workspace, SurfaceId::Inspector);
        let over_conversation = bounds(&workspace, SurfaceId::Transcript);
        for _ in 0..3 {
            step(
                &mut workspace,
                &mut terminal,
                &mouse(
                    MouseEventKind::ScrollUp,
                    over_conversation.x + 2,
                    over_conversation.bottom().saturating_sub(2),
                ),
            );
        }

        assert_ne!(painted_beneath(&terminal, &workspace), conversation);
        assert_eq!(
            painted(&terminal, &workspace, SurfaceId::Inspector),
            inspected,
            "the reader the user was not moving stayed exactly where it was"
        );
    }

    /// INS-5: the input is part of the inspector surface whose conversation scrolls.
    #[test]
    fn a_wheel_over_the_inspector_input_scrolls_that_inspectors_conversation() {
        let (mut workspace, mut terminal) = two_conversations();
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        let (split, _) = workspace
            .state
            .steer_input(&workspace.surfaces)
            .unwrap_or_else(|| panic!("entering the inspector draws its input"));
        assert_eq!(
            workspace
                .state
                .inspector_conversation_bounds(&workspace.surfaces),
            Some(split.conversation),
            "painting and row hit resolution share the inspector's content rectangle"
        );
        assert_eq!(
            workspace.entry_target_at(
                SurfaceId::Inspector,
                Point {
                    x: split.input.x.saturating_add(1),
                    y: split.input.y.saturating_add(1),
                },
            ),
            None,
            "the input strip is not also a transcript row"
        );
        let primary = measured(&workspace, SurfaceId::Transcript).offset;
        let inspected = measured(&workspace, SurfaceId::Inspector).offset;
        assert!(
            inspected > 0,
            "the fixture must leave room to scroll upward"
        );

        step(
            &mut workspace,
            &mut terminal,
            &mouse(
                MouseEventKind::ScrollUp,
                split.input.x.saturating_add(1),
                split.input.y.saturating_add(1),
            ),
        );

        assert!(
            measured(&workspace, SurfaceId::Inspector).offset < inspected,
            "the composite inspector owns the viewport addressed from its input strip"
        );
        assert_eq!(
            measured(&workspace, SurfaceId::Transcript).offset,
            primary,
            "the conversation beneath the shelf does not receive the wheel"
        );
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Inspector),
            "hover routing never changes focus"
        );
    }

    /// TR-5 and SURF-5 together: a conversation remembers its reader, and a surface that comes back
    /// comes back where it was left.
    #[test]
    fn an_inspected_conversation_keeps_its_own_reading_position_across_a_close_and_reopen() {
        let (mut workspace, mut terminal) = two_conversations();
        let over_inspector = bounds(&workspace, SurfaceId::Inspector);
        for _ in 0..4 {
            step(
                &mut workspace,
                &mut terminal,
                &mouse(
                    MouseEventKind::ScrollUp,
                    over_inspector.x + 2,
                    over_inspector.y + 2,
                ),
            );
        }
        // The first content row is the anchor: which message the reader is on and how far into it
        // (TR-3). The whole region is the wrong comparison — the height the user dragged to does
        // not survive a close, so the region differs for reasons that are not the reader.
        let anchor = |terminal: &Terminal<TestBackend>, workspace: &Workspace| {
            painted(terminal, workspace, SurfaceId::Inspector)
                .lines()
                .nth(1)
                .unwrap_or_default()
                .to_owned()
        };
        let parked = anchor(&terminal, &workspace);
        assert!(
            parked.contains("Filler") || parked.contains("wrap across") || parked.contains("panel"),
            "the reader must be parked inside a message, not at a boundary: {parked}"
        );

        // Close it from the conversation, so the Escape ladder resolves the inspector rather than a
        // selection, then reopen B the way it was opened the first time.
        tab_to(&mut workspace, &mut terminal, SurfaceId::Transcript);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_none(),
            "it must actually have closed, or this proves nothing"
        );
        tab_to(&mut workspace, &mut terminal, SurfaceId::Agents);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );

        assert_eq!(
            anchor(&terminal, &workspace),
            parked,
            "the inspected conversation came back on the message its reader had stopped at"
        );
    }

    fn selected(workspace: &Workspace) -> String {
        workspace
            .state
            .selected_agent()
            .map_or_else(|| "none".to_owned(), |agent| agent.id.to_string())
    }

    /// ATT-1: a request arrives and the workspace carries on.
    ///
    /// This is the exit gate's "background action-required events enter the Attention queue without
    /// stealing focus or opening a modal", asserted against a user who is mid-sentence rather than
    /// against an idle screen — which is the only state in which the claim is worth anything.
    #[test]
    fn a_background_request_takes_no_focus_no_selection_and_no_cursor() {
        let mut runtime = canonical_runtime();
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        // Stop the timeline one tick before agent B asks for a decision.
        workspace.emit(runtime.ready(11));
        frame(&mut workspace, &mut terminal);
        assert_eq!(workspace.state.attention_count(), 0, "nothing queued yet");

        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
        for character in "half a thought".chars() {
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char(character), KeyModifiers::NONE),
            );
        }
        let was_focused = focused(&workspace);
        let was_selected = selected(&workspace);
        let caret = cursor(&terminal);
        assert!(caret.is_some(), "the user is typing");

        workspace.emit(runtime.ready(12));
        frame(&mut workspace, &mut terminal);

        assert_eq!(workspace.state.attention_pending(), 1, "it did queue");
        assert!(
            workspace.surfaces.get(SurfaceId::Attention).is_some(),
            "and queueing is visible, or the user has no way to choose when to answer"
        );
        assert_eq!(focused(&workspace), was_focused, "focus did not move");
        assert_eq!(selected(&workspace), was_selected, "nor did the selection");
        assert_eq!(cursor(&terminal), caret, "nor did the cursor");
        assert_eq!(
            workspace.state.composer().text(),
            "half a thought",
            "and the half-written sentence is still there"
        );
        assert!(
            !workspace.surfaces.has_dismissible(),
            "nothing opened over the user's work"
        );
    }

    /// ATT-2 and ATT-3: going to a request is a keypress, and being seen is not being answered.
    #[test]
    fn going_to_a_request_is_the_users_move_and_marks_it_seen() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        assert_eq!(selected(&workspace), "none");
        assert_eq!(workspace.state.attention_pending(), 1);

        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert_eq!(
            selected(&workspace),
            "agent-b",
            "the user chose to go to the agent that asked, which opens its window"
        );
        assert_eq!(
            focused(&workspace),
            Some(SurfaceId::Inspector),
            "and the keyboard went with them, into that window"
        );
        assert_eq!(workspace.state.attention_pending(), 0);
        assert_eq!(
            workspace.state.attention_count(),
            1,
            "the request is still outstanding: the user saw it, nothing granted it"
        );
        assert!(
            !painted(&terminal, &workspace, SurfaceId::Agents).contains(" · !"),
            "and the rail's badge counts what is unanswered, so nothing unanswered is no badge"
        );
    }

    /// INV-10 inside the queue: its cursor is its own, and arrows there are not agent selection.
    #[test]
    fn the_queues_cursor_moves_without_touching_the_agent_selection() {
        let mut conversation = Conversation::canonical();
        // Both from a sub-agent: the queue is what the user is not looking at, and the primary's
        // own approval answers itself in the composer's place rather than waiting in a line.
        conversation.emit(ConversationEvent::AttentionRequested {
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: AttentionId::new("attention-b-2")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::PermissionRequired,
                remember: None,
                approval_id: ApprovalId::new("approval-b-2")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("tool-b-2")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit".into(),
                capabilities: vec![ToolCapability::FileWrite],
                detail: "Approve writing the findings file.".into(),
            },
        });
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);
        assert_eq!(workspace.state.attention_count(), 2);

        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
        let was_selected = selected(&workspace);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );

        assert_eq!(workspace.state.attention_cursor(), 1);
        assert_eq!(
            selected(&workspace),
            was_selected,
            "an arrow in the queue moves the queue, not the rail"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_eq!(
            selected(&workspace),
            "agent-b",
            "and Enter goes to whichever request the cursor is on"
        );
    }

    /// APV-4 and SURF-4: only a user action opens the blocking surface, whose answer echoes the
    /// loop's identity; Escape closes presentation without manufacturing a decision (ATT-3).
    #[test]
    fn an_open_approval_blocks_the_workspace_and_returns_only_the_selected_decision() {
        let mut conversation = Conversation::canonical();
        let attention_id = AttentionId::new("attention-b-approval")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        conversation.emit(ConversationEvent::AttentionRequested {
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: attention_id.clone(),
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::PermissionRequired,
                remember: None,
                approval_id: ApprovalId::new("approval-b-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("tool-b-write")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit".into(),
                capabilities: vec![ToolCapability::FileWrite],
                detail: "Change crates/plexmaton-core/src/lib.rs".into(),
            },
        });
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);

        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert_eq!(focused(&workspace), Some(SurfaceId::Approval));
        let card = bounds(&workspace, SurfaceId::Approval);
        assert_eq!(
            workspace.surfaces.hit_test(Point { x: 0, y: 0 }),
            Some(SurfaceId::Approval),
            "a click outside the card cannot reach the workspace below it"
        );
        let card_text = painted(&terminal, &workspace, SurfaceId::Approval);
        assert!(card_text.contains("Change crates/plexmaton-core/src/lib.rs"));
        assert!(
            card_text.contains("> 2. Deny"),
            "safe answer is highlighted: {card_text}"
        );
        assert!(
            card.width < 120 && card.height < 40,
            "the surface is a card, not a replacement screen"
        );

        let denied = workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        let denied = denied
            .approval
            .unwrap_or_else(|| panic!("Enter returns the highlighted decision"));
        assert_eq!(denied.approval_id.as_str(), "approval-b-1");
        assert_eq!(denied.decision, ApprovalDecision::Deny);
        assert_eq!(
            workspace.state.attention_count(),
            2,
            "the UI did not resolve loop state"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(workspace.surfaces.get(SurfaceId::Approval).is_none());
        assert_eq!(
            workspace.state.attention_count(),
            2,
            "Escape keeps the request pending"
        );
        assert_eq!(focused(&workspace), Some(SurfaceId::Inspector));

        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::NONE),
        );
        let allowed = workspace
            .handle(&press(KeyCode::Enter, KeyModifiers::NONE))
            .approval
            .unwrap_or_else(|| panic!("Enter returns the highlighted decision"));
        assert_eq!(allowed.approval_id.as_str(), "approval-b-1");
        assert_eq!(allowed.decision, ApprovalDecision::AllowOnce);

        conversation.emit(ConversationEvent::AttentionResolved {
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id,
        });
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);
        assert!(workspace.surfaces.get(SurfaceId::Approval).is_none());
        assert_eq!(workspace.state.attention_count(), 1);
    }

    /// SEL-3: copying a detail entry returns the value, not the label that was painted.
    ///
    /// The artifact is the case worth pinning: the panel shows a human label on one row and an
    /// indented pointer on the next, and the pointer is what a paste has to contain.
    #[test]
    fn copying_an_artifact_returns_its_pointer_rather_than_its_label() {
        let mut conversation = Conversation::canonical();
        conversation.emit(ConversationEvent::ArtifactAnnounced {
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            item_id: TranscriptItemId::new("artifact-copy")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            artifact_id: ArtifactId::new("artifact-copy")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            label: "copy source".to_owned(),
            pointer: "artifact://agent-b/copy-source".to_owned(),
        });
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);

        // Agent B owns the newest artifact entry in its conversation.
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );

        let copied = workspace
            .handle(&press(KeyCode::Char('y'), KeyModifiers::CONTROL))
            .copied
            .unwrap_or_else(|| panic!("a selection must copy to something"));
        assert_eq!(copied.text, "artifact://agent-b/copy-source");
        assert_eq!(copied.entries, 1);
        assert!(
            painted(&terminal, &workspace, SurfaceId::Inspector).contains("copy source"),
            "while the conversation is still showing the label, which is the point"
        );

        // One more entry back takes in the preceding outgoing mail in first-appearance order.
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        let copied = workspace
            .handle(&press(KeyCode::Char('y'), KeyModifiers::CONTROL))
            .copied
            .unwrap_or_else(|| panic!("a selection must copy to something"));
        assert_eq!(
            copied.text,
            "agent-a: Routing stays centralized and z-ordered.\nartifact://agent-b/copy-source"
        );
    }

    /// INV-6 with three rungs: `Escape` resolves the selection before the surface holding it.
    #[test]
    fn escape_clears_the_selection_before_it_closes_the_inspector() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_some());

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Up, KeyModifiers::SHIFT),
        );
        assert!(
            workspace.state.selection().is_some(),
            "the inspector's own detail is selectable, even though it holds a cursor"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(workspace.state.selection().is_none());
        assert!(
            workspace.surfaces.get(SurfaceId::Inspector).is_some(),
            "one layer per press: the surface the selection was made in survives it"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(workspace.surfaces.get(SurfaceId::Inspector).is_none());
    }

    /// INV-4 across a resize: a gesture in flight when the terminal changes size stays coherent.
    ///
    /// The exit gate asks for this by name, and it is the case where the two owners of a drag can
    /// disagree: the router holds capture across the resize because capture is about the button,
    /// while the surface the gesture is resizing has been relaid out underneath it. The next drag
    /// therefore has to be measured against where the edge *is*, not where it was grabbed.
    #[test]
    fn a_drag_in_flight_survives_the_terminal_changing_size() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);

        let shelf = bounds(&workspace, SurfaceId::Inspector);
        let column = shelf.x.saturating_add(2);
        workspace.handle(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            column,
            shelf.bottom().saturating_sub(1),
        ));

        // Narrower and much shorter, but still roomy enough for a shelf: the interesting case is
        // the edge being relaid out under a held button, not the inspector taking the region.
        terminal.backend_mut().resize(100, 30);
        workspace.handle(&Event::Resize(100, 30));
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            workspace.router.capture(),
            Some(SurfaceId::Inspector),
            "capture is about the button, which the user is still holding"
        );

        let after_resize = bounds(&workspace, SurfaceId::Inspector);
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            column,
            after_resize.bottom(),
        ));
        frame(&mut workspace, &mut terminal);
        assert!(
            bounds(&workspace, SurfaceId::Transcript).height >= 10,
            "the ten-row guarantee holds at the new size, measured against the new geometry"
        );

        workspace.handle(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            column,
            after_resize.bottom(),
        ));
        assert_eq!(workspace.router.capture(), None, "and the gesture ended");
    }

    /// INV-4: the bottom edge follows the pointer, even out of the rectangle.
    ///
    /// Capture is the whole reason a drag is usable: the edge the user grabbed keeps moving after
    /// the pointer has left the surface, which is where a resize gesture spends most of its time.
    /// The clamp is the other half — a drag is a choice inside the ten-row guarantee, never a way
    /// out of it (INS-2).
    #[test]
    fn dragging_the_inspectors_edge_resizes_it_and_capture_survives_leaving_the_rectangle() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);

        let shelf = bounds(&workspace, SurfaceId::Inspector);
        let edge = shelf.bottom().saturating_sub(1);
        let column = shelf.x.saturating_add(2);
        workspace.handle(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            column,
            edge,
        ));

        // Well past the bottom of the surface, which is where capture starts mattering.
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            column,
            edge.saturating_add(4),
        ));
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            bounds(&workspace, SurfaceId::Inspector).height,
            shelf.height.saturating_add(4),
            "the edge followed the pointer out of the rectangle"
        );

        // Off the bottom of the terminal entirely: it stops where the guarantee does.
        workspace.handle(&mouse(MouseEventKind::Drag(MouseButton::Left), column, 200));
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            readable_rows(&workspace),
            10,
            "the conversation keeps its ten readable rows however far the pointer goes"
        );

        let settled = bounds(&workspace, SurfaceId::Inspector).height;
        workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), column, 200));
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            column,
            shelf.y.saturating_add(4),
        ));
        assert_eq!(
            bounds(&workspace, SurfaceId::Inspector).height,
            settled,
            "a drag after release has no capture and must move nothing (INV-5)"
        );
    }

    /// Every mouse interaction has a keyboard equivalent, and both land in the same place.
    #[test]
    fn the_keyboard_moves_the_inspectors_edge_the_same_way_the_pointer_does() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
        frame(&mut workspace, &mut terminal);
        let grow = press(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let shrink = press(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let rows = |workspace: &Workspace| bounds(workspace, SurfaceId::Inspector).height;

        let opened = rows(&workspace);
        workspace.handle(&grow);
        frame(&mut workspace, &mut terminal);
        assert_eq!(rows(&workspace), opened.saturating_add(1));

        workspace.handle(&shrink);
        frame(&mut workspace, &mut terminal);
        assert_eq!(rows(&workspace), opened, "and back again");

        // A held key at the boundary is idempotent, because each step is measured from the
        // rectangle that was actually drawn rather than from an unclamped running total.
        for _ in 0..40 {
            workspace.handle(&grow);
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"));
        }
        let pinned_at_the_guarantee = rows(&workspace);
        assert_eq!(readable_rows(&workspace), 10);

        workspace.handle(&shrink);
        frame(&mut workspace, &mut terminal);
        assert_eq!(
            rows(&workspace),
            pinned_at_the_guarantee.saturating_sub(1),
            "the first press back off the boundary must move it, not undo forty of them"
        );
    }

    /// INS-8: maximize is a presentation, not a replacement for the remembered shelf height.
    #[test]
    fn resizing_a_maximized_inspector_preserves_the_shelf_height() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        let grow = press(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let shrink = press(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let maximize = press(KeyCode::Char('f'), KeyModifiers::CONTROL);

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        step(&mut workspace, &mut terminal, &shrink);
        let shelf_height = bounds(&workspace, SurfaceId::Inspector).height;

        step(&mut workspace, &mut terminal, &maximize);
        let maximized = bounds(&workspace, SurfaceId::Inspector);
        assert!(
            maximized.height > shelf_height,
            "the fixture must change presentation"
        );
        let revision = workspace.state.revision();

        workspace.handle(&grow);
        let column = maximized.x.saturating_add(2);
        let edge = maximized.bottom().saturating_sub(1);
        workspace.handle(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            column,
            edge,
        ));
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            column,
            maximized.y.saturating_add(4),
        ));
        workspace.handle(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            column,
            maximized.y.saturating_add(4),
        ));
        assert_eq!(
            workspace.state.revision(),
            revision,
            "neither resize path changes a maximized presentation"
        );
        assert_eq!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}")),
            None,
            "a resize that cannot change the screen costs no frame"
        );

        step(&mut workspace, &mut terminal, &maximize);
        assert_eq!(
            bounds(&workspace, SurfaceId::Inspector).height,
            shelf_height,
            "un-maximizing restores the height chosen before maximize"
        );
    }

    /// INS-3 and INS-8: geometry, not the stored maximize flag, decides whether an edge exists.
    #[test]
    fn derived_maximized_and_column_inspectors_have_no_resize_edge() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        let grow = press(KeyCode::Down, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let shrink = press(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::SHIFT);

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        step(&mut workspace, &mut terminal, &shrink);
        let shelf_height = bounds(&workspace, SurfaceId::Inspector).height;

        for (width, label) in [(60, "derived maximized"), (160, "tiled column")] {
            terminal.backend_mut().resize(width, 40);
            workspace.handle(&Event::Resize(width, 40));
            frame(&mut workspace, &mut terminal);
            let presented = bounds(&workspace, SurfaceId::Inspector);
            assert_eq!(
                workspace
                    .surfaces
                    .get(SurfaceId::Inspector)
                    .map(|surface| surface.z_index),
                Some(0),
                "{label} is not a floating shelf"
            );
            let revision = workspace.state.revision();

            workspace.handle(&grow);
            let column = presented.x.saturating_add(2);
            let edge = presented.bottom().saturating_sub(1);
            workspace.handle(&mouse(
                MouseEventKind::Down(MouseButton::Left),
                column,
                edge,
            ));
            workspace.handle(&mouse(
                MouseEventKind::Drag(MouseButton::Left),
                column,
                presented.y.saturating_add(4),
            ));
            workspace.handle(&mouse(
                MouseEventKind::Up(MouseButton::Left),
                column,
                presented.y.saturating_add(4),
            ));
            assert_eq!(
                workspace.state.revision(),
                revision,
                "{label} ignores both resize paths"
            );
            assert_eq!(
                workspace
                    .settled_draw(&mut terminal)
                    .unwrap_or_else(|error| panic!("test render: {error}")),
                None,
                "{label} resize costs no frame"
            );

            terminal.backend_mut().resize(120, 40);
            workspace.handle(&Event::Resize(120, 40));
            frame(&mut workspace, &mut terminal);
            assert_eq!(
                bounds(&workspace, SurfaceId::Inspector).height,
                shelf_height,
                "returning from {label} restores the chosen shelf height"
            );
        }
    }

    /// COM-2 and INV-7 through the loop: `q` is a letter while the cursor is in the composer.
    #[test]
    fn typing_reaches_the_composer_and_submitting_hands_the_text_back() {
        let (mut workspace, mut terminal) = drawn(120, 24);
        let composer = bounds(&workspace, SurfaceId::Composer);
        workspace.handle(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: composer.x.saturating_add(1),
            row: composer.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        }));

        for character in "hi q".chars() {
            let outcome = workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
            assert_eq!(outcome.flow, Flow::Continue, "typing must never quit");
            assert!(outcome.submitted.is_none());
        }
        assert_eq!(workspace.state.composer().text(), "hi q");

        let outcome = workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            outcome.submitted.as_ref().map(|submission| submission.kind),
            Some(SubmissionKind::Message),
            "the primary composer names the next-turn route"
        );
        assert_eq!(
            outcome
                .submitted
                .map(|submission| submission.text)
                .as_deref(),
            Some("hi q")
        );
        assert_eq!(
            workspace.state.composer().text(),
            "",
            "and the draft is cleared"
        );
        assert!(
            workspace
                .settled_draw(&mut terminal)
                .unwrap_or_else(|error| panic!("test render: {error}"))
                .is_some(),
            "typing changed the screen, so the next frame paints"
        );
    }

    /// COM-4: the visible input names both the recipient and the boundary the loop must claim.
    #[test]
    fn the_inspectors_input_submits_steering_for_that_agents_next_step() {
        let (mut workspace, mut terminal) = drawn(120, 40);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        for character in "check the cache".chars() {
            workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
        }

        let submission = workspace
            .handle(&press(KeyCode::Enter, KeyModifiers::NONE))
            .submitted
            .unwrap_or_else(|| panic!("the entered worker input must submit"));

        assert_eq!(submission.to.as_str(), "agent-b");
        assert_eq!(submission.kind, SubmissionKind::Steering);
        assert_eq!(submission.text, "check the cache");
    }

    fn ctrl(code: char) -> Event {
        press(KeyCode::Char(code), KeyModifiers::CONTROL)
    }

    /// COM-6: a terminal paste replaces selected input atomically, without submitting newlines.
    #[test]
    fn terminal_paste_edits_the_focused_input_without_submitting() {
        let (mut workspace, mut terminal) = drawn(95, 40);
        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
        let outcome = workspace.handle(&Event::Paste("中文\r\nsecond".to_owned()));
        assert!(outcome.submitted.is_none());
        assert_eq!(workspace.state.composer().text(), "中文\nsecond");
        frame(&mut workspace, &mut terminal);
        let area = bounds(&workspace, SurfaceId::Composer);
        workspace.handle(&mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.x + 1,
            area.y + 1,
        ));
        workspace.handle(&mouse(
            MouseEventKind::Drag(MouseButton::Left),
            area.x + 5,
            area.y + 1,
        ));
        workspace.handle(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            area.x + 5,
            area.y + 1,
        ));
        workspace.handle(&Event::Paste("replacement".to_owned()));
        assert_eq!(workspace.state.composer().text(), "replacement\nsecond");
        step(&mut workspace, &mut terminal, &ctrl('p'));
        workspace.handle(&Event::Paste("Conf".to_owned()));
        assert_eq!(
            workspace.state.drawer().expect("drawer").chosen_page(),
            Some(Page::Configuration)
        );
        workspace.show_configuration(crate::test_support::configuration_summary());
        frame(&mut workspace, &mut terminal);
        workspace.handle(&Event::Paste("must not edit the covered draft".to_owned()));
        assert_eq!(workspace.state.composer().text(), "replacement\nsecond");
    }

    /// COM-1, COM-2, COM-6: pointer events place, select, copy and replace in every editable input.
    #[test]
    fn pointer_clicks_place_the_caret_in_each_input() {
        for surface in [SurfaceId::Composer, SurfaceId::Inspector, SurfaceId::Drawer] {
            let (mut workspace, mut terminal) = drawn(95, 40);
            match surface {
                SurfaceId::Composer => tab_to(&mut workspace, &mut terminal, surface),
                SurfaceId::Inspector => {
                    tab_to(&mut workspace, &mut terminal, SurfaceId::Agents);
                    step(
                        &mut workspace,
                        &mut terminal,
                        &press(KeyCode::Down, KeyModifiers::NONE),
                    );
                    step(
                        &mut workspace,
                        &mut terminal,
                        &press(KeyCode::Enter, KeyModifiers::NONE),
                    );
                }
                _ => step(&mut workspace, &mut terminal, &ctrl('p')),
            }
            for character in "中文abc".chars() {
                workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
            }
            frame(&mut workspace, &mut terminal);
            let target = workspace.state.text_target(&workspace.surfaces);
            let area = if surface == SurfaceId::Inspector {
                workspace
                    .state
                    .steer_input(&workspace.surfaces)
                    .expect("entered input")
                    .0
                    .input
            } else {
                bounds(&workspace, surface)
            };
            let area = crate::surface::ContentInsets::for_surface(surface, area.height).inset(area);
            let x = area.x + 3;
            let y = area.y + 1;
            workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), x, y));
            workspace.handle(&mouse(MouseEventKind::Up(MouseButton::Left), x, y));
            frame(&mut workspace, &mut terminal);
            assert_eq!(cursor(&terminal), Some((x, y).into()));
            workspace.handle(&press(KeyCode::Char('x'), KeyModifiers::NONE));
            let input = match target {
                Some(agent) => workspace.state.draft(&agent),
                None => workspace.state.drawer().expect("palette").filter(),
            };
            assert_eq!(input.text(), "中x文abc", "{surface:?}");
            frame(&mut workspace, &mut terminal);
            let origin = area.x + 1;
            workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), origin, y));
            workspace.handle(&mouse(
                MouseEventKind::Drag(MouseButton::Left),
                origin + 5,
                y,
            ));
            frame(&mut workspace, &mut terminal);
            assert_ne!(
                terminal.backend().buffer()[(origin, y)].style(),
                terminal.backend().buffer()[(origin + 6, y)].style()
            );
            let copied = workspace
                .handle(&mouse(MouseEventKind::Up(MouseButton::Left), origin + 5, y))
                .copied
                .expect("input drag copies source");
            assert_eq!(copied.text, "中x文");
            workspace.handle(&press(KeyCode::Char('z'), KeyModifiers::NONE));
            let input = if surface == SurfaceId::Drawer {
                workspace.state.drawer().expect("palette").filter()
            } else {
                workspace.state.draft(
                    &workspace
                        .state
                        .text_target(&workspace.surfaces)
                        .expect("input target"),
                )
            };
            assert_eq!(input.text(), "zabc");
            workspace.handle(&mouse(MouseEventKind::Down(MouseButton::Left), origin, y));
            workspace.handle(&mouse(
                MouseEventKind::Drag(MouseButton::Left),
                origin + 1,
                y,
            ));
            assert!(
                workspace
                    .handle(&press(KeyCode::Esc, KeyModifiers::NONE))
                    .copied
                    .is_none()
            );
            assert!(workspace.state.copy_input(&workspace.surfaces).is_none());
            assert_eq!(focused(&workspace), Some(surface));
        }
    }

    /// COM-1: Chinese glyphs and an exactly full row leave the caret inside the input border.
    #[test]
    fn a_full_input_row_never_places_the_caret_on_the_border() {
        let (mut workspace, mut terminal) = drawn(60, 40);
        tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
        let width = bounds(&workspace, SurfaceId::Composer).width - 2;
        for character in "中".repeat(usize::from(width / 2)).chars() {
            workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
        }
        frame(&mut workspace, &mut terminal);
        let area = bounds(&workspace, SurfaceId::Composer);
        let caret = cursor(&terminal).expect("input caret");
        assert!(caret.x < area.right() - 1);
        assert!(caret.y < area.bottom() - 1);
        // The column a box's side would have spent stays reserved and blank (ui-ux §input).
        for row in area.y + 1..area.bottom() - 1 {
            assert_eq!(
                terminal.backend().buffer()[(area.right() - 1, row)].symbol(),
                " "
            );
        }
    }

    /// DRW-3, SURF-4: a page blocks edits beneath it, and `Escape` returns one layer per press:
    /// page, list with its query intact, then the origin with its draft intact.
    #[test]
    fn the_escape_ladder_returns_page_then_list_then_origin() {
        for width in [120, 95, 60, 48] {
            let (mut workspace, mut terminal) = drawn(width, 40);
            tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
            workspace.handle(&press(KeyCode::Char('a'), KeyModifiers::NONE));
            step(&mut workspace, &mut terminal, &ctrl('p'));
            for character in "conf".chars() {
                workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
            }
            let outcome = workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(outcome.page, Some(Page::Configuration));
            workspace.show_configuration(crate::test_support::configuration_summary());
            frame(&mut workspace, &mut terminal);
            assert_eq!(focused(&workspace), Some(SurfaceId::Drawer));
            assert_eq!(cursor(&terminal), None);
            let shown = painted(&terminal, &workspace, SurfaceId::Drawer);
            for text in [
                "Workspace · Configuration",
                "Provider",
                "local",
                "gpt-5.6-sol",
                "Reasoning effort",
                "high",
            ] {
                assert!(shown.contains(text), "{width}: {shown}");
            }
            workspace.handle(&press(KeyCode::Char('x'), KeyModifiers::NONE));
            workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
            workspace.handle(&ctrl('f'));
            assert_eq!(workspace.state.composer().text(), "a");
            assert_eq!(focused(&workspace), Some(SurfaceId::Drawer));
            // The chord is already answered: `Ctrl-P` over an open Drawer changes nothing.
            step(&mut workspace, &mut terminal, &ctrl('p'));
            assert!(workspace.state.configuration().is_some());
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Esc, KeyModifiers::NONE),
            );
            assert!(workspace.state.configuration().is_none());
            let drawer = workspace.state.drawer().expect("the list, one layer down");
            assert_eq!(drawer.filter().text(), "conf");
            assert_eq!(drawer.chosen_page(), Some(Page::Configuration));
            assert_eq!(focused(&workspace), Some(SurfaceId::Drawer));
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Esc, KeyModifiers::NONE),
            );
            assert!(workspace.state.drawer().is_none());
            assert_eq!(focused(&workspace), Some(SurfaceId::Composer));
            assert_eq!(workspace.state.composer().text(), "a");
        }
    }

    /// DRW-2: the Drawer spans every width from its top edge, one row per page, controls visible.
    #[test]
    fn the_drawer_spans_every_width_with_one_row_per_page() {
        for width in [48, 60, 71, 72, 80, 95, 96, 120, 160] {
            let (mut workspace, mut terminal) = drawn(width, 40);
            step(&mut workspace, &mut terminal, &ctrl('p'));
            let area = bounds(&workspace, SurfaceId::Drawer);
            assert_eq!((area.x, area.y, area.width), (0, 0, width));
            assert_eq!(usize::from(area.height), Page::ALL.len() + 8);
            let shown = painted(&terminal, &workspace, SurfaceId::Drawer);
            for text in ["Workspace", "> Configuration", "Permissions", "Esc close"] {
                assert!(shown.contains(text), "{width}: {shown}");
            }
            let viewport = workspace
                .surfaces
                .viewport(SurfaceId::Drawer)
                .expect("measured drawer");
            assert_eq!(
                viewport.content_rows,
                Page::ALL.len() + 4,
                "each page stays one row at {width}"
            );
        }
    }

    /// DRW-2/DRW-3: a short Drawer keeps the marker and its keyboard affordances together, and
    /// the wheel over it steps the choice.
    #[test]
    fn short_drawer_and_wheel_use_the_visible_choice_window() {
        let (mut workspace, mut terminal) = drawn(60, 12);
        step(&mut workspace, &mut terminal, &ctrl('p'));
        let area = bounds(&workspace, SurfaceId::Drawer);
        step(
            &mut workspace,
            &mut terminal,
            &mouse(MouseEventKind::ScrollDown, area.x + 2, area.y + 2),
        );
        let shown = painted(&terminal, &workspace, SurfaceId::Drawer);
        assert!(
            shown.contains("> Permissions") && shown.contains("Enter open"),
            "{shown}"
        );
        assert_eq!(
            workspace
                .handle(&press(KeyCode::Enter, KeyModifiers::NONE))
                .page,
            Some(Page::Permissions)
        );
    }

    /// DRW-4: the smallest Configuration page still exposes every value by keyboard, under a
    /// footer that never scrolls away.
    #[test]
    fn short_configuration_pages_scroll_to_the_remaining_values() {
        let (mut workspace, mut terminal) = drawn(48, 12);
        workspace.show_configuration(crate::test_support::configuration_summary());
        frame(&mut workspace, &mut terminal);
        let shown = painted(&terminal, &workspace, SurfaceId::Drawer);
        assert!(
            shown.contains("local") && shown.contains("Esc back · ↑↓ scroll"),
            "{shown}"
        );
        let mut found_effort = false;
        for _ in 0..4 {
            workspace.handle(&press(KeyCode::Down, KeyModifiers::NONE));
            workspace.settled_draw(&mut terminal).expect("draw scroll");
            let shown = painted(&terminal, &workspace, SurfaceId::Drawer);
            assert!(shown.contains("Esc back · ↑↓ scroll"), "{shown}");
            found_effort |= shown.contains("high");
        }
        assert!(found_effort, "reasoning effort remains reachable");
        assert_eq!(focused(&workspace), Some(SurfaceId::Drawer));
    }

    /// COM-1, COM-2: the painted filter caret follows its own edits at every layout width.
    #[test]
    fn the_drawer_filter_paints_its_own_caret_while_editing() {
        for width in [120, 95, 60] {
            let (mut workspace, mut terminal) = drawn(width, 40);
            tab_to(&mut workspace, &mut terminal, SurfaceId::Composer);
            for character in "unrelated draft".chars() {
                workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
            }
            step(&mut workspace, &mut terminal, &ctrl('p'));
            let area = bounds(&workspace, SurfaceId::Drawer);
            let origin = (area.x + 3, area.y + 2);
            assert_eq!(cursor(&terminal), Some(origin.into()));
            for (code, column, filter) in [
                (KeyCode::Char('宽'), 2, "宽"),
                (KeyCode::Char('o'), 3, "宽o"),
                (KeyCode::Left, 2, "宽o"),
                (KeyCode::Char('t'), 3, "宽to"),
                (KeyCode::Home, 0, "宽to"),
                (KeyCode::Right, 2, "宽to"),
                (KeyCode::Delete, 2, "宽o"),
                (KeyCode::Backspace, 0, "o"),
                (KeyCode::End, 1, "o"),
            ] {
                step(
                    &mut workspace,
                    &mut terminal,
                    &press(code, KeyModifiers::NONE),
                );
                assert_eq!(
                    cursor(&terminal),
                    Some((origin.0 + column, origin.1).into())
                );
                let mut x = origin.0;
                for character in filter.chars() {
                    assert_eq!(
                        terminal.backend().buffer()[(x, origin.1)].symbol(),
                        character.to_string()
                    );
                    x += if character == '宽' { 2 } else { 1 };
                }
            }
            assert_eq!(workspace.state.composer().text(), "unrelated draft");
        }
    }

    /// COM-1: a long filter keeps both its text and caret on the filter row while moving home.
    #[test]
    fn a_long_drawer_filter_keeps_the_caret_inside_its_row() {
        for width in [120, 95, 60] {
            let (mut workspace, mut terminal) = drawn(width, 40);
            step(&mut workspace, &mut terminal, &ctrl('p'));
            for character in "a".repeat(200).chars() {
                workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
            }
            frame(&mut workspace, &mut terminal);
            let area = bounds(&workspace, SurfaceId::Drawer);
            assert_eq!(
                cursor(&terminal),
                Some((area.right() - 4, area.y + 2).into())
            );
            assert!(painted(&terminal, &workspace, SurfaceId::Drawer).contains("No page matches"));
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Home, KeyModifiers::NONE),
            );
            assert_eq!(cursor(&terminal), Some((area.x + 3, area.y + 2).into()));
            step(
                &mut workspace,
                &mut terminal,
                &press(KeyCode::Char('z'), KeyModifiers::NONE),
            );
            assert!(painted(&terminal, &workspace, SurfaceId::Drawer).contains("zaaaa"));
        }
    }

    /// DRW-3: the list finds a page by its name, and `Enter` hands the page to the root without
    /// closing the Drawer: the page opens in place. A slash is a character, not a command.
    #[test]
    fn the_list_finds_a_page_by_name_and_hands_it_to_the_root() {
        for query in ["", "conf", "Configuration", "FIGUR"] {
            let (mut workspace, mut terminal) = drawn(95, 40);
            step(&mut workspace, &mut terminal, &ctrl('p'));
            for character in query.chars() {
                workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
            }
            workspace.settled_draw(&mut terminal).expect("test render");
            let drawer = workspace.state.drawer().expect("open drawer");
            assert_eq!(
                drawer.pages(),
                if query.is_empty() {
                    Page::ALL.to_vec()
                } else {
                    vec![Page::Configuration]
                },
                "{query:?}"
            );
            assert!(painted(&terminal, &workspace, SurfaceId::Drawer).contains("> Configuration"));
            let outcome = workspace.handle(&press(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(outcome.page, Some(Page::Configuration));
            workspace.show_configuration(crate::test_support::configuration_summary());
            let drawer = workspace.state.drawer().expect("still open");
            assert_eq!(drawer.page(), Some(Page::Configuration));
        }
        let (mut workspace, mut terminal) = drawn(95, 40);
        step(&mut workspace, &mut terminal, &ctrl('p'));
        workspace.handle(&Event::Paste("/config".to_owned()));
        assert!(workspace.state.drawer().expect("open").pages().is_empty());
        assert!(
            workspace
                .handle(&press(KeyCode::Enter, KeyModifiers::NONE))
                .page
                .is_none()
        );
    }

    /// `⌃P` pulls the Drawer open and gives it the keyboard in the same gesture (SURF-3, SURF-4).
    #[test]
    fn the_chord_pulls_the_drawer_open_and_takes_the_keyboard() {
        let (mut workspace, mut terminal) = drawn(120, 30);
        workspace.handle(&ctrl('p'));
        workspace.settled_draw(&mut terminal);

        assert!(workspace.state.drawer().is_some());
        assert_eq!(
            workspace.state.focused(&workspace.surfaces),
            Some(SurfaceId::Drawer),
            "the Drawer holds the keyboard on the frame that first draws it"
        );
    }

    /// One `Escape` takes the topmost layer and returns the keyboard where it was (INV-6).
    #[test]
    fn escape_closes_the_drawer_and_returns_the_keyboard() {
        let (mut workspace, mut terminal) = drawn(120, 30);
        let before = workspace.state.focused(&workspace.surfaces);
        workspace.handle(&ctrl('p'));
        workspace.settled_draw(&mut terminal);
        workspace.handle(&press(KeyCode::Esc, KeyModifiers::NONE));
        workspace.settled_draw(&mut terminal);

        assert!(workspace.state.drawer().is_none());
        assert_eq!(workspace.state.focused(&workspace.surfaces), before);
    }

    /// Typing into the Drawer filters its list rather than reaching the composer (COM-4).
    #[test]
    fn typing_filters_the_list_and_never_reaches_the_composer() {
        let (mut workspace, mut terminal) = drawn(120, 30);
        workspace.handle(&ctrl('p'));
        workspace.settled_draw(&mut terminal);
        for character in "con".chars() {
            workspace.handle(&press(KeyCode::Char(character), KeyModifiers::NONE));
        }

        let drawer = workspace.state.drawer().expect("open");
        assert_eq!(drawer.filter().text(), "con");
        assert_eq!(drawer.pages(), vec![Page::Configuration]);
        assert_eq!(workspace.state.composer().text(), "");
    }

    /// DRW-1, SURF-4: the Drawer pulls open over a waiting approval, sits above it, and leaves
    /// the card exactly where it was for when the Drawer closes.
    #[test]
    fn the_drawer_opens_over_a_waiting_approval_and_leaves_the_card_alone() {
        let mut conversation = Conversation::canonical();
        conversation.emit(ConversationEvent::AttentionRequested {
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: AttentionId::new("attention-b-approval")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::PermissionRequired,
                remember: None,
                approval_id: ApprovalId::new("approval-b-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("tool-b-write")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit".into(),
                capabilities: vec![ToolCapability::FileWrite],
                detail: "Change crates/plexmaton-core/src/lib.rs".into(),
            },
        });
        let mut workspace = Workspace::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40))
            .unwrap_or_else(|error| panic!("test terminal: {error}"));
        workspace.emit(conversation.drain());
        frame(&mut workspace, &mut terminal);
        tab_to(&mut workspace, &mut terminal, SurfaceId::Attention);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_eq!(focused(&workspace), Some(SurfaceId::Approval));
        let card = bounds(&workspace, SurfaceId::Approval);
        let card_text = painted(&terminal, &workspace, SurfaceId::Approval);
        assert!(card_text.contains("Approval required"), "{card_text}");

        step(&mut workspace, &mut terminal, &ctrl('p'));
        assert_eq!(focused(&workspace), Some(SurfaceId::Drawer));
        let drawer = workspace.surfaces.get(SurfaceId::Drawer).expect("drawer");
        let approval = workspace
            .surfaces
            .get(SurfaceId::Approval)
            .expect("approval kept");
        assert!(drawer.z_index > approval.z_index);
        assert_eq!(approval.bounds, card);
        assert!(
            workspace
                .handle(&press(KeyCode::Enter, KeyModifiers::NONE))
                .approval
                .is_none(),
            "a key inside the Drawer never reaches the card beneath it"
        );

        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert_eq!(focused(&workspace), Some(SurfaceId::Approval));
        assert_eq!(bounds(&workspace, SurfaceId::Approval), card);
        assert_eq!(
            painted(&terminal, &workspace, SurfaceId::Approval),
            card_text,
            "the card is exactly where it was"
        );
    }

    /// DRW-3: the chosen row carries `Chosen` across its whole width, a bar with weight and a
    /// hue, and the rest `Muted`, so the marker is never the only thing telling them apart.
    #[test]
    fn the_chosen_drawer_row_carries_the_chosen_role_across_its_width() {
        let (mut workspace, mut terminal) = drawn(95, 40);
        step(&mut workspace, &mut terminal, &ctrl('p'));
        let area = bounds(&workspace, SurfaceId::Drawer);
        let palette = Palette::default();
        let chosen = palette.style(Role::Chosen);
        // Border, one blank inset row, the filter, one gap: the first row is four down.
        let first = area.y + 4;
        let cell = |terminal: &Terminal<TestBackend>, x: u16, row: u16| {
            terminal.backend().buffer()[(x, row)].style()
        };
        let name = area.x + 5;
        let far_right = area.right() - 4;
        assert_eq!(cell(&terminal, name, first).fg, chosen.fg);
        assert_eq!(cell(&terminal, name, first).bg, chosen.bg);
        assert!(
            cell(&terminal, name, first)
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert_eq!(
            cell(&terminal, far_right, first).bg,
            chosen.bg,
            "the bar runs the width"
        );
        assert_eq!(
            cell(&terminal, name, first + 1).fg,
            palette.style(Role::Muted).fg
        );
        assert_ne!(cell(&terminal, name, first + 1).bg, chosen.bg);
        step(
            &mut workspace,
            &mut terminal,
            &press(KeyCode::Down, KeyModifiers::NONE),
        );
        assert_eq!(
            cell(&terminal, name, first).fg,
            palette.style(Role::Muted).fg
        );
        assert_eq!(cell(&terminal, name, first + 1).fg, chosen.fg);
        assert_eq!(cell(&terminal, far_right, first + 1).bg, chosen.bg);
    }
}
