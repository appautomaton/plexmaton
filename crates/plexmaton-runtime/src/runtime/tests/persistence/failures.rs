use super::*;

/// JRN-7: direct runtime drop closes and joins the writer instead of detaching its thread.
#[tokio::test]
async fn dropping_the_runtime_joins_its_journal_writer() {
    let (control, store) = StoreControl::pair();
    let runtime = runtime(store, FakeDriver::new([])).await;

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

/// JRN-7: a committed prefix makes the whole transition outcome unknown.
#[tokio::test]
async fn failure_after_one_record_never_claims_the_submission_was_unwritten() {
    let (control, store) = StoreControl::pair();
    let driver = FakeDriver::new([Script::EndWithoutTerminal]);
    let mut runtime = runtime(store, Arc::clone(&driver)).await;
    let _announcement = runtime.try_next_event();
    control.fail_after(2, false);

    let report = runtime
        .submit(agent_id(), submission())
        .await
        .unwrap_or_else(|error| panic!("typed partial transition: {error}"));

    assert_eq!(
        report.persistence_failure,
        Some(PersistenceFailure::OutcomeUnknown)
    );
    assert_eq!(
        control
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
        2,
        "the announcement and user message precede the failed running-status record"
    );
    assert!(driver.calls().await.is_empty());
}
