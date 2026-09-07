"""Real Git/Cargo boundary: private seeds must not replay another checkout's code."""

import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/seed-rust-deps.py"


@unittest.skipUnless(sys.platform == "darwin", "Project seeder requires macOS APFS")
class CargoSeedTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="plexmaton-cargo-seed-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        cargo = subprocess.check_output(["rustup", "which", "cargo"], cwd=ROOT,
                                        text=True, timeout=15).strip()
        self.env = {key: value for key, value in os.environ.items()
                    if key in {"PATH", "HOME", "USER", "LOGNAME", "TMPDIR", "RUSTUP_HOME"}}
        self.env.update(PATH=str(Path(cargo).parent) + os.pathsep + self.env["PATH"],
                        RUSTC=str(Path(cargo).with_name("rustc")),
                        CARGO_HOME=str(self.root / "cargo-home"),
                        GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        dependency = self.root / "dependency"
        self.write(dependency / "Cargo.toml", '[package]\nname="seed-dependency"\nversion="1.0.0"\nedition="2024"\n[features]\nalt=[]\n')
        self.write(dependency / "src/lib.rs", 'pub fn value() -> u32 { if cfg!(feature="alt") { 22 } else { 11 } }\n#[inline(never)]\npub fn inspect() -> String { std::backtrace::Backtrace::force_capture().to_string() }\n')
        self.command(["git", "init", "-q", "-b", "main"], dependency)
        self.commit(dependency)
        revision = self.command(["git", "rev-parse", "HEAD"], dependency).stdout.strip()

        initial = self.root / "initial"
        self.write(initial / "Cargo.toml", '[package]\nname="seed-app"\nversion="1.0.0"\nedition="2024"\n[workspace]\n[dependencies]\nseed-dependency={git="' + dependency.as_uri() + '",rev="' + revision + '"}\n')
        self.write(initial / "src/main.rs", 'fn main() { println!("SOURCE_A {} {}", seed_dependency::value(), env!("CARGO_MANIFEST_DIR")); if std::env::var_os("SEED_BACKTRACE").is_some() { println!("{}", seed_dependency::inspect()); } }\n')
        self.write(initial / ".gitignore", 'target/\n')
        # The only Git dependency is a fixture-owned file:// URL; no remote service is used.
        self.command(["cargo", "generate-lockfile"], initial)
        self.command(["git", "init", "-q", "-b", "main"], initial)
        self.commit(initial)
        self.control = self.root / "control.git"
        self.command(["git", "clone", "--bare", "--local", "--no-hardlinks", str(initial), str(self.control)])
        shutil.rmtree(initial)
        self.producer = self.root / "producer"
        self.consumer = self.root / "consumer"
        for worktree in [self.producer, self.consumer]:
            self.command(["git", "--git-dir=" + str(self.control), "worktree", "add", "--detach", str(worktree), "HEAD"])
        self.addCleanup(self.remove_worktrees)

    def write(self, path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)

    def command(self, args, cwd=None):
        result = subprocess.run(args, cwd=cwd or self.root, env=self.env, text=True,
                                capture_output=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def commit(self, root):
        self.command(["git", "add", "."], root)
        self.command(["git", "-c", "user.name=Seed Fixture", "-c", "user.email=seed@example.invalid",
                      "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"], root)

    def remove_worktrees(self):
        for worktree in [self.producer, self.consumer]:
            if worktree.exists():
                self.command(["git", "restore", "Cargo.toml", "src/main.rs"], worktree)
                self.command(["git", "--git-dir=" + str(self.control), "worktree", "remove", str(worktree)])

    def build(self, worktree):
        result = self.command(["cargo", "build", "--offline", "--locked", "--jobs", "2",
                               "--message-format=json", "--config", 'build.rustc-wrapper=""',
                               "--config", 'build.rustc-workspace-wrapper=""'], worktree)
        events = [json.loads(line) for line in result.stdout.splitlines()]
        artifacts = [event for event in events if event["reason"] == "compiler-artifact"]
        binary = next(event["executable"] for event in artifacts if event.get("executable"))
        dependency = next(event for event in artifacts if event["target"]["name"] == "seed_dependency")
        output = self.command([binary], worktree).stdout.strip()
        return result.stdout, dependency, output

    def test_git_dependency_reuse_local_identity_and_feature_invalidation(self):
        # Both worktrees exist before the donor build: older source mtimes cannot justify reuse.
        source = self.consumer / "src/main.rs"
        source.write_text(source.read_text().replace("SOURCE_A", "SOURCE_B"))
        log, producer_dependency, output = self.build(self.producer)
        self.assertEqual(producer_dependency["profile"]["debuginfo"], 2)
        self.assertEqual(output, f"SOURCE_A 11 {self.producer}")
        build_log = self.root / "build.jsonl"
        build_log.write_text(log)
        seeded = self.command([sys.executable, str(SCRIPT), "--source", str(self.producer),
                               "--destination", str(self.consumer), "--build-log", str(build_log)])
        self.assertEqual(json.loads(seeded.stdout)["packages"], 1)
        self.command(["git", "--git-dir=" + str(self.control), "worktree", "remove", str(self.producer)])
        self.assertFalse(self.producer.exists())
        _, dependency, output = self.build(self.consumer)
        self.assertTrue(dependency["fresh"])
        self.assertEqual(output, f"SOURCE_B 11 {self.consumer}")

        trace = subprocess.run([str(self.consumer / "target/debug/seed-app")], cwd=self.consumer,
                               env={**self.env, "SEED_BACKTRACE": "1"}, text=True,
                               capture_output=True, timeout=10)
        self.assertEqual(trace.returncode, 0, trace.stderr)
        self.assertRegex(trace.stdout, r"inspect\n\s+at .*cargo-home/git/checkouts/.*/src/lib\.rs:\d+")

        manifest = self.consumer / "Cargo.toml"
        manifest.write_text(manifest.read_text().replace('rev="', 'features=["alt"],rev="'))
        _, dependency, output = self.build(self.consumer)
        self.assertFalse(dependency["fresh"])
        self.assertEqual(output, f"SOURCE_B 22 {self.consumer}")

        source.write_text(source.read_text().replace("SOURCE_B", "SOURCE_C"))
        _, dependency, output = self.build(self.consumer)
        self.assertTrue(dependency["fresh"])
        self.assertEqual(output, f"SOURCE_C 22 {self.consumer}")

    def test_real_cargo_build_lock_refuses_seed_without_destination(self):
        log, _, _ = self.build(self.producer)
        build_log = self.root / "build.jsonl"
        build_log.write_text(log)
        gate = self.root / "gate"
        ready = self.root / "gate.ready"
        os.mkfifo(gate)
        build_script = self.producer / "build.rs"
        build_script.write_text('''
use std::io::Read;
fn main() {
    println!("cargo:rerun-if-env-changed=SEED_LOCK_GATE");
    if let Ok(path) = std::env::var("SEED_LOCK_GATE") {
        std::fs::write(format!("{path}.ready"), "ready").unwrap();
        let mut file = std::fs::File::open(path).unwrap();
        file.read_exact(&mut [0]).unwrap();
    }
}
''')
        process = None
        try:
            with (self.root / "busy.out").open("w") as out, (self.root / "busy.err").open("w") as err:
                process = subprocess.Popen(["cargo", "build", "--offline", "--locked", "--jobs", "2"],
                                           cwd=self.producer, env={**self.env, "SEED_LOCK_GATE": str(gate)},
                                           stdout=out, stderr=err, start_new_session=True)
                deadline = time.monotonic() + 20
                while not ready.exists():
                    self.assertIsNone(process.poll(), (self.root / "busy.err").read_text())
                    self.assertLess(time.monotonic(), deadline, "Cargo did not reach the build-script barrier")
                    time.sleep(0.01)
                result = subprocess.run([sys.executable, str(SCRIPT), "--source", str(self.producer),
                                         "--destination", str(self.consumer), "--build-log", str(build_log)],
                                        env=self.env, text=True, capture_output=True, timeout=10)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("busy", result.stderr)
                self.assertFalse((self.consumer / "target").exists())
                self.assertFalse(list(self.consumer.glob(".target-seed-*")))
        finally:
            if process is not None and process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL); process.wait(timeout=5)
            build_script.unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
