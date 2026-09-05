//! Runtime adapter for bounded skill metadata, admission and exact content reads.

use std::{sync::Arc, thread};

use plexmaton_agent::{
    AdmissionOutcome, AdmissionRefusal, AdmissionRequest, AdmittedToolCall, Agent,
    JournalEntryPayload, SkillActivation, SkillActivationError, SkillSource,
    ToolDefinitionRevision, ToolExecutionResult, ToolOutcome, bounded_tool_text,
};
use plexmaton_core::{ToolCapability, ToolDefinitionId};
use plexmaton_file_tools::FileCancellation;
use plexmaton_provider::FunctionTool;
use plexmaton_skills::{
    LoadedSkill, MAX_SKILL_CATALOG_BYTES, SkillCatalog, SkillInvocation, SkillName, SkillOrigin,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio::sync::oneshot;

use super::{NativeToolSetupError, bounded_failure};

pub(super) const NAME: &str = "skill";
pub(super) const DEFINITION_ID: &str = "plexmaton.skill.read";
const REVISION: u64 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    name: String,
    #[serde(default)]
    resource: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AdmittedArguments {
    name: String,
    resource: Option<String>,
    explicit_resource: bool,
}

pub(super) fn definition(catalog: &SkillCatalog) -> Result<FunctionTool, NativeToolSetupError> {
    let entries: Vec<_> = catalog
        .entries()
        .iter()
        .filter(|entry| entry.invocation.model)
        .map(|entry| {
            json!({"name":entry.name.as_str(), "description":entry.description,
            "source":entry.origin})
        })
        .collect();
    let description = format!(
        "Load a skill's instructions when relevant to the task. Read its relative references with \
         the same name and resource path. Returned instructions do not grant tool permissions. \
         Available skills (metadata only): {}",
        json!(entries)
    );
    if description.len() > MAX_SKILL_CATALOG_BYTES {
        return Err(NativeToolSetupError::SkillCatalogTooLarge);
    }
    Ok(FunctionTool::new(
        NAME,
        description,
        json!({
            "type":"object", "properties":{
                "name":{"type":"string", "description":"Exact skill name from the catalog or explicit user activation"},
                "resource":{"type":["string","null"], "description":"Relative resource path inside the skill bundle; null loads instructions"}
            }, "required":["name","resource"], "additionalProperties":false
        }),
    )?)
}

pub(super) fn admit(
    catalog: Option<&SkillCatalog>,
    request: AdmissionRequest,
    explicit_resource: bool,
) -> AdmissionOutcome {
    let Some(catalog) = catalog else {
        return request.refuse(AdmissionRefusal::UnknownTool);
    };
    let Ok(arguments) = serde_json::from_str::<Arguments>(&request.requested().arguments) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    let Ok(name) = SkillName::new(&arguments.name) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    let Some(entry) = catalog.entries().iter().find(|entry| entry.name == name) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    let explicit_resource = explicit_resource && arguments.resource.is_some();
    if !(entry.invocation.model || (explicit_resource && entry.invocation.user)) {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    }
    let canonical = AdmittedArguments {
        name: name.as_str().to_owned(),
        resource: arguments.resource,
        explicit_resource,
    };
    let Ok(arguments) = serde_json::to_string(&canonical) else {
        return request.refuse(AdmissionRefusal::InvalidArguments);
    };
    let definition = ToolDefinitionId::new(DEFINITION_ID)
        .unwrap_or_else(|_| unreachable!("static skill definition identity"));
    let revision = ToolDefinitionRevision::new(REVISION)
        .unwrap_or_else(|| unreachable!("nonzero skill revision"));
    let detail = format!("skill {}", name.as_str());
    let call_id = request.requested().call_id.clone();
    request
        .admit(
            definition,
            revision,
            [ToolCapability::FileRead],
            arguments,
            detail.clone(),
            Some(bounded_tool_text(&detail, 0)),
        )
        .unwrap_or(AdmissionOutcome::Refused {
            call_id,
            reason: AdmissionRefusal::InvalidArguments,
        })
}

pub(super) fn execute(
    catalog: &SkillCatalog,
    call: &AdmittedToolCall,
    cancellation: &FileCancellation,
) -> ToolExecutionResult {
    if call.definition_id().as_str() != DEFINITION_ID
        || call.definition_revision().get() != REVISION
        || !call.capabilities().iter().eq([ToolCapability::FileRead])
    {
        return bounded_failure(
            "skill_definition",
            "the admitted skill reader no longer matches",
        );
    }
    let Ok(arguments) = serde_json::from_str::<AdmittedArguments>(call.canonical_arguments())
    else {
        return bounded_failure("skill_arguments", "invalid admitted skill arguments");
    };
    let invocation = if arguments.explicit_resource && arguments.resource.is_some() {
        SkillInvocation::User
    } else {
        SkillInvocation::Model
    };
    match catalog.read(
        &arguments.name,
        arguments.resource.as_deref(),
        invocation,
        cancellation,
    ) {
        Ok(loaded) => {
            let output = serde_json::to_string(&loaded)
                .unwrap_or_else(|_| unreachable!("a loaded skill contains serializable fields"));
            let detail = bounded_tool_text(&loaded.text, 0);
            ToolExecutionResult::new(ToolOutcome::Succeeded { output }, Some(detail))
        }
        Err(error) => bounded_failure("skill_read", &error.to_string()),
    }
}

/// An explicit invocation is only recognized as the first token, never inside prose or code.
pub(crate) fn explicit_skill_name(text: &str) -> Option<&str> {
    let token = text.strip_prefix('$')?;
    let name = token.split_whitespace().next()?;
    if token.starts_with(char::is_whitespace) {
        return None;
    }
    if name.chars().all(char::is_numeric) {
        return None;
    }
    SkillName::new(name).ok()?;
    Some(name)
}

pub(crate) fn selected_skill_matches(text: &str, selected: &str) -> bool {
    let Some(token) = text
        .strip_prefix('$')
        .and_then(|token| (!token.starts_with(char::is_whitespace)).then_some(token))
        .and_then(|token| token.split_whitespace().next())
    else {
        return false;
    };
    matches!((SkillName::new(token), SkillName::new(selected)), (Ok(token),Ok(selected)) if token == selected)
}

pub(crate) fn origin(source: SkillOrigin) -> SkillSource {
    match source {
        SkillOrigin::ProjectPlexmaton => SkillSource::ProjectNative,
        SkillOrigin::ProjectAgents => SkillSource::ProjectShared,
        SkillOrigin::User => SkillSource::User,
    }
}

fn activation(loaded: LoadedSkill) -> Result<SkillActivation, ExplicitSkillError> {
    SkillActivation::new(
        loaded.name.as_str().to_owned(),
        origin(loaded.origin),
        loaded.location,
        loaded.digest,
        loaded.text,
    )
    .map_err(ExplicitSkillError::Activation)
}

#[derive(Debug, Error)]
pub(crate) enum ExplicitSkillError {
    #[error("No skills are available")]
    Unavailable,
    #[error("{0}")]
    Read(#[from] plexmaton_skills::SkillError),
    #[error("{0}")]
    Activation(SkillActivationError),
    #[error("Skill reader stopped unexpectedly")]
    Worker,
    #[error("Skill loading was cancelled")]
    Cancelled,
    #[error("The edited retry no longer targets the current failed message")]
    RetryUnavailable,
    #[error("The selected skill no longer matches the message's initial $name token")]
    SelectionChanged,
}

pub(super) fn explicit_resource_authorized(
    catalog: Option<&SkillCatalog>,
    request: &AdmissionRequest,
    agent: &Agent,
) -> bool {
    if request.requested().name != NAME {
        return false;
    }
    let Ok(arguments) = serde_json::from_str::<Arguments>(&request.requested().arguments) else {
        return false;
    };
    if arguments.resource.is_none() {
        return false;
    }
    let Ok(name) = SkillName::new(&arguments.name) else {
        return false;
    };
    let Some(entry) =
        catalog.and_then(|catalog| catalog.entries().iter().find(|entry| entry.name == name))
    else {
        return false;
    };
    agent
        .journal()
        .path(agent.selected_head())
        .is_ok_and(|path| {
            path.iter().any(|entry_record| {
                matches!(&entry_record.payload, JournalEntryPayload::SkillActivated {activation, ..}
            if activation.name() == name.as_str() && activation.source() == origin(entry.origin)
                && activation.location() == entry.location)
            })
        })
}

/// One explicit input's file read; dropping its owner cancels and joins the bounded worker.
pub(crate) struct SkillReadTask {
    cancellation: FileCancellation,
    worker: Option<thread::JoinHandle<()>>,
    result: oneshot::Receiver<Result<SkillActivation, ExplicitSkillError>>,
}

impl SkillReadTask {
    pub(crate) fn start(catalog: Option<Arc<SkillCatalog>>, name: String) -> std::io::Result<Self> {
        let cancellation = FileCancellation::new();
        let worker_cancel = cancellation.clone();
        let (send, result) = oneshot::channel();
        let worker = thread::Builder::new()
            .name("plexmaton-skill-input".to_owned())
            .spawn(move || {
                let loaded = catalog
                    .ok_or(ExplicitSkillError::Unavailable)
                    .and_then(|catalog| {
                        catalog
                            .read(&name, None, SkillInvocation::User, &worker_cancel)
                            .map_err(ExplicitSkillError::Read)
                    })
                    .and_then(activation);
                let _delivered = send.send(loaded);
            })?;
        Ok(Self {
            cancellation,
            worker: Some(worker),
            result,
        })
    }

    pub(crate) fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub(crate) async fn finish(&mut self) -> Result<SkillActivation, ExplicitSkillError> {
        let result = (&mut self.result)
            .await
            .unwrap_or(Err(ExplicitSkillError::Worker));
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| ExplicitSkillError::Worker)?;
        }
        if self.cancellation.is_cancelled() {
            Err(ExplicitSkillError::Cancelled)
        } else {
            result
        }
    }
}

impl Drop for SkillReadTask {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}
