#!/usr/bin/env python3
"""Drive the runnable prototype through a real pseudo-terminal.

Ratatui's `TestBackend` cannot prove terminal lifecycle: alternate-screen entry and release, raw
mode, resize handling, and the crossterm event stream only exist in front of a real terminal.
This is the smallest check covering that boundary, so phase evidence can cite a command instead
of a recollection.

Two properties of the byte stream shape the assertions:

* The window size is set explicitly with `TIOCSWINSZ`. A pseudo-terminal without one reports 0x0,
  the frame renders no cells, and a content assertion would pass or fail for the wrong reason.
* Ratatui emits only the cells that differ from the previous frame, so an incremental frame shows
  `1` rather than `attention 1`, and unchanged spaces arrive as cursor moves rather than
  characters. The run therefore resizes the terminal to force one full repaint and asserts
  against that frame, ignoring whitespace on both sides.

Promoting this to a `cargo test` target requires choosing a PTY crate, which Phase 00 has not
audited; until then it stays an out-of-band evidence command.
"""

from __future__ import annotations

import fcntl
import os
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time
from pathlib import Path

# Both sizes stay in the Wide layout class so the activity column is present throughout.
INITIAL_SIZE = (40, 120)
RESIZED = (30, 100)
# Long enough for the deterministic timeline to reach agent B's completion at tick 17.
STREAM_SECONDS = 5.0
REPAINT_SECONDS = 1.5
SHUTDOWN_SECONDS = 3.0
EXPECTED_ON_FULL_FRAME = (
    "Agent A · primary",
    "Agent B · UI study",
    "attention 1",
    "Activity",
    "Mail",
    "agent-b",
)
ALTERNATE_SCREEN_EXIT = b"\x1b[?1049l"
ANSI = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]")
WHITESPACE = re.compile(r"\s+")


def set_size(fd: int, size: tuple[int, int]) -> None:
    rows, columns = size
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))


def drain(master: int, seconds: float, sink: bytearray) -> None:
    """Reads available output until the deadline or end of file."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        ready, _, _ = select.select([master], [], [], 0.1)
        if not ready:
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            return
        if not chunk:
            return
        sink.extend(chunk)


def collapsed(raw: bytes) -> str:
    """Strips escape sequences and whitespace so cursor-move gaps do not break comparison."""
    return WHITESPACE.sub("", ANSI.sub("", raw.decode("utf-8", errors="replace")))


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    subprocess.run(
        ["cargo", "build", "-p", "plexmaton-cli", "--quiet"], cwd=root, check=True
    )

    master, slave = pty.openpty()
    set_size(slave, INITIAL_SIZE)
    process = subprocess.Popen(
        [str(root / "target" / "debug" / "plexmaton")],
        stdin=slave,
        stdout=slave,
        stderr=slave,
        cwd=root,
        # Without its own session and controlling terminal the child never receives SIGWINCH,
        # so the resize below would be silently ignored and prove nothing.
        start_new_session=True,
        preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0),
    )
    os.close(slave)

    captured = bytearray()
    failures: list[str] = []
    try:
        drain(master, STREAM_SECONDS, captured)

        # Resizing forces a full repaint, which is the only frame that carries the whole screen.
        full_frame_start = len(captured)
        set_size(master, RESIZED)
        drain(master, REPAINT_SECONDS, captured)
        full_frame = bytes(captured[full_frame_start:])

        os.write(master, b"q")
        drain(master, SHUTDOWN_SECONDS, captured)
        exit_code = process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        print("smoke: prototype did not exit after the quit key", file=sys.stderr)
        return 1
    finally:
        os.close(master)

    capture_dir = root / "target" / "smoke"
    capture_dir.mkdir(parents=True, exist_ok=True)
    (capture_dir / "tui-session.raw").write_bytes(captured)
    (capture_dir / "tui-full-frame.raw").write_bytes(full_frame)

    if not full_frame:
        print("smoke: terminal resize produced no repaint", file=sys.stderr)
        failures.append("repaint")

    painted = collapsed(full_frame)
    for text in EXPECTED_ON_FULL_FRAME:
        if collapsed(text.encode()) not in painted:
            print(f"smoke: repainted frame is missing {text!r}", file=sys.stderr)
            failures.append(text)

    if ALTERNATE_SCREEN_EXIT not in captured:
        print("smoke: alternate screen was not restored", file=sys.stderr)
        failures.append("alternate screen")
    if exit_code != 0:
        print(f"smoke: prototype exited with {exit_code}", file=sys.stderr)
        failures.append("exit code")

    if failures:
        return 1

    print(
        f"smoke: painted the canonical timeline at {INITIAL_SIZE[0]}x{INITIAL_SIZE[1]}, "
        f"repainted on resize to {RESIZED[0]}x{RESIZED[1]}, accepted the quit key, "
        "and restored the alternate screen"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
