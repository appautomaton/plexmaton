use plexmaton_core::{TranscriptItemId, TurnId};

use crate::{
    AssistantBlock, AssistantOutput, AssistantReplay, ModelStepId, ProviderCodecId,
    ProviderCodecRevision, ProviderModelFamilyId, ProviderReplay, ProviderReplayOwnerId,
    ReplayCompatibility, ToolCall,
};

pub(crate) fn replay_compatibility() -> ReplayCompatibility {
    ReplayCompatibility::new(
        ProviderReplayOwnerId::new("test-route")
            .unwrap_or_else(|error| panic!("fixture replay owner: {error:?}")),
        ProviderCodecId::new("openai_responses")
            .unwrap_or_else(|error| panic!("fixture replay codec: {error:?}")),
        ProviderCodecRevision::new(1)
            .unwrap_or_else(|error| panic!("fixture replay revision: {error:?}")),
        ProviderModelFamilyId::new("test-model")
            .unwrap_or_else(|error| panic!("fixture model family: {error:?}")),
    )
}

pub(crate) fn replay(payload: &str) -> ProviderReplay {
    ProviderReplay::new(replay_compatibility(), payload.to_owned())
        .unwrap_or_else(|error| panic!("fixture replay: {error:?}"))
}

pub(crate) fn step(turn: &str, index: u16) -> ModelStepId {
    let turn_id = TurnId::new(turn).unwrap_or_else(|error| panic!("fixture turn: {error}"));
    ModelStepId::new(turn_id, index)
}

pub(crate) fn text_block(item: &str, text: &str) -> AssistantBlock {
    AssistantBlock::Text {
        item_id: TranscriptItemId::new(item)
            .unwrap_or_else(|error| panic!("fixture transcript item: {error}")),
        text: text.to_owned(),
    }
}

pub(crate) fn reasoning_block(item: &str, text: &str) -> AssistantBlock {
    AssistantBlock::Reasoning {
        item_id: TranscriptItemId::new(item)
            .unwrap_or_else(|error| panic!("fixture transcript item: {error}")),
        text: text.to_owned(),
    }
}

pub(crate) fn call_block(item: &str, call: ToolCall) -> AssistantBlock {
    AssistantBlock::ToolCall {
        item_id: TranscriptItemId::new(item)
            .unwrap_or_else(|error| panic!("fixture transcript item: {error}")),
        call,
    }
}

pub(crate) fn output(blocks: Vec<AssistantBlock>) -> AssistantOutput {
    AssistantOutput::new(blocks, None)
        .unwrap_or_else(|error| panic!("fixture assistant output: {error}"))
}

pub(crate) fn output_with_replay(
    blocks: Vec<AssistantBlock>,
    attachments: impl IntoIterator<Item = (u16, ProviderReplay)>,
) -> AssistantOutput {
    let replay = AssistantReplay::from_positioned(attachments)
        .unwrap_or_else(|error| panic!("fixture assistant replay: {error}"));
    AssistantOutput::new(blocks, replay)
        .unwrap_or_else(|error| panic!("fixture assistant output: {error}"))
}
