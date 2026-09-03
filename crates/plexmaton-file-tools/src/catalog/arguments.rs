use serde::{Deserialize, Serialize};

use crate::{
    ReadRequest, SearchRequest,
    mutation::{CanonicalEdit, CreateArguments, EditArguments},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadArguments {
    pub(super) path: String,
    #[serde(
        default = "default_read_offset",
        deserialize_with = "deserialize_read_offset"
    )]
    pub(super) offset: u64,
    #[serde(
        default = "default_read_limit",
        deserialize_with = "deserialize_read_limit"
    )]
    pub(super) limit: u16,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchArguments {
    pub(super) pattern: String,
    #[serde(
        default = "default_search_path",
        deserialize_with = "deserialize_search_path"
    )]
    pub(super) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) glob: Option<String>,
    #[serde(
        default = "default_search_limit",
        deserialize_with = "deserialize_search_limit"
    )]
    pub(super) limit: u16,
}

pub(super) fn parse_read(raw: &str) -> Option<ReadArguments> {
    let arguments: ReadArguments = serde_json::from_str(raw).ok()?;
    ReadRequest::new(
        arguments.path.clone(),
        Some(arguments.offset),
        Some(arguments.limit),
    )
    .ok()?;
    Some(arguments)
}

pub(super) fn parse_search(raw: &str) -> Option<SearchArguments> {
    let arguments: SearchArguments = serde_json::from_str(raw).ok()?;
    SearchRequest::new(
        arguments.pattern.clone(),
        Some(arguments.path.clone()),
        arguments.glob.clone(),
        Some(arguments.limit),
    )
    .ok()?;
    Some(arguments)
}

pub(super) fn parse_edit(raw: &str) -> Option<EditArguments> {
    serde_json::from_str(raw).ok()
}

pub(super) fn parse_create(raw: &str) -> Option<CreateArguments> {
    serde_json::from_str(raw).ok()
}

pub(super) fn parse_canonical_edit(raw: &str) -> Option<CanonicalEdit> {
    serde_json::from_str(raw).ok()
}

pub(super) fn canonical_read(arguments: &ReadArguments) -> Option<String> {
    serde_json::to_string(arguments).ok()
}

pub(super) fn canonical_search(arguments: &SearchArguments) -> Option<String> {
    serde_json::to_string(arguments).ok()
}

pub(super) fn canonical_edit(arguments: &CanonicalEdit) -> Option<String> {
    serde_json::to_string(arguments).ok()
}

pub(super) fn canonical_create(arguments: &CreateArguments) -> Option<String> {
    serde_json::to_string(arguments).ok()
}

const fn default_read_offset() -> u64 {
    1
}

const fn default_read_limit() -> u16 {
    200
}

fn default_search_path() -> String {
    ".".to_owned()
}

fn deserialize_read_offset<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u64>::deserialize(deserializer).map(|value| value.unwrap_or_else(default_read_offset))
}

const fn default_search_limit() -> u16 {
    100
}

fn deserialize_read_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u16>::deserialize(deserializer).map(|value| value.unwrap_or_else(default_read_limit))
}

fn deserialize_search_path<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
        .map(|value| value.unwrap_or_else(default_search_path))
}

fn deserialize_search_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u16>::deserialize(deserializer).map(|value| value.unwrap_or_else(default_search_limit))
}

#[cfg(test)]
mod tests {
    use plexmaton_agent::{MAX_ADMITTED_ARGUMENT_BYTES, MAX_REQUESTED_TOOL_ARGUMENT_BYTES};
    use serde_json::json;

    use super::canonical_edit;
    use crate::mutation::{
        ByteSplice, CanonicalEdit, MAX_MUTATION_EDITS, MAX_MUTATION_SOURCE_BYTES,
    };

    /// MUT-6: the retained reserve covers the greatest structural expansion of sixteen frozen
    /// splices; string escaping is identical on the raw and canonical sides.
    #[test]
    fn maximum_edit_canonical_structure_fits_the_one_kibibyte_reserve() {
        let observation = "obs-0123456789abcdef";
        let edits = (0..MAX_MUTATION_EDITS)
            .map(|_| json!({"old_text": "a", "new_text": ""}))
            .collect::<Vec<_>>();
        let raw = json!({
            "path": "p",
            "observation": observation,
            "edits": edits,
        })
        .to_string();
        let canonical = CanonicalEdit {
            path: "p".to_owned(),
            observation: observation.to_owned(),
            source_len: MAX_MUTATION_SOURCE_BYTES,
            splices: (0..MAX_MUTATION_EDITS)
                .map(|_| ByteSplice {
                    start: MAX_MUTATION_SOURCE_BYTES - 1,
                    end: MAX_MUTATION_SOURCE_BYTES,
                    expected: "a".to_owned(),
                    replacement: String::new(),
                })
                .collect(),
        };
        let canonical = canonical_edit(&canonical)
            .unwrap_or_else(|| panic!("serialize maximal canonical structure"));
        let expansion = canonical
            .len()
            .checked_sub(raw.len())
            .unwrap_or_else(|| panic!("canonical structure should be the larger shape"));

        assert_eq!(
            MAX_ADMITTED_ARGUMENT_BYTES - MAX_REQUESTED_TOOL_ARGUMENT_BYTES,
            1024
        );
        assert!(expansion < 1024, "structural expansion was {expansion}");
    }
}
