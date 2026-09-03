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
  `1` rather than `Agents · !1`, and unchanged spaces arrive as cursor moves rather than
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
import json
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
# Long enough for the live runtime to announce its idle primary agent; no request reaches a network.
STREAM_SECONDS = 0.8
REPAINT_SECONDS = 1.5
SHUTDOWN_SECONDS = 3.0
EXPECTED_ON_FULL_FRAME = (
    "Plexmaton · idle",
    "Message Plexmaton",
    "Agents",
)
ALTERNATE_SCREEN_EXIT = b"\x1b[?1049l"
# SGR extended mouse mode. Crossterm enables several tracking modes; this is the one that decides
# how a report is encoded, so it is the one worth pinning.
MOUSE_ON = b"\x1b[?1006h"
MOUSE_OFF = b"\x1b[?1006l"
# Cells inside the resized Wide layout: the transcript column, and the status line on the last
# row. The status line is chrome, so a press there must route to nothing and repaint nothing.
CLICK_IN_TRANSCRIPT = (40, 10)
CLICK_IN_STATUS = (5, RESIZED[0] - 1)
CLICK_IN_COMPOSER = (50, INITIAL_SIZE[0] - 3)
CLICK_IN_RESIZED_COMPOSER = (50, RESIZED[0] - 3)
CLICK_IN_ATTENTION = (10, 1)
APPROVAL_PROBE_SIZE = (41, 121)
CLICK_SETTLE_SECONDS = 0.8
LIVE_RESPONSE_SECONDS = 30.0
EXACT_COMMAND = "grep -qx 'after' task.txt && printf 'SMOKE_COMMAND_OK\\n'"
LIVE_REQUEST = (
    b"Complete this task with exactly one tool call per step. First read task.txt. Use its "
    b"observation to replace the exact text 'before' with 'after'. After the edit succeeds, "
    b"call exec_command with cmd exactly: grep -qx 'after' task.txt && printf "
    b"'SMOKE_COMMAND_OK\\n', and timeout_ms 30000. Only after that command exits zero, reply "
    b"exactly SMOKE_MODEL_DONE SMOKE_COMMAND_OK.\r"
)
LIVE_ANSWER = "SMOKE_MODEL_DONE"
LIVE_TOOL_MARKERS = ("[+]read_file", "[+]edit_file", "[+]exec_command")
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


def drain_until_quiet(master: int, sink: bytearray) -> None:
    """Drains the event burst until the PTY has been quiet long enough to be actionable."""
    deadline = time.monotonic() + 2.0
    quiet_since = time.monotonic()
    while time.monotonic() < deadline:
        ready, _, _ = select.select([master], [], [], 0.05)
        if not ready:
            if time.monotonic() - quiet_since >= 0.2:
                return
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            return
        if not chunk:
            return
        sink.extend(chunk)
        quiet_since = time.monotonic()


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


def approval_card(screen: str) -> str:
    """Crops the registered approval rectangle from a full INITIAL_SIZE frame."""
    rows, columns = INITIAL_SIZE
    width = min(columns - 4, 72)
    height = min(rows - 1, 15)
    left = (columns - width) // 2
    top = (rows - height) // 2
    lines = screen.splitlines()
    return "\n".join(line[left : left + width] for line in lines[top : top + height])


def frame_is_settled(raw: bytes, live: bool) -> bool:
    """Recognizes one full repaint after the live agent has returned to idle."""
    painted = collapsed(rendered_screen(raw, RESIZED).encode())
    expected = EXPECTED_ON_FULL_FRAME + (LIVE_TOOL_MARKERS if live else ())
    return all(collapsed(text.encode()) in painted for text in expected)


def full_repaint(master: int, sink: bytearray) -> str:
    """Forces and returns one complete frame at the live interaction size."""
    set_size(master, APPROVAL_PROBE_SIZE)
    drain_until_quiet(master, sink)
    start = len(sink)
    set_size(master, INITIAL_SIZE)
    drain_until_quiet(master, sink)
    return rendered_screen(bytes(sink[start:]), INITIAL_SIZE)


def wait_for_frame(master: int, sink: bytearray, markers: tuple[str, ...]) -> str | None:
    """Waits until one complete repaint contains every expected semantic marker."""
    deadline = time.monotonic() + LIVE_RESPONSE_SECONDS
    while time.monotonic() < deadline:
        painted = full_repaint(master, sink)
        normalized = collapsed(painted.encode())
        if all(collapsed(marker.encode()) in normalized for marker in markers):
            return painted
        drain(master, 0.2, sink)
    return None


