use super::*;

/// JRN-7: direct runtime drop closes and joins the writer instead of detaching its thread.
#[tokio::test]
async fn dropping_the_runtime_joins_its_journal_writer() {
    let (control, store) = StoreControl::pair();
    let runtime = runtime(store, FakeDriver::new([])).await;

    assert!(matches!(
        runtime.context_budget().expect("snapshot"),
        crate::ContextBudgetSnapshot::Unavailable(
            crate::ContextBudgetUnavailable::ModelNotConfigured
        )
    ));

    drop(runtime);

    assert!(control.dropped.load(Ordering::SeqCst));
}

/// JRN-7: a failed user append publishes nothing and returns exact input ownership.
#[tokio::test]
async fn failed_user_append_returns_the_draft_and_starts_no_effect() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    control.fail_after(1, false);

    let report = runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("typed submission failure: {error}"));

    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::NotWritten)
    );
    assert!(matches!(
        runtime.context_budget().expect("snapshot"),
        crate::ContextBudgetSnapshot::Unavailable(
            crate::ContextBudgetUnavailable::PersistenceFailed
        )
    ));
    assert!(matches!(
        report.undelivered.as_slice(),
        [input]
            if input.text == "keep this exact draft"
                && input.reason == UndeliveredReason::PersistenceFailed
    ));
    assert!(driver.calls().await.is_empty());
    assert!(!runtime.has_active_model());
    assert!(runtime.try_next_event().is_none());
    let refused_retry = runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("failed runtime must return new text too: {error}"));
    assert!(matches!(
        refused_retry.undelivered.as_slice(),
        [input] if input.text == "keep this exact draft"
    ));
    assert_eq!(
        control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
        1,
        "only the durable announcement preceded the injected failure"
    );
}

/// JRN-7: command admission failure is definitely unwritten and returns the direct submission.
#[tokio::test]
async fn stopped_writer_returns_the_draft_instead_of_a_bare_runtime_error() {
    let (_control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    runtime
        .journal
        .as_mut()
        .unwrap_or_else(|| panic!("durable runtime owns a writer"))
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("stop writer fixture: {error:?}"));

    let report = runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("stopped writer should return ownership: {error}"));

    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::NotWritten)
    );
    assert!(
        matches!(report.undelivered.as_slice(), [input] if input.text == "keep this exact draft")
    );
    assert!(driver.calls().await.is_empty());
}

/// JRN-7: a dead writer cannot eclipse the exact draft with its own cleanup error.
#[tokio::test]
async fn panicked_writer_returns_input_and_reports_failed_cleanup() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    control.panic_after(1);

    let report = runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("panicked writer should retain report: {error}"));

    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::OutcomeUnknown)
    );
    assert!(
        matches!(report.undelivered.as_slice(), [input] if input.text == "keep this exact draft")
    );
    assert_eq!(report.cleanup_failures, [CleanupFailure::JournalWriter]);
    assert!(driver.calls().await.is_empty());
}

/// JRN-7: a possibly partial write returns the text but requires recovery before retry.
#[tokio::test]
async fn uncertain_user_append_is_typed_and_cannot_start_an_effect() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    control.fail_after(1, true);

    let report = runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("typed uncertain submission: {error}"));

    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::OutcomeUnknown)
    );
    assert!(matches!(
        report.undelivered.as_slice(),
        [input] if input.text == "keep this exact draft"
    ));
    assert!(driver.calls().await.is_empty());
    assert!(!runtime.has_active_model());
}

/// TIM-1/JRN-7: an atomic user/turn start has no committed semantic prefix on refusal.
#[tokio::test]
async fn failed_atomic_turn_start_is_wholly_unwritten() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    control.fail_after(1, false);

    let report = runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("typed partial transition: {error}"));

    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::NotWritten)
    );
    assert_eq!(
        control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
        1,
        "only the prior announcement was committed"
    );
    assert!(driver.calls().await.is_empty());
}

/// TIM-2/JRN-7: failed request authorization preserves the user fact but starts no HTTP effect.
#[tokio::test]
async fn failed_request_authorization_starts_no_model_and_freezes_the_runtime() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    control.fail_after(2, false);

    assert!(matches!(
        runtime.submit(agent_id(), submission()).await,
        Err(RuntimeError::JournalAppendFailed { .. })
    ));

    assert!(driver.calls().await.is_empty());
    assert!(!runtime.has_active_work());
    let records = control
        .records
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(records.iter().any(|record| matches!(
        record,
        JournalRecord::AppendEntry { entry, .. }
            if matches!(&entry.payload, JournalEntryPayload::TurnStarted { .. })
    )));
    assert!(
        records
            .iter()
            .all(|record| !matches!(record, JournalRecord::RequestAttemptAuthorized { .. }))
    );
}

/// TIM-2/JRN-7: a terminal audit failure cannot publish model completion or invent a retry.
#[tokio::test]
async fn failed_request_terminal_publishes_no_semantic_completion() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::Events(vec![ModelEvent::Stopped(
        StopReason::EndOfTurn,
    )])]);
    let mut runtime = runtime(store, driver).await;
    while runtime.try_next_event().is_some() {}
    runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("open request: {error}"));
    while runtime.try_next_event().is_some() {}
    control.fail_after(1, false);

    assert!(matches!(
        runtime.next_update().await,
        Err(RuntimeError::JournalAppendFailed { .. })
    ));

    assert!(!runtime.has_active_work());
    let records = control
        .records
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(
        records
            .iter()
            .any(|record| matches!(record, JournalRecord::RequestAttemptAuthorized { .. }))
    );
    assert!(
        records
            .iter()
            .all(|record| !matches!(record, JournalRecord::RequestAttemptFinished { .. }))
    );
    assert!(records.iter().all(|record| !matches!(
        record,
        JournalRecord::AppendEntry { entry, .. }
            if matches!(&entry.payload, JournalEntryPayload::AssistantOutput { .. })
    )));
}
