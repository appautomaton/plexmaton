//! The draft's grammar: which listing a draft asks for, its query and the token the query is
//! in, and whether the whole draft is a Command (CMC-2).
use super::Listing;

/// A `/name` typed into a conversation's input and run from there (ui-ux §product vocabulary).
///
/// What the conversation's own input can do: start or resume a conversation, compact this one,
/// or review the Session's permissions. Everything wider than a Session is a Drawer page, never
/// a Command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    New,
    Resume,
    Compact,
    Permissions,
    Effort,
    Model,
    /// Browse the current conversation's saved messages and named heads.
    Tree,
}

impl Command {
    /// Every Command, in the order the menu lists them.
    pub const ALL: [Self; 7] = [
        Self::New,
        Self::Resume,
        Self::Compact,
        Self::Permissions,
        Self::Effort,
        Self::Model,
        Self::Tree,
    ];

    /// The name after the slash.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Resume => "resume",
            Self::Compact => "compact",
            Self::Permissions => "permissions",
            Self::Effort => "effort",
            Self::Model => "model",
            Self::Tree => "tree",
        }
    }

    /// Additional truthful completion names for the same typed command action.
    #[must_use]
    pub(super) const fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Tree => &["rewind"],
            _ => &[],
        }
    }

    /// The flags this Command accepts, modifying how it runs (CMC-2).
    ///
    /// A flag is not a listing: a listing names *what* the Command runs on and is chosen from
    /// rows, while a flag changes *how* it runs and is typed. A Command may declare both.
    #[must_use]
    pub const fn flags(self) -> &'static [CommandFlag] {
        match self {
            Self::Compact => &[CommandFlag::Force],
            Self::New
            | Self::Resume
            | Self::Permissions
            | Self::Effort
            | Self::Model
            | Self::Tree => &[],
        }
    }

    /// The listing a completed Command opens, whose query is the text after it (CMC-2).
    #[must_use]
    pub const fn lists(self) -> Option<Listing> {
        match self {
            Self::Resume => Some(Listing::Conversations),
            Self::Permissions => Some(Listing::Permissions),
            Self::Effort => Some(Listing::Effort),
            Self::Model => Some(Listing::Models),
            Self::New | Self::Compact | Self::Tree => None,
        }
    }

    /// What `Enter` does, shown beside the name.
    #[must_use]
    pub const fn summary(self) -> &'static str {
        match self {
            Self::New => "Start a new conversation",
            Self::Resume => "Resume a saved conversation",
            Self::Compact => "Compact this conversation's context now",
            Self::Permissions => "Review this Session's permissions",
            Self::Effort => "Adjust this conversation's reasoning effort",
            Self::Model => "Choose this conversation's model",
            Self::Tree => "Browse branches or rewind to a saved message",
        }
    }

    pub(super) fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|command| command.name() == name)
            .or_else(|| (name == "rewind").then_some(Self::Tree))
    }
}

/// One typed modifier on a Command: what it does differently, never what it acts on (CMC-2).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandFlag {
    /// Run the action even where the runtime would otherwise decline it as unnecessary.
    Force,
}

impl CommandFlag {
    /// The flag as it is typed, sigil included.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Force => "--force",
        }
    }
}

/// The declared flags one draft carries. A set, because a flag is present or it is not.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommandFlags(u8);

impl CommandFlags {
    const fn bit(flag: CommandFlag) -> u8 {
        1 << (flag as u8)
    }

    #[must_use]
    pub const fn contains(self, flag: CommandFlag) -> bool {
        self.0 & Self::bit(flag) != 0
    }

    const fn with(self, flag: CommandFlag) -> Self {
        Self(self.0 | Self::bit(flag))
    }
}

/// Splits the text after a Command name into its leading declared flags and the query after them.
///
/// Flags lead because a query is what the user is naming and may be anything; a `--` token is not
/// a name. An undeclared `--token` is not silently a query: the draft stops being a Command and
/// submits as text, so a mistyped flag is visible as the message it became rather than running
/// something the user did not ask for.
fn split_flags(command: Command, trailing: &str) -> Option<(CommandFlags, &str)> {
    let mut flags = CommandFlags::default();
    let mut rest = trailing.trim_start();
    while let Some(token) = rest.split_whitespace().next() {
        if !token.starts_with("--") {
            break;
        }
        let flag = command
            .flags()
            .iter()
            .copied()
            .find(|flag| flag.name() == token)?;
        flags = flags.with(flag);
        rest = rest[token.len()..].trim_start();
    }
    Some((flags, rest))
}

/// The declared flags a whole-Command draft carries, empty when it is not a Command (CMC-2).
#[must_use]
pub(crate) fn command_flags(text: &str) -> CommandFlags {
    let Some(rest) = text.strip_prefix('/') else {
        return CommandFlags::default();
    };
    let Some(command) = initial_token(text).and_then(Command::parse) else {
        return CommandFlags::default();
    };
    split_flags(command, &rest[command_token_len(text)..])
        .map_or_else(CommandFlags::default, |(flags, _)| flags)
}

fn command_token_len(text: &str) -> usize {
    initial_token(text).map_or(0, str::len)
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
    // A listing Command and a space: what follows is its query, wherever the caret is. Any
    // other command with text after it is text (CMC-2).
    if let Some(listing) = Command::parse(token).and_then(Command::lists)
        && rest.len() > token.len()
    {
        return Some(Completion {
            listing,
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

/// The Command a whole draft is, or nothing: `/compact` runs, `/compact please` is text (CMC-2).
/// A listing Command with a query after it is still that Command, because the query belongs to it.
pub(crate) fn exact_command(text: &str) -> Option<Command> {
    let rest = text.strip_prefix('/')?;
    let token = initial_token(text)?;
    let command = Command::parse(token)?;
    let (_, query) = split_flags(command, &rest[token.len()..])?;
    (query.trim().is_empty() || command.lists().is_some()).then_some(command)
}

pub(crate) fn binding_matches(text: &str, name: &str) -> bool {
    text.strip_prefix('$')
        .and_then(|rest| rest.split_whitespace().next())
        == Some(name)
}
