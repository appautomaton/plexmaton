#!/usr/bin/env python3
"""Drive the runnable Plexmaton TUI through a real pseudo-terminal.

Ratatui's `TestBackend` cannot prove terminal lifecycle: alternate-screen entry and release, raw
mode, resize handling, and the crossterm event stream only exist in front of a real terminal.
This is the smallest check covering that boundary, so phase evidence can cite a command instead
of a recollection.

Two properties of the byte stream shape the assertions:

* The window size is set explicitly with `TIOCSWINSZ`. A pseudo-terminal without one reports 0x0,
  the frame renders no cells, and a content assertion would pass or fail for the wrong reason.
* Ratatui emits only the cells that differ from the previous frame, so an incremental frame shows
  a fragment rather than a whole title, and unchanged spaces arrive as cursor moves rather than
  characters. The run therefore resizes the terminal to force one full repaint and asserts
  against that frame, ignoring whitespace on both sides.

Mouse and focus reporting are checked here rather than in a unit test: their lifecycle is made of
byte sequences no cell buffer contains, and a leaked mode outlives the process. The click itself is
sent as a real SGR sequence, so it exercises crossterm's parser rather than a constructed event.

Promoting this to a `cargo test` target requires choosing a PTY crate, which has not been
audited; until then it stays an out-of-band evidence command.
"""

from __future__ import annotations

import fcntl
import base64
import os
import pty
import re
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unicodedata
from pathlib import Path

# Every size stays in the Wide layout class so the agent column is present throughout.
INITIAL_SIZE = (40, 120)
RESIZED = (30, 100)
REPAINT_PROBE_SIZE = (31, 101)
# Long enough for the runtime to announce its idle primary agent; no request reaches a network.
STREAM_SECONDS = 0.8
REPAINT_SECONDS = 1.5
SHUTDOWN_SECONDS = 3.0
EXPECTED_ON_FULL_FRAME = (
    "Plexmaton · idle",
    "Message Plexmaton",
    # Not the agent rail: a fresh session has delegated nothing, and a roster of nobody is a
    # bordered box saying so in the column the conversation wanted.
    "~/",
)
ALTERNATE_SCREEN_EXIT = b"\x1b[?1049l"
# SGR extended mouse mode. Crossterm enables several tracking modes; this is the one that decides
# how a report is encoded, so it is the one worth pinning.
MOUSE_ON = b"\x1b[?1006h"
MOUSE_OFF = b"\x1b[?1006l"
FOCUS_ON = b"\x1b[?1004h"
FOCUS_OFF = b"\x1b[?1004l"
# Cells inside the resized Wide layout: the transcript column, and the status line on the last
# row. The status line is chrome, so a press there must route to nothing and repaint nothing.
CLICK_IN_TRANSCRIPT = (40, 10)
CLICK_IN_STATUS = (5, RESIZED[0] - 1)
CLICK_IN_RESIZED_COMPOSER = (50, RESIZED[0] - 3)
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
    """Sends a press and returns only the bytes the TUI emitted in response."""
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


def rendered_screen(raw: bytes, size: tuple[int, int]) -> str:
    """Applies the cursor/control subset Ratatui emits to one blank terminal buffer."""
    rows, columns = size
    cells = [[" "] * columns for _ in range(rows)]
    row = 0
    column = 0
    text = raw.decode("utf-8", errors="replace")
    index = 0
    while index < len(text):
        character = text[index]
        if character == "\x1b" and index + 1 < len(text) and text[index + 1] == "[":
            final = index + 2
            while final < len(text) and not ("@" <= text[final] <= "~"):
                final += 1
            if final >= len(text):
                break
            arguments = text[index + 2 : final]
            command = text[final]
            values = [
                int(value) if value.isdigit() else 0
                for value in arguments.lstrip("?").split(";")
            ]
            first = values[0] if values else 0
            if command in ("H", "f"):
                row = max(1, first) - 1
                column = max(1, values[1] if len(values) > 1 else 1) - 1
            elif command == "A":
                row = max(0, row - max(1, first))
            elif command == "B":
                row = min(rows - 1, row + max(1, first))
            elif command == "C":
                column = min(columns, column + max(1, first))
            elif command == "D":
                column = max(0, column - max(1, first))
            elif command == "G":
                column = max(1, first) - 1
            elif command == "d":
                row = max(1, first) - 1
            elif command == "J" and first == 2:
                cells = [[" "] * columns for _ in range(rows)]
            elif command == "K":
                if first == 1:
                    cells[row][: min(column + 1, columns)] = [" "] * min(
                        column + 1, columns
                    )
                elif first == 2:
                    cells[row] = [" "] * columns
                else:
                    cells[row][min(column, columns) :] = [" "] * max(
                        0, columns - column
                    )
            index = final + 1
            continue
        if character == "\r":
            column = 0
        elif character == "\n":
            row = min(rows - 1, row + 1)
        elif ord(character) >= 32:
            width = 0 if unicodedata.combining(character) else 1
            if unicodedata.east_asian_width(character) in ("W", "F"):
                width = 2
            if row < rows and column < columns:
                cells[row][column] = character
                for continuation in range(1, width):
                    if column + continuation < columns:
                        cells[row][column + continuation] = " "
            column = min(columns, column + width)
        index += 1
    return "\n".join("".join(line) for line in cells)


