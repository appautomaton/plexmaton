//! Typed provider profiles and pure home/config resolution.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

use plexmaton_agent::{
    MAX_ASSISTANT_TEXT_BYTES, ProviderCodecId, ProviderCodecRevision, ProviderModelFamilyId,
    ProviderReplayOwnerId, ReplayCompatibility,
};

const DEFAULT_MAX_RETAINED_OUTPUT_BYTES: usize = 1024 * 1024;

/// The selected OpenAI-compatible wire protocol.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Responses,
    ChatCompletions,
}

/// Provider reasoning effort, kept explicit so unsupported values fail at the boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
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

/// The transport family named by a profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenaiCompatible,
}

/// One named provider profile from the user-owned configuration root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderProfile {
    name: String,
    kind: ProviderKind,
    protocol: Protocol,
    base_url: String,
    model: String,
    api_key_env: String,
    reasoning_effort: ReasoningEffort,
    max_retained_output_bytes: usize,
}

/// User-owned configuration containing an explicit active profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderConfig {
    active_provider: String,
    providers: BTreeMap<String, ProviderProfile>,
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
struct RawProviderConfig {
    active_provider: String,
    providers: BTreeMap<String, RawProviderProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProviderProfile {
    kind: ProviderKind,
    protocol: Protocol,
    base_url: String,
    model: String,
    api_key_env: String,
    reasoning_effort: ReasoningEffort,
    #[serde(default = "default_max_retained_output_bytes")]
    max_retained_output_bytes: usize,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid provider configuration: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("active provider profile `{0}` does not exist")]
    UnknownActiveProvider(String),
    #[error("provider profile `{profile}` has an empty `{field}`")]
    EmptyField {
        profile: String,
        field: &'static str,
    },
    #[error("provider profile `{0}` has an invalid environment-variable name")]
    InvalidApiKeyEnvironment(String),
    #[error("provider profile `{0}` must retain at least one output byte")]
    ZeroRetainedOutputBound(String),
    #[error("provider profile `{profile}` retains more than the {limit}-byte semantic bound")]
    RetainedOutputBoundTooLarge { profile: String, limit: usize },
    #[error("provider profile `{0}` has an invalid replay compatibility identity")]
    InvalidReplayIdentity(String),
    #[error("cannot resolve the Plexmaton user configuration root")]
    HomeUnavailable,
    #[error("provider API key environment variable `{0}` is absent")]
    MissingApiKeyEnvironment(String),
    #[error("provider API key environment variable `{0}` is not a header-safe value")]
    InvalidApiKeyValue(String),
}

