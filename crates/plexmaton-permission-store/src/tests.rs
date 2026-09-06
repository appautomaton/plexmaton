#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{DirBuilderExt as _, PermissionsExt as _, symlink},
    path::{Path, PathBuf},
};

use plexmaton_agent::{
    CommandPermission, PermissionDefinition, PermissionGrant, PermissionGrantOrigin,
    PermissionMatcher, ToolDefinitionRevision,
};
use plexmaton_core::{PermissionGrantId, ProjectPermissionRevision, ToolDefinitionId};

use crate::{PermissionStoreError as Error, ProjectPermissionStore};

mod process_tests;

pub(super) struct Fixture {
    root: PathBuf,
    pub home: PathBuf,
    pub project: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("plexmaton-permissions-{}", uuid::Uuid::now_v7()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("reserve fixture");
        let home = root.join("home");
        let project = root.join("project");
        fs::create_dir(&project).expect("project");
        Self {
            root,
            home,
            project,
        }
    }
    pub fn store(&self) -> ProjectPermissionStore {
        ProjectPermissionStore::open(&self.home, &self.project).expect("store")
    }
    pub fn directory(&self, store: &ProjectPermissionStore) -> PathBuf {
        self.home.join("projects").join(store.project().key())
    }
    pub fn log(&self, store: &ProjectPermissionStore) -> PathBuf {
        self.directory(store).join("permissions.jsonl")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn grant(id: &str) -> PermissionGrant {
    PermissionGrant {
        id: PermissionGrantId::new(id).expect("grant id"),
        matcher: PermissionMatcher::ExactCommand {
            definition: PermissionDefinition::new(
                ToolDefinitionId::new("native.command").expect("definition"),
                ToolDefinitionRevision::new(1).expect("revision"),
            ),
            command: CommandPermission::new("git fetch origin".into(), [7; 32]).expect("command"),
        },
        origin: PermissionGrantOrigin::Approval,
    }
}

pub(super) fn add(store: &ProjectPermissionStore, id: &str) -> crate::ProjectPermissionSnapshot {
    let transaction = store.transaction(&|| false).expect("transaction");
    let revision = transaction.snapshot().revision.clone();
    transaction
        .grant(&revision, grant(id), &|| false)
        .map(|transaction| transaction.snapshot().clone())
        .expect("grant")
}

#[test]
fn pgr_5_absence_grants_trust_and_revocation_have_one_personal_source() {
    let fixture = Fixture::new();
    let store = fixture.store();
    assert_eq!(
        store.read(&|| false).expect("empty").revision,
        ProjectPermissionRevision::Absent
    );
    assert!(!fixture.log(&store).exists());
    let granted = add(&store, "first");
    let trusted = store
        .transaction(&|| false)
        .expect("transaction")
        .trust_config(&granted.revision, Some([8; 32]), &|| false)
        .map(|transaction| transaction.snapshot().clone())
        .expect("trust");
    assert_eq!(trusted.grants, [grant("first")]);
    assert_eq!(fixture.store().read(&|| false).expect("restart"), trusted);
    let revoked = store
        .transaction(&|| false)
        .expect("transaction")
        .revoke(&trusted.revision, grant("first").id, &|| false)
        .map(|transaction| transaction.snapshot().clone())
        .expect("revoke");
    assert!(revoked.grants.is_empty());
    assert_eq!(revoked.trusted_config, Some([8; 32]));
    assert!(!fixture.home.join("sessions").exists());
    assert!(!fixture.project.join(".plexmaton").exists());
    fs::remove_file(fixture.log(&store)).expect("delete source");
    assert_eq!(fixture.store().read(&|| false), Err(Error::Corrupt));
}

#[test]
fn pgr_2_stale_writes_and_reset_cannot_recreate_an_old_revision() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let first = add(&store, "first");
    assert_eq!(
        store
            .transaction(&|| false)
            .expect("transaction")
            .grant(&ProjectPermissionRevision::Absent, grant("stale"), &|| {
                false
            })
            .map(|transaction| transaction.snapshot().clone()),
        Err(Error::StaleRevision)
    );
    let reset = store
        .transaction(&|| false)
        .expect("transaction")
        .reset(&first.revision, &|| false)
        .map(|transaction| transaction.snapshot().clone())
        .expect("reset");
    assert!(reset.grants.is_empty());
    let current = add(&store, "second");
    assert_ne!(first.revision, current.revision);
    assert_eq!(
        store
            .transaction(&|| false)
            .expect("transaction")
            .revoke(&first.revision, grant("second").id, &|| false)
            .map(|transaction| transaction.snapshot().clone()),
        Err(Error::StaleRevision)
    );
    assert_eq!(store.read(&|| false).expect("current"), current);
}

#[test]
fn pgr_3_torn_complete_without_newline_and_invalid_records_never_restore_a_prefix() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let original = add(&store, "first");
    let path = fixture.log(&store);
    let valid = fs::read(&path).expect("valid bytes");
    let invalid_suffixes: &[&[u8]] = &[
        b"{\"sequence\":2,\"change\":", // Interrupted revoke.
        b"{\"sequence\":2,\"change\":{\"kind\":\"revoke\",\"id\":\"first\"}}", // Missing newline.
        b"{\"sequence\":3,\"change\":{\"kind\":\"revoke\",\"id\":\"first\"}}\n", // Sequence gap.
        b"{\"sequence\":2,\"change\":{\"kind\":\"revoke\",\"id\":\"missing\"}}\n",
        b"{\"sequence\":2,\"change\":{\"kind\":\"trust\",\"fingerprint\":null},\"surprise\":true}\n",
        b"\n",
    ];
    for suffix in invalid_suffixes {
        let mut bytes = valid.clone();
        bytes.extend_from_slice(suffix);
        fs::write(&path, bytes).expect("damage source");
        assert_eq!(
            store.read(&|| false),
            Err(Error::Corrupt),
            "suffix: {suffix:?}"
        );
    }
    fs::write(&path, &valid).expect("restore fixture");
    assert_eq!(store.read(&|| false).expect("valid source"), original);
    let current = store
        .transaction(&|| false)
        .expect("transaction")
        .revoke(&original.revision, grant("first").id, &|| false)
        .map(|transaction| transaction.snapshot().clone())
        .expect("revoke");
    assert_eq!(
        store
            .transaction(&|| false)
            .expect("transaction")
            .grant(&current.revision, grant("first"), &|| false)
            .map(|transaction| transaction.snapshot().clone()),
        Err(Error::Corrupt)
    );
}

