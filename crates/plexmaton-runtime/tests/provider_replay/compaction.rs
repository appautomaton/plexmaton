use super::{Scratch, deliver};
use plexmaton_agent::{
    Agent, AssistantBlock, AssistantOutput, AssistantReplay, CompactionAttemptFinished,
    CompactionId, CompactionInputMode, CompactionOutcome, ContextEpoch, ConversationJournal,
    DispatchedRequestTiming, ElapsedMillis, Input, JournalRecord, ModelEvent, ModelOutputPosition,
    ProviderReplay, RequestAttemptTerminal, RequestAttemptTerminalState, RequestCost,
    RequestDispatchedOutcome, StopReason, UnixMillis,
};
use plexmaton_core::{
    AgentId, ConversationEntryId, HeadName, JournalRecordId, TokenUsage, TranscriptItemId,
};
use plexmaton_file_tools::FileTools;
use plexmaton_provider::{
    FunctionTool, ModelApi, ModelRegistry, ResolvedModel, encode_request, plan_compaction,
    validate_compaction_output,
};
use plexmaton_session_store::JournalFile;

fn model(api: &str) -> ResolvedModel {
    model_with_tail(api, 16_000, 20_000)
}

fn model_with_tail(api: &str, context_window: u32, keep_recent: u32) -> ResolvedModel {
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "model" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "{api}"
[providers.fixture.models.model]
id = "fixture-model"
instructions = "Preserve the user's project constraints."
context_window_tokens = {context_window}
max_output_tokens = 2000
output_reserve_tokens = 1000
compaction_keep_recent_tokens = {keep_recent}
"#
    ))
    .expect("fixture registry")
    .active_model()
    .clone()
}

fn native_tools() -> Vec<FunctionTool> {
    FileTools::definitions()
        .into_iter()
        .map(|definition| {
            FunctionTool::new(
                definition.name(),
                definition.description(),
                definition.parameters().clone(),
            )
            .expect("native definition")
        })
        .collect()
}

fn answer(agent: &mut Agent, text: &str) {
    deliver(
        agent,
        ModelEvent::TextDelta {
            position: ModelOutputPosition::new(0, 0),
            delta: text.into(),
        },
    );
    deliver(agent, ModelEvent::Stopped(StopReason::EndOfTurn));
}

fn submit(agent: &mut Agent, text: &str) {
    let reaction = agent.handle_at(Input::Submitted { text: text.into() }, UnixMillis::EPOCH);
    assert!(reaction.undelivered.is_empty());
}

fn checkpoint(
    agent: &mut Agent,
    model: &ResolvedModel,
    tools: &[FunctionTool],
    ordinal: u8,
) -> ContextEpoch {
    let prepared = plan_compaction(
        agent.journal(),
        agent.selected_head(),
        model,
        tools,
        CompactionId::new(format!("compact-{ordinal}")).expect("operation"),
    )
    .expect("source can compact");
    let output = AssistantOutput::new(vec![
        AssistantBlock::Reasoning { item_id: TranscriptItemId::new(format!("summary-reasoning-{ordinal}")).expect("item"), text: "Audit reasoning stays out of agent context.".into() },
        AssistantBlock::Text { item_id: TranscriptItemId::new(format!("summary-text-{ordinal}")).expect("item"), text: format!("Checkpoint {ordinal}: preserve earlier decisions and continue the current task.") },
    ], AssistantReplay::from_positioned([(0, ProviderReplay::new(model.replay_compatibility(), "audit-only-opaque-summary-state".into()).expect("replay"))]).expect("sidecar")).expect("summary output");
    validate_compaction_output(&prepared, model, tools, &output)
        .expect("replacement fits and reduces input");
    let (attempt, _) = agent
        .authorize_compaction_attempt(prepared.plan(), UnixMillis::EPOCH)
        .expect("authorization");
    let terminal = RequestAttemptTerminal::new(
        attempt.clone(),
        RequestAttemptTerminalState::Dispatched {
            timing: DispatchedRequestTiming::new(
                UnixMillis::EPOCH,
                Some(ElapsedMillis::new(0)),
                Some(ElapsedMillis::new(1)),
                ElapsedMillis::new(2),
            )
            .expect("timing"),
            outcome: RequestDispatchedOutcome::Completed {
                stop_reason: StopReason::EndOfTurn,
            },
            usage: TokenUsage::Unavailable,
            cost: RequestCost::Unavailable,
        },
    )
    .expect("terminal");
    agent
        .finish_compaction_attempt(
            CompactionAttemptFinished::new(
                terminal,
                CompactionInputMode::Verbatim,
                CompactionOutcome::Complete { output },
            )
            .expect("attempt fact"),
        )
        .expect("audit");
    agent
        .commit_compaction_checkpoint(prepared.plan().clone(), attempt)
        .expect("checkpoint");
    agent
        .journal()
        .project(agent.selected_head())
        .expect("projection")
        .context_epoch()
        .clone()
}

