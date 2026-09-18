//! Explicit role profiles for authenticated collaboration tools.

use super::*;
use crate::collaboration_tools::collaboration_tool_definitions;

/// Synchronized debug-binary witness that a collaboration tool entered its execution boundary.
#[cfg(debug_assertions)]
pub(super) fn record_invocation(call: &AdmittedToolCall) {
    use std::io::Write as _;

    const MAX_TRACE_BYTES: u64 = 4096;
    let Some(path) = std::env::var_os("PLEXMATON_TEST_COLLABORATION_INVOCATIONS") else {
        return;
    };
    let name = call.requested().name.as_str();
    let mut trace = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open collaboration invocation trace");
    let current = trace
        .metadata()
        .expect("inspect collaboration invocation trace")
        .len();
    assert!(
        current + (name.len() as u64) < MAX_TRACE_BYTES,
        "collaboration invocation trace exceeded its fixture bound"
    );
    writeln!(trace, "{name}").expect("append collaboration invocation trace");
    trace
        .sync_all()
        .expect("persist collaboration invocation trace");
}

pub(super) fn tool_result(result: crate::CollaborationIngressResult) -> ToolExecutionResult {
    let (outcome, reference) = result.into_parts();
    let output = match outcome {
        CollaborationIngressOutcome::Delegated { target } => serde_json::json!({
            "status": "delegated",
            "target": target.as_str(),
        }),
        CollaborationIngressOutcome::MailAccepted => {
            serde_json::json!({"status": "mail_accepted"})
        }
        CollaborationIngressOutcome::TaskUpdated => {
            serde_json::json!({"status": "task_updated"})
        }
        CollaborationIngressOutcome::HandoffCompleted => {
            serde_json::json!({"status": "handoff_completed"})
        }
    };
    ToolExecutionResult::new(
        ToolOutcome::Succeeded {
            output: output.to_string(),
        },
        None,
    )
    .with_collaboration_reference(reference)
}

impl NativeToolCatalog {
    /// Adds Main-only collaboration definitions backed by one authenticated bounded ingress.
    pub fn with_main_collaboration(
        mut self,
        ingress: MainCollaborationIngress,
    ) -> Result<Self, NativeToolSetupError> {
        if self.profile != NativeToolProfile::Full || self.collaboration.is_some() {
            return Err(NativeToolSetupError::CollaborationProfile);
        }
        let mut definitions = self.definitions.to_vec();
        definitions.extend(collaboration_tool_definitions(
            CollaborationToolScope::Main,
        )?);
        self.definitions = definitions.into();
        self.profile = NativeToolProfile::MainCollaboration;
        self.collaboration = Some(NativeCollaborationIngress::Main(ingress));
        Ok(self)
    }

    /// Narrows to read/search plus fixed-parent child mail through one authenticated ingress.
    pub(crate) fn with_child_collaboration(
        mut self,
        ingress: ChildCollaborationIngress,
    ) -> Result<Self, NativeToolSetupError> {
        let definitions = self.child_collaboration_definitions()?;
        self.profile = NativeToolProfile::ChildCollaboration;
        self.skills = None;
        self.definitions = definitions.into();
        self.collaboration = Some(NativeCollaborationIngress::Child(ingress));
        Ok(self)
    }

    pub(crate) fn preflight_child_collaboration(&self) -> Result<(), NativeToolSetupError> {
        self.child_collaboration_definitions().map(|_| ())
    }

    fn child_collaboration_definitions(&self) -> Result<Vec<FunctionTool>, NativeToolSetupError> {
        if !matches!(
            self.profile,
            NativeToolProfile::Full | NativeToolProfile::ReadOnly
        ) || self.collaboration.is_some()
        {
            return Err(NativeToolSetupError::CollaborationProfile);
        }
        let child_profile = NativeToolProfile::ChildCollaboration;
        let mut definitions = FileTools::definitions()
            .into_iter()
            .filter(|definition| child_profile.allows(definition.name()))
            .map(|definition| {
                FunctionTool::new(
                    definition.name(),
                    definition.description(),
                    definition.parameters().clone(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        definitions.extend(collaboration_tool_definitions(
            CollaborationToolScope::Child,
        )?);
        Ok(definitions)
    }

    pub(crate) fn main_runtime_identity(
        &self,
        endpoint: plexmaton_agent::collaboration::MailEndpoint,
        runtime: Arc<crate::collaboration_ingress::RuntimeCollaborationIdentity>,
    ) -> Option<crate::MainRuntimeIdentity> {
        match &self.collaboration {
            Some(NativeCollaborationIngress::Main(ingress)) => {
                Some(ingress.identify(endpoint, runtime))
            }
            Some(NativeCollaborationIngress::Child(_)) | None => None,
        }
    }

    pub(crate) fn child_runtime_identity(
        &self,
        endpoint: plexmaton_agent::collaboration::MailEndpoint,
        runtime: Arc<crate::collaboration_ingress::RuntimeCollaborationIdentity>,
    ) -> Option<crate::collaboration_ingress::ChildRuntimeIdentity> {
        match &self.collaboration {
            Some(NativeCollaborationIngress::Child(ingress)) => {
                ingress.runtime_identity(endpoint, runtime)
            }
            Some(NativeCollaborationIngress::Main(_)) | None => None,
        }
    }

    pub(crate) fn collaboration_artifact_source(
        &self,
        endpoint: plexmaton_agent::collaboration::MailEndpoint,
        selected: Vec<plexmaton_agent::ArtifactAnnouncementOrigin>,
        retained: Vec<plexmaton_agent::ArtifactAnnouncementOrigin>,
        runtime: Arc<crate::collaboration_ingress::RuntimeCollaborationIdentity>,
    ) -> Option<crate::CollaborationArtifactSource> {
        match &self.collaboration {
            Some(NativeCollaborationIngress::Main(ingress)) => {
                Some(ingress.artifact_source(endpoint, selected, retained, runtime))
            }
            Some(NativeCollaborationIngress::Child(ingress)) => {
                ingress.artifact_source(endpoint, selected, retained, runtime)
            }
            None => None,
        }
    }

    pub(crate) fn collaboration_session_source(
        &self,
        endpoint: plexmaton_agent::collaboration::MailEndpoint,
        journal: plexmaton_agent::ConversationJournal,
        head: plexmaton_core::HeadName,
        runtime: Arc<crate::collaboration_ingress::RuntimeCollaborationIdentity>,
    ) -> Option<crate::CollaborationSessionSource> {
        match &self.collaboration {
            Some(NativeCollaborationIngress::Main(ingress)) => {
                Some(ingress.session_source(endpoint, journal, head, runtime))
            }
            Some(NativeCollaborationIngress::Child(ingress)) => {
                ingress.session_source(endpoint, journal, head, runtime)
            }
            None => None,
        }
    }
}
