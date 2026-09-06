//! The draft's grammar: which listing a draft asks for, its query and the token the query is
//! in, and whether the whole draft is a Command (CMD-2).
use super::Listing;

/// A `/name` typed into a conversation's input and run from there (ui-ux §product vocabulary).
///
/// What the conversation's own input can do: start or resume a conversation, or compact this
/// one. Everything wider than a conversation is a Drawer page, never a Command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    New,
    Resume,
    Compact,
}

impl Command {
    /// Every Command, in the order the menu lists them.
    pub const ALL: [Self; 3] = [Self::New, Self::Resume, Self::Compact];

    /// The name after the slash.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Resume => "resume",
            Self::Compact => "compact",
        }
    }

    /// What `Enter` does, shown beside the name.
    #[must_use]
    pub const fn summary(self) -> &'static str {
        match self {
            Self::New => "Start a new conversation",
            Self::Resume => "Resume a saved conversation",
            Self::Compact => "Compact this conversation's context now",
        }
    }

    pub(super) fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.name() == name)
    }
}

/// What the draft is completing: the listing, the query so far, and the token the query is in.
pub(super) struct Completion<'a> {
    pub(super) listing: Listing,
    pub(super) query: &'a str,
    pub(super) token: &'a str,
}

pub(super) fn completion(text: &str, cursor: usize) -> Option<Completion<'_>> {
    if let Some(rest) = text.strip_prefix('$') {
        let token = initial_token(text)?;
        if cursor == 0 || cursor.saturating_sub(1) > token.len() {
            return None;
        }
        let query = rest.get(..cursor.saturating_sub(1))?;
        if token.chars().next().is_some_and(|character| {
            character.is_numeric() || character.is_ascii_uppercase() || character == '_'
        }) {
            return None;
        }
        return Some(Completion {
            listing: Listing::Skills,
            query,
            token,
        });
    }
    let rest = text.strip_prefix('/')?;
    let token = initial_token(text)?;
    // `/resume` and a space: what follows is the query over saved conversations, wherever the
    // caret is. Any other command with text after it is text (CMD-2).
    if token == Command::Resume.name() && rest.len() > token.len() {
        return Some(Completion {
            listing: Listing::Conversations,
            query: rest[token.len()..].trim(),
            token,
        });
    }
    if cursor == 0 || cursor.saturating_sub(1) > token.len() {
        return None;
    }
    let query = rest.get(..cursor.saturating_sub(1))?;
    Some(Completion {
        listing: Listing::Commands,
        query,
        token,
    })
}

/// The token after the sigil, up to the first whitespace.
pub(super) fn initial_token(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('$').or_else(|| text.strip_prefix('/'))?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    rest.get(..end)
}

/// The Command a whole draft is, or nothing: `/compact` runs, `/compact please` is text (CMD-2).
/// `/resume` with a query after it is still `/resume`, because the query belongs to it.
pub(crate) fn exact_command(text: &str) -> Option<Command> {
    let rest = text.strip_prefix('/')?;
    let token = initial_token(text)?;
    let command = Command::parse(token)?;
    let trailing = rest[token.len()..].trim();
    (trailing.is_empty() || command == Command::Resume).then_some(command)
}

pub(crate) fn binding_matches(text: &str, name: &str) -> bool {
    text.strip_prefix('$')
        .and_then(|rest| rest.split_whitespace().next())
        == Some(name)
}
