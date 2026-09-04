use plexmaton_agent::{
    Agent, ApprovalPolicy, Input, JournalRecord, ModelEvent, RequestItem, SessionMetadata,
    StopReason, TurnBudget, TurnFinishedAt, UnixMillis,
};
use plexmaton_core::{AgentId, HeadName};
use plexmaton_session_store::JournalFile;

use super::support::{TestDir, id, session};

/// TIM-1/JRN-3/JRN-4: JSONL retains exact chronology while replay rebuilds semantic context.
#[test]
fn tim_1_turn_chronology_reopens_from_jsonl_without_entering_model_context() {
    let directory = TestDir::new("turn-chronology");
    let path = directory.path().join("session.jsonl");
    let session_id = session("timed-session");
    let agent_id = id("agent-a", AgentId::new);
    let mut file = JournalFile::create(&path, session_id.clone(), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create store: {error}"));
    let mut agent = Agent::for_session(
        agent_id,
        SessionMetadata::new(session_id, UnixMillis::EPOCH),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    );
    let mut reactions = vec![agent.announce("Plexmaton")];
    reactions.push(agent.handle_at(
        Input::Submitted {
            text: "remember this".to_owned(),
        },
        UnixMillis::new(1_788_000_000_111),
    ));
    let step_id = agent
        .active_model_step()
        .unwrap_or_else(|| panic!("submission did not open a model step"));
    reactions.push(agent.handle_at(
        Input::Streamed {
            step_id,
            event: ModelEvent::Stopped(StopReason::EndOfTurn),
        },
        UnixMillis::new(1_788_000_000_444),
    ));
    for record in reactions.into_iter().flat_map(|reaction| reaction.records) {
        file.append(record)
            .unwrap_or_else(|failure| panic!("append timed record: {failure:?}"));
    }
    drop(file);

    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read timed journal: {error}"));
    assert!(source.contains(r#""accepted_at":1788000000111"#));
    assert!(source.contains(r#""kind":"turn_finished""#));
    let reopened =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("reopen timed journal: {error}"));
    let finished = reopened
        .journal()
        .records()
        .iter()
        .find_map(|record| match record {
            JournalRecord::TurnFinished { fact, .. } => Some(fact),
            _ => None,
        });
    assert!(matches!(
        finished,
        Some(fact)
            if fact.at == TurnFinishedAt::Observed {
                completed_at: UnixMillis::new(1_788_000_000_444)
            }
    ));
    let projection = reopened
        .journal()
        .project(&id("main", HeadName::new))
        .unwrap_or_else(|error| panic!("project timed journal: {error:?}"));
    assert_eq!(
        projection.request().items,
        [RequestItem::User {
            text: "remember this".to_owned()
        }]
    );
}
