use super::*;
use crate::{TreeAdmission, TreeRequestRefusal};
use plexmaton_core::{TreeEdit, TreeEditAction, TreeLabel, TreeSourceError, TreeSourceRequest};

/// TRE-4/TRE-8: metadata shares the navigation admission/commit fence, but publishes no reset or
/// historical draft. Dropping an awaiting caller cannot abandon the already accepted label write.
#[tokio::test]
async fn tre_4_8_metadata_ack_is_cancellation_safe_and_preserves_context() {
    let driver = FakeDriver::new([]);
    let (control, mut runtime, _, entry_id, head) =
        navigation::seeded_runtime(driver.clone()).await;
    while runtime.try_next_event().is_some() {}
    let origin = runtime.acknowledged_tree_origin().expect("origin");
    let source_request = TreeSourceRequest {
        origin: origin.clone(),
        entry_id: entry_id.clone(),
    };
    let original_source = runtime
        .read_tree_source(&source_request)
        .expect("acknowledged source");
    let before = runtime.agent.journal().project(&head).expect("context");
    let label = TreeLabel::new("review this".to_owned()).expect("label");
    let edit = TreeEdit {
        origin: origin.clone(),
        action: TreeEditAction::SetLabel {
            entry_id: entry_id.clone(),
            label: Some(label.clone()),
        },
    };
    let _release = ReleaseGateOnDrop(&control.gate);
    control.block_after(1);
    assert_eq!(
        runtime.request_tree_edit(edit.clone()).expect("admission"),
        TreeAdmission::Started
    );
    assert!(runtime.report.tree_edit.is_none());
    assert_eq!(runtime.acknowledged_tree_origin(), None);
    assert_eq!(
        runtime.read_tree_source(&source_request),
        Err(TreeSourceError::HistoryUnavailable)
    );
    {
        let entered = control.gate.entered.notified();
        tokio::select! {
            result = runtime.finish_pending_transition() => panic!("passed blocked writer: {result:?}"),
            () = entered => {}
        }
    }
    assert!(
        runtime.has_active_work(),
        "cancellation cannot release accepted ownership"
    );
    assert_eq!(
        runtime.request_tree_edit(edit.clone()).expect("busy"),
        TreeAdmission::Refused(TreeRequestRefusal::Busy)
    );
    control.gate.release();
    runtime.finish_pending_transition().await.expect("ack");
    let current = runtime.acknowledged_tree_origin().expect("new origin");
    assert_ne!(origin.revision, current.revision);
    assert_eq!(
        runtime.read_tree_source(&source_request),
        Err(TreeSourceError::StaleOrigin)
    );
    assert_eq!(
        runtime.read_tree_source(&TreeSourceRequest {
            origin: current.clone(),
            entry_id: entry_id.clone(),
        }),
        Ok(original_source)
    );
    assert_eq!(
        runtime
            .request_tree_edit(TreeEdit {
                origin: current.clone(),
                action: edit.action.clone()
            })
            .expect("report guard"),
        TreeAdmission::Refused(TreeRequestRefusal::PendingReport)
    );
    let report = runtime.take_report();
    assert_eq!(report.tree_edit.expect("receipt").origin, current);
    assert!(report.tree_navigation.is_none());
    assert!(report.projection_reset.is_none());
    assert!(report.undelivered.is_empty());
    assert_eq!(runtime.agent.journal().tree_label(&entry_id), Some(&label));
    assert_eq!(
        runtime
            .agent
            .journal()
            .project(&head)
            .expect("same context"),
        before
    );
    assert!(matches!(
        runtime.request_tree_edit(edit).expect("stale"),
        TreeAdmission::Refused(TreeRequestRefusal::StaleOrigin { .. })
    ));
    assert!(driver.calls().await.is_empty());
    runtime.shutdown().await.expect("shutdown");
}

/// TRE-4/TRE-8: the metadata receipt participates in failure reporting even with no returned input
/// or projection reset; neither definite nor uncertain failure may silently look like success.
#[tokio::test]
async fn tre_4_8_metadata_write_failure_freezes_without_success_or_fake_input() {
    for unknown in [false, true] {
        let driver = FakeDriver::new([]);
        let (control, mut runtime, _, entry_id, _) =
            navigation::seeded_runtime(driver.clone()).await;
        while runtime.try_next_event().is_some() {}
        let origin = runtime.acknowledged_tree_origin().expect("origin");
        control.fail_after(1, unknown);
        assert_eq!(
            runtime
                .request_tree_edit(TreeEdit {
                    origin,
                    action: TreeEditAction::SetLabel {
                        entry_id,
                        label: Some(TreeLabel::new("not published".to_owned()).expect("label"))
                    },
                })
                .expect("admission"),
            TreeAdmission::Started
        );
        let report = await_report(&mut runtime).await;
        assert_eq!(
            report.persistence_failure,
            Some(if unknown {
                PersistenceFailure::OutcomeUnknown
            } else {
                PersistenceFailure::NotWritten
            })
        );
        assert!(report.tree_edit.is_none());
        assert!(report.tree_navigation.is_none());
        assert!(report.projection_reset.is_none());
        assert!(report.undelivered.is_empty());
        assert_eq!(runtime.acknowledged_tree_origin(), None);
        assert!(driver.calls().await.is_empty());
        assert!(
            matches!(
                runtime.shutdown().await,
                Err(RuntimeError::JournalRequiresReopen)
            ),
            "the earlier failure report was consumed, so shutdown retains the reopen requirement"
        );
        assert!(
            control.dropped.load(Ordering::SeqCst),
            "failed writer was joined"
        );
    }
}
