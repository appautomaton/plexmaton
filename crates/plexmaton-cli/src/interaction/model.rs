//! The two settings the composer can change, and what each one costs.
//!
//! Model and effort share one admission boundary in the runtime (MDL-1), so they share one place
//! here: both resolve a configured pair, both report through the workspace, and neither touches the
//! transcript. Resolution stays explicit — only a confirmed pair reaches the runtime, which owns
//! admission.

use crate::session_picker::ConversationPicker;
use plexmaton_provider::resolve_api_key;
use plexmaton_runtime::{LiveRuntime, ModelReplacement};
use plexmaton_tui::{ModelChange, Workspace};

/// Applies whichever of the two settings this outcome carried, and reports each result.
pub(super) fn apply_settings(
    outcome: &plexmaton_tui::Outcome,
    runtime: &mut LiveRuntime,
    workspace: &mut Workspace,
    picker: &ConversationPicker,
    status_line: &mut Option<crate::statusline::StatusLine>,
) {
    if let Some(change) = &outcome.model {
        let result = apply_model(change, runtime, picker);
        if let Ok(replacement) = &result {
            workspace.set_effort_choices(
                replacement
                    .model
                    .allowed_reasoning_efforts()
                    .map(<[_]>::to_vec),
            );
            if let Some(status) = status_line {
                status.mark_dirty();
            }
            // MDL-1: the switch already happened and nothing was destroyed, so this is a receipt
            // rather than a question. It costs the conversation no record (COM-3).
            if replacement.degraded_history {
                workspace.report_degraded_history();
            }
        }
        workspace.report_model(
            result.map(|replacement| crate::configuration_summary(&replacement.model)),
        );
    }
    if let Some(change) = &outcome.effort {
        let result = runtime
            .set_reasoning_effort(&change.agent, change.effort)
            .map_err(|refusal| refusal.to_string())
            .map(|model| crate::configuration_summary(&model));
        if result.is_ok()
            && let Some(status) = status_line
        {
            status.mark_dirty();
        }
        workspace.report_effort(result);
    }
}

fn apply_model(
    change: &ModelChange,
    runtime: &mut LiveRuntime,
    picker: &ConversationPicker,
) -> Result<ModelReplacement, String> {
    let model = picker
        .models()
        .model(&change.identity.provider, &change.identity.model)
        .ok_or_else(|| "This configured model is no longer available.".to_owned())?;
    let key = resolve_api_key(model, std::env::var_os(model.api_key_env()))
        .map_err(|_| "The selected provider credential is missing or invalid.".to_owned())?;
    runtime
        .set_model(&change.agent, model.clone(), key)
        .map_err(|error| error.to_string())
}
