use super::*;
use crate::test_support::{output_with_replay, reasoning_block, replay, text_block};
use crate::{Agent, Input};
use plexmaton_core::{AgentId, TreeRowKind};

/// TRE-8: clipped previews and annotations never become copied message text. The complete source
/// preserves Markdown, whitespace, CRLF and Unicode, with no navigation or journal effect.
#[test]
fn tre_8_tree_copy_reads_full_source_and_rejects_stale_or_foreign_rows() {
    let mut agent = Agent::new(AgentId::new("copy-agent").expect("agent"));
    let _ = agent.announce("Copy agent");
    let text = "**keep** 中文\r\n  whitespace  ".repeat(40);
    let _ = agent.handle(Input::Submitted { text: text.clone() });
    let _ = agent.handle(Input::Interrupted);
    let snapshot = agent
        .journal()
        .tree_snapshot(&agent.tree_origin().agent_id)
        .expect("snapshot");
    let row = snapshot
        .rows
        .iter()
        .find(|row| row.kind == TreeRowKind::User)
        .expect("user");
    assert!(row.preview.truncated);
    let mut request = TreeSourceRequest {
        origin: snapshot.origin,
        entry_id: row.entry_id.clone(),
    };
    let before = agent.journal().clone();
    assert_eq!(agent.journal().tree_source(&request), Ok(text.clone()));
    assert_eq!(agent.journal(), &before);
    request.origin.revision = TreeRevision::new(request.origin.revision.get() + 1);
    assert_eq!(
        agent.journal().tree_source(&request),
        Err(Error::StaleOrigin)
    );
    request.origin = agent.tree_origin();
    request.origin.agent_id = AgentId::new("another-agent").expect("agent");
    assert_eq!(
        agent.journal().tree_source(&request),
        Err(Error::EntryUnavailable)
    );
    request.origin = agent.tree_origin();
    request.entry_id = ConversationEntryId::new("absent").expect("entry");
    assert_eq!(
        agent.journal().tree_source(&request),
        Err(Error::EntryUnavailable)
    );
}

/// TRE-8/PRV-3: visible source blocks keep their exact bytes and order while block-attached opaque
/// replay never enters source assembly. A grouped copy inserts only a blank-line block separator.
#[test]
fn tre_8_assistant_copy_preserves_blocks_without_opaque_replay() {
    let journal =
        ConversationJournal::new(plexmaton_core::ConversationId::new("copy").expect("id"));
    let output = output_with_replay(
        vec![
            text_block("text", "**exact**\r\n  中文  "),
            reasoning_block("reasoning", "visible reasoning"),
        ],
        [(0, replay("OPAQUE_REPLAY_MUST_NOT_COPY"))],
    );
    let mut source = Source::default();
    journal
        .copy_assistant_source(
            &ConversationEntryId::new("source").expect("id"),
            &output,
            &mut source,
        )
        .expect("copy");
    assert_eq!(
        source.finish(),
        Ok("**exact**\r\n  中文  \n\nvisible reasoning".to_owned())
    );
}

/// TRE-8: exact and one-over source limits include separators, and a failed assembly returns no
/// clipped successful payload. No source is also explicit rather than a successful empty copy.
#[test]
fn tre_8_copy_capacity_is_exact_and_never_silently_truncates() {
    let mut exact = Source::default();
    exact
        .push(&"x".repeat(MAX_TREE_SOURCE_BYTES - 3))
        .expect("prefix");
    exact.push("y").expect("exact with separator");
    assert_eq!(
        exact.finish().expect("complete source").len(),
        MAX_TREE_SOURCE_BYTES
    );
    let mut over = Source::default();
    over.push(&"x".repeat(MAX_TREE_SOURCE_BYTES - 2))
        .expect("prefix");
    assert_eq!(over.push("y"), Err(Error::TooLarge));
    assert_eq!(Source::default().finish(), Err(Error::NoSource));
}
