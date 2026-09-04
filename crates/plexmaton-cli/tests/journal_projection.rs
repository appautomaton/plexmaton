use plexmaton_agent::{
    Agent, AssistantBlock, AssistantOutput, ContextAtom, Effect, Input, JournalEntryPayload,
    JournalRecord, ModelError, ModelEvent, ModelOutputPosition, ModelStepId, SessionEntry,
    SessionJournal, StopReason, ToolBatch, ToolBatchResult, ToolCall, ToolOutcome, UnixMillis,
};
use plexmaton_core::{
    AgentId, AgentStatus, HeadName, JournalRecordId, SessionEntryId, SessionId, TokenUsage,
    ToolCallId, ToolCallStatus, ToolDetail, ToolPresentation, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{ApplyOutcome, TranscriptEntryView, ViewState};

trait AgentTestExt {
    fn handle(&mut self, input: Input) -> plexmaton_agent::Reaction;
}

impl AgentTestExt for Agent {
    fn handle(&mut self, input: Input) -> plexmaton_agent::Reaction {
        self.handle_at(input, UnixMillis::EPOCH)
    }
}

fn id<T>(value: &str, build: impl FnOnce(String) -> Result<T, plexmaton_core::IdError>) -> T {
    build(value.to_owned()).unwrap_or_else(|error| panic!("fixture identity: {error}"))
}

fn append(journal: &mut SessionJournal, ordinal: u64, payload: JournalEntryPayload) {
    let head = id("main", HeadName::new);
    let parent_id = journal
        .head_target(&head)
        .unwrap_or_else(|error| panic!("head target: {error:?}"))
        .cloned();
    let expected_head_revision = journal
        .head_revision(&head)
        .unwrap_or_else(|error| panic!("head revision: {error:?}"));
    journal
        .apply(JournalRecord::AppendEntry {
            sequence: journal.next_sequence(),
            record_id: id(&format!("record-{ordinal}"), JournalRecordId::new),
            head,
            expected_head_revision,
            entry: Box::new(SessionEntry {
                id: id(&format!("entry-{ordinal}"), SessionEntryId::new),
                parent_id,
                payload,
            }),
        })
        .unwrap_or_else(|error| panic!("append fixture: {error:?}"));
}

fn first_step(agent_id: AgentId, text: &str) -> ModelStepId {
    let mut agent = Agent::new(agent_id);
    let _ = agent.announce("Plexmaton");
    let opened = agent.handle(Input::Submitted {
        text: text.to_owned(),
    });
    match opened.effects.as_slice() {
        [Effect::CallModel(model)] => model.step_id.clone(),
        other => panic!("expected one model call, got {other:?}"),
    }
}

fn apply_all(
    view: &mut ViewState,
    events: impl IntoIterator<Item = plexmaton_core::SessionEventEnvelope>,
) {
    for envelope in events {
        assert_eq!(view.apply(envelope), ApplyOutcome::Accepted);
    }
}

fn visible_text(view: &ViewState, agent_id: &AgentId) -> Vec<(TranscriptRole, String)> {
    view.agent(agent_id)
        .unwrap_or_else(|| panic!("projected agent missing"))
        .transcript()
        .map(|item| (item.role, item.source.clone()))
        .collect()
}

fn live_and_replayed_after(
    agent_name: &str,
    output: impl IntoIterator<Item = ModelEvent>,
    terminal: Result<StopReason, ModelError>,
) -> (ViewState, ViewState, AgentId) {
    let agent_id = id(agent_name, AgentId::new);
    let mut live = Agent::new(agent_id.clone());
    let mut live_view = ViewState::default();
    apply_all(&mut live_view, live.announce("Plexmaton").events);
    let submitted = live.handle(Input::Submitted {
        text: "exercise ordering".to_owned(),
    });
    let step_id = match submitted.effects.as_slice() {
        [Effect::CallModel(call)] => call.step_id.clone(),
        other => panic!("expected one model call, got {other:?}"),
    };
    apply_all(&mut live_view, submitted.events);
    for event in output {
        let reaction = live.handle(Input::Streamed {
            step_id: step_id.clone(),
            event,
        });
        apply_all(&mut live_view, reaction.events);
    }
    let terminal = match terminal {
        Ok(reason) => live.handle(Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(reason),
        }),
        Err(error) => live.handle(Input::Failed { step_id, error }),
    };
    apply_all(&mut live_view, terminal.events);
    let projection = live
        .rebuild_projection()
        .unwrap_or_else(|error| panic!("rebuild settled projection: {error:?}"));
    let mut replay_view = ViewState::default();
    apply_all(&mut replay_view, projection.events().iter().cloned());
    (live_view, replay_view, agent_id)
}

