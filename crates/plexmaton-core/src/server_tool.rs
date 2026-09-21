//! Tools a provider runs on its own side, inside the model call the owner already authorised.

use serde::{Deserialize, Serialize};

/// One capability a model's route may run for it, named by what it does rather than by any
/// dialect's spelling.
///
/// Configuration declares a subset per model and the selected dialect spells each one (PRV-6).
/// Nothing infers a declaration from a provider or model name: that would put authority in a
/// name, which is what PRV-6 exists to refuse.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerTool {
    /// Search the public web, and open or search within a page it found.
    WebSearch,
}

impl ServerTool {
    /// The one spelling this tool has: what configuration declares and the transcript shows.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::WebSearch => "web_search",
        }
    }
}

/// How a server tool call ended, as the provider reported it.
///
/// A call reaches the record only after it has ended: the provider ran it inside the model call
/// and reports the result, so there is no queued or running state here for anything to project.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerToolStatus {
    /// The provider finished the call and the model went on with what it found.
    Completed,
    /// The provider gave the call up and the model went on without its result.
    Failed,
}

/// What a server tool did, as the provider reported it.
///
/// Findings are not here: a route returns them or it does not, and a transcript shows what
/// arrived. A search may carry no query, because one route flattens an opened page into a search
/// with an empty one, and an action that arrived is carried rather than refused (PRV-5).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerToolAction {
    /// A web search. Routes spell the query once or as a list; the record keeps each once.
    Search {
        /// The queries the provider searched for, in the order reported; may be empty.
        queries: Vec<String>,
    },
    /// The provider opened a page it had found.
    OpenPage {
        /// The page the provider opened.
        url: String,
    },
    /// The provider searched within a page it had opened.
    FindInPage {
        /// The page the provider searched within.
        url: String,
        /// The text it looked for.
        pattern: String,
    },
}

impl ServerToolAction {
    /// Provider-authored text this action carries, bounded by the record the way tool arguments
    /// are: a query or a URL is the provider's to make long and ours to keep bounded.
    #[must_use]
    pub fn text_bytes(&self) -> usize {
        match self {
            Self::Search { queries } => queries
                .iter()
                .fold(0_usize, |total, query| total.saturating_add(query.len())),
            Self::OpenPage { url } => url.len(),
            Self::FindInPage { url, pattern } => url.len().saturating_add(pattern.len()),
        }
    }
}

/// One call the provider made on its own side and finished before reporting it.
///
/// It is never a request for this harness to run anything, so it reaches no admission and no
/// scheduler: the call already happened inside the model call the owner authorised by
/// configuring the route.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerToolCall {
    /// The declared capability the provider used.
    pub tool: ServerTool,
    /// What it did with it.
    pub action: ServerToolAction,
    /// How it ended.
    pub status: ServerToolStatus,
}

#[cfg(test)]
mod tests {
    use super::{ServerTool, ServerToolAction};

    /// PRV-6: the name configuration declares is the name the record and the screen use.
    #[test]
    fn the_tool_name_is_its_configuration_spelling() {
        let declared = serde_json::to_value(ServerTool::WebSearch)
            .unwrap_or_else(|error| panic!("serialize: {error}"));
        assert_eq!(
            declared,
            serde_json::Value::from(ServerTool::WebSearch.name())
        );
    }

    #[test]
    fn action_text_bytes_count_every_provider_authored_string() {
        assert_eq!(
            ServerToolAction::Search {
                queries: vec!["ab".to_owned(), "cde".to_owned()]
            }
            .text_bytes(),
            5
        );
        assert_eq!(
            ServerToolAction::Search {
                queries: Vec::new()
            }
            .text_bytes(),
            0
        );
        assert_eq!(
            ServerToolAction::OpenPage {
                url: "https://x".to_owned()
            }
            .text_bytes(),
            9
        );
        assert_eq!(
            ServerToolAction::FindInPage {
                url: "u".to_owned(),
                pattern: "pat".to_owned()
            }
            .text_bytes(),
            4
        );
    }
}
