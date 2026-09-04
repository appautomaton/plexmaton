//! Typed model registry and pure home/config resolution.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

pub use plexmaton_agent::TokenEstimator;
use plexmaton_agent::{
    ProviderCodecId, ProviderCodecRevision, ProviderModelFamilyId, ProviderReplayOwnerId,
    ReplayCompatibility,
};

#[cfg(test)]
mod tests;

/// Exact model API whose request and response grammar an adapter owns.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelApi {
    OpenaiResponses,
    OpenaiChatCompletions,
}

impl ModelApi {
    pub(crate) const fn codec_id(self) -> &'static str {
        match self {
            Self::OpenaiResponses => "openai_responses",
            Self::OpenaiChatCompletions => "openai_chat_completions",
        }
    }
}

/// Provider reasoning effort selected for one resolved model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningEffort {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}

/// Optional price snapshot expressed in US dollars per million tokens.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCost {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

impl ModelCost {
    #[must_use]
    pub const fn input(&self) -> f64 {
        self.input
    }

    #[must_use]
    pub const fn output(&self) -> f64 {
        self.output
    }

    #[must_use]
    pub const fn cache_read(&self) -> f64 {
        self.cache_read
    }

    #[must_use]
    pub const fn cache_write(&self) -> f64 {
        self.cache_write
    }

    fn validate(&self, provider: &str, model: &str) -> Result<(), ConfigError> {
        for (field, value) in [
            ("input", self.input),
            ("output", self.output),
            ("cache_read", self.cache_read),
            ("cache_write", self.cache_write),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ConfigError::InvalidCost {
                    provider: provider.to_owned(),
                    model: model.to_owned(),
                    field,
                });
            }
        }
        Ok(())
    }
}

/// Exact provider/model pair chosen from the user-owned registry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelSelection {
    provider: String,
    model: String,
}

impl ModelSelection {
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
}

/// One credential-blind model profile after provider defaults and model overrides resolve.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedModel {
    provider_name: String,
    model_name: String,
    api: ModelApi,
    base_url: String,
    wire_id: String,
    display_name: String,
    api_key_env: String,
    reasoning_effort: ReasoningEffort,
    context_window_tokens: u32,
    max_output_tokens: u32,
    output_reserve_tokens: u32,
    token_estimator: TokenEstimator,
    cost: Option<ModelCost>,
}

/// User-owned provider routes and the models reachable through each one.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRegistry {
    active: ModelSelection,
    models: BTreeMap<(String, String), ResolvedModel>,
}

/// Provider bearer credential whose ordinary debug representation is always redacted.
pub struct ApiKey(String);

impl ApiKey {
    /// Exposes the credential only at the HTTP authorization boundary.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey([REDACTED])")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModelRegistry {
    active_model: ModelSelection,
    providers: BTreeMap<String, RawProvider>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    base_url: String,
    api_key_env: String,
    api: Option<ModelApi>,
    models: BTreeMap<String, RawModel>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    id: String,
    display_name: Option<String>,
    api: Option<ModelApi>,
    reasoning_effort: ReasoningEffort,
    context_window_tokens: u32,
    max_output_tokens: u32,
    output_reserve_tokens: u32,
    #[serde(default)]
    token_estimator: TokenEstimator,
    cost: Option<ModelCost>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid model configuration")]
    Toml,
    #[error("active provider `{0}` does not exist")]
    UnknownActiveProvider(String),
    #[error("active model `{model}` does not exist under provider `{provider}`")]
    UnknownActiveModel { provider: String, model: String },
    #[error("provider `{0}` has no models")]
    EmptyProvider(String),
    #[error("provider `{provider}` has an empty `{field}`")]
    EmptyProviderField {
        provider: String,
        field: &'static str,
    },
    #[error("provider `{0}` has an invalid environment-variable name")]
    InvalidApiKeyEnvironment(String),
    #[error("provider `{0}` has an invalid base URL")]
    InvalidBaseUrl(String),
    #[error("model `{model}` under provider `{provider}` has an empty `{field}`")]
    EmptyModelField {
        provider: String,
        model: String,
        field: &'static str,
    },
    #[error("model `{model}` under provider `{provider}` has no API dialect")]
    MissingModelApi { provider: String, model: String },
    #[error("model `{model}` under provider `{provider}` has invalid token limits")]
    InvalidTokenLimits { provider: String, model: String },
    #[error("model `{model}` under provider `{provider}` has invalid `{field}` pricing")]
    InvalidCost {
        provider: String,
        model: String,
        field: &'static str,
    },
    #[error("model `{model}` under provider `{provider}` has an invalid replay identity")]
    InvalidReplayIdentity { provider: String, model: String },
    #[error("cannot resolve the Plexmaton user configuration root")]
    HomeUnavailable,
    #[error("provider API key environment variable `{0}` is absent")]
    MissingApiKeyEnvironment(String),
    #[error("provider API key environment variable `{0}` is not a header-safe value")]
    InvalidApiKeyValue(String),
}

