//! Read-only projection of runtime skill metadata into primary-composer choices.

use plexmaton_agent::SkillSource;
use plexmaton_runtime::LiveRuntime;
use plexmaton_tui::{SkillChoice, SkillChoiceSource, Workspace};

pub(super) fn sync_choices(runtime: &LiveRuntime, workspace: &mut Workspace) {
    let choices = runtime
        .user_skills()
        .into_iter()
        .map(|skill| SkillChoice {
            name: skill.name,
            description: skill.description,
            source: match skill.source {
                SkillSource::ProjectNative => SkillChoiceSource::ProjectNative,
                SkillSource::ProjectShared => SkillChoiceSource::ProjectShared,
                SkillSource::User => SkillChoiceSource::User,
            },
        })
        .collect();
    workspace.set_skills(choices);
}
