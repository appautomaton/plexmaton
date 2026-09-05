use serde::{Deserialize, Serialize};

/// Maximum retained UTF-8 bytes in a skill name.
pub const MAX_SKILL_NAME_BYTES: usize = 256;
/// Maximum retained Unicode scalar values in a skill name.
pub const MAX_SKILL_NAME_CHARS: usize = 64;
/// Maximum retained UTF-8 bytes in a canonical skill source location.
pub const MAX_SKILL_LOCATION_BYTES: usize = 4 * 1024;
/// Maximum exact instruction bytes retained by one explicit activation.
pub const MAX_SKILL_INSTRUCTION_BYTES: usize = 256 * 1024;

/// Durable source category for activated skill content.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Plexmaton's project-local `.plexmaton/skills` root.
    ProjectNative,
    /// The project's shared `.agents/skills` root.
    ProjectShared,
    /// The user's Plexmaton skills root.
    User,
}

/// Exact skill content admitted to one model request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SkillActivation {
    name: String,
    source: SkillSource,
    location: String,
    digest: String,
    instructions: String,
}

impl SkillActivation {
    /// Validates one durable activation before it can enter the journal or an input queue.
    pub fn new(
        name: String,
        source: SkillSource,
        location: String,
        digest: String,
        instructions: String,
    ) -> Result<Self, SkillActivationError> {
        if name.is_empty() {
            return Err(SkillActivationError::EmptyName);
        }
        if name.len() > MAX_SKILL_NAME_BYTES || name.chars().count() > MAX_SKILL_NAME_CHARS {
            return Err(SkillActivationError::NameTooLong);
        }
        if location.is_empty() {
            return Err(SkillActivationError::EmptyLocation);
        }
        if location.len() > MAX_SKILL_LOCATION_BYTES {
            return Err(SkillActivationError::LocationTooLong);
        }
        if digest.len() != 64
            || !digest
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err(SkillActivationError::InvalidDigest);
        }
        if instructions.len() > MAX_SKILL_INSTRUCTION_BYTES {
            return Err(SkillActivationError::InstructionsTooLarge);
        }
        Ok(Self {
            name,
            source,
            location,
            digest,
            instructions,
        })
    }

    /// Validated skill name used for this activation.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Root category from which the exact content was loaded.
    #[must_use]
    pub const fn source(&self) -> SkillSource {
        self.source
    }

    /// Canonical source location of the selected skill's `SKILL.md`.
    ///
    /// The location is provenance retained for source matching; holding it grants no file authority.
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }

    /// Lowercase hexadecimal SHA-256 digest of the loaded content.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Exact instruction text admitted to the model request.
    #[must_use]
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        self.name
            .len()
            .checked_add(self.location.len())?
            .checked_add(self.digest.len())?
            .checked_add(self.instructions.len())
    }
}

impl<'de> Deserialize<'de> for SkillActivation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            name: String,
            source: SkillSource,
            location: String,
            digest: String,
            instructions: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.name,
            wire.source,
            wire.location,
            wire.digest,
            wire.instructions,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Why skill content could not become a durable activation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillActivationError {
    EmptyName,
    NameTooLong,
    EmptyLocation,
    LocationTooLong,
    InvalidDigest,
    InstructionsTooLarge,
}

impl std::fmt::Display for SkillActivationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyName => "skill activation name is empty",
            Self::NameTooLong => "skill activation name exceeds its character or byte bound",
            Self::EmptyLocation => "skill activation source location is empty",
            Self::LocationTooLong => "skill activation source location exceeds its byte bound",
            Self::InvalidDigest => "skill activation digest is not lowercase hexadecimal SHA-256",
            Self::InstructionsTooLarge => "skill activation instructions exceed their byte bound",
        })
    }
}

impl std::error::Error for SkillActivationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn activation(instructions: String) -> Result<SkillActivation, SkillActivationError> {
        SkillActivation::new(
            "review".to_owned(),
            SkillSource::ProjectShared,
            "/workspace/.agents/skills/review/SKILL.md".to_owned(),
            "a".repeat(64),
            instructions,
        )
    }

    /// SKL-5: wire replay retains exact admitted content and rechecks every semantic bound.
    #[test]
    fn skl_5_activation_round_trip_is_exact_and_decode_revalidates_bounds() {
        let exact = activation("preserve\r\nthis\0text".to_owned()).expect("valid activation");
        let json = serde_json::to_string(&exact).expect("encode activation");
        let decoded: SkillActivation = serde_json::from_str(&json).expect("decode activation");
        assert_eq!(decoded, exact);
        assert_eq!(decoded.instructions(), "preserve\r\nthis\0text");

        let mut value = serde_json::to_value(&exact).expect("activation value");
        value["digest"] = serde_json::Value::String("A".repeat(64));
        assert!(serde_json::from_value::<SkillActivation>(value).is_err());
        assert_eq!(
            activation("x".repeat(MAX_SKILL_INSTRUCTION_BYTES + 1)),
            Err(SkillActivationError::InstructionsTooLarge)
        );
    }

    #[test]
    fn activation_name_and_location_bounds_are_independent() {
        assert_eq!(
            SkillActivation::new(
                "review".to_owned(),
                SkillSource::User,
                String::new(),
                "0".repeat(64),
                String::new(),
            ),
            Err(SkillActivationError::EmptyLocation)
        );
        assert_eq!(
            SkillActivation::new(
                "x".repeat(MAX_SKILL_NAME_BYTES + 1),
                SkillSource::User,
                "/home/user/.plexmaton/skills/review/SKILL.md".to_owned(),
                "0".repeat(64),
                String::new(),
            ),
            Err(SkillActivationError::NameTooLong)
        );
        assert_eq!(
            SkillActivation::new(
                "x".repeat(MAX_SKILL_NAME_CHARS + 1),
                SkillSource::User,
                "/home/user/.plexmaton/skills/review/SKILL.md".to_owned(),
                "0".repeat(64),
                String::new(),
            ),
            Err(SkillActivationError::NameTooLong)
        );
        assert_eq!(
            SkillActivation::new(
                "review".to_owned(),
                SkillSource::User,
                "x".repeat(MAX_SKILL_LOCATION_BYTES + 1),
                "0".repeat(64),
                String::new(),
            ),
            Err(SkillActivationError::LocationTooLong)
        );
    }
}
