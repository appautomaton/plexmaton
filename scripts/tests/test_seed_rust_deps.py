"""Dependency seeds preserve Cargo's native build and task-ownership boundaries."""

import importlib.util
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "seed-rust-deps.py"
SPEC = importlib.util.spec_from_file_location("seed_rust_deps", SCRIPT)
SEED = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SEED)
REGISTRY = "registry+https://example.invalid/index"


def package(name, source=REGISTRY, kind="lib", dependencies=(), links=None):
    package_id = f"{source or 'path+file:///fixture' }#{name}@1.0.0"
    return ({"id": package_id, "name": name, "source": source, "links": links,
             "manifest_path": f"/fixture-sources/{name}/Cargo.toml",
             "targets": [{"name": name.replace("-", "_"), "kind": [kind], "crate_types": [kind]}]},
            {"id": package_id, "dependencies": list(dependencies)})


def metadata(*pairs):
    return {"packages": [pair[0] for pair in pairs], "resolve": {"nodes": [pair[1] for pair in pairs]}}


class SelectionTests(unittest.TestCase):
    def test_execution_and_path_boundaries_exclude_transitive_consumers(self):
        # rust-builds.md: local state and execution-dependent results stay with Cargo.
        local = package("local", source=None)
        script = package("script", kind="custom-build")
        macro = package("macro", kind="proc-macro")
        native = package("native", links="native")
        unknown = package("unknown", source="future+source")
        blocked = [local, script, macro, native, unknown]
        direct = package("direct", dependencies=[pair[0]["id"] for pair in blocked])
        transitive = package("transitive", dependencies=[direct[0]["id"]])
        plain = package("plain")
        git = package("git", source="git+file:///immutable?rev=abc")
        packages, eligible = SEED.eligible_packages(metadata(*blocked, direct, transitive, plain, git))
        self.assertEqual({packages[key]["name"] for key in eligible}, {"plain", "git"})

    def test_requires_complete_graph(self):
        data = metadata(package("plain"))
        data["resolve"]["nodes"] = []
        with self.assertRaises(SEED.Refused):
            SEED.eligible_packages(data)

    def make_artifact(self, root, name="plain"):
        item, node = package(name)
        crate = name.replace("-", "_")
        unit_hash = "1234567890abcdef"
        output = root / "debug/deps" / f"lib{crate}-{unit_hash}.rlib"
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(b"immutable compiled library")
        fingerprint = root / "debug/.fingerprint" / f"{name}-{unit_hash}"
        fingerprint.mkdir(parents=True, exist_ok=True)
        (fingerprint / f"lib-{crate}").write_bytes(b"fingerprint")
        event = {"reason": "compiler-artifact", "package_id": item["id"], "target": item["targets"][0],
                 "manifest_path": item["manifest_path"],
                 "profile": {"test": False}, "executable": None, "filenames": [str(output)]}
        return metadata((item, node)), event, output, fingerprint

    def test_selects_exact_unit_and_fingerprint_without_unrelated_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data, event, output, fingerprint = self.make_artifact(root)
            (root / "debug/deps/libunrelated-1234567890abcdef.rlib").write_bytes(b"other")
            (root / "debug/incremental").mkdir()
            files, counts = SEED.select_files(data, [event], root)
            self.assertEqual(set(files), {output.relative_to(root),
                                         (fingerprint / "lib-plain").relative_to(root)})
            self.assertEqual(counts, {"packages": 1, "units": 1, "excluded_packages": 0})

    def test_local_same_name_does_not_become_registry_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data, event, _, _ = self.make_artifact(root)
            local = package("plain", source=None)
            data["packages"].append(local[0]); data["resolve"]["nodes"].append(local[1])
            local_event = {**event, "package_id": local[0]["id"]}
            files, counts = SEED.select_files(data, [local_event, event], root)
            self.assertEqual(set(files.values()), {event["package_id"]})
            self.assertEqual(counts["excluded_packages"], 1)

    def test_rejects_unknown_owner_symlink_and_layout(self):
        for case in ["owner", "symlink", "layout", "source-location"]:
            with self.subTest(case=case), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                data, event, output, _ = self.make_artifact(root)
                if case == "owner":
                    event["package_id"] = "not-in-metadata"
                elif case == "symlink":
                    output.unlink(); output.symlink_to(SCRIPT)
                elif case == "source-location":
                    event["manifest_path"] = "/another-source/Cargo.toml"
                else:
                    event["filenames"] = [str(root / "release/deps" / output.name)]
                with self.assertRaises(SEED.Refused):
                    SEED.select_files(data, [event], root)

    def test_build_log_requires_one_final_success(self):
        cases = [[], [{"reason": "build-finished", "success": False}],
                 [{"reason": "build-finished", "success": True}] * 2,
                 [{"reason": "build-finished", "success": True}, {"reason": "other"}]]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "build.jsonl"
            for events in cases:
                path.write_text("\n".join(map(json.dumps, events)))
                with self.subTest(events=events), self.assertRaises(SEED.Refused):
                    SEED.build_events(path)
            path.write_text('{"reason":"build-finished","success":true}\n')
            self.assertEqual(len(SEED.build_events(path)), 1)

    def test_log_reads_are_bounded_and_reject_symlinks_and_special_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "build.jsonl"
            path.write_bytes(b"x" * 65)
            with mock.patch.object(SEED, "MAX_LOG_BYTES", 64), self.assertRaises(SEED.Refused):
                SEED.build_events(path)
            # Size metadata can be stale while a file grows; the read itself stays bounded.
            info = path.stat()
            with (mock.patch.object(SEED, "MAX_LOG_BYTES", 64),
                  mock.patch.object(SEED.os, "fstat", return_value=mock.Mock(st_mode=info.st_mode, st_size=0)),
                  self.assertRaises(SEED.Refused)):
                SEED.build_events(path)
            link = root / "link"
            link.symlink_to(path)
            with self.assertRaises(OSError):
                SEED.build_events(link)
            fifo = root / "fifo"
            os.mkfifo(fifo)
            code = '''
import runpy,sys
from pathlib import Path
module = runpy.run_path(sys.argv[1])
try: module["build_events"](Path(sys.argv[2]))
except module["Refused"]: sys.exit(0)
sys.exit(1)
'''
            result = subprocess.run([sys.executable, "-c", code, str(SCRIPT), str(fifo)], timeout=5)
            self.assertEqual(result.returncode, 0)

    def test_external_source_locations_cannot_depend_on_a_worktree(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            worktree = root / "worktree"
            worktree.mkdir()
            source = root / "shared-source/Cargo.toml"
            source.parent.mkdir(); source.write_text("source")
            data = metadata(package("plain"))
            data["packages"][0]["manifest_path"] = str(source)
            self.assertEqual(list(SEED.external_sources(data, [worktree]).values()), [str(source)])
            vendored = worktree / "vendor/Cargo.toml"
            vendored.parent.mkdir(); vendored.write_text("vendored")
            data["packages"][0]["manifest_path"] = str(vendored)
            with self.assertRaises(SEED.Refused):
                SEED.external_sources(data, [worktree])


@unittest.skipUnless(sys.platform == "darwin", "APFS publication uses macOS clone and rename APIs")
class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="plexmaton-seed-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.target = self.root / "source-target"
        self.destination = self.root / "target"
        self.relative = Path("debug/deps/libplain-1234567890abcdef.rlib")
        self.source = self.target / self.relative
        self.source.parent.mkdir(parents=True)
        self.source.write_bytes(b"original library data")
        self.selected = {self.relative: "registry+test#plain@1.0.0"}
        for name in [".cargo-build-lock", ".cargo-lock", ".cargo-artifact-lock"]:
            (self.target / "debug" / name).write_bytes(b"")

    def test_clone_is_independent_preserves_mtime_and_omits_locks(self):
        before = SEED.identity(self.source.stat())
        with SEED.source_locks(self.target) as verify:
            result = SEED.seed(self.target, self.destination, self.selected, verify_source=verify)
        clone = self.destination / self.relative
        self.assertEqual(clone.read_bytes(), self.source.read_bytes())
        self.assertNotEqual(clone.stat().st_ino, self.source.stat().st_ino)
        self.assertEqual(clone.stat().st_mtime_ns, self.source.stat().st_mtime_ns)
        clone.write_bytes(b"consumer change")
        self.assertEqual(self.source.read_bytes(), b"original library data")
        self.assertEqual(SEED.identity(self.source.stat()), before)
        self.assertEqual(result, {"files": 1, "logical_bytes": len(b"original library data")})
        self.assertFalse(list(self.destination.rglob(".cargo*lock")))
        self.assertFalse(list(self.root.glob(".target-seed-*")))

    def test_existing_or_racing_destination_is_preserved(self):
        self.destination.mkdir()
        marker = self.destination / "user-data"
        marker.write_text("keep")
        with self.assertRaises(SEED.Refused):
            SEED.seed(self.target, self.destination, self.selected)
        self.assertEqual(marker.read_text(), "keep")
        marker.unlink(); self.destination.rmdir()

        def race(stage, destination):
            destination.mkdir(); (destination / "user-data").write_text("racer")
            SEED.publish(stage, destination)

        with self.assertRaises(SEED.Refused):
            SEED.seed(self.target, self.destination, self.selected, publisher=race)
        self.assertEqual(marker.read_text(), "racer")
        self.assertFalse(list(self.root.glob(".target-seed-*")))

    def test_failure_and_cancellation_remove_only_the_owned_stage(self):
        other = self.root / ".target-seed-other-task"
        other.mkdir()
        for failure in [OSError("clone failure"), KeyboardInterrupt()]:
            def fail(source, destination):
                SEED.clone_file(source, destination)
                raise failure
            with self.subTest(failure=type(failure)), self.assertRaises(type(failure)):
                SEED.seed(self.target, self.destination, self.selected, copier=fail)
            self.assertFalse(self.destination.exists())
            self.assertEqual(list(self.root.glob(".target-seed-*")), [other])

    def test_changed_source_or_replaced_lock_prevents_publication(self):
        def change(source, destination):
            result = SEED.clone_file(source, destination)
            source.write_bytes(b"another writer changed the source")
            return result
        with self.assertRaises(SEED.Refused):
            SEED.seed(self.target, self.destination, self.selected, copier=change)
        self.assertFalse(self.destination.exists())
        with SEED.source_locks(self.target) as verify:
            lock = self.target / "debug/.cargo-build-lock"
            lock.rename(lock.with_suffix(".retired")); lock.write_bytes(b"")
            with self.assertRaises(SEED.Refused):
                SEED.seed(self.target, self.destination, self.selected, verify_source=verify)
        self.assertFalse(self.destination.exists())

    def test_source_lock_conflicts_with_another_process_and_releases(self):
        command = [sys.executable, "-c", '''
import fcntl,sys
with open(sys.argv[1], "rb") as file:
    try: fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError: sys.exit(7)
''', str(self.target / "debug/.cargo-build-lock")]
        with SEED.source_locks(self.target):
            self.assertEqual(subprocess.run(command, timeout=5, check=False).returncode, 7)
        self.assertEqual(subprocess.run(command, timeout=5, check=False).returncode, 0)

    def test_interruption_after_publish_returns_the_complete_committed_result(self):
        def publish_then_interrupt(stage, destination):
            SEED.publish(stage, destination)
            raise KeyboardInterrupt
        result = SEED.seed(self.target, self.destination, self.selected, publisher=publish_then_interrupt)
        self.assertEqual(result["files"], 1)
        self.assertEqual((self.destination / self.relative).read_bytes(), self.source.read_bytes())
        self.assertFalse(list(self.root.glob(".target-seed-*")))

    def test_replaced_stage_is_not_published_or_deleted(self):
        def replace(source, output):
            result = SEED.clone_file(source, output)
            stage = output.parents[len(self.relative.parts) - 1]
            stage.rename(stage.with_name(stage.name + "-moved"))
            stage.mkdir(); (stage / "other-owner").write_text("keep")
            return result
        with self.assertRaises(SEED.Refused):
            SEED.seed(self.target, self.destination, self.selected, copier=replace)
        self.assertFalse(self.destination.exists())
        self.assertEqual(len(list(self.root.glob(".target-seed-*/other-owner"))), 1)

    def test_sigterm_before_publication_cleans_the_private_stage(self):
        code = '''
import runpy,sys,signal
from pathlib import Path
module = runpy.run_path(sys.argv[1])
signal.signal(signal.SIGTERM, module["interrupted"])
def wait_after_clone(source, destination):
    size = module["clone_file"](source, destination)
    print("ready", flush=True)
    sys.stdin.read(1)
    return size
try:
    module["seed"](Path(sys.argv[2]), Path(sys.argv[3]), {Path(sys.argv[4]): "fixture"}, copier=wait_after_clone)
except KeyboardInterrupt: sys.exit(130)
'''
        process = subprocess.Popen([sys.executable, "-c", code, str(SCRIPT), str(self.target),
                                    str(self.destination), str(self.relative)],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, start_new_session=True)
        try:
            ready, _, _ = select.select([process.stdout], [], [], 5)
            self.assertTrue(ready, "Child did not reach clone barrier")
            self.assertEqual(process.stdout.readline().strip(), "ready")
            process.send_signal(signal.SIGTERM)
            _, stderr = process.communicate(timeout=5)
            self.assertEqual(process.returncode, 130, stderr)
        finally:
            if process.poll() is None:
                process.kill(); process.communicate(timeout=5)
        self.assertFalse(self.destination.exists())
        self.assertFalse(list(self.root.glob(".target-seed-*")))


if __name__ == "__main__":
    unittest.main()
