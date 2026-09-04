use std::{fmt, num::NonZeroU16};

use serde::{Deserialize, Serialize, ser::SerializeStruct};

/// Maximum opaque provider replay bytes retained for one model output.
pub const MAX_PROVIDER_REPLAY_BYTES: usize = 256 * 1024;
const MAX_REPLAY_ID_BYTES: usize = 1024;

/// Why opaque replay identity or payload was refused before entering turn state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderReplayError {
    EmptyOwner,
    EmptyCodec,
    EmptyModelFamily,
    IdentityTooLarge,
    ZeroCodecRevision,
    EmptyPayload,
    PayloadTooLarge,
}

macro_rules! replay_id {
    ($name:ident, $empty:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ProviderReplayError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(ProviderReplayError::$empty);
                }
                if value.len() > MAX_REPLAY_ID_BYTES {
                    return Err(ProviderReplayError::IdentityTooLarge);
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value)
                    .map_err(|_| serde::de::Error::custom(concat!($description, " is invalid")))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

replay_id!(
    ProviderReplayOwnerId,
    EmptyOwner,
    "Stable provider route/credential realm allowed to replay opaque data."
);
replay_id!(
    ProviderCodecId,
    EmptyCodec,
    "Stable identity of the codec that interprets opaque replay data."
);
replay_id!(
    ProviderModelFamilyId,
    EmptyModelFamily,
    "Adapter-defined model family allowed to reuse opaque replay data."
);

/// Revision of one codec's private replay grammar.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProviderCodecRevision(NonZeroU16);

impl ProviderCodecRevision {
    pub fn new(value: u16) -> Result<Self, ProviderReplayError> {
        NonZeroU16::new(value)
            .map(Self)
            .ok_or(ProviderReplayError::ZeroCodecRevision)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl<'de> Deserialize<'de> for ProviderCodecRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(|_| serde::de::Error::custom("codec revision must be non-zero"))
    }
}

/// Complete validity realm for provider-private replay data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayCompatibility {
    owner: ProviderReplayOwnerId,
    codec: ProviderCodecId,
    codec_revision: ProviderCodecRevision,
    model_family: ProviderModelFamilyId,
}

impl ReplayCompatibility {
    #[must_use]
    pub const fn new(
        owner: ProviderReplayOwnerId,
        codec: ProviderCodecId,
        codec_revision: ProviderCodecRevision,
        model_family: ProviderModelFamilyId,
    ) -> Self {
        Self {
            owner,
            codec,
            codec_revision,
            model_family,
        }
    }

    #[must_use]
    pub const fn owner(&self) -> &ProviderReplayOwnerId {
        &self.owner
    }

    #[must_use]
    pub const fn codec(&self) -> &ProviderCodecId {
        &self.codec
    }

    #[must_use]
    pub const fn codec_revision(&self) -> ProviderCodecRevision {
        self.codec_revision
    }

    #[must_use]
    pub const fn model_family(&self) -> &ProviderModelFamilyId {
        &self.model_family
    }
}

/// One bounded opaque provider item associated with a model-output position.
#[derive(Clone, Eq, PartialEq)]
pub struct ProviderReplay {
    compatible_with: ReplayCompatibility,
    payload: String,
}

impl ProviderReplay {
    pub fn new(
        compatible_with: ReplayCompatibility,
        payload: String,
    ) -> Result<Self, ProviderReplayError> {
        if payload.is_empty() {
            return Err(ProviderReplayError::EmptyPayload);
        }
        if payload.len() > MAX_PROVIDER_REPLAY_BYTES {
            return Err(ProviderReplayError::PayloadTooLarge);
        }
        Ok(Self {
            compatible_with,
            payload,
        })
    }

    #[must_use]
    pub const fn compatible_with(&self) -> &ReplayCompatibility {
        &self.compatible_with
    }

    #[must_use]
    pub fn payload(&self) -> &str {
        &self.payload
    }

    pub(crate) fn into_parts(self) -> (ReplayCompatibility, String) {
        (self.compatible_with, self.payload)
    }
}

impl fmt::Debug for ProviderReplay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderReplay")
            .field("compatible_with", &self.compatible_with)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}

impl Serialize for ProviderReplay {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("ProviderReplay", 2)?;
        state.serialize_field("compatible_with", &self.compatible_with)?;
        state.serialize_field("payload", &self.payload)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ProviderReplay {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            compatible_with: ReplayCompatibility,
            payload: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.compatible_with, wire.payload).map_err(|error| {
            let message = match error {
                ProviderReplayError::EmptyPayload => "provider replay payload is empty",
                ProviderReplayError::PayloadTooLarge => "provider replay exceeds its byte bound",
                _ => "provider replay is invalid",
            };
            serde::de::Error::custom(message)
        })
    }
}
