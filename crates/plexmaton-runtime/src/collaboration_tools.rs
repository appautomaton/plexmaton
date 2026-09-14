//! Provider-neutral collaboration tool schemas and authority-free arguments.

use std::collections::BTreeSet;

use plexmaton_agent::collaboration::{
    CollaborationText, MAX_COLLABORATION_ID_BYTES, MAX_COLLABORATION_TEXT_BYTES, MAX_MAIL_ARTIFACTS,
};
use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmissionRequest, AdmittedToolCall,
    MAX_REQUESTED_TOOL_ARGUMENT_BYTES, ToolDefinitionRevision,
};
use plexmaton_core::{ToolCapability, ToolDefinitionId};
use plexmaton_provider::{FunctionTool, FunctionToolError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

/// Model-visible Main tool for creating one delegated task.
pub const DELEGATE_TOOL_NAME: &str = "delegate";
/// Model-visible typed mail tool shared by Main and delegated children.
pub const SEND_MAIL_TOOL_NAME: &str = "send_mail";
/// Model-visible Main tool for replacing a delegated task.
pub const UPDATE_TASK_TOOL_NAME: &str = "update_task";
/// Model-visible Main tool for explicit one-way control transfer.
pub const HANDOFF_TOOL_NAME: &str = "handoff";

/// Stable trusted definition identity for `delegate`.
pub const DELEGATE_DEFINITION_ID: &str = "native.collaboration.delegate.v1";
/// Stable trusted definition identity for Main-authored `send_mail`.
pub const SEND_MAIL_MAIN_DEFINITION_ID: &str = "native.collaboration.send-mail-main.v1";
/// Stable trusted definition identity for child-authored `send_mail`.
pub const SEND_MAIL_CHILD_DEFINITION_ID: &str = "native.collaboration.send-mail-child.v1";
/// Stable trusted definition identity for `update_task`.
pub const UPDATE_TASK_DEFINITION_ID: &str = "native.collaboration.update-task.v1";
/// Stable trusted definition identity for `handoff`.
pub const HANDOFF_DEFINITION_ID: &str = "native.collaboration.handoff.v1";

/// Collaboration definitions available to one authenticated runtime role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollaborationToolScope {
    Main,
    Child,
}

mod selectors;
pub use selectors::{ArtifactSelector, TargetSelector};

/// Main intent to create one delegated task using owner-selected execution configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DelegateIntent {
    pub task: CollaborationText,
}

/// Main intent to send typed mail to one owner-resolved child.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MainMailIntent {
    pub target: TargetSelector,
    pub summary: CollaborationText,
    pub artifacts: Vec<ArtifactSelector>,
}

/// Child intent to send typed mail to its fixed authenticated delegator.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChildMailIntent {
    pub summary: CollaborationText,
    pub artifacts: Vec<ArtifactSelector>,
}

/// Main intent to replace the effective task of one owner-resolved delegation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UpdateTaskIntent {
    pub target: TargetSelector,
    pub task: CollaborationText,
}

/// Main intent to transfer one owner-resolved child after an owner-derived quiescence check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HandoffIntent {
    pub target: TargetSelector,
}

/// Parsed model intent. Actor, endpoint, durable and retry identities are absent by construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollaborationToolRequest {
    Delegate(DelegateIntent),
    MainMail(MainMailIntent),
    ChildMail(ChildMailIntent),
    UpdateTask(UpdateTaskIntent),
    Handoff(HandoffIntent),
}

/// Strict collaboration argument refusal before any owner capability is consulted.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CollaborationToolArgumentError {
    #[error("collaboration tool arguments exceed the retained call bound")]
    TooLarge,
    #[error("collaboration tool is unavailable in this runtime role")]
    Unavailable,
    #[error("collaboration tool arguments do not match the exact schema")]
    InvalidArguments,
    #[error("collaboration text is empty or exceeds its UTF-8 byte bound")]
    InvalidText,
    #[error("collaboration selector is empty or exceeds its UTF-8 byte bound")]
    InvalidSelector,
    #[error("mail contains too many artifact selectors")]
    TooManyArtifacts,
    #[error("mail repeats an artifact selector")]
    DuplicateArtifact,
}

