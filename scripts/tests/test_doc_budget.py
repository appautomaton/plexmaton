"""Corpus §Budgets: new documents are measured before staging; warnings stay advisory."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "check-doc-budget.sh"


class DocumentBudgetTests(unittest.TestCase):
    def gate(self, source, *, tracked=False, ignored=False, name=".agents/standards/new.md"):
        with tempfile.TemporaryDirectory(prefix="plexmaton-budget-test-") as folder:
            root = Path(folder)
            (root / "scripts").mkdir()
            shutil.copy2(SCRIPT, root / "scripts/check-doc-budget.sh")
            path = root / name
            path.parent.mkdir(parents=True)
            path.write_text(source)
            env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
            env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
            subprocess.run(["git", "init", "--quiet", str(root)], env=env,
                           check=True, capture_output=True, timeout=5)
            if tracked:
                subprocess.run(["git", "add", name], cwd=root,
                               env=env, check=True, capture_output=True, timeout=5)
            if ignored:
                (root / ".gitignore").write_text(name + "\n")
            return subprocess.run(
                ["bash", str(root / "scripts/check-doc-budget.sh")], cwd=root,
                env=env, text=True, capture_output=True, timeout=10,
            )

    def test_untracked_document_is_measured_at_the_byte_boundary(self):
        for source, warns in (("x" * 8192, False), ("é" * 4097, True)):
            with self.subTest(warns=warns):
                result = self.gate(source)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual("over its" in result.stderr, warns)
                if warns:
                    self.assertIn(".agents/standards/new.md", result.stderr)
                    self.assertIn("8194 bytes", result.stderr)

    def test_staged_document_is_reported_once(self):
        result = self.gate("x" * 8193, tracked=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr.count("over its"), 1)

    def test_git_path_quoting_does_not_hide_a_document(self):
        name = ".agents/standards/审查 notes.md"
        result = self.gate("x" * 8193, name=name)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(name, result.stderr)
        self.assertIn("over its", result.stderr)

    def test_ignored_untracked_material_is_not_part_of_the_corpus(self):
        result = self.gate("x" * 8193, ignored=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("over its", result.stderr)


if __name__ == "__main__":
    unittest.main()
