use super::*;

/// JRN-5/ENT-4: model outcomes above the preview limit survive live projection and restoration.
#[test]
fn large_tool_outcome_survives_live_and_restored_context() {
    let output = "x".repeat(crate::MAX_TOOL_PRESENTATION_TEXT_BYTES + 1);
    let mut live = agent();
    submit(&mut live, "read a large result");
    call(&mut live, "read-1");
    stop(&mut live, StopReason::ToolCalls);
    let completed = finish(&mut live, "read-1", &output);
    assert!(
        completed
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::CallModel(_)))
    );
    let canonical = live.record();
    assert!(matches!(context_results(&canonical).last(), Some(result)
        if result.outcome() == &ToolOutcome::Succeeded { output: output.clone() }));
    let mut restored = Agent::from_journal(
        AgentId::new("agent-a").expect("agent"),
        live.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("restore valid large outcome");
    let recovery = restored
        .recover_after_process_death()
        .expect("unfinished model step");
    assert!(recovery.effects.is_empty());
    assert_eq!(restored.record(), canonical);
}

/// JRN-5: recovery retains completed large results and cancels only the unfinished batch slot.
#[test]
fn partial_large_tool_batch_recovers_and_accepts_continuation() {
    let output = "x".repeat(crate::MAX_TOOL_PRESENTATION_TEXT_BYTES + 1);
    let mut live = agent();
    submit(&mut live, "search three places");
    for call_id in ["search-1", "search-2", "search-3"] {
        call_named(&mut live, call_id, "search");
    }
    stop(&mut live, StopReason::ToolCalls);
    finish(&mut live, "search-1", &output);
    finish(&mut live, "search-2", "small result");

    let mut restored = Agent::from_journal(
        AgentId::new("agent-a").expect("agent"),
        live.journal().clone(),
        TurnBudget::default(),
        ApprovalPolicy::default(),
    )
    .expect("restore incomplete batch");
    let recovery = restored
        .recover_after_process_death()
        .expect("unfinished tool");
    assert!(recovery.effects.is_empty(), "recovery must not rerun tools");
    let continued = restored.handle(Input::Submitted {
        text: "continue".into(),
    });
    let request = continued
        .effects
        .iter()
        .find_map(|effect| match effect {
            Effect::CallModel(call) => Some(&call.request),
            _ => None,
        })
        .expect("continuation reaches a new model request");
    let results = context_results(&request.atoms);
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].call_id(), &id("search-1"));
    assert_eq!(results[0].outcome(), &ToolOutcome::Succeeded { output });
    assert_eq!(
        results[1].outcome(),
        &ToolOutcome::Succeeded {
            output: "small result".into()
        }
    );
    assert_eq!(results[2].call_id(), &id("search-3"));
    assert_eq!(
        results[2].outcome(),
        &ToolOutcome::Cancelled {
            reason: ToolCancellationReason::ProcessDied
        }
    );
}
