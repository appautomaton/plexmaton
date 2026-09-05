"""Source-linked Rust preparation composed with the real native text encoder, offline."""
import copy
from pathlib import Path
import subprocess
import tempfile
import unittest

from reply import Reply, native_run
from transport import Capability


class ReplyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory(prefix="plexmaton-reply-test-")
        cls.addClassCleanup(cls.directory.cleanup)
        root = Path(__file__).resolve().parents[3]
        subprocess.run(["cargo", "run", "--offline", "--locked", "-p", "plexmaton-math", "--example",
                        "native_preview", "--", cls.directory.name], cwd=root, check=True, timeout=60,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        cls.reply = Reply(cls.directory.name)

    def test_every_formula_survives_three_width_preparation_pagination_and_encoding(self):
        # MTH-2/MTH-3: real engine geometry, not configured mock rectangles.
        for width, document in self.reply.documents.items():
            seen = []
            for number, page in enumerate(document["pages"]):
                wire = self.reply.frame(width, 44, Capability.SCALED, number, number + 1).wire()
                self.assertLess(len(wire), 256 * 1024)
                self.assertTrue(wire.endswith(b"\x1b[0m\x1b[?2026l"))
                for index, formula in enumerate(document["formulas"]):
                    if page["start"] <= formula["y"] < page["end"]:
                        self.assertLessEqual(formula["y"] + formula["height"], page["end"])
                        seen.append(index)
                for run in document["runs"]:
                    if page["start"] <= run["y"] < page["end"]:
                        self.assertIn(run["text"].encode(), wire)
            self.assertEqual(seen, list(range(61)))

    def test_source_fallback_withholds_scaled_transport_and_preserves_delimiters(self):
        # MTH-1: refused transport retains the delimited original, not only a rendered body.
        wire = self.reply.frame(60, 44, Capability.UNSUPPORTED, 0, 1).wire()
        self.assertNotIn(b"\x1b]66;", wire)
        self.assertIn(b"\\[", wire)
        self.assertIn(b"\\]", wire)
        self.assertIn(b"\\boxed{", wire)

    def test_encoder_refuses_controls_and_overlapping_reservations(self):
        # MTH-4: an invalid export must not inject terminal control or overprint a multicell.
        run = copy.deepcopy(self.reply.documents[60]["runs"][0])
        run["text"] = "\x1b]52;c;payload\x07"
        with self.assertRaisesRegex(ValueError, "native text"):
            native_run(run, 0, 0)
        reply = copy.deepcopy(self.reply)
        reply.documents[60]["runs"].append(copy.deepcopy(reply.documents[60]["runs"][0]))
        with self.assertRaisesRegex(ValueError, "overlap"):
            reply.frame(60, 44, Capability.SCALED, 0, 1)


if __name__ == "__main__":
    unittest.main()