impl ProviderConfig {
    /// Parses and validates a configuration without reading process-global state.
    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        let raw: RawProviderConfig = toml::from_str(source)?;
        let config = Self {
            active_provider: raw.active_provider,
            providers: raw
                .providers
                .into_iter()
                .map(|(name, profile)| {
                    let profile_name = name.clone();
                    (
                        name,
                        ProviderProfile {
                            name: profile_name,
                            kind: profile.kind,
                            protocol: profile.protocol,
                            base_url: profile.base_url,
                            model: profile.model,
                            api_key_env: profile.api_key_env,
                            reasoning_effort: profile.reasoning_effort,
                            max_retained_output_bytes: profile.max_retained_output_bytes,
                        },
                    )
                })
                .collect(),
        };
        config.validate()?;
        Ok(config)
    }

    /// Name selected by this validated configuration.
    #[must_use]
    pub fn active_name(&self) -> &str {
        &self.active_provider
    }

    /// Validated active profile.
    #[must_use]
    pub fn active(&self) -> &ProviderProfile {
        // Only `parse` constructs this type, and it validates this key before publishing it.
        match self.providers.get(&self.active_provider) {
            Some(profile) => profile,
            None => unreachable!("validated active provider is present"),
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let Some(profile) = self.providers.get(&self.active_provider) else {
            return Err(ConfigError::UnknownActiveProvider(
                self.active_provider.clone(),
            ));
        };

        for (field, value) in [
            ("base_url", profile.base_url.as_str()),
            ("model", profile.model.as_str()),
            ("api_key_env", profile.api_key_env.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ConfigError::EmptyField {
                    profile: self.active_provider.clone(),
                    field,
                });
            }
        }
        if profile.max_retained_output_bytes == 0 {
            return Err(ConfigError::ZeroRetainedOutputBound(
                self.active_provider.clone(),
            ));
        }
        if profile.max_retained_output_bytes > MAX_ASSISTANT_TEXT_BYTES {
            return Err(ConfigError::RetainedOutputBoundTooLarge {
                profile: self.active_provider.clone(),
                limit: MAX_ASSISTANT_TEXT_BYTES,
            });
        }
        if !is_environment_name(&profile.api_key_env) {
            return Err(ConfigError::InvalidApiKeyEnvironment(
                self.active_provider.clone(),
            ));
        }
        if ProviderReplayOwnerId::new(profile.replay_owner_value()).is_err()
            || ProviderModelFamilyId::new(profile.model.clone()).is_err()
        {
            return Err(ConfigError::InvalidReplayIdentity(
                self.active_provider.clone(),
            ));
        }
        Ok(())
    }
}

impl ProviderProfile {
    #[must_use]
    pub const fn kind(&self) -> ProviderKind {
        self.kind
    }