def frame_is_settled(raw: bytes) -> bool:
    """Recognizes one full repaint after the agent has returned to idle."""
    painted = collapsed(rendered_screen(raw, RESIZED).encode())
    return all(collapsed(text.encode()) in painted for text in EXPECTED_ON_FULL_FRAME)


def check_command_palette(master: int, captured: bytearray) -> list[str]:
    """INV-11, INV-12: aliases open the real configuration page through the composition root."""
    failures = []
    for query in (b"config", b"/config", b"settings", b"/settings"):
        os.write(master, b"\x10" + query)
        drain(master, 0.2, captured)
        set_size(master, REPAINT_PROBE_SIZE)
        drain(master, 0.2, captured)
        start = len(captured)
        set_size(master, RESIZED)
        drain(master, REPAINT_SECONDS, captured)
        screen = collapsed(rendered_screen(bytes(captured[start:]), RESIZED).encode())
        if not all(collapsed(text) in screen for text in (b"Commands", b"> /config")):
            print(f"smoke: command discovery failed for {query!r}: {screen!r}", file=sys.stderr)
            failures.append(f"command discovery: {query.decode()}")
        os.write(master, b"\r")
        drain(master, 0.2, captured)
        set_size(master, REPAINT_PROBE_SIZE)
        drain(master, 0.2, captured)
        start = len(captured)
        set_size(master, RESIZED)
        drain(master, REPAINT_SECONDS, captured)
        screen = collapsed(rendered_screen(bytes(captured[start:]), RESIZED).encode())
        if not all(collapsed(text) in screen for text in (
            b"Configuration", b"Provider", b"smoke", b"gpt-5.6-luna", b"Reasoning effort", b"none", b"Esc back"
        )):
            print(f"smoke: configuration did not open for {query!r}", file=sys.stderr)
            failures.append(f"configuration page: {query.decode()}")
        os.write(master, b"\x1b")
        drain(master, 0.2, captured)
        set_size(master, REPAINT_PROBE_SIZE)
        drain(master, 0.2, captured)
        start = len(captured)
        set_size(master, RESIZED)
        drain(master, REPAINT_SECONDS, captured)
        screen = collapsed(rendered_screen(bytes(captured[start:]), RESIZED).encode())
        if not all(collapsed(text) in screen for text in (b"Commands", query, b"> /config")):
            print(f"smoke: Escape did not restore the palette for {query!r}", file=sys.stderr)
            failures.append(f"configuration back: {query.decode()}")
        os.write(master, b"\x1b")
        drain(master, 0.2, captured)
    for query in (b"resume", b"continue", b"sessions", b"session"):
        os.write(master, b"\x10" + query + b"\r")
        drain(master, 0.2, captured)
        set_size(master, REPAINT_PROBE_SIZE)
        drain(master, 0.2, captured)
        start = len(captured)
        set_size(master, RESIZED)
        drain(master, REPAINT_SECONDS, captured)
        screen = collapsed(rendered_screen(bytes(captured[start:]), RESIZED).encode())
        if not all(collapsed(text) in screen for text in (b"Sessions", b"Enter resume", b"Esc close")):
            failures.append(f"session discovery: {query.decode()}")
        os.write(master, b"\x1b")
        drain(master, 0.2, captured)
    return failures


