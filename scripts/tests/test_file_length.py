"""The sentinel must inspect production after external test modules and cfg-gated helpers."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "check-file-length.sh"


class FileLengthGateTests(unittest.TestCase):
    def gate(self, files):
        with tempfile.TemporaryDirectory(prefix="plexmaton-gate-") as folder:
            root = Path(folder)
            (root / "scripts").mkdir()
            target = root / "scripts/check-file-length.sh"
            shutil.copy2(SCRIPT, target)
            for name, source in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source)
            return subprocess.run(
                [str(target)], env=dict(os.environ, FILE_LENGTH_LIMIT="20"),
                capture_output=True, text=True, timeout=5,
            )

    def test_external_test_module_does_not_hide_production(self):
        for header in ("#[cfg(test)]\nmod tests;\n", "#[cfg(test)] mod tests;\n"):
            with self.subTest(header=header):
                result = self.gate({"crates/example/src/lib.rs": header + "// production\n" * 25})
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("lib.rs", result.stderr)

    def test_test_only_helper_does_not_hide_later_production(self):
        source = "impl Example {\n    #[cfg(test)]\n    fn helper() {}\n" + "    // production\n" * 25 + "}\n"
        result = self.gate({"crates/example/src/lib.rs": source})
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_trailing_inline_test_module_is_excluded(self):
        for header in ("#[cfg(test)]\nmod tests {\n", "#[cfg(test)] mod tests {\n"):
            with self.subTest(header=header):
                source = "// production\n" * 8 + header + "    // test\n" * 100 + "}\n"
                result = self.gate({"crates/example/src/lib.rs": source})
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_test_only_filenames_are_exempt(self):
        source = "// test\n" * 100
        result = self.gate({
            "crates/example/tests/contract.rs": source,
            "crates/example/src/model_tests.rs": source,
            "crates/example/src/tests.rs": source,
            "crates/example/src/test_support.rs": source,
        })
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_limit_boundary_counts_all_source_without_inline_tests(self):
        for lines, passes in ((20, True), (21, False)):
            with self.subTest(lines=lines):
                result = self.gate({"crates/example/src/lib.rs": "// production\n" * lines})
                self.assertEqual(result.returncode == 0, passes, result.stdout + result.stderr)

    def test_missing_source_directory_is_not_a_green_gate(self):
        result = self.gate({})
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
