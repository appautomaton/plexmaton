"""Quality gates §Local setup: graph validation must not rewrite dependency choices."""
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class CrateGraphTests(unittest.TestCase):
    def graph_with_cargo_progress(self, *, forbidden_edge=False):
        # Resolve a real dependency graph; only Cargo's progress diagnostic is injected.
        dependencies = {
            "core": [], "math": [], "agent": ["core"], "tui": ["core", "math"],
            "provider": ["agent"], "file-tools": ["agent"], "command": ["agent"],
            "skills": ["file-tools"], "session-store": ["agent"],
            "permission-store": ["agent"],
            "runtime": ["command", "permission-store", "provider", "session-store", "skills"],
        }
        if forbidden_edge:
            dependencies["agent"].append("tui")
        with tempfile.TemporaryDirectory(prefix="plexmaton-graph-progress-") as folder:
            root = Path(folder)
            (root / "scripts").mkdir()
            shutil.copy2(ROOT / "scripts/check-crate-graph.sh", root / "scripts/check-crate-graph.sh")
            shutil.copy2(ROOT / "rust-toolchain.toml", root / "rust-toolchain.toml")
            members = ", ".join(f'"{name}"' for name in dependencies)
            (root / "Cargo.toml").write_text(f'[workspace]\nmembers = [{members}]\nresolver = "3"\n')
            for name, deps in dependencies.items():
                member = root / name
                (member / "src").mkdir(parents=True)
                (member / "src/lib.rs").write_text("")
                manifest = f'[package]\nname = "plexmaton-{name}"\nversion = "0.1.0"\nedition = "2024"\n'
                manifest += "[dependencies]\n"
                manifest += "".join(f'plexmaton-{dep} = {{ path = "../{dep}" }}\n' for dep in deps)
                (member / "Cargo.toml").write_text(manifest)
            env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
            env.update(CARGO_NET_OFFLINE="true", CARGO_TARGET_DIR=str(root / "target"))
            real_cargo = shutil.which("cargo")
            self.assertIsNotNone(real_cargo)
            subprocess.run([real_cargo, "generate-lockfile", "--offline"], cwd=root, env=env,
                           check=True, capture_output=True, timeout=10)
            (root / "bin").mkdir()
            wrapper = root / "bin/cargo"
            wrapper.write_text(
                "#!/bin/sh\nprintf '%s\\n' '    Blocking waiting for file lock on package cache' >&2\n"
                f"exec {shlex.quote(real_cargo)} \"$@\"\n"
            )
            wrapper.chmod(0o755)
            env["PATH"] = str(root / "bin") + os.pathsep + env.get("PATH", "")
            return subprocess.run(
                ["bash", str(root / "scripts/check-crate-graph.sh")], cwd=root, env=env,
                text=True, capture_output=True, timeout=10,
            )

    def test_cargo_progress_does_not_turn_the_root_into_a_dependency(self):
        result = self.graph_with_cargo_progress()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_cargo_progress_does_not_hide_a_forbidden_dependency(self):
        result = self.graph_with_cargo_progress(forbidden_edge=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("plexmaton-agent reaches", result.stderr)
        self.assertIn("plexmaton-tui", result.stderr)

    def test_stale_lockfile_is_refused_without_rewriting_it(self):
        with tempfile.TemporaryDirectory(prefix="plexmaton-graph-test-") as folder:
            root = Path(folder)
            (root / "scripts").mkdir()
            shutil.copy2(ROOT / "scripts/check-crate-graph.sh", root / "scripts/check-crate-graph.sh")
            shutil.copy2(ROOT / "rust-toolchain.toml", root / "rust-toolchain.toml")
            (root / "Cargo.toml").write_text('[workspace]\nmembers = ["core"]\nresolver = "3"\n')
            (root / "core/src").mkdir(parents=True)
            (root / "core/src/lib.rs").write_text("")
            manifest = root / "core/Cargo.toml"
            manifest.write_text('[package]\nname = "plexmaton-core"\nversion = "0.1.0"\nedition = "2024"\n')
            env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
            env.update(CARGO_NET_OFFLINE="true", CARGO_TARGET_DIR=str(root / "target"))
            subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, env=env,
                           check=True, capture_output=True, timeout=10)
            lock = root / "Cargo.lock"
            original = lock.read_bytes()
            manifest.write_text(manifest.read_text().replace('version = "0.1.0"', 'version = "0.2.0"'))
            result = subprocess.run(
                ["bash", str(root / "scripts/check-crate-graph.sh")], cwd=root, env=env,
                text=True, capture_output=True, timeout=10,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("--locked", result.stderr)
            self.assertEqual(lock.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
