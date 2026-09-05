"""Transport witnesses; not a substitute for inspecting actual Kitty glyphs."""
import unittest
from unittest.mock import patch
import launch_macos
import ml_fixtures
import preview


class CapabilityTests(unittest.TestCase):
    def test_scale_width_and_ignored_sequences_are_distinct(self):
        for suffix, expected in [
            (b"5R\x1b[3;7R", preview.Capability.SCALED),
            (b"5R\x1b[3;6R", preview.Capability.WIDTH_ONLY),
            (b"3R\x1b[3;3R", preview.Capability.UNSUPPORTED),
        ]:
            self.assertEqual(preview.classify(b"\x1b[3;3R\x1b[3;" + suffix), expected)

    def test_partial_missing_or_foreign_responses_never_claim_scaling(self):
        complete = b"\x1b[3;3R\x1b[3;5R\x1b[3;7R"
        for end in range(len(complete)):
            self.assertEqual(preview.classify(complete[:end]), preview.Capability.UNVERIFIED)
        for data in [b"x" + complete, complete + complete, complete.replace(b"3;7", b"4;7")]:
            self.assertEqual(preview.classify(data), preview.Capability.UNVERIFIED)
        with self.assertRaises(ValueError):
            preview.classify(b"x" * 97)


class ReservationTests(unittest.TestCase):
    def test_invalid_scale_metadata_is_not_serialized(self):
        for scale in [(1, 0, 2, preview.Align.TOP), (8, 1, 2, preview.Align.TOP),
                      (1, 2, 2, preview.Align.TOP), (1.5, 1, 2, preview.Align.TOP),
                      (1, 1, 2, 9)]:
            with self.assertRaises(ValueError):
                preview.Canvas(60, 34).add(0, 0, "x", scale=scale)

    def test_a_smaller_glyph_still_reserves_its_full_multicell_rectangle(self):
        canvas = preview.Canvas(10, 4)
        canvas.add(0, 0, "x", scale=(2, 1, 2, preview.Align.CENTER))
        self.assertEqual(canvas.occupied, {(0, 0), (1, 0), (0, 1), (1, 1)})
        with self.assertRaisesRegex(ValueError, "overlap"):
            canvas.add(1, 1, "i", scale=(1, 1, 2, preview.Align.BOTTOM))

    def test_controls_unproven_unicode_and_viewport_overflow_are_rejected(self):
        for text in ["\x1b]52;c;xxx\x07", "x\ny", "中", ""]:
            with self.assertRaises(ValueError):
                preview.Canvas(60, 34).add(0, 0, text)
        with self.assertRaisesRegex(ValueError, "viewport"):
            preview.Canvas(60, 34).add(59, 33, "x", scale=(2, 1, 2, preview.Align.CENTER))

    def test_fixed_three_width_frames_have_disjoint_scripts_and_bounded_wire_output(self):
        for width in [120, 88, 60]:
            for variant in [0, 1]:
                canvas = preview.frame(width, 36, preview.Capability.SCALED, variant)
                wire = canvas.wire()
                self.assertLess(len(wire), 8192)
                self.assertIn(b";ij\x07" if variant == 0 else b";p\x07", wire)
                self.assertIn(b";total\x07", wire)
                self.assertIn(b";0\x07", wire)
                self.assertTrue(wire.startswith(b"\x1b[?2026h\x1b[0m\x1b[2J"))
                self.assertTrue(wire.endswith(b"\x1b[?2026l"))

    def test_unproven_capabilities_withhold_scaled_fixtures_and_keep_source(self):
        for capability in preview.Capability:
            if capability is preview.Capability.SCALED:
                continue
            wire = preview.frame(60, 34, capability).wire()
            self.assertNotIn(b"\x1b]66;", wire)
            self.assertIn(b"x_{ij}^{n+1}", wire)

    def test_small_viewports_refuse_instead_of_truncating_formulas(self):
        for width, height in [(59, 34), (60, 33), (4, 2)]:
            wire = preview.frame(width, height, preview.Capability.SCALED).wire()
            self.assertNotIn(b"\x1b]66;", wire)


class LauncherTests(unittest.TestCase):
    def test_direct_window_has_no_live_tmux_authority_or_persistent_settings(self):
        with patch.dict("os.environ", {"TMUX": "live", "KITTY_LISTEN_ON": "live",
                                      "MODEL_API_KEY": "secret", "HTTP_PROXY": "remote"}):
            command, env = launch_macos.launch_configuration("/tmp/owned-preview", 10)
        for key in ("TMUX", "KITTY_LISTEN_ON", "MODEL_API_KEY", "HTTP_PROXY"):
            self.assertNotIn(key, env)
        for key in ("KITTY_CONFIG_DIRECTORY", "KITTY_CACHE_DIRECTORY", "KITTY_RUNTIME_DIRECTORY"):
            self.assertTrue(env[key].startswith("/tmp/owned-preview"))
        self.assertEqual(command[1:3], ["--config", "NONE"])
        self.assertIn("allow_remote_control=no", command)
        self.assertIn("macos_quit_when_last_window_closed=yes", command)


class MachineLearningFixturesTests(unittest.TestCase):
    def test_rlhf_attention_and_matrix_terms_survive_all_review_widths(self):
        for width in [120, 88, 60]:
            canvas = preview.frame(width, 44, preview.Capability.SCALED, page=preview.Page.ML)
            wire = canvas.wire()
            self.assertLess(len(wire), 8192)
            for text in ("π", "φ", "β", "ref", "softmax", "QK", "√", "Σ", "ik", "kj", "19", "50"):
                self.assertIn(text.encode(), wire)
            self.assertEqual(wire.count("π".encode()), 3)
            self.assertEqual(wire.count("⎡".encode()), wire.count("⎤".encode()))

    def test_numeric_matrix_product_is_correct_before_and_after_replacement(self):
        self.assertEqual(ml_fixtures.matrix_values(0)[2], ((19, 22), (43, 50)))
        a, b, c = ml_fixtures.matrix_values(1)
        self.assertEqual(a, ((1, 0), (0, 1)))
        self.assertEqual(c, b)
        wire = preview.frame(60, 44, preview.Capability.SCALED, 1, page=preview.Page.ML).wire()
        self.assertIn("⎛1  0⎞".encode(), wire)
        self.assertNotIn("⎛19  22⎞".encode(), wire)

    def test_ml_without_scaling_preserves_all_tex_source_in_order(self):
        canvas = preview.frame(60, 44, preview.Capability.UNSUPPORTED, page=preview.Page.ML)
        wire = canvas.wire()
        self.assertNotIn(b"\x1b]66;", wire)
        for source in ml_fixtures.SOURCES.values():
            parts = [source[start:start + 56].encode() for start in range(0, len(source), 56)]
            previous = -1
            for part in parts:
                position = wire.find(part, previous + 1)
                self.assertGreater(position, previous)
                previous = position


if __name__ == "__main__":
    unittest.main()
