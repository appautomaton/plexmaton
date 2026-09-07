//! Resolve only the explicitly confirmed configured pair; the runtime owns admission.
use crate::session_picker::ConversationPicker;
use plexmaton_provider::{ResolvedModel, resolve_api_key};
use plexmaton_runtime::LiveRuntime;
use plexmaton_tui::ModelChange;

pub(super) fn apply_model(
    change: &ModelChange,
    runtime: &mut LiveRuntime,
    picker: &ConversationPicker,
) -> Result<ResolvedModel, String> {
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
