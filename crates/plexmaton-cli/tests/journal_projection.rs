use plexmaton_agent::{
    Agent, Effect, Input, JournalEntryPayload, JournalRecord, ModelEvent, RequestItem,
    SessionEntry, SessionJournal, StopReason, ToolCall, ToolOutcome,
};
use plexmaton_core::{
    AgentId, AgentStatus, HeadName, JournalRecordId, SessionEntryId, SessionId, ToolCallId,
    ToolCallStatus, ToolDetail, ToolPresentation, TranscriptItemId, TranscriptRole,
};
use plexmaton_tui::{ApplyOutcome, TranscriptEntryView, ViewState};

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

fn apply_all(
    view: &mut ViewState,
    events: impl IntoIterator<Item = plexmaton_core::SessionEventEnvelope>,
) {
    for envelope in events {
        assert_eq!(view.apply(envelope), ApplyOutcome::Accepted);
    }
}

/// JRN-5: the composition root's two consumers accept projections from only the journal path.
#[test]
fn jrn_5_journal_projection_builds_the_model_request_and_tui_state() {
    let agent_id = id("agent-primary", AgentId::new);
    let call_id = id("call-1", ToolCallId::new);
    let mut journal = SessionJournal::new(id("session-a", SessionId::new));
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
        JournalEntryPayload::Message {
            agent_id: agent_id.clone(),
            item_id: id("user-item", TranscriptItemId::new),
            role: TranscriptRole::User,
            text: "inspect the workspace".to_owned(),
        },
    );
    append(
        &mut journal,
        3,
        JournalEntryPayload::Message {
            agent_id: agent_id.clone(),
            item_id: id("assistant-item", TranscriptItemId::new),
            role: TranscriptRole::Assistant,
            text: "I will read it.".to_owned(),
        },
    );
    let call = ToolCall {
        call_id: call_id.clone(),
        name: "read_file".to_owned(),
        arguments: r#"{"path":"README.md"}"#.to_owned(),
    };
    append(
        &mut journal,
        4,
        JournalEntryPayload::ToolCallRequested {
            agent_id: agent_id.clone(),
            item_id: id("tool-item", TranscriptItemId::new),
            call: call.clone(),
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
    assert_eq!(
        projection.request().items,
        [
            RequestItem::User {
                text: "inspect the workspace".to_owned(),
            },
            RequestItem::Assistant {
                text: "I will read it.".to_owned(),
            },
            RequestItem::ToolCall(call),
            RequestItem::ToolResult { call_id, outcome },
        ]
    );

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
    apply_all(&mut live_view, live.announce("Plexmaton").events);
    let submitted = live.handle(Input::Submitted {
        text: "hello".to_owned(),
    });
    let step_id = match submitted.effects.as_slice() {
        [Effect::CallModel(call)] => call.step_id.clone(),
        other => panic!("expected one model call, got {other:?}"),
    };
    apply_all(&mut live_view, submitted.events);
    for text in ["h", "i"] {
        let reaction = live.handle(Input::Streamed {
            step_id: step_id.clone(),
            event: ModelEvent::TextDelta(text.to_owned()),
        });
        apply_all(&mut live_view, reaction.events);
    }
    let stopped = live.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
    });
    apply_all(&mut live_view, stopped.events);

    let mut journal = SessionJournal::new(id("session-replay", SessionId::new));
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
        JournalEntryPayload::Message {
            agent_id: agent_id.clone(),
            item_id: id("agent-primary-1", TranscriptItemId::new),
            role: TranscriptRole::User,
            text: "hello".to_owned(),
        },
    );
    append(
        &mut journal,
        3,
        JournalEntryPayload::AgentStatusChanged {
            agent_id: agent_id.clone(),
            status: AgentStatus::Running,
        },
    );
    append(
        &mut journal,
        4,
        JournalEntryPayload::Message {
            agent_id: agent_id.clone(),
            item_id: id("agent-primary-2", TranscriptItemId::new),
            role: TranscriptRole::Assistant,
            text: "hi".to_owned(),
        },
    );
    append(
        &mut journal,
        5,
        JournalEntryPayload::AgentStatusChanged {
            agent_id: agent_id.clone(),
            status: AgentStatus::Idle,
        },
    );
    let projection = journal
        .project(&id("main", HeadName::new))
        .unwrap_or_else(|error| panic!("project fixture: {error:?}"));
    assert_eq!(projection.request().items, live.record());
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
}