impl ModelRegistry {
    /// Parses every provider/model entry and publishes one immutable resolved registry.
    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        let raw: RawModelRegistry = toml::from_str(source).map_err(|_| ConfigError::Toml)?;
        let mut models = BTreeMap::new();
        for (provider_name, provider) in raw.providers {
            let base_url = validate_provider(&provider_name, &provider)?;
            let RawProvider {
                base_url: _,
                api_key_env,
                api,
                models: provider_models,
            } = provider;
            for (model_name, model) in provider_models {
                let resolved = ResolvedModel::resolve(
                    provider_name.clone(),
                    model_name.clone(),
                    &base_url,
                    &api_key_env,
                    api,
                    model,
                )?;
                models.insert((provider_name.clone(), model_name), resolved);
            }
        }
        if !models
            .keys()
            .any(|(provider, _)| provider == raw.active_model.provider())
        {
            return Err(ConfigError::UnknownActiveProvider(
                raw.active_model.provider,
            ));
        }
        if !models.contains_key(&(
            raw.active_model.provider.clone(),
            raw.active_model.model.clone(),
        )) {
            return Err(ConfigError::UnknownActiveModel {
                provider: raw.active_model.provider,
                model: raw.active_model.model,
            });
        }
        Ok(Self {
            active: raw.active_model,
            models,
        })
    }

    /// Exact selected provider/model identity.
    #[must_use]
    pub const fn active_selection(&self) -> &ModelSelection {
        &self.active
    }

    /// Validated active resolved model.
    #[must_use]
    pub fn active_model(&self) -> &ResolvedModel {
        self.models
            .get(&(self.active.provider.clone(), self.active.model.clone()))
            .unwrap_or_else(|| unreachable!("validated active model remains present"))
    }

    /// Resolves one exact provider/model pair without fuzzy matching.
    #[must_use]
    pub fn model(&self, provider: &str, model: &str) -> Option<&ResolvedModel> {
        self.models.get(&(provider.to_owned(), model.to_owned()))
    }
}

