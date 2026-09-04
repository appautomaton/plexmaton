use std::fmt;

use serde::{Deserialize, Serialize};

use super::RequestTimingError;
use crate::ReplayCompatibility;

const FINGERPRINT_BYTES: usize = 32;

/// Fixed-width digest of request inputs that are not context atoms.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct RequestEnvironmentFingerprint([u8; FINGERPRINT_BYTES]);

impl RequestEnvironmentFingerprint {
    /// Retains the digest produced by the request adapter's canonical fingerprint algorithm.
    #[must_use]
    pub const fn new(bytes: [u8; FINGERPRINT_BYTES]) -> Self {
        Self(bytes)
    }

    /// Exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; FINGERPRINT_BYTES] {
        &self.0
    }
}

impl fmt::Debug for RequestEnvironmentFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "RequestEnvironmentFingerprint({self})")
    }
}

impl fmt::Display for RequestEnvironmentFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for RequestEnvironmentFingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RequestEnvironmentFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        decode_fingerprint(&value).map_err(serde::de::Error::custom)
    }
}

fn decode_fingerprint(value: &str) -> Result<RequestEnvironmentFingerprint, RequestTimingError> {
    if value.len() != FINGERPRINT_BYTES * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RequestTimingError::InvalidEnvironmentFingerprint);
    }
    let mut bytes = [0_u8; FINGERPRINT_BYTES];
    for (slot, pair) in bytes.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        *slot = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    Ok(RequestEnvironmentFingerprint::new(bytes))
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!("fingerprint grammar was checked before decoding"),
    }
}

/// Immutable non-context inputs that identify one encoded request environment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestEnvironment {
    compatibility: ReplayCompatibility,
    fingerprint: RequestEnvironmentFingerprint,
}

impl RequestEnvironment {
    /// Pairs the selected provider/model route with its canonical non-context digest.
    #[must_use]
    pub const fn new(
        compatibility: ReplayCompatibility,
        fingerprint: RequestEnvironmentFingerprint,
    ) -> Self {
        Self {
            compatibility,
            fingerprint,
        }
    }

    /// Provider route, codec revision and model family selected for this attempt.
    #[must_use]
    pub const fn compatibility(&self) -> &ReplayCompatibility {
        &self.compatibility
    }

    /// Digest of instructions, tools and other non-context request inputs.
    #[must_use]
    pub const fn fingerprint(&self) -> RequestEnvironmentFingerprint {
        self.fingerprint
    }
}
