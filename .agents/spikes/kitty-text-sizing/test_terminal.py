"""Real-PTY lifecycle coverage with scripted terminal replies, never a model endpoint."""
from contextlib import contextmanager
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import pty
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import unittest

import preview

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("math_smoke_support", ROOT / "scripts/smoke_support.py")
support = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(support)


def settings(attributes):
    # macOS sets PENDIN when canonical input is restored, even in a plain setraw/tcsetattr
    # round trip. Its SDK labels this kernel-owned pending-input state, not a mode setting.
    result = attributes.copy()
    result[3] &= ~getattr(termios, "PENDIN", 0)
    return result


@contextmanager
def child_preview(response, seconds=5):
    with tempfile.TemporaryDirectory(prefix="plexmaton-kitty-test-") as directory:
        master, slave = pty.openpty()
        child = None
        original = termios.tcgetattr(slave)
        received = bytearray()
        report = Path(directory) / "lifecycle.jsonl"
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
            child = subprocess.Popen(
                [sys.executable, str(Path(preview.__file__)), "--seconds", str(seconds), "--report", str(report)],
                stdin=slave, stdout=slave, stderr=slave, close_fds=True,
                env={"PATH": os.defpath, "TERM": "xterm-256color", "LANG": "en_US.UTF-8"},
            )
            support.read_until(master, received, lambda: preview.PROBE in received)
            if response is not None:
                os.write(master, response)
            yield child, master, slave, original, received, report
        finally:
            if child is not None:
                if child.poll() is None:
                    child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=3)
            os.close(master)
            os.close(slave)


class TerminalLifecycleTests(unittest.TestCase):
    def test_probe_resize_replacement_redraw_and_quit_restore_terminal(self):
        response = b"\x1b[3;3R\x1b[3;5R\x1b[3;7R"
        with child_preview(response) as (child, master, slave, original, received, report):
            support.read_until(master, received, lambda: b";total\x07" in received)
            for columns in [88, 60]:
                fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, columns, 0, 0))
                marker = f"{columns}x36".encode()
                support.read_until(master, received, lambda: marker in received)
            os.write(master, b" ")
            support.read_until(master, received, lambda: b";p\x07" in received)
            os.write(master, b"r")
            support.read_until(master, received, lambda: b"frame 5" in received)
            os.write(master, b"q")
            support.read_until(master, received, lambda: preview.LEAVE in received)
            self.assertEqual(child.wait(timeout=3), 0)
            self.assertEqual(settings(termios.tcgetattr(slave)), settings(original))
            events = [json.loads(line) for line in report.read_text().splitlines()]
            self.assertEqual(events[0]["capability"], "SCALED")
            self.assertEqual([e["columns"] for e in events if e["event"] == "frame"], [120, 88, 60, 60, 60])
            self.assertEqual(events[-1], {"event": "closed", "frames": 5})

    def test_ignored_protocol_shows_source_and_emits_no_scaled_fixture(self):
        response = b"\x1b[3;3R" * 3
        with child_preview(response) as (child, master, slave, original, received, _report):
            support.read_until(master, received, lambda: b"Exact source:" in received)
            os.write(master, b"q")
            support.read_until(master, received, lambda: preview.LEAVE in received)
            self.assertEqual(child.wait(timeout=3), 0)
            payload = bytes(received).split(preview.PROBE, 1)[1]
            self.assertNotIn(b"\x1b]66;", payload)
            self.assertEqual(settings(termios.tcgetattr(slave)), settings(original))

    def test_sigterm_restores_screen_and_raw_mode(self):
        response = b"\x1b[3;3R\x1b[3;5R\x1b[3;7R"
        with child_preview(response) as (child, master, slave, original, received, report):
            support.read_until(master, received, lambda: b";total\x07" in received)
            child.send_signal(signal.SIGTERM)
            support.read_until(master, received, lambda: preview.LEAVE in received)
            self.assertEqual(child.wait(timeout=3), 0)
            self.assertEqual(settings(termios.tcgetattr(slave)), settings(original))
            self.assertIn('"event": "interrupted"', report.read_text())

    def test_missing_probe_reply_expires_and_restores_terminal(self):
        with child_preview(None, seconds=1) as (child, master, slave, original, received, report):
            support.read_until(master, received, lambda: preview.LEAVE in received)
            self.assertEqual(child.wait(timeout=3), 0)
            self.assertEqual(settings(termios.tcgetattr(slave)), settings(original))
            self.assertIn('"capability": "UNVERIFIED"', report.read_text())


if __name__ == "__main__":
    unittest.main()
