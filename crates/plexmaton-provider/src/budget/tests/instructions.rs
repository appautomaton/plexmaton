use super::*;

/// AGI-4/BUD-2/BUD-3: exact workspace context is charged once; equal-size edits invalidate usage.
#[test]
fn agi_4_instruction_bytes_are_budgeted_and_changed_rules_invalidate_measurements() {
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        let base = model(api);
        let model = base
            .with_workspace_instructions("rules alpha".repeat(100))
            .expect("snapshot");
        let changed = base
            .with_workspace_instructions("rules bravo".repeat(100))
            .expect("replacement");
        assert_eq!(
            model.workspace_instructions().len(),
            changed.workspace_instructions().len()
        );
        let mut agent = open();
        let before = agent.journal().clone();
        let base_budget = budgeted_context(agent.journal(), &head(), &base, &[]).expect("base");
        let budget = budgeted_context(agent.journal(), &head(), &model, &[]).expect("budget");
        assert_eq!(base_budget.request, budget.request);
        assert_eq!(base_budget.ledger.atoms, budget.ledger.atoms);
        assert!(
            budget.ledger.environment_estimate.tokens
                > base_budget.ledger.environment_estimate.tokens
        );
        assert_eq!(
            estimate_request(&model, &budget.request, &[])
                .expect("whole estimate")
                .tokens,
            budget.ledger.input_tokens
        );
        assert_eq!(agent.journal(), &before);
        assert!(!format!("{:?}", budget.ledger).contains("rules alpha"));
        let (id, _) = agent
            .authorize_request_attempt(
                agent.active_model_step().expect("step"),
                request_environment(&model, &[], Some(model.max_output_tokens())),
                UnixMillis::new(1),
            )
            .expect("authorize");
        let terminal = RequestAttemptTerminal::new(
            id,
            RequestAttemptTerminalState::Dispatched {
                timing: DispatchedRequestTiming::new(
                    UnixMillis::new(2),
                    None,
                    None,
                    ElapsedMillis::new(1),
                )
                .expect("timing"),
                outcome: RequestDispatchedOutcome::Completed {
                    stop_reason: StopReason::EndOfTurn,
                },
                usage: TokenUsage::Complete(TokenCounts {
                    input: 500,
                    output: 10,
                    total: 510,
                    cached_input: Some(0),
                    cache_write_input: Some(0),
                    reasoning_output: Some(0),
                }),
                cost: RequestCost::Unavailable,
            },
        )
        .expect("terminal");
        agent
            .finish_request_attempt(&terminal)
            .expect("record usage");
        let measured = budget_ledger(agent.journal(), &head(), &model, &[]).expect("matched");
        assert!(measured.anchor.is_some());
        assert_eq!(measured.input_tokens, 500);
        assert_eq!(measured.estimated_remainder.tokens, 0);
        let fresh = budget_ledger(agent.journal(), &head(), &changed, &[]).expect("changed");
        assert!(
            fresh.anchor.is_none(),
            "{api}: changed rules cannot reuse old usage"
        );
        assert_eq!(
            fresh.input_tokens,
            estimate_request(&changed, &budget.request, &[])
                .expect("fresh estimate")
                .tokens
        );
    }
}
