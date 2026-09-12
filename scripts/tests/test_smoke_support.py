import importlib.util
from pathlib import Path
import socket
import select
import os
import threading
import sys
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("smoke_support", Path(__file__).resolve().parents[1] / "smoke_support.py")
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)

terminal_spec = importlib.util.spec_from_file_location("terminal_smoke", Path(__file__).resolve().parents[1] / "smoke-tui.py")
terminal = importlib.util.module_from_spec(terminal_spec)
with patch.dict(sys.modules, smoke_support=support):
    terminal_spec.loader.exec_module(terminal)


class SmokeBoundaryTests(unittest.TestCase):
    def test_frame_boundary_requires_a_complete_draw_and_cursor_sequence(self):
        complete = b"caption\x1b[0m\x1b[3;4H\x1b[?25h"
        self.assertIsNotNone(terminal.FRAME_END.search(complete))
        self.assertIsNone(terminal.FRAME_END.search(complete[:-1]))
        self.assertIsNone(terminal.FRAME_END.search(b"caption\x1b[0"))

    def test_palette_caption_cannot_stand_in_for_the_complete_filter(self):
        for query, expected in (("con", False), ("config", True)):
            # The composer's bottom rule, a whole row of "─", is the frame-complete signal.
            raw = f"\x1b[1;1HCommands\x1b[2;1H{query}\x1b[3;1H> /config\x1b[4;1H{'─' * 40}\x1b[0m".encode()
            with patch.object(terminal, "read_until") as read:
                terminal.await_screen(-1, bytearray(raw), (4, 40), ("Commands", "/config"), exact_lines=("config",))
                self.assertEqual(read.call_args.args[2](), expected)

    def test_ready_state_needs_no_read_or_delay(self):
        support.read_until(-1, bytearray(b"ready"), lambda: True)

    def test_chunked_output_waits_for_the_complete_predicate(self):
        read, write = os.pipe()
        first_observed = threading.Event()
        received = bytearray()
        def producer():
            os.write(write, b"rea")
            if first_observed.wait(5):
                os.write(write, b"dy")
        worker = threading.Thread(target=producer)
        worker.start()
        try:
            def ready():
                if received == b"rea":
                    first_observed.set()
                return received == b"ready"
            support.read_until(read, received, ready, timeout=5)
            self.assertTrue(first_observed.is_set())
            self.assertEqual(received, b"ready")
        finally:
            first_observed.set()
            worker.join(timeout=5)
            os.close(read)
            os.close(write)
        self.assertFalse(worker.is_alive())

    def test_quiet_stream_cannot_satisfy_a_missing_predicate(self):
        read, write = os.pipe()
        try:
            with self.assertRaisesRegex(TimeoutError, "missing state"):
                support.read_until(read, bytearray(), lambda: False, timeout=0.01, description="missing state")
        finally:
            os.close(read)
            os.close(write)

    def test_eof_before_the_requested_state_is_a_failure(self):
        read, write = os.pipe()
        os.close(write)
        try:
            with self.assertRaises(EOFError):
                support.read_until(read, bytearray(), lambda: False)
        finally:
            os.close(read)

    def test_capture_bound_refuses_unbounded_terminal_output(self):
        read, write = os.pipe()
        try:
            os.write(write, b"too much")
            with self.assertRaisesRegex(AssertionError, "byte bound"):
                support.read_until(read, bytearray(), lambda: False, limit=2)
        finally:
            os.close(read)
            os.close(write)

    def test_unused_model_endpoint_closes_its_socket(self):
        with support.NoModelRequests() as fixture:
            self.assertTrue(fixture.base_url.startswith("http://127.0.0.1:"))
        self.assertEqual(fixture.listener.fileno(), -1)

    def test_model_connection_is_a_failure_without_reading_request_content(self):
        with self.assertRaisesRegex(AssertionError, "model connection"):
            with support.NoModelRequests() as fixture:
                with socket.create_connection(fixture.listener.getsockname(), timeout=1) as client:
                    client.sendall(b"GET /v1 HTTP/1.1\r\nHost: fixture\r\n\r\n")
                    # Client connect/send can finish before the listener becomes readable.
                    # Establish arrival without accepting or reading request content.
                    self.assertEqual(select.select([fixture.listener], [], [], 1)[0], [fixture.listener])
        self.assertEqual(fixture.listener.fileno(), -1)

    def test_failure_still_closes_the_endpoint(self):
        with self.assertRaisesRegex(ValueError, "original failure"):
            with support.NoModelRequests() as fixture:
                raise ValueError("original failure")
        self.assertEqual(fixture.listener.fileno(), -1)

    def test_smoke_environment_excludes_credentials_and_live_multiplexers(self):
        with patch.dict("os.environ", {"PRIVATE_MODEL_TOKEN": "secret", "TMUX": "live", "STY": "live", "HTTP_PROXY": "remote", "PATH": "/bin"}):
            env = support.fixture_environment()
        for key in ("PRIVATE_MODEL_TOKEN", "TMUX", "STY", "HTTP_PROXY"):
            self.assertNotIn(key, env)
        self.assertEqual(env["PATH"], "/bin")
        self.assertEqual(env["SSH_TTY"], "/dev/plexmaton-smoke")


if __name__ == "__main__":
    unittest.main()
