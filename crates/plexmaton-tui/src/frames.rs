//! The composition, frozen: what the canonical scenario looks like at each width, and which words
//! the screen is allowed to say.
//!
//! Every other render test proves one mechanism. These families prove the whole frame, because a
//! layout can satisfy every mechanism and still not be the contract's composition. The fixtures
//! under `frames/` are text, so a reviewer reads the diff; colour is proven by the role tests.
//! Refresh them with `PLEXMATON_WRITE_FRAMES=1 cargo test -p plexmaton-tui frames`, and review the
//! diff as the behaviour change it is.

#[cfg(test)]
mod tests {
    use plexmaton_core::{
        AgentId, AgentStatus, ApprovalId, AttentionId, AttentionRequest, ConversationEvent,
        ConversationEventEnvelope, EventSequence, ToolCallId, ToolCallStatus, ToolCapability,
        ToolDetail, ToolPresentation, TranscriptItemId,
    };
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers},
        layout::Rect,
    };

    use crate::{
        CleanupNotice, ConversationRestoration, ConversationTailRepair, PersistenceNotice,
        SkillChoice, SkillChoiceSource, TranscriptMetrics, ViewState, Workspace,
        intent::{AttentionIntent, Direction, InspectorIntent},
        state::EntryTarget,
        surface::{SurfaceId, SurfaceTree},
        test_support::{
            Conversation, canonical_state, current_responding_state, current_running_tool_state,
            draw, draw_frame, region_text,
        },
        theme::Palette,
    };

    /// One frame per width class, at a height that shows the whole composition.
    const FRAMES: [(&str, u16, u16); 3] = [
        ("canonical-wide", 120, 40),
        ("canonical-medium", 95, 40),
        ("canonical-narrow", 60, 40),
    ];

    /// The first blocking surface, at the same three product widths.
    const APPROVAL_FRAMES: [(&str, u16, u16); 3] = [
        ("approval-wide", 120, 40),
        ("approval-medium", 95, 40),
        ("approval-narrow", 60, 40),
    ];

    /// Slice 10's live primary-agent command approval, across the same product widths.
    const NATIVE_APPROVAL_FRAMES: [(&str, u16, u16); 3] = [
        ("native-approval-wide", 124, 40),
        ("native-approval-medium", 95, 40),
        ("native-approval-narrow", 60, 40),
    ];

    /// The three product widths, for the families whose layout actually changes with width.
    const PRODUCT_WIDTHS: [(&str, u16, u16); 3] =
        [("wide", 120, 40), ("medium", 95, 40), ("narrow", 60, 40)];

    /// SKL-4's returned explicit invocation and visible load diagnostic at each product width.
    const SKILL_DIAGNOSTIC_FRAMES: [(&str, u16, u16); 3] = [
        ("skill-diagnostic-wide", 120, 40),
        ("skill-diagnostic-medium", 95, 40),
        ("skill-diagnostic-narrow", 60, 40),
    ];

    const SKILL_PICKER_FRAMES: [(&str, u16, u16); 3] = [
        ("skill-picker-wide", 120, 32),
        ("skill-picker-medium", 95, 32),
        ("skill-picker-narrow", 60, 32),
    ];

    /// One disclosed native-tool entry across the same product widths.
    const DISCLOSURE_FRAMES: [(&str, u16, u16); 3] = [
        ("tool-open-wide", 120, 40),
        ("tool-open-medium", 95, 40),
        ("tool-open-narrow", 60, 40),
    ];

    /// Slice 6's remaining transcript roles and a disclosed canonical patch.
    const GRAMMAR_FRAMES: [(&str, u16, u16); 3] = [
        ("transcript-grammar-wide", 120, 40),
        ("transcript-grammar-medium", 95, 40),
        ("transcript-grammar-narrow", 60, 40),
    ];

    /// Compact composer-only frames for work states absent from the canonical frames.
    const CURRENT_WORK_FRAMES: [(&str, u16, u16); 3] =
        [("wide", 120, 40), ("medium", 95, 40), ("narrow", 60, 40)];

    const PERSISTENCE_FAILURES: [(PersistenceNotice, &str, &str); 2] = [
        (
            PersistenceNotice::NotWritten,
            "not-written",
            "message was not saved; draft restored",
        ),
        (
            PersistenceNotice::OutcomeUnknown,
            "outcome-unknown",
            "write outcome unknown; reopen before retrying",
        ),
    ];

    /// INV-13: the command list stays a compact overlay across all three widths.
    const COMMAND_PALETTE_FRAMES: [(&str, u16, u16); 3] = [
        ("command-palette-wide", 120, 40),
        ("command-palette-medium", 95, 40),
        ("command-palette-narrow", 60, 40),
    ];

    fn composer_frame(state: &ViewState, width: u16, height: u16) -> String {
        let (surfaces, buffer) = draw_frame(state, &Palette::default(), width, height);
        let composer = surfaces
            .get(SurfaceId::Composer)
            .unwrap_or_else(|| panic!("the composer is registered at {width}x{height}"));
        region_text(&buffer, composer.bounds)
    }

    /// COM-5: every accepted ambient label is frozen at all three widths.
    #[test]
    fn the_current_work_frames_match_their_fixtures() {
        let states = [
            ("responding", current_responding_state(), "Responding"),
            (
                "running-tool",
                current_running_tool_state(),
                "Running read_file",
            ),
        ];
        for (state_name, state, label) in states {
            for (width_name, width, height) in CURRENT_WORK_FRAMES {
                let name = format!("current-work-{state_name}-{width_name}");
                let drawn = composer_frame(&state, width, height);
                assert!(
                    drawn.contains(&format!(" · {label}")),
                    "{name}: the accepted label is visible"
                );
                assert_eq!(
                    drawn.lines().count(),
                    3,
                    "{name}: the fixture is only the composer boundary and body"
                );

                crate::test_support::assert_frame(&name, &drawn);
            }
        }
    }

    /// Phase 01 §scope 1: the composition at wide, medium and narrow, checked in.
    #[test]
    fn the_command_palette_frames_match_their_fixtures() {
        for (name, width, height) in COMMAND_PALETTE_FRAMES {
            let mut state = canonical_state();
            state.open_command_palette(&SurfaceTree::default());
            let drawn = draw(&state, width, height);
            for signature in ["Commands", "/config", "/new", "Esc close"] {
                assert!(
                    drawn.contains(signature),
                    "{name}: {signature:?} is not on screen"
                );
            }
            assert_eq!(
                drawn.lines().count(),
                usize::from(height),
                "{name}: every row painted"
            );

            crate::test_support::assert_frame(name, &drawn);
        }
    }

    /// INV-12, INV-13: the configuration page is readable in the complete workspace at each width.
    #[test]
    fn the_configuration_frames_match_their_fixtures() {
        for (name, width) in [
            ("configuration-wide", 120),
            ("configuration-medium", 95),
            ("configuration-narrow", 60),
        ] {
            let mut state = canonical_state();
            state.show_configuration(crate::test_support::configuration_summary());
            let drawn = draw(&state, width, 40);
            for signature in [
                "Configuration",
                "Provider",
                "local",
                "gpt-5.6-sol",
                "Reasoning effort",
                "high",
                "restart",
            ] {
                assert!(drawn.contains(signature), "{name}: {signature} absent");
            }
            crate::test_support::assert_frame(name, &drawn);
        }
    }

    #[test]
    fn the_canonical_frames_match_their_fixtures() {
        for (name, width, height) in FRAMES {
            let drawn = draw(&canonical_state(), width, height);
            // Structural first, so an empty or truncated fixture cannot pass by matching nothing.
            for signature in [
                "Agents",
                "Agent A · primary",
                "Message Agent A",
                "~/plexmaton",
            ] {
                assert!(
                    drawn.contains(signature),
                    "{name}: {signature:?} is not on screen"
                );
            }
            assert_eq!(
                drawn.lines().count(),
                usize::from(height),
                "{name}: every row painted"
            );

            crate::test_support::assert_frame(name, &drawn);
        }
    }

    fn tool_state(status: ToolCallStatus) -> ViewState {
        tool_state_with_presentation(status, "read_file", ToolPresentation::default())
    }

    fn tool_state_with_presentation(
        status: ToolCallStatus,
        label: &str,
        final_presentation: ToolPresentation,
    ) -> ViewState {
        let agent_id =
            AgentId::new("agent-primary").unwrap_or_else(|error| panic!("fixture: {error}"));
        let item_id =
            TranscriptItemId::new("tool-entry").unwrap_or_else(|error| panic!("fixture: {error}"));
        let call_id =
            ToolCallId::new("tool-call").unwrap_or_else(|error| panic!("fixture: {error}"));
        let mut state = ViewState::default();
        let mut sequence = 0_u64;
        let mut apply = |state: &mut ViewState, event| {
            sequence = sequence.saturating_add(1);
            let outcome = state.apply(ConversationEventEnvelope {
                sequence: EventSequence::new(sequence),
                event,
            });
            assert!(
                matches!(outcome, crate::ApplyOutcome::Accepted),
                "tool-state fixture was rejected: {outcome:?}"
            );
        };
        apply(
            &mut state,
            ConversationEvent::AgentCreated {
                agent_id: agent_id.clone(),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Running,
            },
        );
        let mut revision = 0_u64;
        let mut move_tool = |state: &mut ViewState, next| {
            let presentation = if next == status {
                final_presentation.clone()
            } else {
                ToolPresentation::default()
            };
            apply(
                state,
                ConversationEvent::ToolCallChanged {
                    agent_id: agent_id.clone(),
                    item_id: item_id.clone(),
                    item_revision: revision,
                    call_id: call_id.clone(),
                    label: label.to_owned(),
                    status: next,
                    presentation,
                },
            );
            revision = revision.saturating_add(1);
        };
        move_tool(&mut state, ToolCallStatus::Queued);
        match status {
            ToolCallStatus::Queued => {}
            ToolCallStatus::AwaitingApproval => {
                move_tool(&mut state, ToolCallStatus::AwaitingApproval);
            }
            ToolCallStatus::Running => move_tool(&mut state, ToolCallStatus::Running),
            ToolCallStatus::Succeeded | ToolCallStatus::Failed | ToolCallStatus::Cancelled => {
                move_tool(&mut state, ToolCallStatus::Running);
                move_tool(&mut state, status);
            }
            ToolCallStatus::Denied => {
                move_tool(&mut state, ToolCallStatus::AwaitingApproval);
                move_tool(&mut state, ToolCallStatus::Denied);
            }
        }
        state.set_working_directory("~/plexmaton".to_owned());
        state
    }

    fn disclosed_tool_state(width: u16, height: u16) -> ViewState {
        let agent =
            AgentId::new("agent-primary").unwrap_or_else(|error| panic!("fixture: {error}"));
        let item =
            TranscriptItemId::new("tool-entry").unwrap_or_else(|error| panic!("fixture: {error}"));
        let mut state = tool_state_with_presentation(
            ToolCallStatus::Succeeded,
            "exec_command",
            ToolPresentation {
                invocation: Some(ToolDetail::Text {
                    source: "Command \"cargo test -p plexmaton-tui\"\ncwd: \"~/plexmaton\"\ntimeout_ms: 120000"
                        .to_owned(),
                    omitted_bytes: 0,
                }),
                outcome: Some(ToolDetail::Text {
                    source: "status: exited\nexit_code: 0\nstdout:\n197 tests passed\nstderr:\n[empty]"
                        .to_owned(),
                    omitted_bytes: 0,
                }),
            },
        );
        let (surfaces, _) = draw_frame(&state, &Palette::default(), width, height);
        state.toggle_entry(
            &surfaces,
            &TranscriptMetrics::default(),
            EntryTarget {
                surface: SurfaceId::Transcript,
                agent,
                item,
                index: 0,
            },
        );
        state
    }

    fn apply_frame_event(state: &mut ViewState, sequence: &mut u64, event: ConversationEvent) {
        *sequence = sequence.saturating_add(1);
        let outcome = state.apply(ConversationEventEnvelope {
            sequence: EventSequence::new(*sequence),
            event,
        });
        assert!(
            matches!(outcome, crate::ApplyOutcome::Accepted),
            "frame event was rejected: {outcome:?}"
        );
    }

    fn append_frame_text(
        state: &mut ViewState,
        sequence: &mut u64,
        agent: &AgentId,
        name: &str,
        role: plexmaton_core::TranscriptRole,
        source: &str,
    ) {
        let item = TranscriptItemId::new(name).unwrap_or_else(|error| panic!("fixture: {error}"));
        apply_frame_event(
            state,
            sequence,
            ConversationEvent::TranscriptItemStarted {
                agent_id: agent.clone(),
                item_id: item.clone(),
                role,
            },
        );
        apply_frame_event(
            state,
            sequence,
            ConversationEvent::TranscriptDelta {
                agent_id: agent.clone(),
                item_id: item.clone(),
                item_revision: 1,
                text: source.to_owned(),
            },
        );
        apply_frame_event(
            state,
            sequence,
            ConversationEvent::TranscriptItemFinalized {
                agent_id: agent.clone(),
                item_id: item,
                item_revision: 2,
            },
        );
    }

    fn transcript_grammar_state(width: u16, height: u16) -> ViewState {
        use plexmaton_core::TranscriptRole;

        let agent =
            AgentId::new("agent-primary").unwrap_or_else(|error| panic!("fixture: {error}"));
        let mut state = ViewState::default();
        let mut sequence = 0_u64;
        apply_frame_event(
            &mut state,
            &mut sequence,
            ConversationEvent::AgentCreated {
                agent_id: agent.clone(),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
        );
        // The two everyday turns lead, because they are what the grammar is mostly made of and
        // the only two with no word above them: what separates them has to be visible in a frame.
        append_frame_text(
            &mut state,
            &mut sequence,
            &agent,
            "user-turn",
            TranscriptRole::User,
            "Change the colour constant and show me the patch you applied.",
        );
        append_frame_text(
            &mut state,
            &mut sequence,
            &agent,
            "assistant-turn",
            TranscriptRole::Assistant,
            "Done. The retained patch is below, with the reasoning that led to it.",
        );
        append_frame_text(
            &mut state,
            &mut sequence,
            &agent,
            "visible-reasoning",
            TranscriptRole::Reasoning,
            "The retained patch matches the requested change.",
        );
        append_frame_text(
            &mut state,
            &mut sequence,
            &agent,
            "system-message",
            TranscriptRole::System,
            "Tool execution resumed after approval.",
        );
        apply_frame_event(
            &mut state,
            &mut sequence,
            ConversationEvent::RuntimeWarning {
                agent_id: agent.clone(),
                item_id: TranscriptItemId::new("runtime-warning")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                message: "Provider usage omitted cached-token detail.".to_owned(),
            },
        );
        apply_frame_event(
            &mut state,
            &mut sequence,
            ConversationEvent::RuntimeError {
                agent_id: agent.clone(),
                item_id: TranscriptItemId::new("runtime-error")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                message: "The model request failed before producing an answer.".to_owned(),
            },
        );

        let item = TranscriptItemId::new("canonical-edit")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        let call =
            ToolCallId::new("canonical-edit").unwrap_or_else(|error| panic!("fixture: {error}"));
        let invocation = ToolDetail::Text {
            source: "path: src/lib.rs\nobservation: obs-7".to_owned(),
            omitted_bytes: 0,
        };
        for (revision, status, outcome) in [
            (0, ToolCallStatus::Queued, None),
            (1, ToolCallStatus::Running, None),
            (
                2,
                ToolCallStatus::Succeeded,
                Some(ToolDetail::Diff {
                    patch: "*** Begin Patch\n*** Update File: src/lib.rs\n@@ bytes 0..36; old_bytes=36; new_bytes=38 @@\n-pub const COLOR: &str = \"blue\";\n+pub const COLOR: &str = \"pastel\";\n*** End Patch"
                        .to_owned(),
                }),
            ),
        ] {
            apply_frame_event(
                &mut state,
                &mut sequence,
                ConversationEvent::ToolCallChanged {
                    agent_id: agent.clone(),
                    item_id: item.clone(),
                    item_revision: revision,
                    call_id: call.clone(),
                    label: "edit_file".to_owned(),
                    status,
                    presentation: ToolPresentation {
                        invocation: (revision > 0).then(|| invocation.clone()),
                        outcome,
                    },
                },
            );
        }
        state.set_working_directory("~/plexmaton".to_owned());
        let index = state
            .primary_agent()
            .and_then(|agent| agent.entries().position(|entry| entry.id() == &item))
            .unwrap_or_else(|| panic!("the edit is in the transcript"));
        let (surfaces, _) = draw_frame(&state, &Palette::pastel(), width, height);
        state.toggle_entry(
            &surfaces,
            &TranscriptMetrics::default(),
            EntryTarget {
                surface: SurfaceId::Transcript,
                agent,
                item,
                index,
            },
        );
        state
    }

    /// JRN-7/ui-ux §responsive interaction: a failed durable boundary is legible at every width.
    #[test]
    fn the_persistence_failure_frames_match_their_fixtures() {
        for (failure, failure_name, signature) in PERSISTENCE_FAILURES {
            let mut state = canonical_state();
            state.report_cleanup_failure(CleanupNotice::JournalWriter);
            state.report_persistence_failure(failure);
            for (width_name, width, height) in PRODUCT_WIDTHS {
                let name = format!("persistence-{failure_name}-{width_name}");
                let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, height);
                let bounds = surfaces
                    .get(SurfaceId::Notices)
                    .expect("notice strip")
                    .bounds;
                let drawn = crate::test_support::snapshot_text(&buffer, bounds);
                assert!(drawn.contains(signature), "{name}: failure copy is absent");
                assert_eq!(bounds, Rect::new(0, 0, width, 4));
                if width_name == "wide" {
                    crate::test_support::assert_frame(&name, &drawn);
                }
            }
        }
    }

    /// JRN-7/ui-ux §responsive interaction: failed cleanup remains one bounded visible strip.
    #[test]
    fn the_cleanup_failure_frames_match_their_fixtures() {
        let mut state = canonical_state();
        state.report_cleanup_failure(CleanupNotice::JournalWriter);
        for (width_name, width, height) in PRODUCT_WIDTHS {
            let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, height);
            let bounds = surfaces
                .get(SurfaceId::Notices)
                .expect("notice strip")
                .bounds;
            let drawn = crate::test_support::snapshot_text(&buffer, bounds);
            assert!(
                drawn.contains("journal writer cleanup failed"),
                "{width_name}: cleanup copy is absent"
            );
            assert_eq!(bounds, Rect::new(0, 0, width, 4));
            if width_name == "wide" {
                crate::test_support::assert_frame("cleanup-failure-wide", &drawn);
            }
        }
    }

    /// SKL-4/ui-ux §responsive interaction: failed activation keeps input and failure observable.
    #[test]
    fn the_skill_diagnostic_frames_match_their_fixtures() {
        let mut conversation = Conversation::canonical();
        let agent = conversation
            .state
            .primary_agent()
            .map(|agent| agent.id.clone())
            .expect("canonical primary agent");
        let item = TranscriptItemId::new("skill-review").expect("item id");
        let call = ToolCallId::new("skill-review").expect("call id");
        conversation.emit(ConversationEvent::ToolCallChanged {
            agent_id: agent.clone(),
            item_id: item.clone(),
            item_revision: 0,
            call_id: call.clone(),
            label: "skill".to_owned(),
            status: ToolCallStatus::Queued,
            presentation: ToolPresentation::default(),
        });
        conversation.emit(ConversationEvent::ToolCallChanged {
            agent_id: agent.clone(),
            item_id: item,
            item_revision: 1,
            call_id: call,
            label: "skill".to_owned(),
            status: ToolCallStatus::Failed,
            presentation: ToolPresentation {
                invocation: Some(ToolDetail::Text {
                    source: "name: review".to_owned(),
                    omitted_bytes: 0,
                }),
                outcome: Some(ToolDetail::Text {
                    source: "skill file was unavailable".to_owned(),
                    omitted_bytes: 0,
                }),
            },
        });
        conversation
            .state
            .return_input(agent, "$review check this change".to_owned());
        conversation.state.report_skill_diagnostic(
            "Skill unavailable · input returned to the composer; SKILL.md could not be read"
                .to_owned(),
        );

        for (name, width, height) in SKILL_DIAGNOSTIC_FRAMES {
            let drawn = draw(&conversation.state, width, height);
            for signature in [
                "Skills · Skill unavailable",
                "[!] skill",
                "$review check this change",
            ] {
                assert!(drawn.contains(signature), "{name}: {signature:?} is absent");
            }
            crate::test_support::assert_frame(name, &drawn);
        }
    }

    /// SKP-4: the primary composer, contextual list, and source summaries remain legible together.
    #[test]
    fn the_skill_picker_frames_match_their_fixtures() {
        for (name, width, height) in SKILL_PICKER_FRAMES {
            let mut workspace = Workspace::default();
            workspace.emit(vec![ConversationEventEnvelope {
                sequence: EventSequence::new(1),
                event: ConversationEvent::AgentCreated {
                    agent_id: AgentId::new("primary").expect("agent"),
                    label: "Plexmaton".to_owned(),
                    status: AgentStatus::Idle,
                },
            }]);
            workspace.set_skills(vec![
                SkillChoice {
                    name: "review".to_owned(),
                    description:
                        "Review this change for correctness across every affected boundary"
                            .to_owned(),
                    source: SkillChoiceSource::ProjectNative,
                },
                SkillChoice {
                    name: "research".to_owned(),
                    description: "Gather focused source evidence".to_owned(),
                    source: SkillChoiceSource::ProjectShared,
                },
                SkillChoice {
                    name: "release".to_owned(),
                    description: "Prepare release notes".to_owned(),
                    source: SkillChoiceSource::User,
                },
            ]);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
            workspace.draw(&mut terminal).expect("initial frame");
            workspace.handle(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
            for character in "$r".chars() {
                workspace.handle(&Event::Key(KeyEvent::new(
                    KeyCode::Char(character),
                    KeyModifiers::NONE,
                )));
                workspace.draw(&mut terminal).expect("picker frame");
            }
            let picker = workspace
                .surfaces()
                .get(SurfaceId::SkillPicker)
                .expect("skill picker");
            let composer = workspace
                .surfaces()
                .get(SurfaceId::Composer)
                .expect("composer");
            assert_eq!(picker.bounds.bottom(), composer.bounds.y, "{name}");
            let drawn = crate::test_support::snapshot_text(
                terminal.backend().buffer(),
                terminal.backend().buffer().area,
            );
            for signature in ["Skills", "project · $review", "shared · $research", "$r"] {
                assert!(drawn.contains(signature), "{name}: {signature:?} is absent");
            }
            if name == "skill-picker-narrow" {
                assert!(
                    !drawn.contains("affected boundary"),
                    "the long summary should truncate after its visible source"
                );
            }
            crate::test_support::assert_frame(name, &drawn);
        }
    }

    /// JRN-4/JRN-5: tail repair and turn interruption each have one visible source at every width.
    #[test]
    fn the_session_recovery_frames_match_their_fixtures() {
        let mut state = ViewState::default();
        let agent_id = AgentId::new("agent-primary")
            .unwrap_or_else(|error| panic!("recovery agent id: {error}"));
        let _created = state.apply(ConversationEventEnvelope {
            sequence: EventSequence::new(1),
            event: ConversationEvent::AgentCreated {
                agent_id: agent_id.clone(),
                label: "Plexmaton".to_owned(),
                status: AgentStatus::Idle,
            },
        });
        let _warned = state.apply(ConversationEventEnvelope {
            sequence: EventSequence::new(2),
            event: ConversationEvent::RuntimeWarning {
                agent_id,
                item_id: TranscriptItemId::new("recovery-warning")
                    .unwrap_or_else(|error| panic!("recovery item id: {error}")),
                message: "The previous turn didn't finish. You can continue from here; no model requests or tools were rerun.".to_owned(),
            },
        });
        state.report_conversation_recovery(ConversationRestoration {
            tail: Some(ConversationTailRepair::IsolatedFinalTail { bytes: 37 }),
        });
        assert_eq!(state.notices().count(), 0);
        assert_eq!(
            state
                .primary_agent()
                .unwrap_or_else(|| panic!("recovered agent missing"))
                .transcript()
                .filter(|item| {
                    item.source == "The previous turn didn't finish. You can continue from here; no model requests or tools were rerun."
                })
                .count(),
            1
        );
        for (width_name, width, height) in PRODUCT_WIDTHS {
            let name = format!("session-recovery-{width_name}");
            let drawn = draw(&state, width, height);
            assert!(drawn.contains("Conversation restored."));
            assert!(!drawn.contains("Notices"));
            assert!(
                drawn.find("warning").expect("warning")
                    < drawn.find("Conversation restored.").expect("confirmation")
            );
            assert!(
                drawn.contains("isolated 37-byte incomplete tail"),
                "{name}: recovery copy is absent"
            );
            assert_eq!(drawn.lines().count(), usize::from(height));

            crate::test_support::assert_frame(&name, &drawn);
        }
    }

    /// ENT-2/TR-2: every compact state reaches a real drawn frame at all three widths.
    ///
    /// Deliberately not a fixture family. The compact grammar itself — marker, word, one logical
    /// line, monochrome legibility — is proven per state by
    /// `content::tool::every_tool_status_is_one_named_logical_line`, and the chrome around the row
    /// is frozen by the canonical frames. Seven full-screen snapshots at three widths each froze
    /// 819 lines to assert seven, and the row's text is identical at every width, so twenty-one of
    /// those frames could only ever drift together.
    #[test]
    fn every_tool_status_reaches_the_drawn_frame_at_every_width() {
        for (status, status_text) in [
            (ToolCallStatus::Queued, "queued"),
            (ToolCallStatus::AwaitingApproval, "approval required"),
            (ToolCallStatus::Running, "running"),
            (ToolCallStatus::Succeeded, "succeeded"),
            (ToolCallStatus::Failed, "failed"),
            (ToolCallStatus::Denied, "denied"),
            (ToolCallStatus::Cancelled, "cancelled"),
        ] {
            let state = tool_state(status);
            for (width_name, width, height) in PRODUCT_WIDTHS {
                let drawn = draw(&state, width, height);
                for signature in ["read_file", status_text, "Message Plexmaton", "~/plexmaton"] {
                    assert!(
                        drawn.contains(signature),
                        "{status:?} at {width_name}: {signature:?} is not on screen"
                    );
                }
                assert!(
                    !drawn.contains("Activity"),
                    "{status:?} at {width_name}: the retired detail surface returned"
                );
                assert_eq!(drawn.lines().count(), usize::from(height));
            }
        }
    }

    /// ENT-4: one expanded entry remains part of its conversation at every product width.
    #[test]
    fn the_open_tool_frames_match_their_fixtures() {
        for (name, width, height) in DISCLOSURE_FRAMES {
            let drawn = draw(&disclosed_tool_state(width, height), width, height);
            for signature in [
                "exec_command · succeeded",
                "invocation",
                "cargo test -p plexmaton-tui",
                "outcome",
                "197 tests passed",
            ] {
                assert!(drawn.contains(signature), "{name}: {signature:?} is absent");
            }
            assert_eq!(drawn.lines().count(), usize::from(height));

            crate::test_support::assert_frame(name, &drawn);
        }
    }

    /// ENT-4 and `ui-ux.md` §transcript grammar: every remaining role and a complete canonical
    /// patch stay legible together at wide, medium, and narrow.
    #[test]
    fn the_remaining_transcript_grammar_frames_match_their_fixtures() {
        for (name, width, height) in GRAMMAR_FRAMES {
            let drawn = draw(&transcript_grammar_state(width, height), width, height);
            for signature in [
                // The user's turn wears a bar down its whole height and the agent's wears
                // nothing; neither wears a word. `you` and `assistant` above every message
                // labelled what the shape of the screen already said (ui-ux §transcript grammar).
                "▌Change the colour constant",
                "Done. The retained patch",
                "reasoning",
                "system",
                "warning",
                "error",
                "edit_file · succeeded",
                "*** Begin Patch",
                "*** Update File: src/lib.rs",
                "-pub const COLOR",
                "+pub const COLOR",
            ] {
                assert!(drawn.contains(signature), "{name}: {signature:?} is absent");
            }
            for absent in ["you", "assistant"] {
                assert!(
                    !drawn
                        .lines()
                        .any(|line| line.trim_matches(|c: char| c == '│' || c == ' ').eq(absent)),
                    "{name}: the everyday turns carry no role word, and {absent:?} is one"
                );
            }
            assert_eq!(drawn.lines().count(), usize::from(height));

            crate::test_support::assert_frame(name, &drawn);
        }
    }

    fn approval_state(width: u16, height: u16) -> ViewState {
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
                tool: "edit".to_owned(),
                capabilities: vec![ToolCapability::FileWrite],
                detail:
                    "Change crates/plexmaton-core/src/lib.rs and preserve its current revision."
                        .to_owned(),
            },
        });
        conversation
            .state
            .set_working_directory("~/plexmaton".to_owned());
        let (surfaces, _) = draw_frame(&conversation.state, &Palette::default(), width, height);
        conversation
            .state
            .attend(&surfaces, AttentionIntent::Move(Direction::Forward));
        conversation.state.attend(&surfaces, AttentionIntent::GoTo);
        conversation.state
    }

    /// APV-4 and SURF-4: the blocking decision surface is frozen at every supported composition.
    #[test]
    fn the_approval_frames_match_their_fixtures() {
        for (name, width, height) in APPROVAL_FRAMES {
            let drawn = draw(&approval_state(width, height), width, height);
            for signature in [
                "Approval required",
                "edit",
                "Allow once",
                "> Deny",
                "Enter decide",
            ] {
                assert!(
                    drawn.contains(signature),
                    "{name}: {signature:?} is not on screen"
                );
            }
            crate::test_support::assert_frame(name, &drawn);
        }
    }

    fn native_approval_state(_width: u16, _height: u16) -> ViewState {
        command_approval_state(
            "cargo test",
            "exact command; same cwd/environment",
            Some("no prefix suggestion for this command"),
            plexmaton_core::PermissionScopes::Session,
        )
    }

    fn command_approval_state(
        command: &str,
        label: &str,
        note: Option<&str>,
        scopes: plexmaton_core::PermissionScopes,
    ) -> ViewState {
        let mut conversation = Conversation::canonical();
        let item_id = TranscriptItemId::new("agent-a-command-native-1")
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        conversation.emit(ConversationEvent::ToolCallChanged {
            agent_id: AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            item_id: item_id.clone(),
            item_revision: 0,
            call_id: ToolCallId::new("command-native-1")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            label: "exec_command".to_owned(),
            status: ToolCallStatus::Queued,
            presentation: ToolPresentation::default(),
        });
        conversation.emit(ConversationEvent::ToolCallChanged {
            agent_id: AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            item_id,
            item_revision: 1,
            call_id: ToolCallId::new("command-native-1")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            label: "exec_command".to_owned(),
            status: ToolCallStatus::AwaitingApproval,
            presentation: ToolPresentation::default(),
        });
        conversation.emit(ConversationEvent::AttentionRequested {
            agent_id: AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: AttentionId::new("attention-a-command")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                reason: plexmaton_core::ApprovalReason::CommandExecution,
                remember: Some(plexmaton_core::RememberPermissionOffer {
                    id: plexmaton_core::PermissionOfferId::new(1),
                    label: label.into(),
                    note: note.map(str::to_owned),
                    scopes,
                }),
                approval_id: ApprovalId::new("approval-a-command")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("command-native-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "exec_command".to_owned(),
                capabilities: vec![
                    ToolCapability::FileRead,
                    ToolCapability::FileWrite,
                    ToolCapability::ProcessSpawn,
                ],
                detail: format!(
                    "Command {command:?} · cwd \"/home/dev/plexmaton\" · timeout 120000 ms"
                ),
            },
        });
        conversation
            .state
            .set_working_directory("~/plexmaton".to_owned());
        conversation.state
    }

    /// LIVE-1 and APV-4: the concrete native command decision remains legible at every width.
    #[test]
    fn the_native_approval_frames_match_their_fixtures() {
        for (name, width, height) in NATIVE_APPROVAL_FRAMES {
            let state = native_approval_state(width, height);
            let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, height);
            let drawn = region_text(&buffer, Rect::new(0, 0, width, height));
            let approval = surfaces
                .get(SurfaceId::Approval)
                .unwrap_or_else(|| panic!("{name}: approval surface is not registered"));
            let approval = region_text(&buffer, approval.bounds);
            for signature in [
                "Approval required",
                "exec_command",
                "No current permission",
                "Allow once",
                "Allow and remember…",
                "> Deny",
                "cargo test",
            ] {
                assert!(
                    approval.contains(signature),
                    "{name}: {signature:?} is not on the approval card"
                );
            }
            crate::test_support::assert_frame(name, &drawn);
        }
    }

    /// PER-5: remembered scopes occupy the same card and grant nothing until the lifetime is confirmed.
    #[test]
    fn per_5_remembered_scope_frames_preserve_the_operation_and_composer() {
        for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
            let mut state = native_approval_state(width, 40);
            state.decide_approval(crate::ApprovalIntent::Move(Direction::Backward));
            assert!(
                state
                    .decide_approval(crate::ApprovalIntent::Decide)
                    .is_none()
            );
            let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, 40);
            let approval = region_text(
                &buffer,
                surfaces.get(SurfaceId::Approval).expect("same card").bounds,
            );
            for signature in [
                "Remember permission",
                "cargo test",
                "Scope:",
                "cwd",
                "environment",
                "> This Session",
                "Back",
                "Esc back",
            ] {
                assert!(
                    approval.contains(signature),
                    "{name}: {signature}: {approval}"
                );
            }
            assert_eq!(
                surfaces
                    .get(SurfaceId::Composer)
                    .expect("composer")
                    .bounds
                    .height,
                3
            );
            crate::test_support::assert_frame(&format!("remember-permission-{name}"), &approval);
        }
    }

    /// PER-10/PER-5: concrete prefix and both lifetimes remain visible in the reviewed card.
    #[test]
    fn per_10_prefix_permission_frames_show_tokens_context_and_project_lifetime() {
        for (width, name) in [(120, "wide"), (95, "medium"), (60, "narrow")] {
            let mut state = command_approval_state(
                "git fetch 'team origin'",
                "git fetch …; same cwd/environment",
                None,
                plexmaton_core::PermissionScopes::SessionAndProject,
            );
            state.decide_approval(crate::ApprovalIntent::Move(Direction::Backward));
            assert!(
                state
                    .decide_approval(crate::ApprovalIntent::Decide)
                    .is_none()
            );
            let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, 40);
            let approval = region_text(
                &buffer,
                surfaces.get(SurfaceId::Approval).expect("card").bounds,
            );
            for text in [
                "Remember permission",
                "team origin",
                "git fetch …",
                "cwd",
                "environment",
                "This Session",
                "This Project",
                "Back",
                "Esc back",
            ] {
                assert!(approval.contains(text), "{name}: {text}: {approval}");
            }
            crate::test_support::assert_frame(&format!("prefix-permission-{name}"), &approval);
        }
    }

    /// PER-10: the smallest supported confirmation cannot hide a prefix's binding or lifetime.
    #[test]
    fn per_10_short_prefix_confirmation_retains_scope_and_all_choices() {
        for width in [48, 60] {
            let mut state = command_approval_state(
                "git fetch origin",
                "git fetch …; same cwd/environment",
                None,
                plexmaton_core::PermissionScopes::SessionAndProject,
            );
            state.decide_approval(crate::ApprovalIntent::Move(Direction::Backward));
            state.decide_approval(crate::ApprovalIntent::Decide);
            let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, 12);
            let approval = region_text(
                &buffer,
                surfaces.get(SurfaceId::Approval).expect("card").bounds,
            );
            for text in [
                "Scope:",
                "git fetch",
                "environment",
                "This Session",
                "This Project",
                "Back",
            ] {
                assert!(approval.contains(text), "{width}: {text}: {approval}");
            }
        }
    }

    /// APV-4: even the smallest supported terminal shows what will execute before Enter can
    /// grant it; scrolling is for the remainder, not for discovering the command exists.
    #[test]
    fn native_command_is_visible_before_decision_at_the_smallest_terminal() {
        let (width, height) = (48, 12);
        let state = native_approval_state(width, height);
        let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, height);
        let approval = surfaces
            .get(SurfaceId::Approval)
            .unwrap_or_else(|| panic!("approval surface is not registered"));
        let status = surfaces
            .get(SurfaceId::Status)
            .unwrap_or_else(|| panic!("status surface is not registered"));
        assert!(
            approval.bounds.bottom() <= status.bounds.top(),
            "the compact modal must not cover the status line"
        );
        let approval = region_text(&buffer, approval.bounds);

        for signature in ["Allow once", "> Deny", "Command \"cargo test\""] {
            assert!(
                approval.contains(signature),
                "smallest approval card hid {signature:?}:\n{approval}"
            );
        }
    }

    /// APV-4: disclosing the request grows what is read, never moves what is answered.
    ///
    /// The region used to scroll, and `Ctrl-O` scrolled `Allow once` off the top of it — a
    /// decision surface whose decision could leave the screen. It does not scroll now: the two
    /// options are its last two rows at either size, and the composer keeps its own rows under
    /// both, because answering a tool call is not the input the user types the next instruction
    /// into (ui-ux §input).
    #[test]
    fn disclosing_the_request_moves_neither_the_options_nor_the_composer_off_the_region() {
        for (_, width, height) in PRODUCT_WIDTHS {
            let mut state = native_approval_state(width, height);
            for expanded in [false, true] {
                let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, height);
                let region = surfaces
                    .get(SurfaceId::Approval)
                    .unwrap_or_else(|| panic!("{width}x{height}: the region is registered"));
                let composer = surfaces
                    .get(SurfaceId::Composer)
                    .unwrap_or_else(|| panic!("{width}x{height}: the composer keeps its rows"));
                assert_eq!(
                    composer.bounds.y,
                    region.bounds.bottom(),
                    "{width}x{height} expanded={expanded}: the composer sits under the region"
                );
                assert!(
                    composer.bounds.height >= 2,
                    "{width}x{height} expanded={expanded}: the composer is still a place to type"
                );

                let conversation = surfaces
                    .get(SurfaceId::Transcript)
                    .unwrap_or_else(|| panic!("{width}x{height}: the conversation is registered"));
                let border = region_text(
                    &buffer,
                    Rect {
                        height: 1,
                        ..conversation.bounds
                    },
                );
                assert!(
                    border.contains("( !2 )"),
                    "{width}x{height} expanded={expanded}: the pill counts the two background \
                     requests and excludes the visible primary request:\n{border}"
                );

                let rows: Vec<String> = region_text(&buffer, region.bounds)
                    .lines()
                    .map(str::to_owned)
                    .collect();
                assert!(
                    rows.iter().any(|row| row.contains("Allow once"))
                        && rows.iter().any(|row| row.contains("Allow and remember…"))
                        && rows.iter().any(|row| row.contains("Deny")),
                    "{width}x{height} expanded={expanded}: all three options remain visible:\n{}",
                    rows.join("\n")
                );

                state.decide_approval(crate::intent::ApprovalIntent::ToggleDetail);
            }
        }
    }

    /// The product's own text: every cell except the two conversations' bodies, which hold what
    /// a producer said and are not the screen's copy.
    fn chrome_text(buffer: &Buffer, surfaces: &SurfaceTree) -> String {
        let mut text = String::new();
        for surface in surfaces.iter() {
            let bounds = surface.bounds;
            let region = match surface.id {
                SurfaceId::Transcript | SurfaceId::Inspector => {
                    text.push_str(&region_text(
                        buffer,
                        Rect {
                            height: 1,
                            ..bounds
                        },
                    ));
                    text.push('\n');
                    Rect {
                        y: bounds.bottom().saturating_sub(1),
                        height: 1,
                        ..bounds
                    }
                }
                _ => bounds,
            };
            text.push_str(&region_text(buffer, region));
            text.push('\n');
        }
        text
    }

    /// Phase 01 §scope 1: no user-facing word is `inspector`, `shelf` or `column`.
    ///
    /// Three states, because the second window's copy exists only while it is open, and its
    /// input only while it is entered.
    #[test]
    fn no_word_on_screen_names_a_mechanism() {
        let palette = Palette::default();
        let mut closed = canonical_state();
        closed.set_working_directory("~/plexmaton".to_owned());
        let mut open = closed.clone();
        open.move_selection(Direction::Forward);
        let mut entered = open.clone();
        let (surfaces, _) = draw_frame(&entered, &palette, 120, 40);
        entered.inspect(&surfaces, InspectorIntent::Open);

        let states: [(&str, &ViewState); 3] =
            [("closed", &closed), ("open", &open), ("entered", &entered)];
        for (label, state) in states {
            for (width, height) in [(140, 40), (120, 40), (95, 40), (60, 40), (60, 12)] {
                let (surfaces, buffer) = draw_frame(state, &palette, width, height);
                let text = chrome_text(&buffer, &surfaces).to_lowercase();
                for word in ["inspector", "shelf", "column"] {
                    assert!(
                        !text.contains(word),
                        "{label} at {width}x{height} says {word:?}:\n{text}"
                    );
                }
            }
        }
    }
}