/// Exact model-visible definitions for Main or a read-only delegated child (CTL-1/CTL-2).
pub fn collaboration_tool_definitions(
    scope: CollaborationToolScope,
) -> Result<Vec<FunctionTool>, FunctionToolError> {
    let specs = match scope {
        CollaborationToolScope::Main => vec![
            (
                DELEGATE_TOOL_NAME,
                "Delegate one bounded task to a read-only child. The runtime selects and authenticates the child.",
                delegate_schema(),
            ),
            (
                SEND_MAIL_TOOL_NAME,
                "Send an attributed typed summary to one owner-issued child target. Artifact selectors are resolved in the sender's conversation.",
                main_mail_schema(),
            ),
            (
                UPDATE_TASK_TOOL_NAME,
                "Replace one Main-controlled delegated task selected through an owner-issued target.",
                task_update_schema(),
            ),
            (
                HANDOFF_TOOL_NAME,
                "Transfer one quiescent delegated conversation selected through an owner-issued target from Main control to the user.",
                handoff_schema(),
            ),
        ],
        CollaborationToolScope::Child => vec![(
            SEND_MAIL_TOOL_NAME,
            "Send an attributed typed summary to this child's fixed delegator. Artifact selectors are resolved in this child conversation.",
            child_mail_schema(),
        )],
    };
    specs
        .into_iter()
        .map(|(name, description, schema)| FunctionTool::new(name, description, schema))
        .collect()
}

/// Parses exact arguments without accepting serialized authority or retry identity (CTL-1/CTL-2).
pub fn parse_collaboration_tool(
    scope: CollaborationToolScope,
    name: &str,
    arguments: &str,
) -> Result<CollaborationToolRequest, CollaborationToolArgumentError> {
    if arguments.len() > MAX_REQUESTED_TOOL_ARGUMENT_BYTES {
        return Err(CollaborationToolArgumentError::TooLarge);
    }
    match (scope, name) {
        (CollaborationToolScope::Main, DELEGATE_TOOL_NAME) => {
            let args: DelegateArguments = decode(arguments)?;
            Ok(CollaborationToolRequest::Delegate(DelegateIntent {
                task: text(args.task)?,
            }))
        }
        (CollaborationToolScope::Main, SEND_MAIL_TOOL_NAME) => {
            let args: MainMailArguments = decode(arguments)?;
            Ok(CollaborationToolRequest::MainMail(MainMailIntent {
                target: TargetSelector::new(args.target)?,
                summary: text(args.summary)?,
                artifacts: artifacts(args.artifacts)?,
            }))
        }
        (CollaborationToolScope::Child, SEND_MAIL_TOOL_NAME) => {
            let args: ChildMailArguments = decode(arguments)?;
            Ok(CollaborationToolRequest::ChildMail(ChildMailIntent {
                summary: text(args.summary)?,
                artifacts: artifacts(args.artifacts)?,
            }))
        }
        (CollaborationToolScope::Main, UPDATE_TASK_TOOL_NAME) => {
            let args: TaskUpdateArguments = decode(arguments)?;
            Ok(CollaborationToolRequest::UpdateTask(UpdateTaskIntent {
                target: TargetSelector::new(args.target)?,
                task: text(args.task)?,
            }))
        }
        (CollaborationToolScope::Main, HANDOFF_TOOL_NAME) => {
            let args: HandoffArguments = decode(arguments)?;
            Ok(CollaborationToolRequest::Handoff(HandoffIntent {
                target: TargetSelector::new(args.target)?,
            }))
        }
        _ => Err(CollaborationToolArgumentError::Unavailable),
    }
}

pub(crate) fn admit_collaboration_tool(
    scope: CollaborationToolScope,
    request: AdmissionRequest,
) -> AdmissionOutcome {
    let name = request.requested().name.clone();
    let intent = match parse_collaboration_tool(scope, &name, &request.requested().arguments) {
        Ok(intent) => intent,
        Err(_) => return request.refuse(AdmissionRefusal::InvalidArguments),
    };
    let Some(definition) = definition_identity(scope, &name) else {
        return request.refuse(AdmissionRefusal::UnknownTool);
    };
    let Ok(arguments) = canonical_arguments(&intent) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    let definition = ToolDefinitionId::new(definition)
        .unwrap_or_else(|_| unreachable!("static collaboration definition identity"));
    let revision = ToolDefinitionRevision::new(1)
        .unwrap_or_else(|| unreachable!("collaboration definition revision is nonzero"));
    let detail = collaboration_detail(&intent).to_owned();
    let call_id = request.requested().call_id.clone();
    request
        .admit(
            definition,
            revision,
            [ToolCapability::Collaboration],
            arguments,
            detail,
            None,
        )
        .unwrap_or(AdmissionOutcome::Refused {
            call_id,
            reason: AdmissionRefusal::InvalidArguments,
        })
}

