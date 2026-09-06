//! Literal command facts and token scopes, independent of a shell parser or executor (PER-10).
use std::ops::Range;

use serde::{Deserialize, Serialize};

const MAX_LITERAL_BYTES: usize = 24 * 1024;
const MAX_COMMANDS: usize = 32;
const MAX_ARGUMENTS: usize = 128;
const MAX_PREFIX_ARGUMENTS: usize = 32;
const MAX_PREFIX_BYTES: usize = 4096;

/// One complete literal command, with its original span for explanation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiteralCommand {
    arguments: Vec<String>,
    span: Range<usize>,
}

impl LiteralCommand {
    /// Validates bounds for parser-issued facts; only the trusted adapter interprets shell syntax.
    #[must_use]
    pub fn new(arguments: Vec<String>, span: Range<usize>) -> Option<Self> {
        if !valid_arguments(&arguments, MAX_ARGUMENTS, MAX_LITERAL_BYTES)
            || span.is_empty()
            || span.end > MAX_LITERAL_BYTES
        {
            return None;
        }
        Some(Self { arguments, span })
    }

    /// Literal argv, including empty or quoted arguments without splitting them again.
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// Byte range in the immutable admitted source, excluding separating shell operators.
    #[must_use]
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }
}

/// A complete supported sequence. Partial parses cannot enter prefix evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiteralShell(Vec<LiteralCommand>);

impl LiteralShell {
    /// Validates a finite ordered decomposition of one admitted source.
    #[must_use]
    pub fn new(commands: Vec<LiteralCommand>, source_length: usize) -> Option<Self> {
        if commands.is_empty()
            || commands.len() > MAX_COMMANDS
            || source_length > MAX_LITERAL_BYTES
            || commands.last()?.span.end > source_length
            || commands
                .windows(2)
                .any(|pair| pair[0].span.end > pair[1].span.start)
            || commands
                .iter()
                .flat_map(|command| &command.arguments)
                .map(String::len)
                .sum::<usize>()
                > MAX_LITERAL_BYTES
        {
            return None;
        }
        Some(Self(commands))
    }

    /// Every operation whose literal arguments are understood; never a best-effort subset.
    #[must_use]
    pub fn commands(&self) -> &[LiteralCommand] {
        &self.0
    }
}

/// Why the catalog could not supply a reusable literal interpretation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrefixUnavailable {
    /// Expansion, redirects, control flow or other syntax outside the supported literal subset.
    UnsupportedSyntax,
    /// Input, parse work, traversal or literal-result capacity was exhausted.
    Limit,
    /// The pinned parser could not load its grammar.
    ParserUnavailable,
}

/// One catalog-issued interpretation of the whole admitted shell source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandSyntax {
    /// Fully lowered literal command sequence.
    Literal(LiteralShell),
    /// Exact source remains reviewable; no prefix matcher can authorize this script.
    ExactOnly(PrefixUnavailable),
}

impl CommandSyntax {
    pub(super) fn suggested_prefix(&self, context: [u8; 32]) -> Option<CommandPrefix> {
        let Self::Literal(literal) = self else {
            return None;
        };
        let [command] = literal.commands() else {
            return None;
        };
        let arguments = command.arguments();
        // These are offer floors, not an effect classifier. Never discard a leading option,
        // resolve a path basename, peel an environment wrapper or suggest a bare interpreter.
        let count = match arguments.first()?.as_str() {
            "ls" => 1,
            "git"
                if matches!(
                    arguments.get(1)?.as_str(),
                    "fetch" | "status" | "diff" | "log" | "show"
                ) =>
            {
                2
            }
            _ => return None,
        };
        CommandPrefix::new(arguments[..count].to_vec(), context)
    }

    pub(super) const fn exact_only_reason(&self) -> &'static str {
        match self {
            Self::Literal(_) => "no prefix suggestion for this command",
            Self::ExactOnly(PrefixUnavailable::UnsupportedSyntax) => {
                "shell syntax has no supported prefix"
            }
            Self::ExactOnly(PrefixUnavailable::Limit) => "prefix analysis reached its limit",
            Self::ExactOnly(PrefixUnavailable::ParserUnavailable) => {
                "prefix analysis is unavailable"
            }
        }
    }
}

/// A bounded literal argument prefix, bound to the command executor's captured context.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommandPrefix {
    arguments: Vec<String>,
    context: [u8; 32],
}

impl CommandPrefix {
    /// Keeps token boundaries and empty arguments; the executable itself must be nonempty.
    #[must_use]
    pub fn new(arguments: Vec<String>, context: [u8; 32]) -> Option<Self> {
        valid_arguments(&arguments, MAX_PREFIX_ARGUMENTS, MAX_PREFIX_BYTES)
            .then_some(Self { arguments, context })
    }

    /// Exact tokens selected for this scope, never a string split at dispatch.
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// Physical workspace, shell and captured environment fingerprint.
    #[must_use]
    pub const fn context(&self) -> &[u8; 32] {
        &self.context
    }

    pub(super) fn matches(&self, command: &LiteralCommand) -> bool {
        command.arguments.starts_with(&self.arguments)
    }

    pub(super) fn label(&self) -> String {
        self.arguments
            .iter()
            .map(|argument| {
                if !argument.is_empty()
                    && argument
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_./:-".contains(&byte))
                {
                    argument.clone()
                } else {
                    format!("{argument:?}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl<'de> Deserialize<'de> for CommandPrefix {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            arguments: Vec<String>,
            context: [u8; 32],
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.arguments, wire.context)
            .ok_or_else(|| serde::de::Error::custom("invalid bounded command prefix"))
    }
}

fn valid_arguments(arguments: &[String], count: usize, bytes: usize) -> bool {
    !arguments.is_empty()
        && arguments.len() <= count
        && !arguments[0].is_empty()
        && arguments.iter().all(|argument| !argument.contains('\0'))
        && arguments.iter().map(String::len).sum::<usize>() <= bytes
}