#[test]
fn pgr_3_format_binding_and_resource_bounds_refuse_the_whole_source() {
    let fixture = Fixture::new();
    let store = fixture.store();
    add(&store, "first");
    let path = fixture.log(&store);
    let original = fs::read_to_string(&path).expect("source");
    for (changed, expected) in [
        (
            original.replacen("\"format\":1", "\"format\":2", 1),
            Error::UnsupportedFormat,
        ),
        (
            original.replacen("\"inode\":", "\"inode\":0,\"old_inode\":", 1),
            Error::Corrupt,
        ),
        (
            original.replacen("\"revision\":1", "\"revision\":0", 1),
            Error::Corrupt,
        ),
        (original.replacen("git fetch origin", "", 1), Error::Corrupt),
    ] {
        assert_ne!(changed, original, "fixture mutation matched");
        fs::write(&path, changed).expect("change");
        assert_eq!(store.read(&|| false), Err(expected));
    }
    let mut huge = original.into_bytes();
    huge.extend(vec![b' '; crate::MAX_RECORD_BYTES]);
    huge.push(b'\n');
    fs::write(&path, huge).expect("oversize record");
    assert_eq!(store.read(&|| false), Err(Error::Capacity));
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open")
        .set_len(crate::MAX_STORE_BYTES + 1)
        .expect("oversize source");
    assert_eq!(store.read(&|| false), Err(Error::Capacity));
}

#[test]
fn pgr_3_active_grant_and_retained_mutation_limits_do_not_partially_apply() {
    let fixture = Fixture::new();
    let store = fixture.store();
    for index in 0..plexmaton_agent::MAX_PERMISSION_ENTRIES {
        add(&store, &format!("grant-{index}"));
    }
    let full = store.read(&|| false).expect("full");
    assert!(!full.can_remember());
    assert_eq!(
        store
            .transaction(&|| false)
            .expect("transaction")
            .grant(&full.revision, grant("excess"), &|| false)
            .map(|transaction| transaction.snapshot().clone()),
        Err(Error::Capacity)
    );
    assert_eq!(store.read(&|| false).expect("unchanged"), full);
    let path = fixture.log(&store);
    let source = fs::read_to_string(&path).expect("source");
    let header = source.lines().next().expect("header");
    let mut retained = format!("{header}\n");
    for sequence in 1..=crate::MAX_STORE_RECORDS {
        retained.push_str(&format!(
            "{{\"sequence\":{sequence},\"change\":{{\"kind\":\"trust\",\"fingerprint\":null}}}}\n"
        ));
    }
    fs::write(&path, retained).expect("bounded retention");
    let full = store.read(&|| false).expect("at retention limit");
    assert!(!full.can_remember());
    assert_eq!(
        store
            .transaction(&|| false)
            .expect("transaction")
            .grant(&full.revision, grant("excess"), &|| false)
            .map(|transaction| transaction.snapshot().clone()),
        Err(Error::Capacity)
    );
    assert_eq!(store.read(&|| false).expect("unchanged"), full);
}