def check_input_pointer(master: int, captured: bytearray) -> list[str]:
    """COM-1, COM-2: click and drag Chinese input through actual SGR reports, preserving borders."""
    failures = []
    row = RESIZED[0] - 3

    def report(button: int, column: int, release: bool = False) -> None:
        end = "m" if release else "M"
        os.write(master, f"\x1b[<{button};{column + 1};{row + 1}{end}".encode())

    def screen() -> str:
        set_size(master, REPAINT_PROBE_SIZE)
        drain(master, 0.2, captured)
        start = len(captured)
        set_size(master, RESIZED)
        drain(master, REPAINT_SECONDS, captured)
        return rendered_screen(bytes(captured[start:]), RESIZED)

    report(0, 1)
    report(0, 1, True)
    os.write(master, b"\x1b[200~" + "中文abc".encode() + b"\x1b[201~")
    drain(master, 0.2, captured)
    report(0, 3)
    report(0, 3, True)
    os.write(master, b"X")
    drain(master, 0.2, captured)
    painted = screen()
    if "中X文abc" not in collapsed(painted.encode()):
        failures.append("composer click insertion")
    if painted.splitlines()[row][-1] != "│":
        failures.append("composer right border after Chinese input")
    start = len(captured)
    report(0, 1)
    report(32, 6)
    report(0, 6, True)
    drain(master, 0.2, captured)
    if b"]52;c;" + base64.b64encode("中X文".encode()) not in bytes(captured[start:]):
        failures.append("composer drag source copy")
    os.write(master, b"z")
    drain(master, 0.2, captured)
    if "zabc" not in collapsed(screen().encode()):
        failures.append("composer replace selection")
    os.write(master, b"\x03")
    drain(master, 0.2, captured)
    for failure in failures:
        print(f"smoke: {failure}", file=sys.stderr)
    return failures


