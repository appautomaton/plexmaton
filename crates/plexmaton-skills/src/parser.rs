use std::{collections::HashSet, fmt};

use serde::de::{Error as _, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use unicode_normalization::UnicodeNormalization as _;

use crate::{
    MAX_FRONTMATTER_BYTES, SkillInvocationPolicy, SkillMetadataError, SkillMetadataField,
    SkillName, types::MAX_SKILL_DESCRIPTION_CHARACTERS,
};

const OPENING_DELIMITER_LF: &[u8] = b"---\n";
const OPENING_DELIMITER_CRLF: &[u8] = b"---\r\n";

pub(crate) struct ParsedSkill {
    pub(crate) name: SkillName,
    pub(crate) description: String,
    pub(crate) invocation: SkillInvocationPolicy,
    pub(crate) body_offset: usize,
}

#[derive(Default)]
struct Frontmatter {
    name: Option<ScalarValue>,
    description: Option<ScalarValue>,
    disable_model_invocation: Option<ScalarValue>,
    user_invocable: Option<ScalarValue>,
}

enum ScalarValue {
    String(String),
    Bool(bool),
    Other,
}

impl<'de> Deserialize<'de> for ScalarValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ScalarValueVisitor)
    }
}

struct ScalarValueVisitor;

impl<'de> Visitor<'de> for ScalarValueVisitor {
    type Value = ScalarValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a scalar skill metadata value")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(ScalarValue::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(ScalarValue::String(value))
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(ScalarValue::Bool(value))
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(ScalarValue::Other)
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(ScalarValue::Other)
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(ScalarValue::Other)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(ScalarValue::Other)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(ScalarValue::Other)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<IgnoredAny>()?.is_some() {}
        Ok(ScalarValue::Other)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(ScalarValue::Other)
    }
}

impl<'de> Deserialize<'de> for Frontmatter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(FrontmatterVisitor)
    }
}

struct FrontmatterVisitor;

impl<'de> Visitor<'de> for FrontmatterVisitor {
    type Value = Frontmatter;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an Agent Skills frontmatter mapping")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut parsed = Frontmatter::default();
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(A::Error::custom(format!("duplicate field {key}")));
            }
            match key.as_str() {
                "name" => parsed.name = Some(map.next_value()?),
                "description" => parsed.description = Some(map.next_value()?),
                "disable-model-invocation" => {
                    parsed.disable_model_invocation = Some(map.next_value()?);
                }
                "user-invocable" => parsed.user_invocable = Some(map.next_value()?),
                _ => {
                    let _ignored = map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(parsed)
    }
}

pub(crate) fn parse_skill(
    bytes: &[u8],
    complete: bool,
    directory_name: &str,
) -> Result<ParsedSkill, SkillMetadataError> {
    let opening_end = if bytes.starts_with(OPENING_DELIMITER_LF) {
        OPENING_DELIMITER_LF.len()
    } else if bytes.starts_with(OPENING_DELIMITER_CRLF) {
        OPENING_DELIMITER_CRLF.len()
    } else {
        return Err(SkillMetadataError::MissingFrontmatter);
    };
    let (frontmatter_end, body_offset) =
        closing_delimiter(bytes, opening_end, complete).ok_or({
            if complete {
                SkillMetadataError::MissingFrontmatter
            } else {
                SkillMetadataError::FrontmatterTooLarge
            }
        })?;
    if frontmatter_end.saturating_sub(opening_end) > MAX_FRONTMATTER_BYTES {
        return Err(SkillMetadataError::FrontmatterTooLarge);
    }
    let yaml = std::str::from_utf8(&bytes[opening_end..frontmatter_end])
        .map_err(|_| SkillMetadataError::InvalidUtf8)?;
    let parsed: Frontmatter =
        yaml_serde::from_str(yaml).map_err(|_| SkillMetadataError::InvalidYaml)?;
    metadata(parsed, directory_name, body_offset)
}

fn closing_delimiter(bytes: &[u8], mut cursor: usize, complete: bool) -> Option<(usize, usize)> {
    while cursor <= bytes.len() {
        let relative_end = bytes[cursor..].iter().position(|byte| *byte == b'\n');
        if relative_end.is_none() && !complete {
            return None;
        }
        let line_end = relative_end.map_or(bytes.len(), |offset| cursor + offset);
        let content_end = line_end.checked_sub(usize::from(
            line_end > cursor && bytes[line_end - 1] == b'\r',
        ))?;
        if &bytes[cursor..content_end] == b"---" {
            let body_offset = if line_end < bytes.len() {
                line_end + 1
            } else {
                line_end
            };
            return Some((cursor, body_offset));
        }
        let Some(next) = relative_end.map(|offset| cursor + offset + 1) else {
            break;
        };
        cursor = next;
    }
    None
}

fn metadata(
    parsed: Frontmatter,
    directory_name: &str,
    body_offset: usize,
) -> Result<ParsedSkill, SkillMetadataError> {
    let name_text = required_string(parsed.name, SkillMetadataField::Name)?;
    let name =
        SkillName::new(&name_text).map_err(|error| SkillMetadataError::InvalidName { error })?;
    let directory_normalized: String = directory_name.nfkc().collect();
    if name.as_str() != directory_normalized {
        return Err(SkillMetadataError::NameDirectoryMismatch);
    }
    let description = required_string(parsed.description, SkillMetadataField::Description)?;
    if description.trim().is_empty() {
        return Err(SkillMetadataError::EmptyDescription);
    }
    if description.chars().count() > MAX_SKILL_DESCRIPTION_CHARACTERS {
        return Err(SkillMetadataError::DescriptionTooLong);
    }
    Ok(ParsedSkill {
        name,
        description,
        invocation: SkillInvocationPolicy {
            model: !optional_bool(
                parsed.disable_model_invocation,
                SkillMetadataField::DisableModelInvocation,
            )?
            .unwrap_or(false),
            user: optional_bool(parsed.user_invocable, SkillMetadataField::UserInvocable)?
                .unwrap_or(true),
        },
        body_offset,
    })
}

fn required_string(
    value: Option<ScalarValue>,
    field: SkillMetadataField,
) -> Result<String, SkillMetadataError> {
    match value {
        Some(ScalarValue::String(value)) => Ok(value),
        Some(ScalarValue::Bool(_) | ScalarValue::Other) => {
            Err(SkillMetadataError::InvalidFieldType { field })
        }
        None => Err(SkillMetadataError::MissingField { field }),
    }
}

fn optional_bool(
    value: Option<ScalarValue>,
    field: SkillMetadataField,
) -> Result<Option<bool>, SkillMetadataError> {
    match value {
        Some(ScalarValue::Bool(value)) => Ok(Some(value)),
        Some(ScalarValue::String(_) | ScalarValue::Other) => {
            Err(SkillMetadataError::InvalidFieldType { field })
        }
        None => Ok(None),
    }
}