/// JRN-5: the composition root's two consumers accept projections from only the journal path.
#[test]
fn jrn_5_journal_projection_builds_the_model_request_and_tui_state() {
    let agent_id = id("agent-primary", AgentId::new);
    let call_id = id("call-1", ToolCallId::new);
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
    let step_id = first_step(agent_id.clone(), "inspect the workspace");
    append(
        &mut journal,
        1,
        JournalEntryPayload::AgentCreated {
            agent_id: agent_id.clone(),
            label: "Plexmaton".to_owned(),
            status: AgentStatus::Idle,
        },
    );
    append(
        &mut journal,
        2,
        JournalEntryPayload::TurnStarted {
            agent_id: agent_id.clone(),
            item_id: id("user-item", TranscriptItemId::new),
            turn_id: step_id.turn_id().clone(),
            text: "inspect the workspace".to_owned(),
            accepted_at: UnixMillis::EPOCH,
            opened_at: UnixMillis::EPOCH,
        },
    );
    let call = ToolCall {
        call_id: call_id.clone(),
        name: "read_file".to_owned(),
        arguments: r#"{"path":"README.md"}"#.to_owned(),
    };
    let output = AssistantOutput::new(
        vec![
            AssistantBlock::Text {
                item_id: id("assistant-item", TranscriptItemId::new),
                text: "I will read it.".to_owned(),
            },
            AssistantBlock::ToolCall {
                item_id: id("tool-item", TranscriptItemId::new),
                call: call.clone(),
            },
        ],
        None,
    )
    .unwrap_or_else(|error| panic!("assistant output fixture: {error:?}"));
    append(
        &mut journal,
        3,
        JournalEntryPayload::AssistantOutput {
            agent_id: agent_id.clone(),
            step_id,
            output: output.clone(),
        },
    );
    append(
        &mut journal,
        4,
        JournalEntryPayload::ToolCallRequested {
            agent_id: agent_id.clone(),
            call_id: call_id.clone(),
            presentation: ToolPresentation::default(),
        },
    );
    append(
        &mut journal,
        5,
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent_id.clone(),
            call_id: call_id.clone(),
            item_revision: 1,
            status: ToolCallStatus::Running,
            presentation: ToolPresentation {
                invocation: Some(ToolDetail::Text {
                    source: "Read README.md".to_owned(),
                    omitted_bytes: 0,
                }),
                outcome: None,
            },
            outcome: None,
        },
    );
    let outcome = ToolOutcome::Succeeded {
        output: "workspace read".to_owned(),
    };
    append(
        &mut journal,
        6,
        JournalEntryPayload::ToolCallChanged {
            agent_id: agent_id.clone(),
            call_id: call_id.clone(),
            item_revision: 2,
            status: ToolCallStatus::Succeeded,
            presentation: ToolPresentation::default(),
            outcome: Some(outcome.clone()),
        },
    );

    let projection = journal
        .project(&id("main", HeadName::new))
        .unwrap_or_else(|error| panic!("project fixture: {error:?}"));
    let batch = ToolBatch::new(output, vec![ToolBatchResult::new(call_id, outcome)])
        .unwrap_or_else(|error| panic!("tool batch fixture: {error:?}"));
    let expected_atoms = [
        ContextAtom::user(
            id("entry-2", SessionEntryId::new),
            "inspect the workspace".to_owned(),
        ),
        ContextAtom::tool_batch(
            [3, 4, 5, 6]
                .into_iter()
                .map(|ordinal| id(&format!("entry-{ordinal}"), SessionEntryId::new))
                .collect(),
            batch,
        )
        .unwrap_or_else(|error| panic!("context atom fixture: {error:?}")),
    ];
    assert_eq!(projection.request().atoms, expected_atoms);

    let mut view = ViewState::default();
    for envelope in projection.events() {
        assert_eq!(view.apply(envelope.clone()), ApplyOutcome::Accepted);
    }
    let agent = view
        .agent(&agent_id)
        .unwrap_or_else(|| panic!("projected agent missing"));
    assert_eq!(agent.label, "Plexmaton");
    assert_eq!(agent.entries().count(), 3);
    let tool = agent
        .tools()
        .next()
        .unwrap_or_else(|| panic!("projected tool missing"));
    assert_eq!(tool.status, ToolCallStatus::Succeeded);
    assert_eq!(
        tool.presentation.invocation,
        Some(ToolDetail::Text {
            source: "Read README.md".to_owned(),
            omitted_bytes: 0,
        })
    );
    let text: Vec<_> = agent
        .entries()
        .filter_map(|entry| match entry {
            TranscriptEntryView::Text(item) => Some(item.source.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, ["inspect the workspace", "I will read it."]);
    assert_eq!(view.notices().count(), 0);
}

/// JRN-5: reload normalizes provider chunks while preserving the visible semantic state.
#[test]
fn jrn_5_multi_delta_live_turn_and_replay_have_equal_visible_semantics() {
    let agent_id = id("agent-primary", AgentId::new);
    let mut live = Agent::new(agent_id.clone());
    let mut live_view = ViewState::default();
    let mut records = Vec::new();
    let mut announcement = live.announce("Plexmaton");
    records.append(&mut announcement.records);
    apply_all(&mut live_view, announcement.events);
    let mut submitted = live.handle(Input::Submitted {
        text: "hello".to_owned(),
    });
    let step_id = match submitted.effects.as_slice() {
        [Effect::CallModel(call)] => call.step_id.clone(),
        other => panic!("expected one model call, got {other:?}"),
    };
    records.append(&mut submitted.records);
    apply_all(&mut live_view, submitted.events);
    for text in ["h", "i"] {
        let mut reaction = live.handle(Input::Streamed {
            step_id: step_id.clone(),
            event: ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: text.to_owned(),
            },
        });
        records.append(&mut reaction.records);
        apply_all(&mut live_view, reaction.events);
    }
    let mut stopped = live.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
    });
    records.append(&mut stopped.records);
    apply_all(&mut live_view, stopped.events);

    let mut rebuilt = SessionJournal::new(live.journal().session_id().clone());
    for record in records {
        rebuilt
            .apply(record)
            .unwrap_or_else(|error| panic!("replay live record: {error:?}"));
    }
    assert_eq!(&rebuilt, live.journal());
    let expected_projection = rebuilt
        .project(&id("main", HeadName::new))
        .unwrap_or_else(|error| panic!("project rebuilt live journal: {error:?}"));
    let projection = live
        .rebuild_projection()
        .unwrap_or_else(|error| panic!("rebuild idle live projection: {error:?}"));
    assert_eq!(projection, expected_projection);
    assert_eq!(projection.request().atoms, live.record());
    let mut replay_view = ViewState::default();
    apply_all(&mut replay_view, projection.events().iter().cloned());

    let live_agent = live_view
        .agent(&agent_id)
        .unwrap_or_else(|| panic!("live agent missing"));
    let replay_agent = replay_view
        .agent(&agent_id)
        .unwrap_or_else(|| panic!("replayed agent missing"));
    assert_eq!(live_agent.label, replay_agent.label);
    assert_eq!(live_agent.status, replay_agent.status);
    let visible = |agent: &plexmaton_tui::AgentView| {
        agent
            .transcript()
            .map(|item| {
                (
                    item.id.clone(),
                    item.role,
                    item.kind,
                    item.source.clone(),
                    item.finalized,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(visible(live_agent), visible(replay_agent));
    assert_eq!(live_view.notices().count(), 0);
    assert_eq!(replay_view.notices().count(), 0);

    let continued = live.handle(Input::Submitted {
        text: "continue after rebuild".to_owned(),
    });
    apply_all(&mut replay_view, continued.events);
    let continued_agent = replay_view
        .agent(&agent_id)
        .unwrap_or_else(|| panic!("continued replay agent missing"));
    assert!(continued_agent.transcript().any(|item| {
        item.role == TranscriptRole::User && item.source == "continue after rebuild"
    }));
    assert_eq!(replay_view.notices().count(), 0);
}

/// JRN-6: diagnostics that end a partial answer retain its visible position after rebuilding.
#[test]
fn jrn_6_partial_failure_and_output_limit_keep_live_transcript_order() {
    for (agent, terminal) in [
        ("agent-output-limit", Ok(StopReason::OutputLimit)),
        (
            "agent-failure",
            Err(ModelError::Transport {
                message: "offline".to_owned(),
            }),
        ),
    ] {
        let (live, replayed, agent_id) = live_and_replayed_after(
            agent,
            [ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "partial answer".to_owned(),
            }],
            terminal,
        );
        assert_eq!(
            visible_text(&live, &agent_id),
            visible_text(&replayed, &agent_id)
        );
    }
}

/// JRN-6: completed message records retain the order in which their live rows first opened.
#[test]
fn jrn_6_interleaved_answer_and_reasoning_keep_first_open_order() {
    let (live, replayed, agent_id) = live_and_replayed_after(
        "agent-interleaved",
        [
            ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "answer".to_owned(),
            },
            ModelEvent::ReasoningDelta {
                position: ModelOutputPosition::new(1, 0),
                delta: "reasoning".to_owned(),
            },
        ],
        Ok(StopReason::EndOfTurn),
    );

    assert_eq!(
        visible_text(&live, &agent_id),
        visible_text(&replayed, &agent_id)
    );
}

/// JRN-6: a provider defect noticed mid-stream is placed after the completed message in both views.
#[test]
fn jrn_6_streaming_usage_warning_keeps_live_transcript_order() {
    let (live, replayed, agent_id) = live_and_replayed_after(
        "agent-usage-warning",
        [
            ModelEvent::TextDelta {
                position: ModelOutputPosition::new(0, 0),
                delta: "answer".to_owned(),
            },
            ModelEvent::Usage(TokenUsage::Unavailable),
            ModelEvent::Usage(TokenUsage::Unavailable),
        ],
        Ok(StopReason::EndOfTurn),
    );

    assert_eq!(
        visible_text(&live, &agent_id),
        visible_text(&replayed, &agent_id)
    );
}
