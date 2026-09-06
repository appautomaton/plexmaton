use plexmaton_agent::{
    AssistantBlock, CompactionFailure, CompactionOutcome, DispatchedRequestTiming, ElapsedMillis,
    ModelEvent, ModelOutputPosition, ProviderReplay, RequestAttemptId, RequestAttemptTerminal,
    RequestAttemptTerminalState, RequestCost, RequestDispatchedOutcome, StopReason, ToolCall,
    UnixMillis,
};
use plexmaton_core::{TokenUsage, ToolCallId};
use plexmaton_provider::ModelRegistry;

use super::{MAX_SUMMARY_BLOCKS, SummaryCollector};
use crate::{http::timing::AttemptReport, runtime::ModelCompletion};

fn attempt() -> RequestAttemptId {
    RequestAttemptId::new("summary-collector-attempt").expect("fixture identity")
}

fn terminal(completion: ModelCompletion) -> AttemptReport {
    let outcome = match &completion {
        ModelCompletion::Stopped(stop_reason) => RequestDispatchedOutcome::Completed {
            stop_reason: *stop_reason,
        },
        ModelCompletion::Cancelled => RequestDispatchedOutcome::Cancelled,
        ModelCompletion::Failed(_) => RequestDispatchedOutcome::Malformed,
    };
    let timing = DispatchedRequestTiming::new(
        UnixMillis::EPOCH,
        Some(ElapsedMillis::new(0)),
        Some(ElapsedMillis::new(0)),
        ElapsedMillis::new(1),
    )
    .expect("ordered fixture timing");
    AttemptReport {
        terminal: RequestAttemptTerminal::new(
            attempt(),
            RequestAttemptTerminalState::Dispatched {
                timing,
                outcome,
                usage: TokenUsage::Unavailable,
                cost: RequestCost::Unavailable,
            },
        )
        .expect("fixture terminal"),
        completion,
    }
}

fn text(item: u16, value: &str) -> ModelEvent {
    ModelEvent::TextDelta {
        position: ModelOutputPosition::new(item, 0),
        delta: value.to_owned(),
    }
}

fn replay() -> ProviderReplay {
    let registry = ModelRegistry::parse(
        r#"
active_model = { provider = "fixture", model = "fixture" }
[providers.fixture]
api = "openai_responses"
base_url = "http://127.0.0.1:1/v1"
api_key_env = "UNUSED_FIXTURE_KEY"
[providers.fixture.models.fixture]
id = "fixture"
context_window_tokens = 8192
max_output_tokens = 1024
output_reserve_tokens = 512
"#,
    )
    .expect("fixture registry");
    ProviderReplay::new(
        registry.active_model().replay_compatibility(),
        r#"{"type":"reasoning","encrypted_content":"opaque-summary-capsule"}"#.to_owned(),
    )
    .expect("bounded fixture replay")
}

/// CPL-6: ordered full output and opaque attachments survive collection without a model step.
#[test]
fn cpl_6_collector_preserves_ordered_text_reasoning_and_exact_replay() {
    let mut collector = SummaryCollector::new(attempt(), 1024);
    collector.push(text(2, "Continue "));
    collector.push(ModelEvent::ReasoningDelta {
        position: ModelOutputPosition::new(0, 0),
        delta: "private reasoning".into(),
    });
    let replay = replay();
    collector.push(ModelEvent::Replay {
        position: ModelOutputPosition::new(0, 0),
        replay: replay.clone(),
    });
    collector.push(text(2, "with the next slice."));
    let report = collector.finish(terminal(ModelCompletion::Stopped(StopReason::EndOfTurn)));
    let CompactionOutcome::Complete { output } = report.outcome() else {
        panic!("expected complete collected output: {:?}", report.outcome());
    };
    assert_eq!(output.blocks().len(), 2);
    assert!(matches!(
        &output.blocks()[0],
        AssistantBlock::Reasoning { text, .. } if text == "private reasoning"
    ));
    assert!(matches!(
        &output.blocks()[1],
        AssistantBlock::Text { text, .. } if text == "Continue with the next slice."
    ));
    let attachment = &output.replay().expect("retained sidecar").attachments()[0];
    assert_eq!(attachment.block(), 0);
    assert_eq!(attachment.payload(), replay.payload());
    assert!(!format!("{report:?}").contains("opaque-summary-capsule"));
}

