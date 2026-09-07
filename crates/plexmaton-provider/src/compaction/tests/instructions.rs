use super::*;

/// AGI-4/CPL-3: compaction replaces history while the exact workspace environment remains once.
#[test]
fn agi_4_workspace_instructions_remain_outside_the_compaction_cut() {
    let rules = "EXACT_WORKSPACE_RULES\r\nRead nested AGENTS.md before editing.";
    for api in [
        "openai_responses",
        "openai_chat_completions",
        "anthropic_messages",
        "google_generate_content",
    ] {
        let model = model(api)
            .with_workspace_instructions(rules.into())
            .expect("snapshot");
        let agent = long_history(4, 4_000);
        let prepared = plan_compaction(
            agent.journal(),
            &head(),
            &model,
            &[],
            CompactionId::new("workspace-compaction").expect("id"),
        )
        .expect("plan");
        let output = AssistantOutput::new(
            vec![AssistantBlock::Text {
                item_id: TranscriptItemId::new("summary").expect("item"),
                text: "Earlier work is complete; continue the latest request.".into(),
            }],
            None,
        )
        .expect("summary");
        let replacement =
            validate_compaction_output(&prepared, &model, &[], &output).expect("replacement");
        assert!(replacement.atoms.len() < prepared.input().request().atoms.len());
        let wire = encode_request(&model, &replacement, &[], Some(model.max_output_tokens()))
            .expect("replacement wire");
        let prefix = match api {
            "openai_responses" => &wire["input"][0]["content"],
            "openai_chat_completions" => &wire["messages"][0]["content"],
            "anthropic_messages" => &wire["messages"][0]["content"][0]["text"],
            "google_generate_content" => &wire["contents"][0]["parts"][0]["text"],
            _ => unreachable!("fixture dialect"),
        };
        assert_eq!(prefix, rules, "{api}");
        assert_eq!(wire.to_string().matches("EXACT_WORKSPACE_RULES").count(), 1);
        assert_eq!(
            prepared.plan().environment(),
            &request_environment(&model, &[], Some(model.max_output_tokens()))
        );
    }
}
