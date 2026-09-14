//! Root-owned construction of one provider-configured delegated Conversation.

use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use plexmaton_agent::UnixMillis;
use plexmaton_core::ConversationId;
use plexmaton_provider::{ApiKey, ResolvedModel};
use plexmaton_session_store::{DelegatedConversationDirectory, DelegatedJournalFile, StoreError};
use thiserror::Error;

use crate::{
    ChildCollaborationIngress, CollaborationIngressRefusal, DelegatedRuntimeBinding,
    HttpSetupError, LiveRuntime, NativeToolCatalog, NativeToolSetupError, OwnedCollaboration,
    RuntimeError,
};

/// Failure before a child runtime can enter its owned runner.
#[derive(Debug, Error)]
pub enum DelegatedChildFactoryError {
    #[error("selected child provider has no typed collaboration representation")]
    ProviderUnsupported,
    #[error("cannot open the delegated session journal: {0}")]
    Store(#[from] StoreError),
    #[error("cannot construct the delegated child tool profile: {0}")]
    Tools(#[from] NativeToolSetupError),
    #[error("cannot construct the delegated runtime: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("system time cannot be represented as a session timestamp")]
    Clock,
    #[cfg(test)]
    #[error("injected delegated child build failure")]
    InjectedBuildFailure,
}

enum ChildDriver {
    Provider {
        model: Box<ResolvedModel>,
        key: ApiKey,
    },
    #[cfg(test)]
    Synthetic {
        driver: Arc<dyn crate::runtime::ModelDriver>,
        clock: Arc<dyn crate::runtime::WallClock>,
    },
}

/// Configured root capability that creates children without accepting model-selected authority.
pub struct DelegatedChildFactory {
    directory: DelegatedConversationDirectory,
    tools: NativeToolCatalog,
    driver: ChildDriver,
    #[cfg(test)]
    fail_next_build: Arc<AtomicBool>,
}

impl DelegatedChildFactory {
    /// Captures one exact child model, credential and workspace tool base.
    #[must_use]
    pub fn new(
        directory: DelegatedConversationDirectory,
        model: ResolvedModel,
        key: ApiKey,
        tools: NativeToolCatalog,
    ) -> Self {
        Self {
            directory,
            tools,
            driver: ChildDriver::Provider {
                model: Box::new(model),
                key,
            },
            #[cfg(test)]
            fail_next_build: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    pub(crate) fn synthetic(
        directory: DelegatedConversationDirectory,
        tools: NativeToolCatalog,
        driver: Arc<dyn crate::runtime::ModelDriver>,
        clock: Arc<dyn crate::runtime::WallClock>,
    ) -> Self {
        Self {
            directory,
            tools,
            driver: ChildDriver::Synthetic { driver, clock },
            fail_next_build: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_next_build_for_test(&self) {
        self.fail_next_build.store(true, Ordering::SeqCst);
    }

    /// Pure capability preflight; it creates no directory entry or collaboration fact.
    #[must_use]
    pub fn supports_collaboration(&self) -> bool {
        match &self.driver {
            ChildDriver::Provider { model, .. } => model.carries_collaboration_context(),
            #[cfg(test)]
            ChildDriver::Synthetic { driver, .. } => driver.supports_collaboration(),
        }
    }

    /// Validates every pure provider, credential and child-profile failure before durable creation.
    pub(crate) fn preflight(&self) -> Result<(), DelegatedChildFactoryError> {
        if !self.supports_collaboration() {
            return Err(DelegatedChildFactoryError::ProviderUnsupported);
        }
        self.tools.preflight_child_collaboration()?;
        if let ChildDriver::Provider { model, .. } = &self.driver
            && !self.tools.excludes_api_key_environment(model.api_key_env())
        {
            return Err(DelegatedChildFactoryError::Runtime(
                HttpSetupError::ToolCredentialEnvironmentMismatch.into(),
            ));
        }
        Ok(())
    }

    pub(crate) async fn build(
        &self,
        worker: plexmaton_agent::collaboration::MailEndpoint,
        journal: DelegatedJournalFile,
        binding: DelegatedRuntimeBinding,
        ingress: ChildCollaborationIngress,
    ) -> Result<LiveRuntime, DelegatedChildFactoryError> {
        self.preflight()?;
        if ingress.worker() != &worker || ingress.provenance() != binding.provenance() {
            return Err(DelegatedChildFactoryError::Runtime(
                RuntimeError::DelegatedControlMismatch,
            ));
        }
        #[cfg(test)]
        if self.fail_next_build.swap(false, Ordering::SeqCst) {
            return Err(DelegatedChildFactoryError::InjectedBuildFailure);
        }
        let resumed = !journal.journal().records().is_empty();
        let tools = self.tools.clone().with_child_collaboration(ingress)?;
        match &self.driver {
            ChildDriver::Provider { model, key } if resumed => {
                LiveRuntime::provider_with_resumed_delegated_journal(
                    worker.agent,
                    model.as_ref().clone(),
                    key.clone(),
                    tools,
                    journal,
                    binding,
                )
                .await
                .map(|(runtime, _)| runtime)
                .map_err(Into::into)
            }
            ChildDriver::Provider { model, key } => {
                LiveRuntime::provider_with_fresh_delegated_journal(
                    worker.agent,
                    "Plexmaton child",
                    model.as_ref().clone(),
                    key.clone(),
                    tools,
                    journal,
                    binding,
                )
                .await
                .map_err(Into::into)
            }
            #[cfg(test)]
            ChildDriver::Synthetic { driver, clock } if resumed => {
                LiveRuntime::with_resumed_bound_delegated_driver(
                    worker.agent,
                    Arc::clone(driver),
                    tools,
                    journal,
                    binding,
                    Arc::clone(clock),
                )
                .await
                .map_err(Into::into)
            }
            #[cfg(test)]
            ChildDriver::Synthetic { driver, clock } => {
                LiveRuntime::with_fresh_bound_delegated_driver(
                    worker.agent,
                    "Plexmaton child".into(),
                    Arc::clone(driver),
                    tools,
                    journal,
                    binding,
                    Arc::clone(clock),
                )
                .await
                .map_err(Into::into)
            }
        }
    }

    pub(crate) fn reserve(
        &self,
        conversation: ConversationId,
    ) -> Result<DelegatedJournalFile, DelegatedChildFactoryError> {
        let path = self.directory.path_for(&conversation)?;
        if path.try_exists().map_err(|source| StoreError::Io {
            operation: "inspect delegated session path",
            source,
        })? {
            return self.directory.resume(&conversation).map_err(Into::into);
        }
        match self.directory.create(conversation.clone(), now()?) {
            Ok(journal) => Ok(journal),
            Err(StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                self.directory.resume(&conversation).map_err(Into::into)
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl OwnedCollaboration {
    /// Installs the sole configured child factory; model arguments cannot replace its authority.
    pub fn bind_child_factory(
        &mut self,
        factory: DelegatedChildFactory,
    ) -> Result<(), CollaborationIngressRefusal> {
        if self.child_factory.is_some() {
            return Err(CollaborationIngressRefusal::CapabilityMismatch);
        }
        self.child_factory = Some(factory);
        Ok(())
    }
}

fn now() -> Result<UnixMillis, DelegatedChildFactoryError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DelegatedChildFactoryError::Clock)?;
    let millis =
        u64::try_from(elapsed.as_millis()).map_err(|_| DelegatedChildFactoryError::Clock)?;
    Ok(UnixMillis::new(millis))
}