/// CPL-6: tool output stays in the failed-attempt audit and cannot become accepted summary text.
#[test]
fn cpl_6_collector_rejects_tools_but_preserves_the_received_output() {
    let mut collector = SummaryCollector::new(attempt(), 1024);
    collector.push(ModelEvent::Called {
        position: ModelOutputPosition::new(0, 0),
        call: ToolCall {
            call_id: ToolCallId::new("summary-tool-call").expect("call id"),
            name: "exec_command".into(),
            arguments: r#"{"command":"should never execute"}"#.into(),
        },
    });
    collector.push(text(1, "I also returned text."));
    let report = collector.finish(terminal(ModelCompletion::Stopped(StopReason::EndOfTurn)));
    assert_eq!(
        report.outcome().failure(),
        Some(CompactionFailure::ToolCallOutput)
    );
    let output = report
        .outcome()
        .output()
        .expect("audit retains valid output");
    assert_eq!(output.tool_calls().count(), 1);
    assert_eq!(output.blocks().len(), 2);
}

/// CPL-6/CPL-8: complete transport is insufficient for empty, truncated or oversized summaries.
#[test]
fn cpl_6_collector_refuses_unusable_output_without_losing_valid_partials() {
    for (value, limit, stop, expected) in [
        (
            "",
            32,
            StopReason::EndOfTurn,
            CompactionFailure::EmptyOutput,
        ),
        (
            " \n\t",
            32,
            StopReason::EndOfTurn,
            CompactionFailure::EmptyOutput,
        ),
        (
            "unfinished",
            32,
            StopReason::OutputLimit,
            CompactionFailure::OutputLimit,
        ),
        (
            "larger than allowed",
            3,
            StopReason::EndOfTurn,
            CompactionFailure::OutputTooLarge,
        ),
    ] {
        let mut collector = SummaryCollector::new(attempt(), limit);
        collector.push(text(0, value));
        let report = collector.finish(terminal(ModelCompletion::Stopped(stop)));
        assert_eq!(report.outcome().failure(), Some(expected));
        if !value.is_empty() {
            let output = report
                .outcome()
                .output()
                .expect("nonempty partial retained");
            assert!(
                matches!(&output.blocks()[0], AssistantBlock::Text { text, .. } if text == value)
            );
        }
    }
}

/// CPL-6/CPL-7: cancellation preserves a bounded partial instead of manufacturing a checkpoint.
#[test]
fn cpl_6_collector_keeps_cancelled_partial_output_and_bounds_block_growth() {
    let mut collector = SummaryCollector::new(attempt(), 1024);
    collector.push(text(0, "partial"));
    let report = collector.finish(terminal(ModelCompletion::Cancelled));
    assert_eq!(
        report.outcome().failure(),
        Some(CompactionFailure::Cancelled)
    );
    assert!(report.outcome().output().is_some());

    let mut collector = SummaryCollector::new(attempt(), 1024);
    for index in 0..=MAX_SUMMARY_BLOCKS {
        collector.push(text(u16::try_from(index).expect("bounded index"), "x"));
    }
    let report = collector.finish(terminal(ModelCompletion::Stopped(StopReason::EndOfTurn)));
    assert_eq!(
        report.outcome().failure(),
        Some(CompactionFailure::OutputTooLarge)
    );
    assert_eq!(
        report
            .outcome()
            .output()
            .expect("bounded partial")
            .blocks()
            .len(),
        MAX_SUMMARY_BLOCKS
    );
}