fn bytes(
    journal: &ConversationJournal,
    head: &HeadName,
    model: &ResolvedModel,
    tools: &[FunctionTool],
) -> Vec<u8> {
    let projection = journal.project(head).expect("selected projection");
    serde_json::to_vec(
        &encode_request(
            model,
            projection.request(),
            tools,
            Some(model.max_output_tokens()),
        )
        .expect("wire request"),
    )
    .expect("wire bytes")
}

fn fork(file: &mut JournalFile, name: &str, target: ConversationEntryId) -> HeadName {
    let head = HeadName::new(name).expect("fresh head");
    file.append(JournalRecord::CreateHead {
        sequence: file.journal().next_sequence(),
        record_id: JournalRecordId::new(format!("fork-{name}")).expect("record"),
        head: head.clone(),
        at: Some(target),
    })
    .expect("historical fork");
    head
}

/// CPL-4/CPL-5/PRV-4: repeated checkpoints and historical forks reconstruct exact wire bytes
/// from a real JSONL file, and the next request extends the persisted epoch without summary replay.
#[test]
fn cpl_5_repeated_checkpoints_and_historical_forks_reopen_with_identical_wire_bytes() {
    let tools = native_tools();
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        let model = model(api);
        let mut agent = Agent::new(AgentId::new("primary").expect("agent"));
        agent.announce("Plexmaton");
        submit(&mut agent, "Inspect the earlier implementation.");
        answer(&mut agent, &"Earlier implementation details. ".repeat(900));
        let main = agent.selected_head().clone();
        let original_target = agent
            .journal()
            .head_target(&main)
            .expect("head")
            .expect("entry")
            .clone();
        let original_bytes = bytes(agent.journal(), &main, &model, &tools);
        submit(&mut agent, "Preserve the current request exactly.");
        let first_epoch = checkpoint(&mut agent, &model, &tools, 1);
        answer(&mut agent, "First continuation.");
        let between_target = agent
            .journal()
            .head_target(&main)
            .expect("head")
            .expect("entry")
            .clone();
        let between_bytes = bytes(agent.journal(), &main, &model, &tools);
        submit(&mut agent, "Inspect the remaining implementation.");
        answer(&mut agent, &"Later implementation details. ".repeat(900));
        submit(&mut agent, "Continue after the second checkpoint.");
        let second_epoch = checkpoint(&mut agent, &model, &tools, 2);
        assert_ne!(second_epoch, first_epoch);
        answer(&mut agent, "Second continuation.");
        let final_bytes = bytes(agent.journal(), &main, &model, &tools);
        assert!(!String::from_utf8_lossy(&final_bytes).contains("audit-only-opaque-summary-state"));
        assert!(!String::from_utf8_lossy(&final_bytes).contains("Audit reasoning"));
        let scratch = Scratch::new(api);
        let path = scratch.0.join("compaction.jsonl");
        let mut file = JournalFile::create(
            &path,
            agent.journal().conversation_id().clone(),
            UnixMillis::EPOCH,
        )
        .expect("journal");
        for record in agent.journal().records() {
            file.append(record.clone()).expect("append journal fact");
        }
        let original_head = fork(&mut file, "before-first", original_target);
        let between_head = fork(&mut file, "between-checkpoints", between_target);
        let original_main = file.journal().head_target(&main).expect("head").cloned();
        drop(file);
        let reopened = JournalFile::open(&path).expect("reopen");
        assert_eq!(
            reopened.journal().head_target(&main).expect("head"),
            original_main.as_ref()
        );
        for (head, expected, epoch) in [
            (&main, &final_bytes, &second_epoch),
            (&original_head, &original_bytes, &ContextEpoch::Original),
            (&between_head, &between_bytes, &first_epoch),
        ] {
            assert_eq!(
                &bytes(reopened.journal(), head, &model, &tools),
                expected,
                "{api}: {head}"
            );
            assert_eq!(
                reopened
                    .journal()
                    .project(head)
                    .expect("head projection")
                    .context_epoch(),
                epoch
            );
        }
        let mut resumed = Agent::from_journal(
            AgentId::new("primary").expect("agent"),
            reopened.journal().clone(),
            plexmaton_agent::TurnBudget::default(),
            plexmaton_agent::ApprovalPolicy::default(),
        )
        .expect("resume agent");
        submit(&mut resumed, "Next request.");
        let next: serde_json::Value =
            serde_json::from_slice(&bytes(resumed.journal(), &main, &model, &tools))
                .expect("next JSON");
        let prior: serde_json::Value = serde_json::from_slice(&final_bytes).expect("prior JSON");
        let key = match model.api() {
            ModelApi::OpenaiResponses => "input",
            ModelApi::OpenaiChatCompletions | ModelApi::AnthropicMessages => "messages",
            ModelApi::GoogleGenerateContent => "contents",
        };
        let prefix = prior[key].as_array().expect("prior input");
        assert_eq!(
            &next[key].as_array().expect("next input")[..prefix.len()],
            prefix,
            "{api}: epoch prefix"
        );
    }
}

