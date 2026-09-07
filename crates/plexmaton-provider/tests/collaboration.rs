//! Collaboration is an explicit unsupported representation until each wire contract is defined.

use plexmaton_agent::collaboration::{
    CollaborationEvent, CollaborationLedger, CollaborationLimits, CollaborationText, MailEndpoint,
    Preparation, TurnBoundary,
};
use plexmaton_agent::{ContextAtom, HeadRevision, ModelRequest};
use plexmaton_core::{
    AgentId, CollaborationId, CollaborationItemId, ConversationEntryId, ConversationId,
    DelegationId, HeadName, TurnId,
};
use plexmaton_provider::{
    ContextBudgetError, EncodeError, ModelRegistry, ResolvedModel, encode_request, estimate_request,
};

fn model(api: &str) -> ResolvedModel {
    ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "test" }}
[providers.fixture]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
api = "{api}"
[providers.fixture.models.test]
id = "fixture-model"
context_window_tokens = 10000
max_output_tokens = 2000
output_reserve_tokens = 1000
"#
    ))
    .expect("model")
    .active_model()
    .clone()
}

fn endpoint(name: &str) -> MailEndpoint {
    MailEndpoint {
        conversation: ConversationId::new(format!("session-{name}")).expect("id"),
        agent: AgentId::new(name).expect("id"),
    }
}

/// CIN-3: both projection states refuse through every public codec/budget path, with no user fallback.
#[test]
fn cin_3_all_codecs_refuse_collaboration_context_explicitly() {
    let mut ledger = CollaborationLedger::new(
        CollaborationId::new("collaboration").expect("id"),
        CollaborationLimits::default(),
    )
    .expect("ledger");
    let Preparation::Append(record) = ledger
        .prepare(
            CollaborationItemId::new("create").expect("id"),
            CollaborationEvent::DelegationCreated {
                delegation: DelegationId::new("task").expect("id"),
                delegator: endpoint("a"),
                worker: endpoint("b"),
                task: CollaborationText::new("Read tests").expect("text"),
            },
        )
        .expect("create")
    else {
        panic!("new record")
    };
    ledger.apply(*record).expect("apply");
    let id = CollaborationItemId::new("admit").expect("id");
    let Preparation::Append(record) = ledger
        .prepare_turn(
            id.clone(),
            TurnBoundary {
                recipient: endpoint("b"),
                head: HeadName::new("main").expect("head"),
                head_revision: HeadRevision::new(1),
                parent: Some(ConversationEntryId::new("announced").expect("id")),
                turn: TurnId::new("turn").expect("id"),
            },
            None,
        )
        .expect("prepare turn")
    else {
        panic!("new turn")
    };
    ledger.apply(*record).expect("admit");
    let reference = ledger.item_reference(&id).expect("reference");
    let source = ledger.resolve_turn(&reference).expect("resolved source");
    let unresolved = ContextAtom::collaboration(
        ConversationEntryId::new("inclusion").expect("id"),
        reference,
    );
    let mut resolved = unresolved.clone();
    resolved.resolve_collaboration(source).expect("materialize");
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        let model = model(api);
        for atom in [&unresolved, &resolved] {
            let request = ModelRequest {
                session_id: endpoint("b").conversation,
                atoms: vec![atom.clone()],
            };
            let result = encode_request(&model, &request, &[], Some(model.max_output_tokens()));
            assert!(
                matches!(result, Err(EncodeError::UnsupportedCollaboration)),
                "{api}: {result:?}"
            );
            let result = estimate_request(&model, &request, &[]);
            assert!(
                matches!(
                    result,
                    Err(ContextBudgetError::Encoding(
                        EncodeError::UnsupportedCollaboration
                    ))
                ),
                "{api}: {result:?}"
            );
        }
    }
}
