use std::{
    io::Read as _,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use plexmaton_agent::collaboration::CollaborationEvent;
use plexmaton_session_store::collaboration::{CollaborationFile, CollaborationStoreError};

use super::*;

const ROOT_ENV: &str = "PLEXMATON_TEST_PROVISIONING_ROOT";
const CUT_ENV: &str = "PLEXMATON_TEST_PROVISIONING_CUT";
const READY_ENV: &str = "PLEXMATON_TEST_PROVISIONING_READY";
const FIXTURE: &str = "collaboration_ingress::tests::process_death::provisioning_process_fixture";

struct ProvisioningProcess {
    child: Option<Child>,
    ready: PathBuf,
}

impl ProvisioningProcess {
    fn start(root: &Path, cut: &str) -> Self {
        let ready = root.join(format!("{cut}.ready"));
        let child = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", FIXTURE, "--nocapture"])
            .env(ROOT_ENV, root)
            .env(CUT_ENV, cut)
            .env(READY_ENV, &ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn provisioning process fixture");
        Self {
            child: Some(child),
            ready,
        }
    }

    fn wait_until_ready(&mut self, cut: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.ready.exists() {
                let marker = std::fs::read_to_string(&self.ready).expect("read readiness marker");
                let mut lines = marker.lines();
                assert_eq!(lines.next(), Some(cut), "wrong process-cut marker");
                let target = lines.next().expect("marker retains target").to_owned();
                assert!(lines.next().is_none(), "marker contains unexpected data");
                assert!(
                    self.child
                        .as_mut()
                        .expect("live fixture")
                        .try_wait()
                        .expect("poll ready fixture")
                        .is_none(),
                    "fixture exited after publishing its barrier"
                );
                return target;
            }
            let child = self.child.as_mut().expect("live fixture");
            if let Some(status) = child.try_wait().expect("poll provisioning fixture") {
                let mut stderr = String::new();
                child
                    .stderr
                    .as_mut()
                    .expect("captured fixture stderr")
                    .read_to_string(&mut stderr)
                    .expect("read fixture stderr");
                panic!("fixture exited before its barrier ({status}): {stderr}");
            }
            assert!(
                Instant::now() < deadline,
                "provisioning fixture readiness timeout"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn kill(&mut self) {
        let mut child = self.child.take().expect("live fixture");
        child.kill().expect("kill provisioning owner");
        let status = child.wait().expect("reap provisioning owner");
        assert!(
            !status.success(),
            "fixture exited successfully instead of being killed"
        );
    }
}

impl Drop for ProvisioningProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct ReopenedCut {
    file: CollaborationFile,
    worker: MailEndpoint,
    collaboration_bytes: Vec<u8>,
    child_path: PathBuf,
}

fn reopen_cut(directory: &Directory, cut: &str) -> ReopenedCut {
    let collaboration_path = directory.0.join("collaboration.jsonl");
    let mut file = CollaborationFile::open(&collaboration_path)
        .unwrap_or_else(|error| panic!("reopen {cut} cut: {error}"));
    let records = file.ledger().records().to_vec();
    let worker = records
        .iter()
        .find_map(|record| match &record.event {
            CollaborationEvent::DelegationCreated { worker, .. } => Some(worker.clone()),
            _ => None,
        })
        .expect("canonical worker");
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record.event, CollaborationEvent::DelegationCreated { .. }))
            .count(),
        1,
        "{cut} cut changed canonical delegation count"
    );
    let accepted_mail = records.iter().find_map(|record| match &record.event {
        CollaborationEvent::MailAccepted { mail } => Some((record, mail)),
        _ => None,
    });
    if cut == "mail" {
        let (record, mail) = accepted_mail.expect("acknowledged mail record");
        assert_eq!(mail.from, endpoint("main"));
        assert_eq!(mail.to, worker);
        assert_eq!(mail.summary.as_str(), "Retain this accepted mail");
        let inbox: Vec<_> = file.ledger().mail_for(&worker).collect();
        assert_eq!(inbox, [(record, mail)]);
    } else {
        assert!(accepted_mail.is_none(), "{cut} cut invented mail");
    }
    let collaboration_bytes = std::fs::read(&collaboration_path).expect("canonical bytes");
    for record in &records {
        assert_eq!(
            file.admit(record.id.clone(), record.event.clone())
                .expect("exact retry after process death"),
            record.receipt(),
            "{cut} cut changed retry identity"
        );
    }
    assert_eq!(
        std::fs::read(&collaboration_path).expect("bytes after exact retries"),
        collaboration_bytes,
        "{cut} cut appended an exact retry"
    );
    let child_path = DelegatedConversationDirectory::under(&directory.0)
        .expect("delegated directory")
        .path_for(&worker.conversation)
        .expect("canonical child path");
    assert_eq!(
        child_path.exists(),
        cut != "canonical",
        "{cut} cut retained the wrong canonical child path"
    );
    ReopenedCut {
        file,
        worker,
        collaboration_bytes,
        child_path,
    }
}

async fn assert_explicit_recovery(
    directory: &Directory,
    cut: &str,
    target_before_death: &str,
    reopened: ReopenedCut,
) {
    let writer = CollaborationWriter::spawn(reopened.file).expect("recovered writer");
    let mut recovered = OwnedCollaboration::new(writer, SchedulerLimits::new(1).expect("limits"));
    recovered
        .bind_main_ingress(endpoint("main"))
        .expect("bind recovered Main");
    let driver = FakeDriver::new(Vec::<Script>::new());
    recovered
        .bind_child_factory(DelegatedChildFactory::synthetic(
            DelegatedConversationDirectory::under(&directory.0).expect("delegated directory"),
            catalog(directory),
            driver.clone(),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind recovered child factory");
    let targets = recovered
        .register_collaboration_targets()
        .await
        .expect("passively rebuild exact target");
    assert_eq!(targets.len(), 1, "{cut} cut changed target count");
    assert_eq!(targets[0].selector().as_str(), target_before_death);
    assert!(driver.calls().await.is_empty(), "{cut} passive dispatch");
    let child_directory = directory.0.join("delegated-sessions");
    assert_eq!(
        std::fs::read_dir(&child_directory)
            .expect("delegated directory")
            .count(),
        usize::from(cut != "canonical"),
        "{cut} cut retained the wrong child-journal count"
    );
    let first = recovered
        .resume_collaboration_target(targets[0].selector())
        .await
        .expect("explicitly recover exact child");
    let second = recovered
        .resume_collaboration_target(targets[0].selector())
        .await
        .expect("repeat exact child recovery");
    assert_eq!(first, second, "{cut} cut created a second live runner");
    assert_eq!(
        std::fs::read_dir(&child_directory)
            .expect("delegated directory")
            .map(|entry| entry.expect("child directory entry").path())
            .collect::<Vec<_>>(),
        vec![reopened.child_path.clone()],
        "{cut} cut created a second child journal"
    );
    assert!(driver.calls().await.is_empty(), "{cut} recovery dispatched");
    assert_eq!(
        std::fs::read(directory.0.join("collaboration.jsonl"))
            .expect("collaboration after recovery"),
        reopened.collaboration_bytes,
        "{cut} recovery rewrote canonical evidence"
    );
    shutdown(&mut recovered).await;
    let child = DelegatedConversationDirectory::under(&directory.0)
        .expect("delegated directory")
        .resume(&reopened.worker.conversation)
        .expect("reopen exact recovered child");
    assert_eq!(child.path(), reopened.child_path);
    assert_eq!(
        child.journal().conversation_id(),
        &reopened.worker.conversation
    );
    assert_eq!(
        child
            .journal()
            .records()
            .iter()
            .filter(|record| matches!(
                record,
                JournalRecord::AppendEntry { entry, .. }
                    if matches!(&entry.payload, JournalEntryPayload::AgentCreated { .. })
            ))
            .count(),
        1,
        "{cut} cut changed the child bootstrap identity"
    );
}

/// CTL-1/COL-4/COL-5/CHB-2/CHB-3: every provisioning file boundary survives process death.
#[tokio::test]
async fn provisioning_process_death_recovers_one_exact_passive_child() {
    for cut in ["canonical", "child-journal", "runner", "mail"] {
        let directory = Directory::new();
        let collaboration_path = directory.0.join("collaboration.jsonl");
        let mut process = ProvisioningProcess::start(&directory.0, cut);
        let target_before_death = process.wait_until_ready(cut);
        assert!(matches!(
            CollaborationFile::open(&collaboration_path),
            Err(CollaborationStoreError::Framing(
                plexmaton_session_store::StoreError::WriterLocked
            ))
        ));
        process.kill();
        let reopened = reopen_cut(&directory, cut);
        assert_explicit_recovery(&directory, cut, &target_before_death, reopened).await;
    }
}

#[tokio::test]
async fn provisioning_process_fixture() {
    let Some(root) = std::env::var_os(ROOT_ENV) else {
        return;
    };
    let cut = std::env::var(CUT_ENV).expect("fixture cut");
    let directory = Directory(PathBuf::from(root));
    let mut owner = empty_owner(&directory);
    let main_ingress = owner
        .bind_main_ingress(endpoint("main"))
        .expect("bind Main");
    owner
        .bind_child_factory(DelegatedChildFactory::synthetic(
            DelegatedConversationDirectory::under(&directory.0).expect("delegated directory"),
            catalog(&directory),
            FakeDriver::new(Vec::<Script>::new()),
            Arc::new(FixedWallClock(UnixMillis::EPOCH)),
        ))
        .expect("bind child factory");
    let main = catalog(&directory)
        .with_main_collaboration(main_ingress)
        .expect("Main catalog");
    let delegate = admit(
        &main,
        "process-delegate",
        DELEGATE_TOOL_NAME,
        json!({"task": "Recover this exact child"}),
    )
    .await;
    let (result, settlement) = execute_and_settle(&main, &mut owner, delegate).await;
    let target = match settled_outcome(settlement.result()) {
        Some(CollaborationIngressOutcome::Delegated { target }) => target.clone(),
        outcome => panic!("delegation did not settle before {cut} cut: {outcome:?}"),
    };
    assert!(matches!(result.outcome(), ToolOutcome::Succeeded { .. }));
    assert_eq!(
        cut, "mail",
        "{cut} fixture crossed its provisioning barrier"
    );

    let mail = admit(
        &main,
        "process-mail",
        SEND_MAIL_TOOL_NAME,
        json!({
            "target": target.as_str(),
            "summary": "Retain this accepted mail",
            "artifacts": [],
        }),
    )
    .await;
    let (result, settlement) = execute_and_settle(&main, &mut owner, mail).await;
    assert!(matches!(result.outcome(), ToolOutcome::Succeeded { .. }));
    assert!(matches!(
        settled_outcome(settlement.result()),
        Some(CollaborationIngressOutcome::MailAccepted)
    ));
    provisioning::provisioning_process_barrier("mail", &target);
    unreachable!("the parent terminates the fixture at its barrier");
}