pub(crate) fn parse_admitted_collaboration_tool(
    scope: CollaborationToolScope,
    call: &AdmittedToolCall,
) -> Result<CollaborationToolRequest, CollaborationToolArgumentError> {
    if call.definition_revision().get() != 1
        || !call
            .capabilities()
            .iter()
            .eq([ToolCapability::Collaboration])
        || definition_identity(scope, &call.requested().name) != Some(call.definition_id().as_str())
    {
        return Err(CollaborationToolArgumentError::Unavailable);
    }
    parse_collaboration_tool(scope, &call.requested().name, call.canonical_arguments())
}

fn definition_identity(scope: CollaborationToolScope, name: &str) -> Option<&'static str> {
    match (scope, name) {
        (CollaborationToolScope::Main, DELEGATE_TOOL_NAME) => Some(DELEGATE_DEFINITION_ID),
        (CollaborationToolScope::Main, SEND_MAIL_TOOL_NAME) => Some(SEND_MAIL_MAIN_DEFINITION_ID),
        (CollaborationToolScope::Child, SEND_MAIL_TOOL_NAME) => Some(SEND_MAIL_CHILD_DEFINITION_ID),
        (CollaborationToolScope::Main, UPDATE_TASK_TOOL_NAME) => Some(UPDATE_TASK_DEFINITION_ID),
        (CollaborationToolScope::Main, HANDOFF_TOOL_NAME) => Some(HANDOFF_DEFINITION_ID),
        _ => None,
    }
}

pub(crate) fn is_collaboration_tool_name(name: &str) -> bool {
    matches!(
        name,
        DELEGATE_TOOL_NAME | SEND_MAIL_TOOL_NAME | UPDATE_TASK_TOOL_NAME | HANDOFF_TOOL_NAME
    )
}

pub(crate) fn is_collaboration_call(call: &AdmittedToolCall) -> bool {
    is_collaboration_tool_name(&call.requested().name)
        || matches!(
            call.definition_id().as_str(),
            DELEGATE_DEFINITION_ID
                | SEND_MAIL_MAIN_DEFINITION_ID
                | SEND_MAIL_CHILD_DEFINITION_ID
                | UPDATE_TASK_DEFINITION_ID
                | HANDOFF_DEFINITION_ID
        )
        || call
            .capabilities()
            .iter()
            .any(|capability| capability == ToolCapability::Collaboration)
}

fn canonical_arguments(intent: &CollaborationToolRequest) -> Result<String, serde_json::Error> {
    match intent {
        CollaborationToolRequest::Delegate(intent) => serde_json::to_string(intent),
        CollaborationToolRequest::MainMail(intent) => serde_json::to_string(intent),
        CollaborationToolRequest::ChildMail(intent) => serde_json::to_string(intent),
        CollaborationToolRequest::UpdateTask(intent) => serde_json::to_string(intent),
        CollaborationToolRequest::Handoff(intent) => serde_json::to_string(intent),
    }
}

fn collaboration_detail(intent: &CollaborationToolRequest) -> &'static str {
    match intent {
        CollaborationToolRequest::Delegate(_) => "delegate one child task",
        CollaborationToolRequest::MainMail(_) => "send typed mail to one child",
        CollaborationToolRequest::ChildMail(_) => "send typed mail to the delegator",
        CollaborationToolRequest::UpdateTask(_) => "update one delegated task",
        CollaborationToolRequest::Handoff(_) => "hand off one delegated conversation",
    }
}

fn delegate_schema() -> Value {
    object_schema(
        json!({
            "task": text_schema("The bounded task for the child.")
        }),
        &["task"],
    )
}

