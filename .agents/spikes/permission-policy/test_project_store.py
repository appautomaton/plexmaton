"""Real temporary files and independent processes for the project grant-log proposal."""

from pathlib import Path
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest

from project_store_probe import (
    Failure, Fault, MAX_LOG_BYTES, MAX_RECORDS, ProjectGrantLog, StoreFailure, Version, line,
)


PROBE = Path(__file__).with_name("project_store_probe.py")


class OwnedProcess:
    def __init__(self, owner, path):
        self.owner = owner
        self.child = subprocess.Popen(
            [sys.executable, "-I", "-B", str(PROBE), str(path)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env={"PATH": "/usr/bin:/bin"}, start_new_session=True,
        )
        owner.addCleanup(self.close)

    def collect(self, expected_exit=0):
        output, error = self.child.communicate(timeout=8)
        self.owner.assertEqual(self.child.returncode, expected_exit, error.decode(errors="replace"))
        if expected_exit:
            self.owner.assertEqual(output, b"")
            return None
        return json.loads(output)

    def close(self):
        if self.child.poll() is None:
            try:
                os.killpg(self.child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        self.child.communicate(timeout=2)


class ProjectStoreTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="plexmaton-policy-store-")
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.store = ProjectGrantLog(self.root / "policy", "project-a")
        self.store.initialize("store-a")
        self.jobs = 0

    def grant(self, grant="grant-a", scope="ordinary-edits"):
        return {"kind": "grant", "grant": grant, "scope": scope}

    def mutate(self, value):
        return self.store.mutate(self.store.read().version, value)

    def spawn(self, operation, version=None, **values):
        self.jobs += 1
        version = version or Version("store-a", 0)
        job = dict(
            operation=operation, directory=str(self.store.directory), project=self.store.project,
            store=version.store, revision=version.revision, **values,
        )
        path = self.root / f"job-{self.jobs}.json"
        path.write_text(json.dumps(job))
        return OwnedProcess(self, path)

    def marker(self, name):
        return str(self.root / name)

    def wait(self, marker, child):
        deadline = time.monotonic() + 5
        while not Path(marker).exists():
            if child.child.poll() is not None:
                self.fail(f"worker exited before {Path(marker).name}: {child.collect()}")
            if time.monotonic() >= deadline:
                self.fail(f"worker did not reach {Path(marker).name}")
            time.sleep(0.005)

    def hold(self):
        locked, release = self.marker("locked"), self.marker("release")
        child = self.spawn("hold", locked=locked, release=release)
        self.wait(locked, child)
        return child, release

    def assert_failure(self, expected, operation):
        with self.assertRaises(StoreFailure) as captured:
            operation()
        self.assertIs(captured.exception.kind, expected)

    # P5: two actual writers share one revision boundary; the loser must reevaluate explicitly.
    def test_two_processes_cannot_overwrite_each_other_from_the_same_revision(self):
        start = self.marker("start")
        first_ready, second_ready = self.marker("first-ready"), self.marker("second-ready")
        first = self.spawn("mutate", mutation=self.grant("first"), start=start, ready=first_ready)
        second = self.spawn("mutate", mutation=self.grant("second"), start=start, ready=second_ready)
        self.wait(first_ready, first)
        self.wait(second_ready, second)
        Path(start).touch()
        results = [first.collect(), second.collect()]
        self.assertCountEqual([result["status"] for result in results], ["ok", Failure.STALE.value])
        current = self.store.read()
        self.assertEqual(current.version.revision, 1)
        winner = next(iter(current.active))
        loser = "second" if winner == "first" else "first"
        retried = self.spawn("mutate", current.version, mutation=self.grant(loser)).collect()
        self.assertEqual(retried, {"status": "ok", "revision": 2})
        self.assertEqual(set(self.store.read().active), {"first", "second"})

    # P1/P5: a stale operation and reuse of a revoked identity are independently refused.
    def test_revocation_survives_a_stale_writer_and_a_fresh_process(self):
        granted = self.mutate(self.grant())
        ready, start = self.marker("ready"), self.marker("start")
        stale = self.spawn("mutate", granted.version, mutation=self.grant("another"), ready=ready, start=start)
        self.wait(ready, stale)
        revoked = self.mutate({"kind": "revoke", "grant": "grant-a"})
        Path(start).touch()
        self.assertEqual(stale.collect()["status"], Failure.STALE.value)
        resumed = self.spawn("read").collect()
        self.assertEqual(resumed["active"], {})
        self.assertEqual(resumed["revision"], revoked.version.revision)
        reused = self.spawn("mutate", revoked.version, mutation=self.grant()).collect()
        self.assertEqual(reused["status"], Failure.INVALID.value)
        self.assertEqual(self.store.read(), revoked)

    # P4/P5: revocation ordered before authorization prevents the marker effect.
    def test_authorization_refreshes_after_revocation(self):
        granted = self.mutate(self.grant())
        revoked = self.mutate({"kind": "revoke", "grant": "grant-a"})
        for version, expected in ((granted.version, Failure.STALE), (revoked.version, Failure.NOT_GRANTED)):
            effect = self.marker("effect")
            result = self.spawn("authorize", version, grant="grant-a", scope="ordinary-edits", effect=effect).collect()
            self.assertEqual(result["status"], expected.value)
            self.assertFalse(Path(effect).exists())

    # The proposed dispatch ordering point authorizes before a later revoke; it is not rollback.
    def test_revoke_does_not_retroactively_cancel_an_authorized_operation(self):
        granted = self.mutate(self.grant())
        authorized, resume, effect = self.marker("authorized"), self.marker("continue"), self.marker("effect")
        worker = self.spawn("authorize", granted.version, grant="grant-a", scope="ordinary-edits",
                            authorized=authorized, effect=effect, **{"continue": resume})
        self.wait(authorized, worker)
        self.mutate({"kind": "revoke", "grant": "grant-a"})
        self.assertFalse(Path(effect).exists())
        Path(resume).touch()
        self.assertEqual(worker.collect()["status"], "ok")
        self.assertEqual(Path(effect).read_text(), "executed\n")
        self.assertEqual(self.store.read().active, {})

    # P5: readers cannot observe the file while an exclusive transaction is in progress.
    def test_reader_participates_in_the_stable_file_lock(self):
        holder, release = self.hold()
        waiting = self.marker("reader-waiting")
        reader = self.spawn("read", waiting=waiting)
        self.wait(waiting, reader)
        self.assertIsNone(reader.child.poll())
        Path(release).touch()
        self.assertEqual(holder.collect()["status"], "ok")
        self.assertEqual(reader.collect()["revision"], 0)

    def test_cancelled_lock_wait_writes_nothing(self):
        holder, release = self.hold()
        waiting, cancel = self.marker("waiting"), self.marker("cancel")
        writer = self.spawn("mutate", mutation=self.grant(), waiting=waiting, cancel=cancel)
        self.wait(waiting, writer)
        Path(cancel).touch()
        self.assertEqual(writer.collect()["status"], Failure.CANCELLED.value)
        Path(release).touch()
        self.assertEqual(holder.collect()["status"], "ok")
        self.assertEqual(self.store.read().version.revision, 0)

    def test_exhausted_lock_wait_has_a_typed_timeout(self):
        holder, release = self.hold()
        result = self.spawn("mutate", mutation=self.grant(), timeout=0).collect()
        self.assertEqual(result["status"], Failure.TIMEOUT.value)
        Path(release).touch()
        holder.collect()
        self.assertEqual(self.store.read().active, {})

    def test_process_death_releases_the_lock_for_a_waiting_reader(self):
        holder, _release = self.hold()
        waiting = self.marker("waiting")
        reader = self.spawn("read", waiting=waiting)
        self.wait(waiting, reader)
        os.killpg(holder.child.pid, signal.SIGKILL)
        holder.collect(-signal.SIGKILL)
        self.assertEqual(reader.collect()["status"], "ok")

    # P5: an error before the write and an unacknowledged completed write are distinct outcomes.
    def test_write_failure_distinguishes_unwritten_from_complete_but_unacknowledged(self):
        version = self.store.read().version
        original = self.store.path.read_bytes()
        self.assert_failure(Failure.NOT_WRITTEN, lambda: self.store.mutate(version, self.grant(), fault=Fault.BEFORE_WRITE))
        self.assertEqual(self.store.path.read_bytes(), original)
        result = self.spawn("mutate", version, mutation=self.grant(), fault=Fault.AFTER_WRITE.value).collect()
        self.assertEqual(result["status"], Failure.UNKNOWN_WRITE.value)
        self.assertEqual(self.spawn("read").collect()["active"], {"grant-a": "ordinary-edits"})

    def test_process_exit_before_reply_keeps_a_completed_grant(self):
        worker = self.spawn("crash_after_commit", mutation=self.grant())
        worker.collect(91)
        self.assertEqual(self.spawn("read").collect()["active"], {"grant-a": "ordinary-edits"})

    # P1/P5: salvaging old allows from a torn revoke is forbidden in this candidate.
    def test_partial_grant_or_revoke_never_becomes_a_usable_old_snapshot(self):
        for mutation in (self.grant(), {"kind": "revoke", "grant": "grant-a"}):
            with self.subTest(mutation=mutation["kind"]):
                if mutation["kind"] == "revoke":
                    self.store.path.unlink()
                    self.store.initialize("store-b")
                    self.mutate(self.grant())
                current = self.store.read()
                self.assert_failure(Failure.UNKNOWN_WRITE,
                                    lambda: self.store.mutate(current.version, mutation, fault=Fault.PARTIAL_WRITE))
                self.assertEqual(self.spawn("read").collect()["status"], Failure.CORRUPT.value)
                self.assert_failure(Failure.CORRUPT,
                                    lambda: self.store.authorize(current.version, "grant-a", "ordinary-edits"))

    def test_store_reset_cannot_reuse_an_old_revision_number(self):
        stale = self.store.read().version
        self.store.path.unlink()
        self.store.initialize("store-b")
        fresh = self.store.read().version
        self.assertEqual(stale.revision, fresh.revision)
        self.assertNotEqual(stale.store, fresh.store)
        result = self.spawn("mutate", stale, mutation=self.grant()).collect()
        self.assertEqual(result["status"], Failure.STALE.value)
        self.assertEqual(self.store.read().active, {})

    def test_corrupt_and_mismatched_sources_are_not_absent(self):
        header = self.store.path.read_bytes()
        cases = [
            b"", b"\xff\n", b"{}\n", header[:-1],
            header + line(dict(self.grant(), revision=2)),
            header + line(dict(self.grant(), revision=True)),
            header + line(dict(self.grant(), revision=1, ignored="no")),
            header + b'{"kind":"grant","revision":1,"grant":"a","grant":"b","scope":"edits"}\n',
            header + line({"kind": "revoke", "revision": 1, "grant": "missing"}),
        ]
        for encoded in cases:
            with self.subTest(encoded=encoded):
                self.store.path.write_bytes(encoded)
                self.assert_failure(Failure.CORRUPT, self.store.read)
        self.store.path.write_bytes(line({"kind": "header", "project": "other", "store": "store-a"}))
        self.assert_failure(Failure.BINDING, self.store.read)
        self.store.path.unlink()
        self.assert_failure(Failure.ABSENT, self.store.read)

    def test_absent_project_store_does_not_create_an_empty_directory(self):
        absent = ProjectGrantLog(self.root / "absent", "project-a")
        self.assert_failure(Failure.ABSENT, absent.read)
        self.assertFalse(absent.directory.exists())

    def test_store_byte_and_record_bounds_refuse_without_publishing_a_grant(self):
        self.store.path.write_bytes(b"x" * (MAX_LOG_BYTES + 1))
        self.assert_failure(Failure.LIMIT, self.store.read)
        self.store.path.unlink()
        self.store.initialize("store-a")
        for index in range(MAX_RECORDS):
            self.mutate(self.grant(f"grant-{index}"))
        before = self.store.path.read_bytes()
        self.assert_failure(Failure.LIMIT,
                            lambda: self.store.mutate(self.store.read().version, self.grant("overflow")))
        self.assertEqual(self.store.path.read_bytes(), before)
        self.assertEqual(len(self.store.read().active), MAX_RECORDS)

    # P5/JRN-7: project commit cannot imply session audit or tool execution succeeded.
    def test_session_audit_failure_keeps_the_project_grant_but_starts_no_effect(self):
        audit, effect = self.root / "audit-is-a-directory", self.marker("effect")
        audit.mkdir()
        result = self.spawn("grant_then_audit", mutation=self.grant(), audit=str(audit), effect=effect).collect()
        self.assertEqual(result["status"], Failure.AUDIT.value)
        self.assertFalse(Path(effect).exists())
        self.assertEqual(self.spawn("read").collect()["active"], {"grant-a": "ordinary-edits"})

    def test_successful_audit_precedes_the_effect(self):
        audit, effect = self.marker("audit.jsonl"), self.marker("effect")
        result = self.spawn("grant_then_audit", mutation=self.grant(), audit=audit, effect=effect).collect()
        self.assertEqual(result["status"], "ok")
        self.assertEqual(json.loads(Path(audit).read_text()), {"store": "store-a", "revision": 1})
        self.assertEqual(Path(effect).read_text(), "executed\n")


if __name__ == "__main__":
    unittest.main()
