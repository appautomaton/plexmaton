"""Corpus §Specs: identifiers have one owner and citations survive document changes."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]


class CitationGateTests(unittest.TestCase):
    def gate(self, files):
        with tempfile.TemporaryDirectory(prefix="plexmaton-citations-test-") as folder:
            root = Path(folder)
            (root / "scripts").mkdir()
            (root / "crates").mkdir()
            (root / ".agents/specs").mkdir(parents=True)
            for name in ("check-citations.sh", "check-citations.py"):
                shutil.copy2(SCRIPTS / name, root / "scripts" / name)
            for name, source in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source)
            return subprocess.run(
                ["bash", str(root / "scripts/check-citations.sh")], cwd=root,
                text=True, capture_output=True, timeout=10,
            )

    def test_duplicate_invariant_cannot_hide_behind_a_resolving_citation(self):
        result = self.gate({
            "crates/example.rs": "// CMD-1\n",
            ".agents/specs/shell.md": "**CMD-1 — Shell admission.**\n",
            ".agents/specs/menu.md": "**CMD-1 — Composer commands.**\n",
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate invariant CMD-1", result.stderr)
        self.assertIn("shell.md:1", result.stderr)
        self.assertIn("menu.md:1", result.stderr)

    def test_a_prefix_cannot_be_split_even_when_ids_are_distinct(self):
        result = self.gate({
            ".agents/specs/first.md": "**OWN-1 — First.**\n",
            ".agents/specs/second.md": "**OWN-2 — Second.**\n",
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("prefix OWN belongs to multiple specs", result.stderr)

    def test_missing_invariants_and_renamed_proofs_still_fail(self):
        result = self.gate({
            "crates/example.rs": "// LOST-1\n#[test]\nfn new_name() {}\n",
            ".agents/specs/example.md": (
                "**OWN-1 — An invariant.**\n## Evidence\n| OWN-1 | `old_name` |\n"
            ),
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("LOST-1 is cited in code", result.stderr)
        self.assertIn("old_name is named as proof", result.stderr)

    def test_valid_owners_and_proofs_resolve(self):
        result = self.gate({
            "crates/example.rs": "// OWN-1 OWN-2; ui-ux §interaction.\n#[test]\nfn witness() {}\n",
            ".agents/specs/example.md": (
                "**OWN-1 — First.**\n**OWN-2 — Second.**\n"
                "## Evidence\n| OWN-1 | `witness` |\n| OWN-2 | `witness` |\n"
            ),
            ".agents/ui-ux.md": "# Interaction\n",
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("2 invariants and 1 contract sections", result.stdout)

    def test_a_renamed_heading_fails_even_when_the_file_exists(self):
        result = self.gate({
            "AGENTS.md": "[compaction](.agents/phase.md#compaction--implemented)\n",
            ".agents/phase.md": "### Compaction — complete\n",
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing heading .agents/phase.md#compaction--implemented", result.stderr)

    def test_unicode_duplicate_and_setext_headings_resolve(self):
        result = self.gate({
            "AGENTS.md": (
                "[current](.agents/phase.md#compaction--complete)\n"
                "[second](.agents/phase.md#compaction--complete-1)\n"
                "[unicode](<.agents/phase.md#%E4%B8%AD%E6%96%87>)\n"
                "[setext](.agents/phase.md#appendix)\n"
                "[formatting](.agents/phase.md#some-inline-code)\n"
                "[remote](https://example.invalid/page#unfetched)\n"
            ),
            ".agents/phase.md": (
                "### Compaction — complete\n\n### Compaction — complete\n\n"
                "# 中文\n\nAppendix\n=======\n\n# Some _inline_ `code`\n"
            ),
        })
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_fenced_examples_cannot_supply_a_missing_heading(self):
        result = self.gate({
            "AGENTS.md": "[example](.agents/phase.md#example)\n",
            ".agents/phase.md": "```markdown\n# Example\n```\n",
        })
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing heading", result.stderr)

    def test_missing_file_and_missing_self_anchor_fail(self):
        result = self.gate({"AGENTS.md": "[file](gone.md)\n[self](#gone)\n"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("gone.md, which does not exist", result.stderr)
        self.assertIn("missing heading #gone", result.stderr)


if __name__ == "__main__":
    unittest.main()
