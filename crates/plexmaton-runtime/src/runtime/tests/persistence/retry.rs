use super::super::retry::settle;
use super::*;
use plexmaton_agent::ModelError;

fn driver() -> Arc<FakeDriver> {
    FakeDriver::new([
        Script::Fail(ModelError::RateLimited { retry_after: None }),
        Script::Events(vec![ModelEvent::Stopped(StopReason::EndOfTurn)]),
    ])
}

/// JRN-7: both retry variants are subject to the ordinary write/effect gate.
#[tokio::test]
async fn failed_retry_append_returns_edited_input_without_starting_another_request() {
    for edited in [None, Some("exact edited question".to_owned())] {
        let (control, store) = StoreControl::pair();
        let driver = driver();
        let mut runtime = runtime(store, driver.clone()).await;
        runtime
            .submit(agent_id(), submission())
            .await
            .expect("submit");
        settle(&mut runtime).await;
        let target = runtime.retry_candidate().expect("eligible").target;
        control.fail_after(1, false);
        let result = runtime.retry(target, edited.clone()).await;
        let report = match result {
            Ok(report) => report,
            Err(RuntimeError::JournalAppendFailed { .. }) => runtime.take_report(),
            Err(error) => panic!("unexpected retry failure: {error}"),
        };
        assert_eq!(
            report.persistence_failure,
            edited.as_ref().map(|_| PersistenceFailure::NotWritten)
        );
        assert!(report.projection_reset.is_none());
        assert_eq!(
            report
                .undelivered
                .iter()
                .map(|v| &v.text)
                .collect::<Vec<_>>(),
            edited.iter().collect::<Vec<_>>()
        );
        assert_eq!(driver.calls().await.len(), 1);
        assert!(runtime.retry_candidate().is_none());
        assert!(runtime.try_next_event().is_none());
    }
}

/// JRN-7: cancelled edited retry and interrupt leave reset ahead of dependent events.
#[tokio::test]
async fn cancelled_edit_retry_delivers_projection_reset_before_later_interrupt_events() {
    let (control, store) = StoreControl::pair();
    let driver = driver();
    let mut runtime = runtime(store, driver.clone()).await;
    runtime
        .submit(agent_id(), submission())
        .await
        .expect("submit");
    settle(&mut runtime).await;
    let target = runtime.retry_candidate().expect("eligible").target;
    control.block_after(1);
    let _release = ReleaseGateOnDrop(&control.gate);
    {
        let entered = control.gate.entered.notified();
        let retry = runtime.retry(target, Some("edited".into()));
        tokio::pin!(retry);
        tokio::select! {
            result = &mut retry => panic!("retry did not wait: {result:?}"),
            () = entered => {}
        }
    }
    assert_eq!(driver.calls().await.len(), 1);
    // Resume retry, then cancel a second transition after it has queued the reset.
    control.gate.release();
    // Drain the retry transition explicitly to establish the pending reset. Interrupt's events
    // then share the same queue, exactly as after cancelling its waiter before report consumption.
    runtime
        .finish_transition()
        .await
        .expect("finish retained edit");
    runtime
        .apply_agent_input(
            Input::Interrupted,
            None,
            crate::runtime::transition::AfterCommit::Interrupt,
        )
        .await
        .expect("interrupt");
    assert!(!runtime.pending.is_empty());
    assert!(
        runtime.try_next_event().is_none(),
        "reset must be consumed first"
    );
    let RuntimeUpdate::Report(report) = runtime.next_update().await.expect("ordered update") else {
        panic!("post-reset event overtook reset");
    };
    let projection = report.projection_reset.expect("reset retained");
    let last = projection
        .last()
        .expect("nonempty projection")
        .sequence
        .get();
    let next = runtime.try_next_event().expect("interrupt follows reset");
    assert_eq!(next.sequence.get(), last + 1);
    runtime.shutdown().await.expect("shutdown");
}
