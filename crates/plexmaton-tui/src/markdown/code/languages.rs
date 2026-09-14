//! Explicit bundled grammar admission. Fence text is never a filename or executable.
use std::sync::OnceLock;

use tree_sitter_highlight::HighlightConfiguration;

use super::{CodeRole, PlainCode};

pub(super) const CAPTURES: &[(&str, CodeRole)] = &[
    ("keyword", CodeRole::Keyword),
    ("type", CodeRole::Type),
    ("constructor", CodeRole::Type),
    ("function", CodeRole::Function),
    ("string", CodeRole::String),
    ("escape", CodeRole::String),
    ("number", CodeRole::Constant),
    ("constant", CodeRole::Constant),
    ("boolean", CodeRole::Constant),
    ("comment", CodeRole::Comment),
    ("property", CodeRole::Property),
    ("string.special.key", CodeRole::Property),
    ("attribute", CodeRole::Type),
    ("tag", CodeRole::Type),
    ("operator", CodeRole::Text),
    ("punctuation", CodeRole::Text),
    ("variable", CodeRole::Text),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Language {
    Rust,
    Python,
    Json,
    JavaScript,
    TypeScript,
    Tsx,
    Bash,
}

impl Language {
    pub(super) fn from_info(info: &str) -> Result<Self, PlainCode> {
        match info.to_ascii_lowercase().as_str() {
            "" | "text" | "txt" | "plaintext" => Err(PlainCode::Unspecified),
            "rust" | "rs" => Ok(Self::Rust),
            "python" | "py" | "python3" => Ok(Self::Python),
            "json" | "jsonc" => Ok(Self::Json),
            "javascript" | "js" | "jsx" => Ok(Self::JavaScript),
            "typescript" | "ts" => Ok(Self::TypeScript),
            "tsx" => Ok(Self::Tsx),
            "bash" | "sh" | "shell" => Ok(Self::Bash),
            _ => Err(PlainCode::UnknownLanguage),
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Rust => 0,
            Self::Python => 1,
            Self::Json => 2,
            Self::JavaScript => 3,
            Self::TypeScript => 4,
            Self::Tsx => 5,
            Self::Bash => 6,
        }
    }

    /// A compiled query is a pure function of a bundled constant, so each grammar is compiled at
    /// most once per process and then only read. Compiling per render cost more than highlighting
    /// by two orders of magnitude; a failed compile stays failed instead of retrying every block.
    pub(super) fn configuration(self) -> Result<&'static HighlightConfiguration, PlainCode> {
        static COMPILED: [OnceLock<Option<HighlightConfiguration>>; 7] =
            [const { OnceLock::new() }; 7];
        COMPILED[self.index()]
            .get_or_init(|| self.compile())
            .as_ref()
            .ok_or(PlainCode::Unavailable)
    }

    fn compile(self) -> Option<HighlightConfiguration> {
        let js = tree_sitter_javascript::HIGHLIGHT_QUERY;
        let jsx = tree_sitter_javascript::JSX_HIGHLIGHT_QUERY;
        let ts = tree_sitter_typescript::HIGHLIGHTS_QUERY;
        let (language, query, locals) = match self {
            Self::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY.to_owned(),
                "",
            ),
            Self::Python => (
                tree_sitter_python::LANGUAGE.into(),
                tree_sitter_python::HIGHLIGHTS_QUERY.to_owned(),
                "",
            ),
            // Later same-node captures win: keep JSON object keys distinct from string values.
            Self::Json => (
                tree_sitter_json::LANGUAGE.into(),
                format!(
                    "{}\n(pair key: (string) @property)",
                    tree_sitter_json::HIGHLIGHTS_QUERY
                ),
                "",
            ),
            Self::JavaScript => (
                tree_sitter_javascript::LANGUAGE.into(),
                format!("{js}\n{jsx}"),
                tree_sitter_javascript::LOCALS_QUERY,
            ),
            Self::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                format!("{js}\n{ts}"),
                tree_sitter_typescript::LOCALS_QUERY,
            ),
            Self::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                format!("{js}\n{jsx}\n{ts}"),
                tree_sitter_typescript::LOCALS_QUERY,
            ),
            Self::Bash => (
                tree_sitter_bash::LANGUAGE.into(),
                tree_sitter_bash::HIGHLIGHT_QUERY.to_owned(),
                "",
            ),
        };
        let mut config =
            HighlightConfiguration::new(language, "fenced-code", &query, "", locals).ok()?;
        config.configure(&CAPTURES.iter().map(|(name, _)| *name).collect::<Vec<_>>());
        Some(config)
    }
}
