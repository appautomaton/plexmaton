mod support;

use std::fs::OpenOptions;
use std::io::Write;

use plexmaton_agent::{
    JournalEntryPayload, ProviderCodecId, ProviderReplay, RequestItem, UnixMillis,
};
use plexmaton_core::HeadName;
use plexmaton_session_store::{JournalFile, JournalRecovery, StoreError};

use support::{TestDir, agent_created, append, id, session};

/// JRN-3/JRN-4: every journal-derived file is owner-only on Unix.
#[cfg(unix)]
#[test]
fn jrn_3_and_jrn_4_journal_fork_and_tail_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDir::new("permissions");
    let source_path = directory.path().join("source.jsonl");
    let fork_path = directory.path().join("fork.jsonl");
    let source = JournalFile::create(&source_path, session("source"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create source: {error}"));
    let forked = source
        .fork(&fork_path, session("fork"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("fork source: {error}"));
    drop(forked);
    drop(source);
    let mut file = OpenOptions::new()
        .append(true)
        .open(&source_path)
        .unwrap_or_else(|error| panic!("open raw writer: {error}"));
    file.write_all(b"{")
        .unwrap_or_else(|error| panic!("write incomplete tail: {error}"));
    drop(file);
    let recovered =
        JournalFile::open(&source_path).unwrap_or_else(|error| panic!("recover source: {error}"));
    let tail_path = match recovered.recovery() {
        JournalRecovery::IsolatedFinalTail { path, .. } => path,
        other => panic!("expected isolated tail, got {other:?}"),
    };

    for path in [&source_path, &fork_path, tail_path] {
        let mode = std::fs::metadata(path)
            .unwrap_or_else(|error| panic!("read permissions: {error}"))
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "unexpected mode for {}", path.display());
    }
}

/// JRN-3/JRN-4: an existing journal exposed to another account is not opened.
#[cfg(unix)]
#[test]
fn jrn_3_and_jrn_4_insecure_existing_journal_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDir::new("insecure-permissions");
    let path = directory.path().join("session.jsonl");
    let store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create store: {error}"));
    drop(store);
    let mut permissions = std::fs::metadata(&path)
        .unwrap_or_else(|error| panic!("read permissions: {error}"))
        .permissions();
    permissions.set_mode(0o640);
    std::fs::set_permissions(&path, permissions)
        .unwrap_or_else(|error| panic!("widen permissions: {error}"));

    assert!(matches!(
        JournalFile::open(&path),
        Err(StoreError::InsecurePermissions(0o640))
    ));
}

/// JRN-3/JRN-4: exact opaque replay survives a real file without entering presentation.
#[test]
fn jrn_3_and_jrn_4_encrypted_replay_round_trips_through_the_file() {
    let directory = TestDir::new("replay");
    let path = directory.path().join("session.jsonl");
    let mut store = JournalFile::create(&path, session("session-a"), UnixMillis::EPOCH)
        .unwrap_or_else(|error| panic!("create store: {error}"));
    store
        .append(agent_created(store.journal(), 1))
        .unwrap_or_else(|failure| panic!("append agent: {}", failure.error()));
    let replay = ProviderReplay::new(
        ProviderCodecId::new("openai_responses")
            .unwrap_or_else(|error| panic!("fixture codec: {error:?}")),
        r#"{"encrypted_content":"secret-ciphertext"}"#.to_owned(),
    )
    .unwrap_or_else(|error| panic!("fixture replay: {error:?}"));
    store
        .append(append(
            store.journal(),
            2,
            JournalEntryPayload::ProviderReplay(replay.clone()),
        ))
        .unwrap_or_else(|failure| panic!("append replay: {}", failure.error()));
    drop(store);

    let reopened =
        JournalFile::open(&path).unwrap_or_else(|error| panic!("reopen replay: {error}"));
    let projected = reopened
        .journal()
        .project(&id("main", HeadName::new))
        .unwrap_or_else(|error| panic!("project replay: {error:?}"));
    assert_eq!(
        projected.request().items,
        [RequestItem::ProviderReplay(replay)]
    );
    assert!(!format!("{projected:?}").contains("secret-ciphertext"));
}