fn main_mail_schema() -> Value {
    object_schema(
        json!({
            "target": selector_schema("Owner-issued handle for the receiving delegated task."),
            "summary": text_schema("Bounded typed summary."),
            "artifacts": artifact_schema()
        }),
        &["target", "summary", "artifacts"],
    )
}

fn child_mail_schema() -> Value {
    object_schema(
        json!({
            "summary": text_schema("Bounded typed summary for this child's delegator."),
            "artifacts": artifact_schema()
        }),
        &["summary", "artifacts"],
    )
}

fn task_update_schema() -> Value {
    object_schema(
        json!({
            "target": selector_schema("Owner-issued handle for one Main-controlled delegated task."),
            "task": text_schema("Replacement bounded task.")
        }),
        &["target", "task"],
    )
}

fn handoff_schema() -> Value {
    object_schema(
        json!({
            "target": selector_schema("Owner-issued handle for the Main-controlled delegated task to transfer.")
        }),
        &["target"],
    )
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn text_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAX_COLLABORATION_TEXT_BYTES,
        "description": description
    })
}

fn selector_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAX_COLLABORATION_ID_BYTES,
        "description": description
    })
}

fn artifact_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAX_MAIL_ARTIFACTS,
        "items": selector_schema("Owner-issued artifact handle in the sender's conversation.")
    })
}

fn decode<T: for<'de> Deserialize<'de>>(
    arguments: &str,
) -> Result<T, CollaborationToolArgumentError> {
    serde_json::from_str(arguments).map_err(|_| CollaborationToolArgumentError::InvalidArguments)
}

fn text(value: String) -> Result<CollaborationText, CollaborationToolArgumentError> {
    CollaborationText::new(value).map_err(|_| CollaborationToolArgumentError::InvalidText)
}