def main() -> int:
    live = sys.argv[1:] == ["--live"]
    if sys.argv[1:] not in ([], ["--live"]):
        print("usage: smoke-tui.py [--live]", file=sys.stderr)
        return 2
    root = Path(__file__).resolve().parent.parent
    capture_dir = root / "target" / "smoke"
    capture_dir.mkdir(parents=True, exist_ok=True)
    child_env = os.environ.copy()
    child_cwd = root
    live_directory = None
    if live:
        if "PLEXMATON_HOME" not in child_env:
            print("smoke: --live requires PLEXMATON_HOME", file=sys.stderr)
            return 2
        # Preserve the caller's config resolution before changing the child's workspace. A
        # relative PLEXMATON_HOME is relative to the invoking shell, not to the tool sandbox.
        profile_root = Path(child_env["PLEXMATON_HOME"]).expanduser().resolve()
        child_env["PLEXMATON_HOME"] = str(profile_root)
        # Exclusive creation prevents a stale file or symlink from turning the smoke's write into
        # an effect outside its disposable workspace.
        # Keep the canonical root short enough that the approval card can show it in full; an
        # authority string clipped by the terminal cannot be meaningfully approved or asserted.
        live_directory = tempfile.TemporaryDirectory(
            prefix="plexmaton-smoke-", dir="/tmp"
        )
        live_workspace = Path(live_directory.name)
        (live_workspace / "task.txt").write_text("before\n", encoding="utf-8")
        child_cwd = live_workspace
    else:
        config_root = capture_dir / "plexmaton-home"
        config_root.mkdir(exist_ok=True)
        (config_root / "config.toml").write_text(
            """active_provider = "smoke"

[providers.smoke]
kind = "openai_compatible"
protocol = "responses"
base_url = "http://127.0.0.1:9/v1"
model = "gpt-5.6-luna"
api_key_env = "PLEXMATON_SMOKE_API_KEY"
reasoning_effort = "none"
""",
            encoding="utf-8",
        )
        child_env["PLEXMATON_HOME"] = str(config_root)
        child_env["PLEXMATON_SMOKE_API_KEY"] = "fixture-only"
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
        cwd=child_cwd,
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

        if live:
            click(master, CLICK_IN_COMPOSER, captured)
            os.write(master, LIVE_REQUEST)
            approvals = (
                (
                    "edit_file",
                    "Access read files, change files",
                    ("edit task.txt (1 exact replacement)",),
                ),
                (
                    "exec_command",
                    "Access read files, change files, run processes",
                    (
                        f"Command {json.dumps(EXACT_COMMAND)}",
                        f"cwd {json.dumps(str(live_workspace.resolve()))}",
                        "timeout 30000 ms",
                    ),
                ),
            )
            for approval, (tool, access, detail_markers) in enumerate(approvals, start=1):
                if wait_for_frame(master, captured, ("Approval required",)) is None:
                    print(
                        f"smoke: live approval {approval} did not reach the TUI",
                        file=sys.stderr,
                    )
                    failures.append(f"live approval {approval}")
                    break
                # Tool lifecycle reaches `Approval required` one event before the Attention item
                # joins the queue. Wait until that event burst is painted, explicitly go to the
                # request, and move off the safe default Deny before deciding (ATT-1, APV-4).
                click(master, CLICK_IN_ATTENTION, captured)
                os.write(master, b"\r")
                drain_until_quiet(master, captured)
                card = collapsed(
                    approval_card(full_repaint(master, captured)).encode()
                )
                expected_identity = (
                    f"Tool   {tool}",
                    access,
                    "Allow once",
                    "> Deny",
                )
                missing_identity = [
                    text
                    for text in expected_identity
                    if collapsed(text.encode()) not in card
                ]
                detail_pages = card
                missing_detail = [
                    text
                    for text in detail_markers
                    if collapsed(text.encode()) not in detail_pages
                ]
                if missing_detail:
                    os.write(master, b"\x1b[6~")  # PgDn reveals bounded approval detail.
                    drain_until_quiet(master, captured)
                    detail_pages += collapsed(
                        approval_card(full_repaint(master, captured)).encode()
                    )
                    missing_detail = [
                        text
                        for text in detail_markers
                        if collapsed(text.encode()) not in detail_pages
                    ]
                missing = missing_identity + missing_detail
                if missing:
                    # The card is still on its safe default. Refuse unexpected authority before
                    # failing the lane; prompt prose is never an approval boundary.
                    os.write(master, b"\x1b")
                    drain_until_quiet(master, captured)
                    print(
                        f"smoke: live approval {approval} did not match {tool}; missing {missing!r}",
                        file=sys.stderr,
                    )
                    failures.append(f"live approval {approval} identity")
                    break
                if tool == "edit_file" and "runprocesses" in card:
                    os.write(master, b"\x1b")
                    drain_until_quiet(master, captured)
                    print("smoke: edit approval requested extra capability", file=sys.stderr)
                    failures.append("edit approval capability")
                    break
                os.write(master, b"k\r")
                if wait_for_frame(master, captured, (f"[+] {tool}",)) is None:
                    print(
                        f"smoke: approved {tool} did not succeed",
                        file=sys.stderr,
                    )
                    failures.append(f"approved {tool}")
                    break
            if not failures and wait_for_frame(
                master, captured, (LIVE_ANSWER,)
            ) is None:
                print(
                    f"smoke: live answer {LIVE_ANSWER!r} did not reach the transcript",
                    file=sys.stderr,
                )
                failures.append(f"live answer {LIVE_ANSWER}")

        # Resizing forces a full repaint, which is the only frame that carries the whole screen.
        full_frame_start = len(captured)
        set_size(master, RESIZED)
        drain(master, REPAINT_SECONDS, captured)
        full_frame = bytes(captured[full_frame_start:])
        settle_seconds = LIVE_RESPONSE_SECONDS if live else 5.0
        repaint_deadline = time.monotonic() + settle_seconds
        while not frame_is_settled(full_frame, live) and time.monotonic() < repaint_deadline:
            # A semantic event can land during a repaint, leaving a full older frame plus an
            # incremental patch. Toggle through another Wide size until one complete settled
            # buffer exists; this applies to startup announcement as well as a streamed answer.
            set_size(master, REPAINT_PROBE_SIZE)
            drain(master, 0.2, captured)
            full_frame_start = len(captured)
            set_size(master, RESIZED)
            drain(master, REPAINT_SECONDS, captured)
            full_frame = bytes(captured[full_frame_start:])

        # Put focus somewhere known before testing the transcript press. A completed approval
        # returns focus to the transcript, where pressing it again is correctly a no-op (FR-1).
        click(master, CLICK_IN_RESIZED_COMPOSER, captured)
        # The idle projection is static, so any repaint from here on was caused by the click.
        on_chrome = click(master, CLICK_IN_STATUS, captured)
        on_transcript = click(master, CLICK_IN_TRANSCRIPT, captured)

        os.write(master, b"\x04\x04")  # Ctrl-D twice, the quit chord (INV-7)
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
    if live:
        for marker in LIVE_TOOL_MARKERS:
            if marker not in painted:
                print(
                    f"smoke: live tool result {marker!r} is absent from the full frame",
                    file=sys.stderr,
                )
                failures.append(marker)
        if "Notices" in painted:
            print(
                "smoke: the real tool turn produced a projection notice",
                file=sys.stderr,
            )
            failures.append("projection notice")
        if (live_workspace / "task.txt").read_text(encoding="utf-8") != "after\n":
            print("smoke: the approved edit did not reach task.txt", file=sys.stderr)
            failures.append("approved edit")

    if MOUSE_ON not in captured:
        print("smoke: mouse reporting was never enabled", file=sys.stderr)
        failures.append("mouse on")
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

    if exit_code != 0:
        print(f"smoke: Plexmaton exited with {exit_code}", file=sys.stderr)
        failures.append("exit code")

    if failures:
        return 1

    print(
        f"smoke: painted the {'answering' if live else 'idle'} live runtime at "
        f"{INITIAL_SIZE[0]}x{INITIAL_SIZE[1]}, "
        f"repainted on resize to {RESIZED[0]}x{RESIZED[1]}, routed an SGR click to the "
        "transcript and none to the status line, accepted the quit chord, and released mouse "
        "reporting before the alternate screen"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
