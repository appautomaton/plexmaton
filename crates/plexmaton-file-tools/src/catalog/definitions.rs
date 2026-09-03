use serde_json::{Value, json};

use crate::mutation::{MAX_MUTATION_ARGUMENT_BYTES, MAX_MUTATION_EDITS};

pub const READ_TOOL_NAME: &str = "read_file";
pub const SEARCH_TOOL_NAME: &str = "search";
pub const EDIT_TOOL_NAME: &str = "edit_file";
pub const CREATE_TOOL_NAME: &str = "create_file";

pub(super) const READ_DEFINITION_ID: &str = "native-read-file-v1";
pub(super) const SEARCH_DEFINITION_ID: &str = "native-search-v1";
pub(super) const EDIT_DEFINITION_ID: &str = "native-edit-file-v1";
pub(super) const CREATE_DEFINITION_ID: &str = "native-create-file-v1";
pub(super) const DEFINITION_REVISION: u64 = 1;

/// Provider-neutral strict function definition.
#[derive(Clone, Debug, PartialEq)]
pub struct FileToolDefinition {
    name: &'static str,
    description: &'static str,
    parameters: Value,
}

impl FileToolDefinition {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.description
    }

    #[must_use]
    pub const fn parameters(&self) -> &Value {
        &self.parameters
    }
}

pub(super) fn definitions() -> [FileToolDefinition; 4] {
    [
        read_definition(),
        search_definition(),
        edit_definition(),
        create_definition(),
    ]
}

fn read_definition() -> FileToolDefinition {
    FileToolDefinition {
        name: READ_TOOL_NAME,
        description: "Read an exact UTF-8 line window from a file in the workspace. Returns an opaque observation required for later edits and a next offset when more text remains.",
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": { "type": "string", "maxLength": 4096, "description": "Workspace-relative file path." },
                "offset": { "type": ["integer", "null"], "minimum": 1, "description": "First line to return; null means 1." },
                "limit": { "type": ["integer", "null"], "minimum": 1, "maximum": 1000, "description": "Maximum lines to return; null means 200." }
            },
            "required": ["path", "offset", "limit"]
        }),
    }
}

fn search_definition() -> FileToolDefinition {
    FileToolDefinition {
        name: SEARCH_TOOL_NAME,
        description: "Search UTF-8 text in the workspace with ripgrep. Returns bounded file/line previews; refine the query when a result bound is reached.",
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "pattern": { "type": "string", "description": "Regular expression to search for." },
                "path": { "type": ["string", "null"], "minLength": 1, "maxLength": 4096, "description": "Workspace-relative file or directory; null means the workspace root." },
                "glob": { "type": ["string", "null"], "description": "Ripgrep glob filter for a directory search; null applies no filter." },
                "limit": { "type": ["integer", "null"], "minimum": 1, "maximum": 500, "description": "Maximum matches to return; null means 100." }
            },
            "required": ["pattern", "path", "glob", "limit"]
        }),
    }
}

fn edit_definition() -> FileToolDefinition {
    FileToolDefinition {
        name: EDIT_TOOL_NAME,
        description: "Atomically edit one existing workspace file named by a prior read observation. Each old_text must be non-empty and occur exactly once inside that observed byte window. All edits resolve against the same original file, may not overlap, and either all commit or none. Matching is byte-exact UTF-8; re-read after a stale_precondition result. This tool cannot create files.",
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": { "type": "string", "minLength": 1, "maxLength": 4096, "description": "Workspace-relative path returned by read_file." },
                "observation": { "type": "string", "minLength": 20, "maxLength": 20, "pattern": "^obs-[0-9a-f]{16}$", "description": "Opaque observation returned by read_file for this exact file window." },
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_MUTATION_EDITS,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "old_text": { "type": "string", "minLength": 1, "maxLength": MAX_MUTATION_ARGUMENT_BYTES, "description": "Exact observed UTF-8 text with enough context to be unique in the returned window." },
                            "new_text": { "type": "string", "maxLength": MAX_MUTATION_ARGUMENT_BYTES, "description": "Exact replacement text; empty deletes old_text." }
                        },
                        "required": ["old_text", "new_text"]
                    }
                }
            },
            "required": ["path", "observation", "edits"]
        }),
    }
}

fn create_definition() -> FileToolDefinition {
    FileToolDefinition {
        name: CREATE_TOOL_NAME,
        description: "Create one new UTF-8 file in an existing workspace directory. The operation fails if the leaf already exists, never follows a symbolic link, and never overwrites. Never use it after edit_file fails to replace an existing file.",
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "path": { "type": "string", "minLength": 1, "maxLength": 4096, "description": "Workspace-relative path whose parents already exist." },
                "content": { "type": "string", "maxLength": MAX_MUTATION_ARGUMENT_BYTES, "description": "Exact UTF-8 file contents." }
            },
            "required": ["path", "content"]
        }),
    }
}
