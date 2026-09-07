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