#[test]
fn pgr_1_aliases_share_identity_but_distinct_and_replaced_roots_do_not() {
    let fixture = Fixture::new();
    let store = fixture.store();
    add(&store, "first");
    let alias = fixture.root.join("alias");
    symlink(&fixture.project, &alias).expect("alias");
    assert_eq!(
        ProjectPermissionStore::open(&fixture.home, &alias)
            .expect("alias store")
            .project(),
        store.project()
    );
    let other = fixture.root.join("linked-worktree");
    fs::create_dir(&other).expect("worktree");
    fs::write(
        other.join(".git"),
        "gitdir: ../common.git/worktrees/linked\n",
    )
    .expect("gitfile");
    assert_ne!(
        ProjectPermissionStore::open(&fixture.home, &other)
            .expect("other store")
            .project(),
        store.project()
    );
    fs::rename(&fixture.project, fixture.root.join("old-project")).expect("move project");
    fs::create_dir(&fixture.project).expect("replace project");
    assert_eq!(store.read(&|| false), Err(Error::IdentityChanged));
    assert_eq!(
        fixture
            .store()
            .read(&|| false)
            .expect("new namespace")
            .revision,
        ProjectPermissionRevision::Absent
    );
}

#[test]
fn pgr_1_private_permissions_and_pinned_parents_reject_replacement() {
    let fixture = Fixture::new();
    let store = fixture.store();
    add(&store, "first");
    let directory = fixture.directory(&store);
    let log = fixture.log(&store);
    assert_eq!(
        fs::metadata(&directory)
            .expect("dir mode")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&log).expect("file mode").permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&log, fs::Permissions::from_mode(0o644)).expect("insecure file");
    assert_eq!(store.read(&|| false), Err(Error::UnsafePath));
    fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).expect("restore file mode");
    fs::rename(&directory, directory.with_extension("old")).expect("move directory");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .expect("replacement");
    assert!(store.read(&|| false).is_err());
}

#[test]
fn pgr_1_symlinks_hardlinks_and_replaced_locks_supply_no_authority() {
    for target in ["permissions.jsonl", "permissions.lock"] {
        let fixture = Fixture::new();
        let store = fixture.store();
        add(&store, "first");
        let path = fixture.directory(&store).join(target);
        let moved = path.with_extension("original");
        fs::rename(&path, &moved).expect("move");
        symlink(&moved, &path).expect("symlink");
        assert!(store.read(&|| false).is_err());
        fs::remove_file(&path).expect("unlink");
        fs::hard_link(&moved, &path).expect("hardlink");
        assert_eq!(store.read(&|| false), Err(Error::UnsafePath));
        fs::remove_file(&path).expect("unlink");
        fs::copy(&moved, &path).expect("replacement");
        if target == "permissions.lock" {
            assert_eq!(store.read(&|| false), Err(Error::IdentityChanged));
        }
    }
}

#[test]
fn pgr_2_cancelled_wait_and_mutation_apply_nothing() {
    let fixture = Fixture::new();
    let store = fixture.store();
    assert!(matches!(store.transaction(&|| true), Err(Error::Cancelled)));
    let transaction = store.transaction(&|| false).expect("transaction");
    assert_eq!(
        transaction
            .grant(
                &ProjectPermissionRevision::Absent,
                grant("cancelled"),
                &|| true
            )
            .map(|transaction| transaction.snapshot().clone()),
        Err(Error::Cancelled)
    );
    assert!(!fixture.log(&store).exists());
    let held = store.transaction(&|| false).expect("held lock");
    let attempts = std::cell::Cell::new(0);
    let result = store.transaction(&|| {
        attempts.set(attempts.get() + 1);
        attempts.get() > 1
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    drop(held);
    add(&store, "after-cancellation");
}

pub(super) fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("fixture bytes");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("private fixture");
}
