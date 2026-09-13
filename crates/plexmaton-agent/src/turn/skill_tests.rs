use plexmaton_core::{AgentId, ConversationEvent, HeadName};

use super::Agent;
use crate::{
    ContextAtomValue, Effect, Input, JournalEntryPayload, JournalRecord, ModelEvent, Reaction,
    SkillActivation, SkillSource, StopReason, UndeliveredReason,
};

fn agent() -> Agent {
    let mut agent = Agent::new(AgentId::new("agent-a").expect("agent id"));
    let _ = agent.announce("Agent A");
    agent
}

fn skill(instructions: &str) -> SkillActivation {
    named_skill("review", instructions)
}

fn named_skill(name: &str, instructions: &str) -> SkillActivation {
    SkillActivation::new(
        name.to_owned(),
        SkillSource::ProjectShared,
        format!("/workspace/.agents/skills/{name}/SKILL.md"),
        "b".repeat(64),
        instructions.to_owned(),
    )
    .expect("skill fixture")
}

fn append_payloads(reaction: &Reaction) -> Vec<&JournalEntryPayload> {
    reaction
        .records
        .iter()
        .filter_map(|record| match record {
            JournalRecord::AppendEntry { entry, .. } => Some(&entry.payload),
            JournalRecord::CreateHead { .. }
            | JournalRecord::MoveHead { .. }
            | JournalRecord::RenameHead { .. }
            | JournalRecord::AbandonHead { .. }
            | JournalRecord::ForkAndSelectHead { .. }
            | JournalRecord::SelectHead { .. }
            | JournalRecord::SetEntryLabel { .. }
            | JournalRecord::TurnFinished { .. }
            | JournalRecord::RequestAttemptAuthorized { .. }
            | JournalRecord::CompactionAttemptFinished { .. }
            | JournalRecord::RequestAttemptFinished { .. } => None,
        })
        .collect()
}

/// SKL-5: explicit instructions are a distinct durable atom and never alter visible user text.
#[test]
fn skl_5_explicit_submission_records_skill_separately_before_dispatch() {
    let mut agent = agent();
    let activation = skill("exact\r\ninstructions\0");
    let original = "$review inspect parser";

    let reaction = agent.handle(Input::SkillSubmitted {
        text: original.to_owned(),
        skill: activation.clone(),
    });

    assert!(matches!(
        append_payloads(&reaction).as_slice(),
        [
            JournalEntryPayload::TurnStarted { text, turn_id: started, .. },
            JournalEntryPayload::SkillActivated { turn_id: activated, activation: recorded, .. },
        ] if text == original && started == activated && recorded == &activation
    ));
    let deltas: Vec<_> = reaction
        .events
        .iter()
        .filter_map(|event| match &event.event {
            ConversationEvent::TranscriptDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, [original]);

    let [Effect::CallModel(call)] = reaction.effects.as_slice() else {
        panic!("explicit submission must dispatch exactly once");
    };
    assert!(matches!(
        call.request.atoms.as_slice(),
        [user, activated]
            if matches!(user.value(), ContextAtomValue::User { text } if text == original)
                && activated.value() == &ContextAtomValue::Skill(activation.clone())
    ));
    let replay = agent
        .journal()
        .project(&HeadName::new("main").expect("head"))
        .expect("replay projection");
    assert_eq!(replay.request(), &call.request);
}

/// SKL-5: both queue boundaries retain the exact activation until their semantic input is claimed.
#[test]
fn skl_5_queued_turn_and_steering_retain_skill_activation() {
    let mut agent = agent();
    let _ = agent.handle(Input::Submitted {
        text: "first".to_owned(),
    });
    let queued_turn_skill = skill("queued turn instructions");
    assert_eq!(
        agent.handle(Input::SkillSubmitted {
            text: "$review second".to_owned(),
            skill: queued_turn_skill.clone(),
        }),
        Reaction::default()
    );
    let step_id = agent.active_model_step().expect("active first step");
    let boundary = agent.handle(Input::Streamed {
        step_id,
        event: ModelEvent::Stopped(StopReason::EndOfTurn),
    });
    let payloads = append_payloads(&boundary);
    let turn_pair = payloads
        .windows(2)
        .find(|pair| matches!(pair, [JournalEntryPayload::TurnStarted { text, .. }, JournalEntryPayload::SkillActivated { activation, .. }] if text == "$review second" && activation == &queued_turn_skill));
    assert!(
        turn_pair.is_some(),
        "queued activation follows its turn input"
    );
    assert!(
        boundary
            .released_inputs
            .iter()
            .any(|input| { input.text() == "$review second" && input.skill() == Some("review") })
    );

    let queued_steering_skill = skill("queued steering instructions");
    assert_eq!(
        agent.handle(Input::SkillSteered {
            text: "$review also tests".to_owned(),
            skill: queued_steering_skill.clone(),
        }),
        Reaction::default()
    );
    let turn_id = agent
        .active_model_step()
        .expect("queued turn opened")
        .turn_id()
        .clone();
    let mut claimed = Reaction::default();
    agent.claim_next_step_input(&turn_id, &mut claimed);
    assert!(matches!(
        append_payloads(&claimed).as_slice(),
        [
            JournalEntryPayload::SteeringAccepted { text, turn_id: steered, .. },
            JournalEntryPayload::SkillActivated { turn_id: activated, activation, .. },
        ] if text == "$review also tests"
            && steered == activated
            && activation == &queued_steering_skill
    ));
    assert!(
        claimed.released_inputs.iter().any(|input| {
            input.text() == "$review also tests" && input.skill() == Some("review")
        })
    );
    assert!(matches!(
        agent.record().as_slice(),
        [first, second, second_skill, steering, steering_skill]
            if matches!(first.value(), ContextAtomValue::User { text } if text == "first")
                && matches!(second.value(), ContextAtomValue::User { text } if text == "$review second")
                && second_skill.value() == &ContextAtomValue::Skill(queued_turn_skill)
                && matches!(steering.value(), ContextAtomValue::User { text } if text == "$review also tests")
                && steering_skill.value() == &ContextAtomValue::Skill(queued_steering_skill)
    ));
}

/// SKL-5: cancellation returns exactly what the user typed even when queued context was attached.
#[test]
fn skl_5_interruption_returns_original_skill_invocation_text() {
    let mut idle = agent();
    let refused = "$review cannot steer idle";
    let no_turn = idle.handle(Input::SkillSteered {
        text: refused.to_owned(),
        skill: skill("unused steering"),
    });
    assert!(no_turn.undelivered.iter().any(|input| {
        input.text == refused
            && input.skill.as_deref() == Some("review")
            && input.reason == UndeliveredReason::NoActiveTurn
    }));
    let numeric = idle.handle(Input::SkillSteered {
        text: "$100 request".to_owned(),
        skill: named_skill("100", "numeric selection"),
    });
    assert!(numeric.undelivered.iter().any(|input| {
        input.text == "$100 request"
            && input.skill.as_deref() == Some("100")
            && input.reason == UndeliveredReason::NoActiveTurn
    }));

    let mut agent = agent();
    let _ = agent.handle(Input::Submitted {
        text: "first".to_owned(),
    });
    let original = "$review retain this exactly";
    let _ = agent.handle(Input::SkillSubmitted {
        text: original.to_owned(),
        skill: skill("never dispatched"),
    });

    let interrupted = agent.handle(Input::Interrupted);
    assert!(interrupted.undelivered.iter().any(|input| {
        input.text == original
            && input.skill.as_deref() == Some("review")
            && input.reason == UndeliveredReason::Interrupted
    }));
}