impl ResolvedModel {
    fn resolve(
        provider_name: String,
        model_name: String,
        base_url: &str,
        api_key_env: &str,
        provider_api: Option<ModelApi>,
        model: RawModel,
    ) -> Result<Self, ConfigError> {
        let api = model
            .api
            .or(provider_api)
            .ok_or_else(|| ConfigError::MissingModelApi {
                provider: provider_name.clone(),
                model: model_name.clone(),
            })?;
        let display_name = model.display_name.unwrap_or_else(|| model.id.clone());
        let resolved = Self {
            provider_name,
            model_name,
            api,
            base_url: base_url.to_owned(),
            wire_id: model.id,
            display_name,
            api_key_env: api_key_env.to_owned(),
            reasoning_effort: model.reasoning_effort,
            context_window_tokens: model.context_window_tokens,
            max_output_tokens: model.max_output_tokens,
            output_reserve_tokens: model.output_reserve_tokens,
            token_estimator: model.token_estimator,
            cost: model.cost,
        };
        resolved.validate()?;
        Ok(resolved)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        for (field, value) in [
            ("id", self.wire_id.as_str()),
            ("display_name", self.display_name.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ConfigError::EmptyModelField {
                    provider: self.provider_name.clone(),
                    model: self.model_name.clone(),
                    field,
                });
            }
        }
        if self.context_window_tokens == 0
            || self.max_output_tokens == 0
            || self.output_reserve_tokens == 0
            || self.max_output_tokens >= self.context_window_tokens
            || self.output_reserve_tokens > self.max_output_tokens
        {
            return Err(ConfigError::InvalidTokenLimits {
                provider: self.provider_name.clone(),
                model: self.model_name.clone(),
            });
        }
        if let Some(cost) = &self.cost {
            cost.validate(&self.provider_name, &self.model_name)?;
        }
        if ProviderReplayOwnerId::new(self.replay_owner_value()).is_err()
            || ProviderModelFamilyId::new(self.wire_id.clone()).is_err()
        {
            return Err(ConfigError::InvalidReplayIdentity {
                provider: self.provider_name.clone(),
                model: self.model_name.clone(),
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn provider_name(&self) -> &str {
        &self.provider_name
    }

    #[must_use]
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    #[must_use]
    pub const fn api(&self) -> ModelApi {
        self.api
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub fn wire_id(&self) -> &str {
        &self.wire_id
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    #[must_use]
    pub fn api_key_env(&self) -> &str {
        &self.api_key_env
    }

    #[must_use]
    pub const fn reasoning_effort(&self) -> ReasoningEffort {
        self.reasoning_effort
    }

    #[must_use]
    pub const fn context_window_tokens(&self) -> u32 {
        self.context_window_tokens
    }

    #[must_use]
    pub const fn max_output_tokens(&self) -> u32 {
        self.max_output_tokens
    }

    #[must_use]
    pub const fn output_reserve_tokens(&self) -> u32 {
        self.output_reserve_tokens
    }

    #[must_use]
    pub const fn token_estimator(&self) -> TokenEstimator {
        self.token_estimator
    }

    #[must_use]
    pub const fn cost(&self) -> Option<&ModelCost> {
        self.cost.as_ref()
    }

    /// Adapter-owned realm in which opaque replay remains valid.
    #[must_use]
    pub fn replay_compatibility(&self) -> ReplayCompatibility {
        let owner = ProviderReplayOwnerId::new(self.replay_owner_value())
            .unwrap_or_else(|_| unreachable!("model registry validates replay owner"));
        let codec = ProviderCodecId::new(self.api.codec_id())
            .unwrap_or_else(|_| unreachable!("static codec identity is valid"));
        let revision = ProviderCodecRevision::new(1)
            .unwrap_or_else(|_| unreachable!("static codec revision is valid"));
        let family = ProviderModelFamilyId::new(self.wire_id.clone())
            .unwrap_or_else(|_| unreachable!("model registry validates model family"));
        ReplayCompatibility::new(owner, codec, revision, family)
    }

    fn replay_owner_value(&self) -> String {
        format!(
            "provider_route:{}",
            serde_json::json!([self.provider_name, self.base_url, self.api_key_env])
        )
    }
}

fn validate_provider(name: &str, provider: &RawProvider) -> Result<String, ConfigError> {
    for (field, value) in [
        ("name", name),
        ("base_url", provider.base_url.as_str()),
        ("api_key_env", provider.api_key_env.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ConfigError::EmptyProviderField {
                provider: name.to_owned(),
                field,
            });
        }
    }
    if !is_environment_name(&provider.api_key_env) {
        return Err(ConfigError::InvalidApiKeyEnvironment(name.to_owned()));
    }
    let base_url =
        Url::parse(&provider.base_url).map_err(|_| ConfigError::InvalidBaseUrl(name.to_owned()))?;
    if !matches!(base_url.scheme(), "http" | "https")
        || !base_url.has_host()
        || base_url.cannot_be_a_base()
        || !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
    {
        return Err(ConfigError::InvalidBaseUrl(name.to_owned()));
    }
    if provider.models.is_empty() {
        return Err(ConfigError::EmptyProvider(name.to_owned()));
    }
    for model in provider.models.keys() {
        if model.trim().is_empty() {
            return Err(ConfigError::EmptyModelField {
                provider: name.to_owned(),
                model: model.clone(),
                field: "name",
            });
        }
    }
    Ok(base_url.to_string())
}

/// Resolves `PLEXMATON_HOME`, or the user-level `~/.plexmaton` default.
///
/// Project-local `.plexmaton` discovery is deliberately absent: project corpus belongs in
/// `.agents/`, while provider authority remains user-owned (PRV-6).
pub fn resolve_home(
    plexmaton_home: Option<&OsStr>,
    user_home: Option<&Path>,
) -> Result<PathBuf, ConfigError> {
    if let Some(root) = plexmaton_home.filter(|root| !root.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    user_home
        .map(|root| root.join(".plexmaton"))
        .ok_or(ConfigError::HomeUnavailable)
}

/// Validates a value read from the resolved model's named environment variable without globals.
pub fn resolve_api_key(
    model: &ResolvedModel,
    value: Option<OsString>,
) -> Result<ApiKey, ConfigError> {
    let environment = model.api_key_env().to_owned();
    let Some(value) = value else {
        return Err(ConfigError::MissingApiKeyEnvironment(environment));
    };
    let value = value
        .into_string()
        .map_err(|_| ConfigError::InvalidApiKeyValue(environment.clone()))?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ConfigError::InvalidApiKeyValue(environment));
    }
    Ok(ApiKey(value))
}

fn is_environment_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