fn artifacts(values: Vec<String>) -> Result<Vec<ArtifactSelector>, CollaborationToolArgumentError> {
    if values.len() > MAX_MAIL_ARTIFACTS {
        return Err(CollaborationToolArgumentError::TooManyArtifacts);
    }
    let mut seen = BTreeSet::new();
    let mut artifacts = Vec::with_capacity(values.len());
    for value in values {
        let artifact = ArtifactSelector::new(value)?;
        if !seen.insert(artifact.clone()) {
            return Err(CollaborationToolArgumentError::DuplicateArtifact);
        }
        artifacts.push(artifact);
    }
    Ok(artifacts)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DelegateArguments {
    task: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MainMailArguments {
    target: String,
    summary: String,
    artifacts: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildMailArguments {
    summary: String,
    artifacts: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskUpdateArguments {
    target: String,
    task: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffArguments {
    target: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexmaton_agent::{
        Agent, Effect, Input, ModelEvent, ModelOutputPosition, StopReason, ToolCall, UnixMillis,
    };
    use plexmaton_core::{AgentId, ToolCallId};

    const TARGET: &str =
        "target-v1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ARTIFACT: &str =
        "artifact-v1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn admission_request(name: &str, arguments: &str) -> AdmissionRequest {
        let mut agent = Agent::new(AgentId::new("main").expect("agent"));
        agent.handle_at(
            Input::Submitted {
                text: "coordinate the work".into(),
            },
            UnixMillis::EPOCH,
        );
        let step_id = agent.active_model_step().expect("active step");
        agent.handle_at(
            Input::Streamed {
                step_id: step_id.clone(),
                event: ModelEvent::Called {
                    position: ModelOutputPosition::new(0, 0),
                    call: ToolCall {
                        call_id: ToolCallId::new("call").expect("call"),
                        name: name.into(),
                        arguments: arguments.into(),
                    },
                },
            },
            UnixMillis::EPOCH,
        );
        let stopped = agent.handle_at(
            Input::Streamed {
                step_id,
                event: ModelEvent::Stopped(StopReason::ToolCalls),
            },
            UnixMillis::EPOCH,
        );
        let effects: [Effect; 1] = stopped.effects.try_into().expect("one effect");
        let [Effect::AdmitTool(request)] = effects else {
            panic!("expected one admission request")
        };
        request
    }

    /// CTL-1/CTL-2: schemas are exact, role-scoped and expose no serialized authority fields.
    #[test]
    fn ctl_1_collaboration_tool_schemas_are_exact_scoped_and_authority_free() {
        let main = collaboration_tool_definitions(CollaborationToolScope::Main)
            .expect("valid Main definitions");
        let child = collaboration_tool_definitions(CollaborationToolScope::Child)
            .expect("valid child definitions");
        assert_eq!(main.len(), 4);
        assert_eq!(child.len(), 1);
        let names = [
            DELEGATE_TOOL_NAME,
            SEND_MAIL_TOOL_NAME,
            UPDATE_TASK_TOOL_NAME,
            HANDOFF_TOOL_NAME,
        ];
        assert_eq!(names.into_iter().collect::<BTreeSet<_>>().len(), main.len());
        for schema in [
            delegate_schema(),
            main_mail_schema(),
            child_mail_schema(),
            task_update_schema(),
            handoff_schema(),
        ] {
            let properties = schema["properties"].as_object().expect("properties");
            for forbidden in [
                "author",
                "from",
                "to",
                "item_id",
                "mail_id",
                "conversation",
                "delegation",
                "expected_revision",
            ] {
                assert!(
                    !properties.contains_key(forbidden),
                    "{forbidden} entered schema"
                );
            }
            assert_eq!(schema["additionalProperties"], false);
        }
        assert_eq!(
            main_mail_schema()["required"],
            json!(["target", "summary", "artifacts"])
        );
        assert_eq!(
            child_mail_schema()["required"],
            json!(["summary", "artifacts"])
        );
        assert_eq!(task_update_schema()["required"], json!(["target", "task"]));
        assert_eq!(handoff_schema()["required"], json!(["target"]));
    }

    /// CTL-1: extra authority, malformed bounds and duplicate pointers fail before owner ingress.
    #[test]
    fn ctl_1_arguments_cannot_supply_authority_or_escape_semantic_bounds() {
        assert_eq!(
            parse_collaboration_tool(
                CollaborationToolScope::Main,
                DELEGATE_TOOL_NAME,
                r#"{"task":"inspect","author":"forged"}"#,
            ),
            Err(CollaborationToolArgumentError::InvalidArguments)
        );
        assert_eq!(
            parse_collaboration_tool(
                CollaborationToolScope::Main,
                UPDATE_TASK_TOOL_NAME,
                r#"{"target":"child-1","expected_revision":7,"task":"continue"}"#,
            ),
            Err(CollaborationToolArgumentError::InvalidArguments)
        );
        assert_eq!(
            parse_collaboration_tool(
                CollaborationToolScope::Main,
                HANDOFF_TOOL_NAME,
                r#"{"delegation":"durable-id"}"#,
            ),
            Err(CollaborationToolArgumentError::InvalidArguments)
        );
        assert_eq!(
            parse_collaboration_tool(
                CollaborationToolScope::Child,
                SEND_MAIL_TOOL_NAME,
                &json!({"summary": "found it", "artifacts": [ARTIFACT, ARTIFACT]}).to_string(),
            ),
            Err(CollaborationToolArgumentError::DuplicateArtifact)
        );
        assert_eq!(
            parse_collaboration_tool(
                CollaborationToolScope::Child,
                HANDOFF_TOOL_NAME,
                r#"{"target":"task"}"#,
            ),
            Err(CollaborationToolArgumentError::Unavailable)
        );
        let oversized = "界".repeat(MAX_COLLABORATION_TEXT_BYTES / 3 + 1);
        assert!(oversized.len() > MAX_COLLABORATION_TEXT_BYTES);
        assert_eq!(
            parse_collaboration_tool(
                CollaborationToolScope::Main,
                DELEGATE_TOOL_NAME,
                &json!({"task": oversized}).to_string(),
            ),
            Err(CollaborationToolArgumentError::InvalidText)
        );
        let oversized_selector = "界".repeat(MAX_COLLABORATION_ID_BYTES / 3 + 1);
        assert_eq!(
            TargetSelector::new(oversized_selector),
            Err(CollaborationToolArgumentError::InvalidSelector)
        );
        assert_eq!(
            ArtifactSelector::new(" \n\t "),
            Err(CollaborationToolArgumentError::InvalidSelector)
        );
        let raw_oversized = " ".repeat(MAX_REQUESTED_TOOL_ARGUMENT_BYTES + 1);
        assert_eq!(
            parse_collaboration_tool(
                CollaborationToolScope::Main,
                DELEGATE_TOOL_NAME,
                &raw_oversized,
            ),
            Err(CollaborationToolArgumentError::TooLarge)
        );
        let too_many_artifacts = (0..=MAX_MAIL_ARTIFACTS)
            .map(|index| format!("artifact-{index}"))
            .collect::<Vec<_>>();
        assert_eq!(
            parse_collaboration_tool(
                CollaborationToolScope::Child,
                SEND_MAIL_TOOL_NAME,
                &json!({
                    "summary": "bounded",
                    "artifacts": too_many_artifacts,
                })
                .to_string(),
            ),
            Err(CollaborationToolArgumentError::TooManyArtifacts)
        );
    }

    /// CTL-2: Main selects a target while a child can mail only its fixed delegator.
    #[test]
    fn ctl_2_mail_target_is_role_derived_and_typed() {
        let main = parse_collaboration_tool(
            CollaborationToolScope::Main,
            SEND_MAIL_TOOL_NAME,
            &json!({"target": TARGET, "summary": "continue", "artifacts": []}).to_string(),
        )
        .expect("valid Main mail intent");
        let child = parse_collaboration_tool(
            CollaborationToolScope::Child,
            SEND_MAIL_TOOL_NAME,
            r#"{"summary":"done","artifacts":[]}"#,
        )
        .expect("valid child mail intent");
        assert!(matches!(main, CollaborationToolRequest::MainMail(_)));
        assert!(matches!(child, CollaborationToolRequest::ChildMail(_)));
    }

    #[test]
    fn collaboration_definition_ids_remain_unique_and_role_specific() {
        let ids = [
            DELEGATE_DEFINITION_ID,
            SEND_MAIL_MAIN_DEFINITION_ID,
            SEND_MAIL_CHILD_DEFINITION_ID,
            UPDATE_TASK_DEFINITION_ID,
            HANDOFF_DEFINITION_ID,
        ];
        assert_eq!(ids.into_iter().collect::<BTreeSet<_>>().len(), ids.len());
    }

    /// CTL-1/CTL-2: admission freezes role-specific identity and execution rechecks every fact.
    #[test]
    fn ctl_1_admission_canonicalizes_and_rechecks_role_definition_and_capability() {
        let AdmissionOutcome::Admitted(main) = admit_collaboration_tool(
            CollaborationToolScope::Main,
            admission_request(
                SEND_MAIL_TOOL_NAME,
                &json!({"summary": "continue", "artifacts": [], "target": TARGET}).to_string(),
            ),
        ) else {
            panic!("Main mail admitted")
        };
        assert_eq!(main.definition_id().as_str(), SEND_MAIL_MAIN_DEFINITION_ID);
        assert_eq!(main.definition_revision().get(), 1);
        assert!(
            main.capabilities()
                .iter()
                .eq([ToolCapability::Collaboration])
        );
        assert_eq!(
            main.canonical_arguments(),
            format!(r#"{{"target":"{TARGET}","summary":"continue","artifacts":[]}}"#)
        );
        assert!(matches!(
            parse_admitted_collaboration_tool(CollaborationToolScope::Main, &main),
            Ok(CollaborationToolRequest::MainMail(_))
        ));
        assert_eq!(
            parse_admitted_collaboration_tool(CollaborationToolScope::Child, &main),
            Err(CollaborationToolArgumentError::Unavailable)
        );

        let AdmissionOutcome::Admitted(forged) = admission_request(
            SEND_MAIL_TOOL_NAME,
            r#"{"summary":"continue","artifacts":[]}"#,
        )
        .admit(
            ToolDefinitionId::new(SEND_MAIL_CHILD_DEFINITION_ID).expect("definition"),
            ToolDefinitionRevision::new(1).expect("revision"),
            [ToolCapability::FileRead],
            r#"{"summary":"continue","artifacts":[]}"#.into(),
            "forged child mail".into(),
            None,
        )
        .expect("bounded fixture") else {
            panic!("fixture admitted")
        };
        assert_eq!(
            parse_admitted_collaboration_tool(CollaborationToolScope::Child, &forged),
            Err(CollaborationToolArgumentError::Unavailable)
        );
    }
}
