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


def main():
    root = Path(__file__).resolve().parent.parent
    spec = importlib.util.spec_from_file_location("terminal_smoke", root / "scripts/smoke-tui.py")
    smoke = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(smoke)
    subprocess.run(["cargo", "build", "-p", "plexmaton-cli", "--bin", "plexmaton", "--quiet"], cwd=root, check=True)
    with tempfile.TemporaryDirectory(prefix="plexmaton-status-smoke-", dir="/tmp") as folder:
        home = Path(folder)
        command = "bash " + shlex.quote(str(root / "examples/statusline-pastel.sh"))
        # JSON quoting is valid for this TOML basic string too.
        import json
        (home / "config.toml").write_text(f'''active_model = {{ provider = "fixture", model = "luna" }}
[status_line]
command = {json.dumps(command)}
timeout_ms = 5000
max_rows = 6
[providers.fixture]
base_url = "http://127.0.0.1:9/v1"
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
        env = dict(os.environ, PLEXMATON_HOME=str(home), PLEXMATON_STATUS_FIXTURE_KEY="fixture-only")
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
            smoke.drain(master, 1.0, capture)
            for width in [120, 95, 60]:
                # A second resize forces a complete frame after the script's debounced update.
                smoke.set_size(master, (30, width + 1))
                smoke.drain(master, 0.7, capture)
                start = len(capture)
                smoke.set_size(master, (30, width))
                smoke.drain(master, 0.7, capture)
                screen = smoke.rendered_screen(bytes(capture[start:]), (30, width))
                assert "FixtureLuna" in screen, screen
                assert "ctx" not in screen and "272.0k" not in screen and "" not in screen, screen
                assert "null" not in screen and "status line:" not in screen, screen
                assert "plexmaton-status-smoke-" in screen, screen
            assert re.search(rb"\x1b\[[0-9;:]*48[;:](2|5)[;:]", capture), "pastel backgrounds never reached the terminal"
            # Resizing below the usable layout must not poison status-command replacement.
            start = len(capture)
            smoke.set_size(master, (8, 40))
            smoke.drain(master, 0.7, capture)
            screen = smoke.rendered_screen(bytes(capture[start:]), (8, 40))
            assert "Terminal too small" in screen, screen
            for size in [(30, 60), (8, 40), (30, 95), (8, 40), (30, 61)]:
                smoke.set_size(master, size)
                smoke.drain(master, 0.04, capture)
            smoke.drain(master, 0.7, capture)
            start = len(capture)
            smoke.set_size(master, (30, 60))
            smoke.drain(master, 0.7, capture)
            screen = smoke.rendered_screen(bytes(capture[start:]), (30, 60))
            assert "FixtureLuna" in screen and "status line:" not in screen, screen
            os.write(master, b"\x04")
            smoke.drain(master, 0.15, capture)
            screen = smoke.rendered_screen(bytes(capture), (30, 60)).splitlines()
            assert "press Ctrl-D again to quit" in screen[-1], screen
            assert any("FixtureLuna" in row for row in screen[:-1]), screen
            os.write(master, b"\x04")
            smoke.drain(master, 0.3, capture)
            assert process.wait(timeout=3) == 0
            assert smoke.ALTERNATE_SCREEN_EXIT in capture
            sessions = list((home / "sessions").glob("*.jsonl"))
            assert len(sessions) == 1
            records = [json.loads(line) for line in sessions[0].read_text().splitlines()]
            assert all(record.get("kind") != "request_attempt_authorized" for record in records)
        finally:
            capture_dir = root / "target/smoke"
            capture_dir.mkdir(parents=True, exist_ok=True)
            (capture_dir / "statusline-session.raw").write_bytes(capture)
            if process.poll() is None:
                process.kill()
                process.wait(timeout=3)
            os.close(master)
    print("status-line smoke: config, snapshot, script, three widths, undersize recovery, last-row quit and shutdown passed; no model request")


if __name__ == "__main__":
    main()
