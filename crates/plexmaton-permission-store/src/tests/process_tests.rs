use std::{
    fs,
    io::Write as _,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use super::{Fixture, add, grant, write_private};
use crate::{PermissionStoreError as Error, ProjectPermissionStore};

struct Worker {
    child: Child,
    result: PathBuf,
    ready: PathBuf,
}
impl Worker {
    fn start(
        fixture: &Fixture,
        name: &str,
        operation: &str,
        expected: &plexmaton_core::ProjectPermissionRevision,
    ) -> Self {
        let result = fixture.root.join(format!("{name}.result"));
        let ready = fixture.root.join(format!("{name}.ready"));
        let child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "tests::process_tests::permission_process_fixture",
                "--nocapture",
            ])
            .env("PLEXMATON_PERMISSION_FIXTURE", operation)
            .env("PLEXMATON_PERMISSION_HOME", &fixture.home)
            .env("PLEXMATON_PERMISSION_PROJECT", &fixture.project)
            .env(
                "PLEXMATON_PERMISSION_EXPECTED",
                serde_json::to_string(expected).expect("expected"),
            )
            .env("PLEXMATON_PERMISSION_RESULT", &result)
            .env("PLEXMATON_PERMISSION_READY", &ready)
            .env("PLEXMATON_PERMISSION_NAME", name)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn store process");
        Self {
            child,
            result,
            ready,
        }
    }
    fn ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.ready.exists() {
            assert!(
                self.child.try_wait().expect("poll fixture").is_none(),
                "worker exited before ready"
            );
            assert!(Instant::now() < deadline, "worker readiness timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn release(&mut self) {
        self.child
            .stdin
            .as_mut()
            .expect("worker stdin")
            .write_all(b"go\n")
            .expect("release worker");
    }
    fn finish(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().expect("poll fixture") {
                assert!(status.success(), "worker failed: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "worker completion timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::read_to_string(&self.result).expect("worker result")
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn permission_process_fixture() {
    let Some(operation) = std::env::var_os("PLEXMATON_PERMISSION_FIXTURE") else {
        return;
    };
    let home = PathBuf::from(std::env::var_os("PLEXMATON_PERMISSION_HOME").expect("fixture home"));
    let project =
        PathBuf::from(std::env::var_os("PLEXMATON_PERMISSION_PROJECT").expect("fixture project"));
    let result =
        PathBuf::from(std::env::var_os("PLEXMATON_PERMISSION_RESULT").expect("fixture result"));
    let ready =
        PathBuf::from(std::env::var_os("PLEXMATON_PERMISSION_READY").expect("fixture ready"));
    let name = std::env::var("PLEXMATON_PERMISSION_NAME").expect("fixture name");
    let expected =
        serde_json::from_str(&std::env::var("PLEXMATON_PERMISSION_EXPECTED").expect("revision"))
            .expect("decode expected");
    let store = ProjectPermissionStore::open(&home, &project).expect("child store");
    write_private(&ready, b"ready");
    let mut signal = String::new();
    std::io::stdin()
        .read_line(&mut signal)
        .expect("start signal");
    let transaction = store.transaction(&|| false).expect("child transaction");
    let outcome = if operation == "grant" {
        transaction
            .grant(&expected, grant(&name), &|| false)
            .map(|transaction| transaction.snapshot().clone())
    } else if operation == "revoke" {
        transaction
            .revoke(&expected, grant("first").id, &|| false)
            .map(|transaction| transaction.snapshot().clone())
    } else if operation == "authorize" {
        let allowed = transaction
            .snapshot()
            .grants
            .iter()
            .any(|entry| entry.id == grant("first").id);
        write_private(&result, if allowed { b"allowed" } else { b"refused" });
        return;
    } else if operation == "hold" || operation == "tear" {
        if operation == "tear" {
            let log = home
                .join("projects")
                .join(store.project().key())
                .join("permissions.jsonl");
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(log)
                .expect("append source");
            file.write_all(b"{\"sequence\":2,\"change\":{\"kind\":\"revoke\"")
                .expect("partial revoke");
            file.sync_all().expect("sync partial fixture");
        }
        write_private(&result, b"locked");
        signal.clear();
        std::io::stdin()
            .read_line(&mut signal)
            .expect("hold until killed");
        drop(transaction);
        return;
    } else {
        panic!("unknown fixture operation");
    };
    write_private(
        &result,
        match outcome {
            Ok(_) => b"committed",
            Err(Error::StaleRevision) => b"stale",
            Err(error) => panic!("unexpected store failure: {error}"),
        },
    );
}

#[test]
fn pgr_2_two_process_writers_commit_once_and_never_merge_stale_grants() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let absent = store.read(&|| false).expect("empty").revision;
    let mut first = Worker::start(&fixture, "first", "grant", &absent);
    let mut second = Worker::start(&fixture, "second", "grant", &absent);
    first.ready();
    second.ready();
    first.release();
    second.release();
    let mut outcomes = [first.finish(), second.finish()];
    outcomes.sort();
    assert_eq!(outcomes, ["committed", "stale"]);
    assert_eq!(store.read(&|| false).expect("one grant").grants.len(), 1);
}

#[test]
fn pgr_2_dispatch_after_another_process_revokes_observes_the_revoke() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let granted = add(&store, "first");
    let mut revoker = Worker::start(&fixture, "revoker", "revoke", &granted.revision);
    revoker.ready();
    revoker.release();
    assert_eq!(revoker.finish(), "committed");
    let mut dispatcher = Worker::start(&fixture, "dispatcher", "authorize", &granted.revision);
    dispatcher.ready();
    dispatcher.release();
    assert_eq!(dispatcher.finish(), "refused");
}

#[test]
fn pgr_2_a_process_waits_for_authorization_lock_then_observes_current_policy() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let granted = add(&store, "first");
    let mut revoker = Worker::start(&fixture, "revoker", "revoke", &granted.revision);
    revoker.ready();
    let authorization = store
        .transaction(&|| false)
        .expect("authorization ordering point");
    assert_eq!(authorization.snapshot().grants.len(), 1);
    revoker.release();
    // This guard represents authorization, not the command lifetime; revocation follows its drop.
    drop(authorization);
    assert_eq!(revoker.finish(), "committed");
    assert!(
        store
            .read(&|| false)
            .expect("after revoke")
            .grants
            .is_empty()
    );
}

#[test]
fn pgr_2_process_death_releases_the_stable_lock_and_torn_writes_stay_refused() {
    for operation in ["hold", "tear"] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let initial = add(&store, "first");
        let mut worker = Worker::start(&fixture, operation, operation, &initial.revision);
        worker.ready();
        worker.release();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !worker.result.exists() {
            assert!(worker.child.try_wait().expect("worker").is_none());
            assert!(Instant::now() < deadline, "lock readiness timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(store.transaction(&|| false), Err(Error::Busy)));
        worker.child.kill().expect("kill lock owner");
        worker.child.wait().expect("reap lock owner");
        if operation == "hold" {
            assert_eq!(store.read(&|| false).expect("released lock"), initial);
        } else {
            assert_eq!(store.read(&|| false), Err(Error::Corrupt));
        }
    }
}