/// CPL-5: a new retention setting affects future plans, never the persisted checkpoint base.
/// The fixture window admits distinct 20k and 25k cuts, so clamping cannot mask a replay bug.
#[test]
fn cpl_5_retention_config_change_preserves_checkpoint_and_continuation_bytes() {
    let tools = native_tools();
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        let twenty = model_with_tail(api, 128_000, 20_000);
        let twenty_five = model_with_tail(api, 128_000, 25_000);
        let mut agent = Agent::new(AgentId::new("primary").expect("agent"));
        agent.announce("Plexmaton");
        for index in 0..32 {
            submit(&mut agent, &format!("Inspect component {index}."));
            answer(
                &mut agent,
                &format!("Component {index}: {}", "detail ".repeat(720)),
            );
        }
        submit(
            &mut agent,
            "Preserve the decisions and continue implementation.",
        );
        let main = agent.selected_head().clone();
        let cuts: Vec<_> = [&twenty, &twenty_five]
            .into_iter()
            .map(|model| {
                plan_compaction(
                    agent.journal(),
                    &main,
                    model,
                    &tools,
                    CompactionId::new("verify-effective-setting").expect("operation"),
                )
                .expect("source fits both settings")
                .plan()
                .cut()
                .clone()
            })
            .collect();
        assert_ne!(
            cuts[0], cuts[1],
            "{api}: this setting must affect fresh planning"
        );

        let epoch = checkpoint(&mut agent, &twenty, &tools, 1);
        let checkpoint_base = agent
            .journal()
            .project(&main)
            .expect("checkpoint")
            .into_request();
        answer(&mut agent, "Continued from the checkpoint.");
        submit(&mut agent, "Now check the remaining work.");
        answer(&mut agent, "The remaining work is identified.");
        let expected_history = bytes(agent.journal(), &main, &twenty, &tools);
        let expected_journal = agent.journal().clone();

        let scratch = Scratch::new(&format!("retention-change-{api}"));
        let path = scratch.0.join("session.jsonl");
        let mut file = JournalFile::create(
            &path,
            expected_journal.conversation_id().clone(),
            UnixMillis::EPOCH,
        )
        .expect("journal");
        for record in expected_journal.records() {
            file.append(record.clone()).expect("persist immutable fact");
        }
        drop(file);
        let reopened = JournalFile::open(&path).expect("reopen under changed configuration");
        assert_eq!(reopened.journal(), &expected_journal);
        assert_eq!(
            bytes(reopened.journal(), &main, &twenty_five, &tools),
            expected_history,
            "{api}: 25k must not reselect the existing 20k checkpoint tail"
        );

        let mut resumed = Agent::from_journal(
            AgentId::new("primary").expect("agent"),
            reopened.journal().clone(),
            plexmaton_agent::TurnBudget::default(),
            plexmaton_agent::ApprovalPolicy::default(),
        )
        .expect("resumed agent");
        submit(&mut agent, "Next user message.");
        submit(&mut resumed, "Next user message.");
        assert_eq!(
            bytes(resumed.journal(), &main, &twenty_five, &tools),
            bytes(agent.journal(), &main, &twenty, &tools),
            "{api}: continuation request bytes"
        );
        let continued = resumed
            .journal()
            .project(&main)
            .expect("continued projection");
        assert_eq!(continued.context_epoch(), &epoch);
        assert_eq!(
            &continued.request().atoms[..checkpoint_base.atoms.len()],
            checkpoint_base.atoms.as_slice(),
            "{api}: original checkpoint base is still the prefix"
        );
    }
}