def main() -> int:
    if sys.argv[1:]:
        print("usage: smoke-tui.py", file=sys.stderr)
        return 2
    root = Path(__file__).resolve().parent.parent
    capture_dir = root / "target" / "smoke"
    capture_dir.mkdir(parents=True, exist_ok=True)
    child_env = os.environ.copy()
    # Exercise terminal delivery without touching the machine's native clipboard.
    child_env["SSH_TTY"] = "/dev/plexmaton-smoke"
    config_directory = tempfile.TemporaryDirectory(
        prefix="plexmaton-smoke-home-", dir="/tmp"
    )
    config_root = Path(config_directory.name)
    (config_root / "config.toml").write_text(
        """active_model = { provider = "smoke", model = "fixture" }

[providers.smoke]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "PLEXMATON_SMOKE_API_KEY"
[providers.smoke.models.fixture]
api = "openai_responses"
id = "gpt-5.6-luna"
reasoning_effort = "none"
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
""",
        encoding="utf-8",
    )
    child_env["PLEXMATON_HOME"] = str(config_root)
    child_env["PLEXMATON_SMOKE_API_KEY"] = "fixture-only"
    default_sessions_root = config_root / "sessions"
    subprocess.run(
        ["cargo", "build", "-p", "plexmaton-cli", "--quiet"], cwd=root, check=True
    )

    master, slave = pty.openpty()
    set_size(slave, INITIAL_SIZE)
    child_command = [str(root / "target" / "debug" / "plexmaton")]
    process = subprocess.Popen(
        child_command,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        cwd=root,
        env=child_env,
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
        repaint_deadline = time.monotonic() + 5.0
        while not frame_is_settled(full_frame) and time.monotonic() < repaint_deadline:
            # A semantic event can land during a repaint, leaving a full older frame plus an
            # incremental patch. Toggle through another Wide size until one complete settled
            # buffer exists; this applies to the startup announcement as well.
            set_size(master, REPAINT_PROBE_SIZE)
            drain(master, 0.2, captured)
            full_frame_start = len(captured)
            set_size(master, RESIZED)
            drain(master, REPAINT_SECONDS, captured)
            full_frame = bytes(captured[full_frame_start:])

        # Put focus somewhere known before testing the transcript press.
        click(master, CLICK_IN_RESIZED_COMPOSER, captured)
        # The idle projection is static, so any repaint from here on was caused by the click.
        on_chrome = click(master, CLICK_IN_STATUS, captured)
        on_transcript = click(master, CLICK_IN_TRANSCRIPT, captured)

        # Finish the pointer probe before opening a keyboard surface: an active capture consumes
        # the first Escape (INV-5, INV-6), so a held press is not a completed click.
        column, row = CLICK_IN_TRANSCRIPT
        os.write(master, f"\x1b[<0;{column + 1};{row + 1}m".encode())
        drain(master, 0.2, captured)
        failures.extend(check_command_palette(master, captured))
        failures.extend(check_input_pointer(master, captured))

        # The first question expires without another input. A later Ctrl-D must re-arm rather than
        # confirm the stale question; the immediately following press then confirms the new one.
        os.write(master, b"\x04")
        drain(master, 1.2, captured)
        set_size(master, REPAINT_PROBE_SIZE)
        drain(master, 0.2, captured)
        expired_frame_start = len(captured)
        set_size(master, RESIZED)
        drain(master, REPAINT_SECONDS, captured)
        expired_frame = bytes(captured[expired_frame_start:])
        expired_screen = collapsed(rendered_screen(expired_frame, RESIZED).encode())
        if collapsed(EXPECTED_ON_FULL_FRAME[0].encode()) not in expired_screen:
            print(
                "smoke: the quit-deadline probe did not produce a complete frame",
                file=sys.stderr,
            )
            failures.append("quit deadline frame")
        elif collapsed(b"press Ctrl-D again to quit") in expired_screen:
            print(
                "smoke: the quit question remained visible after its deadline",
                file=sys.stderr,
            )
            failures.append("quit deadline repaint")
        os.write(master, b"\x04")
        drain(master, 0.2, captured)
        if process.poll() is not None:
            failures.append("expired quit chord")
        else:
            os.write(master, b"\x04")
        drain(master, SHUTDOWN_SECONDS, captured)
        exit_code = process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        print("smoke: Plexmaton did not exit after the quit key", file=sys.stderr)
        return 1
    finally:
        os.close(master)

    (capture_dir / "tui-session.raw").write_bytes(captured)
    (capture_dir / "tui-full-frame.raw").write_bytes(full_frame)

    if not full_frame:
        print("smoke: terminal resize produced no repaint", file=sys.stderr)
        failures.append("repaint")

    painted = collapsed(rendered_screen(full_frame, RESIZED).encode())
    for text in EXPECTED_ON_FULL_FRAME:
        if collapsed(text.encode()) not in painted:
            print(f"smoke: repainted frame is missing {text!r}", file=sys.stderr)
            failures.append(text)
    if MOUSE_ON not in captured:
        print("smoke: mouse reporting was never enabled", file=sys.stderr)
        failures.append("mouse on")
    if FOCUS_ON not in captured:
        print("smoke: focus reporting was never enabled", file=sys.stderr)
        failures.append("focus on")
    if b"\x1b[?2004h" not in captured or b"\x1b[?2004l" not in captured:
        failures.append("bracketed paste lifecycle")
    elif ALTERNATE_SCREEN_EXIT in captured and captured.index(b"\x1b[?2004l") > captured.index(ALTERNATE_SCREEN_EXIT):
        failures.append("bracketed paste release ordering")
    if on_chrome:
        print(
            "smoke: a press on the status line repainted; chrome must route to nothing",
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
    if FOCUS_OFF not in captured:
        print("smoke: focus reporting was left on for the user's shell", file=sys.stderr)
        failures.append("focus off")
    elif ALTERNATE_SCREEN_EXIT in captured and captured.index(FOCUS_OFF) > captured.index(
        ALTERNATE_SCREEN_EXIT
    ):
        print("smoke: focus reporting was released after the alternate screen", file=sys.stderr)
        failures.append("focus off ordering")

    if exit_code != 0:
        print(f"smoke: Plexmaton exited with {exit_code}", file=sys.stderr)
        failures.append("exit code")

    created_sessions = set(default_sessions_root.glob("*.jsonl"))
    if created_sessions:
        print(
            f"smoke: blank launch created {len(created_sessions)} sessions, expected none",
            file=sys.stderr,
        )
        failures.append("lazy automatic session")
    if b"Session saved:" in captured or b"Session ID:" in captured:
        failures.append("blank launch reported a nonexistent session")

    if failures:
        return 1

    print(
        f"smoke: painted the idle live runtime at {INITIAL_SIZE[0]}x{INITIAL_SIZE[1]}, "
        f"repainted on resize to {RESIZED[0]}x{RESIZED[1]}, routed an SGR click to the "
        "transcript and none to the status line, expired and re-armed the quit chord, and released "
        "mouse and focus reporting before the alternate screen, without creating an empty JSONL"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
