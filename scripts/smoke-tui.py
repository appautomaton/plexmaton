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

Mouse reporting is checked here rather than in a unit test for two reasons. Enabling and releasing
it are byte sequences no cell buffer contains, and a leaked capture is the failure that outlives the
process: the user's shell keeps reporting movement with nothing on screen to explain it. The click
itself is sent as a real SGR sequence, so it exercises crossterm's parser rather than a constructed
event.

Promoting this to a `cargo test` target requires choosing a PTY crate, which has not been
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
# SGR extended mouse mode. Crossterm enables several tracking modes; this is the one that decides
# how a report is encoded, so it is the one worth pinning.
MOUSE_ON = b"\x1b[?1006h"
MOUSE_OFF = b"\x1b[?1006l"
# Cells inside the resized Wide layout: the transcript column, and the key-hint strip on the last
# row. The strip is chrome, so a press there must route to nothing and repaint nothing.
CLICK_IN_TRANSCRIPT = (40, 10)
CLICK_IN_FOOTER = (5, RESIZED[0] - 1)
CLICK_SETTLE_SECONDS = 0.8
ANSI = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]")
WHITESPACE = re.compile(r"\s+")


def set_size(fd: int, size: tuple[int, int]) -> None:
    rows, columns = size
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))


def sgr_press(column: int, row: int) -> bytes:
    """One left-button press, in the SGR encoding crossterm asks the terminal for."""
    return f"\x1b[<0;{column + 1};{row + 1}M".encode()


def click(master: int, at: tuple[int, int], sink: bytearray) -> bytes:
    """Sends a press and returns only the bytes the prototype emitted in response."""
    before = len(sink)
    os.write(master, sgr_press(*at))
    drain(master, CLICK_SETTLE_SECONDS, sink)
    return bytes(sink[before:])


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

        # The timeline is drained by now and the projection is static, so any repaint from here on
        # was caused by the click and nothing else.
        on_chrome = click(master, CLICK_IN_FOOTER, captured)
        on_transcript = click(master, CLICK_IN_TRANSCRIPT, captured)

        os.write(master, b"\x03")  # Ctrl-C, the only quit (INV-7)
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

    if MOUSE_ON not in captured:
        print("smoke: mouse reporting was never enabled", file=sys.stderr)
        failures.append("mouse on")
    if on_chrome:
        print(
            "smoke: a press on the key-hint strip repainted; chrome must route to nothing",
            file=sys.stderr,
        )
        failures.append("chrome click")
    if not on_transcript:
        print("smoke: a press on the transcript did not reach the workspace", file=sys.stderr)
        failures.append("transcript click")

    if ALTERNATE_SCREEN_EXIT not in captured:
        print("smoke: alternate screen was not restored", file=sys.stderr)
        failures.append("alternate screen")
    elif MOUSE_OFF not in captured:
        print("smoke: mouse reporting was left on for the user's shell", file=sys.stderr)
        failures.append("mouse off")
    elif captured.index(MOUSE_OFF) > captured.index(ALTERNATE_SCREEN_EXIT):
        # Releasing capture after handing the screen back means the modes are switched off on the
        # terminal the user is looking at, which is the thing this ordering exists to prevent.
        print("smoke: mouse reporting was released after the alternate screen", file=sys.stderr)
        failures.append("mouse off ordering")

    if exit_code != 0:
        print(f"smoke: prototype exited with {exit_code}", file=sys.stderr)
        failures.append("exit code")

    if failures:
        return 1

    print(
        f"smoke: painted the canonical timeline at {INITIAL_SIZE[0]}x{INITIAL_SIZE[1]}, "
        f"repainted on resize to {RESIZED[0]}x{RESIZED[1]}, routed an SGR click to the "
        "transcript and none to the hint strip, accepted the quit key, and released mouse "
        "reporting before the alternate screen"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
