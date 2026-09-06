#!/usr/bin/env python3
"""STL-2–4: configured shell footer in the real TUI; no request is submitted to a model."""

import fcntl
import importlib.util
import os
from pathlib import Path
import pty
import re
import shlex
import subprocess
import tempfile
import termios
from smoke_support import NoModelRequests, fixture_environment


def run_smoke(model_url):
    root = Path(__file__).resolve().parent.parent
    spec = importlib.util.spec_from_file_location("terminal_smoke", root / "scripts/smoke-tui.py")
    smoke = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(smoke)
    subprocess.run(["cargo", "build", "-p", "plexmaton-cli", "--bin", "plexmaton", "--quiet"], cwd=root, check=True)
    with tempfile.TemporaryDirectory(prefix="plexmaton-status-smoke-", dir="/tmp") as folder:
        home = Path(folder)
        # Width in the model label is a fixture marker, not a product field or a sleep heuristic.
        transform = '.model.display_name = (.model.display_name + ":" + (.plexmaton.terminal.columns | tostring))'
        command = "jq " + shlex.quote(transform) + " | bash " + shlex.quote(str(root / "examples/statusline-pastel.sh"))
        # JSON quoting is valid for this TOML basic string too.
        import json
        (home / "config.toml").write_text(f'''active_model = {{ provider = "fixture", model = "luna" }}
[status_line]
command = {json.dumps(command)}
timeout_ms = 5000
max_rows = 6
[providers.fixture]
base_url = "{model_url}"
api_key_env = "PLEXMATON_STATUS_FIXTURE_KEY"
api = "openai_responses"
[providers.fixture.models.luna]
id = "fixture-only"
display_name = "FixtureLuna"
reasoning_effort = "high"
context_window_tokens = 272000
max_output_tokens = 8192
output_reserve_tokens = 8192
''')
        env = dict(fixture_environment(), PLEXMATON_HOME=str(home), PLEXMATON_STATUS_FIXTURE_KEY="fixture-only")
        # The fixture models a truecolor terminal even when the test harness requests plain logs.
        env.pop("NO_COLOR", None)
        env.update(TERM="xterm-256color", COLORTERM="truecolor")
        master, slave = pty.openpty()
        smoke.set_size(slave, (30, 120))
        process = subprocess.Popen([str(root / "target/debug/plexmaton")], cwd=home, env=env,
                                   stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
                                   preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0))
        os.close(slave)
        capture = bytearray()
        try:
            def resize(size, published=False):
                start = len(capture)
                smoke.set_size(master, size)
                small = size[0] < 12 or size[1] < 48
                markers = ("Terminal too small",) if small else ("Plexmaton", "Message Plexmaton")
                if published:
                    markers += (f"FixtureLuna:{size[1]}",)
                screen = smoke.await_screen(master, capture, size, markers,
                                            ("status line:", "null"), start, complete=not small)
                return screen, start

            smoke.await_screen(master, capture, (30, 120), ("FixtureLuna:120", "Message Plexmaton"))
            for width in [120, 95, 60]:
                resize((30, width + 1), published=True)
                screen, _ = resize((30, width), published=True)
                assert "ctx" not in screen and "272.0k" not in screen and "" not in screen, screen
                assert "plexmaton-status-smoke-" in screen, screen
            assert re.search(rb"\x1b\[[0-9;:]*48[;:](2|5)[;:]", capture), "pastel backgrounds never reached the terminal"
            resize((8, 40))
            # Each layout must be observed, while command replacement remains free to overlap.
            for size in [(30, 60), (8, 40), (30, 95), (8, 40), (30, 61)]:
                resize(size)
            # A width-specific value proves the NEW script result was published after churn.
            smoke.await_screen(master, capture, (30, 61), ("FixtureLuna:61",), ("status line:",))
            _, frame_start = resize((30, 60), published=True)
            os.write(master, b"\x04")
            screen = smoke.await_screen(master, capture, (30, 60),
                                        ("press Ctrl-D again to quit", "FixtureLuna:60"), start=frame_start).splitlines()
            assert "press Ctrl-D again to quit" in screen[-1], screen
            assert any("FixtureLuna:60" in row for row in screen[:-1]), screen
            os.write(master, b"\x04\x04")
            smoke.read_until(master, capture, lambda: smoke.ALTERNATE_SCREEN_EXIT in capture,
                             description="status-line terminal release")
            assert process.wait(timeout=3) == 0
            smoke.read_to_eof(master, capture)
            sessions = list((home / "sessions").glob("*.jsonl"))
            assert not sessions, f"blank status-line launch created {sessions!r}"
            assert b"To continue this conversation, run:" not in capture
        finally:
            capture_dir = root / "target/smoke"
            capture_dir.mkdir(parents=True, exist_ok=True)
            (capture_dir / "statusline-session.raw").write_bytes(capture)
            if process.poll() is None:
                process.kill()
                process.wait(timeout=3)
            os.close(master)


def main():
    with NoModelRequests() as model:
        run_smoke(model.base_url)
    print("status-line smoke: config, snapshot, script, three widths, undersize recovery, last-row quit and shutdown passed; no model request")


if __name__ == "__main__":
    main()
