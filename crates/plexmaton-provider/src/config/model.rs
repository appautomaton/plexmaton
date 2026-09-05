//! Validated model request options and replay identity, independent of registry lookup.
use super::{
    ConfigError, ModelApi, ModelCost, PromptCache, RawModel, ReasoningEffort, TokenEstimator,
};
use plexmaton_agent::{
    ProviderCodecId, ProviderCodecRevision, ProviderModelFamilyId, ProviderReplayOwnerId,
    ReplayCompatibility,
};

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
    instructions: String,
    prompt_cache: PromptCache,
    context_window_tokens: u32,
    max_output_tokens: u32,
    output_reserve_tokens: u32,
    token_estimator: TokenEstimator,
    cost: Option<ModelCost>,
}

impl ResolvedModel {
    pub(super) fn resolve(
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
            instructions: model.instructions,
            prompt_cache: model.prompt_cache,
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
        if self.instructions.len() > 64 * 1024 {
            return Err(self.invalid_option("instructions"));
        }
        if self.api == ModelApi::GoogleGenerateContent {
            if matches!(
                self.reasoning_effort,
                ReasoningEffort::None | ReasoningEffort::Xhigh | ReasoningEffort::Max
            ) {
                return Err(self.invalid_option("reasoning_effort"));
            }
            if !self
                .wire_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            {
                return Err(self.invalid_option("id"));
            }
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

    fn invalid_option(&self, field: &'static str) -> ConfigError {
        ConfigError::InvalidRequestOption {
            provider: self.provider_name.clone(),
            model: self.model_name.clone(),
            field,
        }
    }

    /// Stable explicit instructions, rendered in the selected dialect's instruction slot.
    #[must_use]
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    #[must_use]
    pub const fn prompt_cache(&self) -> PromptCache {
        self.prompt_cache
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
