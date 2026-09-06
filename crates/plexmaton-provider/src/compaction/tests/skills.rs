use super::*;
use plexmaton_agent::{
    CompactionAttemptFinished, CompactionInputMode, CompactionOutcome, DispatchedRequestTiming,
    ElapsedMillis, SkillActivation, SkillSource,
};
use plexmaton_core::TokenUsage;

/// CPL-3/SKL-5/SKL-6: the latest explicit invocation is required context as one user/skill pair,
/// both when retained as a suffix and when pinned after a digest. Preview and journal agree.
#[test]
fn cpl_3_skill_invocation_survives_compaction_in_every_dialect() {
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        for later_answer in [false, true] {
            let model = model_with_tail(api, 12_000, 2_000, 1_000, 1);
            let mut agent = history();
            let skill = SkillActivation::new(
                "review".into(),
                SkillSource::ProjectShared,
                "/workspace/.agents/skills/review/SKILL.md".into(),
                "b".repeat(64),
                "EXACT_SKILL_INSTRUCTIONS\r\n".repeat(40),
            )
            .expect("skill");
            agent.handle_at(
                Input::SkillSubmitted {
                    text: "$review preserve these instructions".into(),
                    skill: skill.clone(),
                },
                UnixMillis::new(50),
            );
            if later_answer {
                let step_id = agent.active_model_step().expect("step");
                agent.handle_at(
                    Input::Streamed {
                        step_id: step_id.clone(),
                        event: ModelEvent::TextDelta {
                            position: ModelOutputPosition::new(0, 0),
                            delta: "large answer ".repeat(600),
                        },
                    },
                    UnixMillis::new(51),
                );
                agent.handle_at(
                    Input::Streamed {
                        step_id,
                        event: ModelEvent::Stopped(StopReason::EndOfTurn),
                    },
                    UnixMillis::new(52),
                );
            }
            let prepared = plan_compaction(
                agent.journal(),
                &head(),
                &model,
                &[],
                CompactionId::new("compact-skill").expect("id"),
            )
            .expect("complete input fits");
            let output = AssistantOutput::new(
                vec![AssistantBlock::Text {
                    item_id: TranscriptItemId::new("summary").expect("id"),
                    text: "digest".into(),
                }],
                None,
            )
            .expect("summary");
            let preview = validate_compaction_output(&prepared, &model, &[], &output).expect("fit");
            assert_eq!(
                preview
                    .atoms
                    .iter()
                    .filter(|atom| atom.value() == &ContextAtomValue::Skill(skill.clone()))
                    .count(),
                1,
                "{api}: exact instructions must survive once, later_answer={later_answer}",
            );
            assert!(
                matches!(preview.atoms.as_slice(), [summary, user, activation]
                if matches!(summary.value(), ContextAtomValue::CompactionSummary { .. })
                    && matches!(user.value(), ContextAtomValue::User { text } if text == "$review preserve these instructions")
                    && activation.value() == &ContextAtomValue::Skill(skill))
            );
            let (attempt, _) = agent
                .authorize_compaction_attempt(prepared.plan(), UnixMillis::new(100))
                .expect("authorize");
            let terminal = RequestAttemptTerminal::new(
                attempt.clone(),
                RequestAttemptTerminalState::Dispatched {
                    timing: DispatchedRequestTiming::new(
                        UnixMillis::new(100),
                        None,
                        None,
                        ElapsedMillis::new(1),
                    )
                    .expect("timing"),
                    outcome: RequestDispatchedOutcome::Completed {
                        stop_reason: StopReason::EndOfTurn,
                    },
                    usage: TokenUsage::Unavailable,
                    cost: RequestCost::Unavailable,
                },
            )
            .expect("terminal");
            agent
                .finish_compaction_attempt(
                    CompactionAttemptFinished::new(
                        terminal,
                        CompactionInputMode::Verbatim,
                        CompactionOutcome::Complete { output },
                    )
                    .expect("finished"),
                )
                .expect("record output");
            agent
                .commit_compaction_checkpoint(prepared.plan().clone(), attempt)
                .expect("checkpoint");
            let projected = agent.journal().project(&head()).expect("replay");
            assert_eq!(
                encode_request(
                    &model,
                    projected.request(),
                    &[],
                    Some(model.max_output_tokens())
                )
                .expect("journal wire"),
                encode_request(&model, &preview, &[], Some(model.max_output_tokens()))
                    .expect("preview wire"),
            );
        }
    }
}