    #[must_use]
    pub const fn protocol(&self) -> Protocol {
        self.protocol
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
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
    pub const fn max_retained_output_bytes(&self) -> usize {
        self.max_retained_output_bytes
    }

    /// Adapter-owned realm in which opaque replay remains valid.
    #[must_use]
    pub fn replay_compatibility(&self) -> ReplayCompatibility {
        let owner = ProviderReplayOwnerId::new(self.replay_owner_value())
            .unwrap_or_else(|_| unreachable!("provider config validates replay owner"));
        let codec = ProviderCodecId::new(match self.protocol {
            Protocol::Responses => "openai_responses",
            Protocol::ChatCompletions => "openai_chat_completions",
        })
        .unwrap_or_else(|_| unreachable!("static codec identity is valid"));
        let revision = ProviderCodecRevision::new(1)
            .unwrap_or_else(|_| unreachable!("static codec revision is valid"));
        let family = ProviderModelFamilyId::new(self.model.clone())
            .unwrap_or_else(|_| unreachable!("provider config validates model family"));
        ReplayCompatibility::new(owner, codec, revision, family)
    }

    fn replay_owner_value(&self) -> String {
        format!(
            "openai_compatible:{}",
            serde_json::json!([self.name, self.base_url, self.api_key_env])
        )
    }
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

/// Validates a value read from the profile's named environment variable without reading globals.
pub fn resolve_api_key(
    profile: &ProviderProfile,
    value: Option<OsString>,
) -> Result<ApiKey, ConfigError> {
    let environment = profile.api_key_env().to_owned();
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

const fn default_max_retained_output_bytes() -> usize {
    DEFAULT_MAX_RETAINED_OUTPUT_BYTES
}

fn is_environment_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, path::Path};

    use super::{
        ConfigError, Protocol, ProviderConfig, ReasoningEffort, resolve_api_key, resolve_home,
    };

    const LOCAL_CONFIG: &str = r#"
active_provider = "local_luna"

[providers.local_luna]
kind = "openai_compatible"
protocol = "responses"
base_url = "http://127.0.0.1:8317/v1"
model = "gpt-5.6-luna"
api_key_env = "PLEXMATON_LOCAL_API_KEY"
reasoning_effort = "xhigh"
"#;

    #[test]
    fn prv_6_parses_a_named_profile_without_inline_authority() {
        let config = ProviderConfig::parse(LOCAL_CONFIG).expect("valid local profile");

        assert_eq!(config.active_name(), "local_luna");
        assert_eq!(config.active().protocol(), Protocol::Responses);
        assert_eq!(config.active().reasoning_effort(), ReasoningEffort::Xhigh);
        assert_eq!(config.active().max_retained_output_bytes(), 1024 * 1024);
    }

    #[test]
    fn prv_6_rejects_an_inline_api_key() {
        let source = LOCAL_CONFIG.replace(
            "api_key_env = \"PLEXMATON_LOCAL_API_KEY\"",
            "api_key_env = \"PLEXMATON_LOCAL_API_KEY\"\napi_key = \"inline-secret-value\"",
        );

        assert!(matches!(
            ProviderConfig::parse(&source),
            Err(ConfigError::Toml(_))
        ));

        let key_as_name = LOCAL_CONFIG.replace("PLEXMATON_LOCAL_API_KEY", "not-an-env-name");
        assert!(matches!(
            ProviderConfig::parse(&key_as_name),
            Err(ConfigError::InvalidApiKeyEnvironment(_))
        ));
    }

    #[test]
    fn prv_2_and_prv_7_reject_output_bounds_the_journal_cannot_retain() {
        let source = LOCAL_CONFIG.replace(
            "reasoning_effort = \"xhigh\"",
            &format!(
                "reasoning_effort = \"xhigh\"\nmax_retained_output_bytes = {}",
                plexmaton_agent::MAX_ASSISTANT_TEXT_BYTES + 1
            ),
        );

        assert!(matches!(
            ProviderConfig::parse(&source),
            Err(ConfigError::RetainedOutputBoundTooLarge { .. })
        ));
    }

    #[test]
    fn prv_6_resolves_only_an_override_or_the_user_root() {
        assert_eq!(
            resolve_home(
                Some(OsStr::new(".local/plexmaton")),
                Some(Path::new("/users/ac"))
            )
            .expect("explicit development root"),
            Path::new(".local/plexmaton")
        );
        assert_eq!(
            resolve_home(None, Some(Path::new("/users/ac"))).expect("user root"),
            Path::new("/users/ac/.plexmaton")
        );
        assert!(matches!(
            resolve_home(None, None),
            Err(ConfigError::HomeUnavailable)
        ));
    }

    #[test]
    fn prv_6_key_resolution_is_explicit_and_redacted() {
        let config = ProviderConfig::parse(LOCAL_CONFIG).expect("valid local profile");
        assert!(matches!(
            resolve_api_key(config.active(), None),
            Err(ConfigError::MissingApiKeyEnvironment(environment))
                if environment == "PLEXMATON_LOCAL_API_KEY"
        ));
        assert!(matches!(
            resolve_api_key(config.active(), Some("not header safe".into())),
            Err(ConfigError::InvalidApiKeyValue(_))
        ));
        let key = resolve_api_key(config.active(), Some("fixture-secret".into()))
            .expect("header-safe fixture key");
        assert_eq!(key.expose(), "fixture-secret");
        assert_eq!(format!("{key:?}"), "ApiKey([REDACTED])");
    }

    #[test]
    fn prv_3_replay_route_owner_encoding_is_unambiguous() {
        let source = |name: &str, base_url: &str| {
            format!(
                r#"
active_provider = "{name}"
[providers."{name}"]
kind = "openai_compatible"
protocol = "responses"
base_url = "{base_url}"
model = "gpt-5.6-luna"
api_key_env = "KEY"
reasoning_effort = "low"
"#
            )
        };
        let first = ProviderConfig::parse(&source("a|b", "https://x"))
            .expect("first delimiter-bearing route");
        let second = ProviderConfig::parse(&source("a", "b|https://x"))
            .expect("second delimiter-bearing route");

        assert_ne!(
            first.active().replay_compatibility().owner(),
            second.active().replay_compatibility().owner()
        );
    }
}
