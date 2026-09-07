"""Public checkouts exclude private/generated state without hiding source or evidence."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

IGNORE = Path(__file__).resolve().parents[2] / ".gitignore"


class GitIgnoreTests(unittest.TestCase):
    def ignored(self, paths):
        with tempfile.TemporaryDirectory(prefix="plexmaton-ignore-") as folder:
            root = Path(folder)
            shutil.copy2(IGNORE, root / ".gitignore")
            # Fixture Git must not inherit a hook's repository or the user's ignore rules.
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith("GIT_")}
            env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
            subprocess.run(["git", "init", "--quiet", str(root)], env=env,
                           check=True, capture_output=True, timeout=5)
            result = subprocess.run(
                ["git", "-C", str(root), "-c", f"core.excludesFile={os.devnull}",
                 "check-ignore", "--no-index", "--stdin", "-z"],
                input="\0".join(paths) + "\0", text=True, capture_output=True,
                env=env, timeout=5,
            )
            self.assertIn(result.returncode, (0, 1), result.stderr)
            return set(filter(None, result.stdout.split("\0")))

    def test_private_and_generated_paths_are_ignored(self):
        paths = [
            ".references/upstream/src/lib.rs", ".worktrees/review/Cargo.toml",
            "target/debug/plexmaton", "crates/example/target/debug/build.o",
            "target-review/debug/plexmaton", ".local/plexmaton/config.toml",
            ".target-seed-owned/debug/deps/libexample.rlib",
            ".local/plexmaton/sessions/private.jsonl",
            ".plexmaton/sessions/private.jsonl",
            ".plexmaton/projects/private/permissions.jsonl",
            ".env", ".env.production", "nested/.env.local",
            "private.pem", "signing.key", "identity.p12", "id_ed25519",
            "scripts/__pycache__/helper.pyc", ".venv/bin/python",
            "node_modules/example/index.js", "coverage/report.html",
            "fuzz/artifacts/crash", "bench-results/run.json",
        ]
        self.assertEqual(self.ignored(paths), set(paths))

    def test_source_configuration_and_sanitized_evidence_remain_trackable(self):
        paths = [
            "Cargo.lock", "Cargo.toml", "AGENTS.md", "README.md", "LICENSE",
            ".agents/specs/permission-policy.md", ".agents/plans/stage.md",
            ".agents/skills/review/SKILL.md", ".plexmaton/skills/review/SKILL.md",
            ".plexmaton/config.toml", ".env.example", ".env.template",
            "crates/example/tests/fixtures/journal.jsonl",
            "crates/example/frames/review.svg", "examples/demo.png",
            "crates/example/proptest-regressions/input.txt", "scripts/smoke-tui.py",
            ".github/workflows/ci.yml",
        ]
        self.assertEqual(self.ignored(paths), set())


if __name__ == "__main__":
    unittest.main()
