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
import subprocess
import sys
import tempfile
import termios
from pathlib import Path
from smoke_support import (ALTERNATE_SCREEN_EXIT, await_screen, click, collapsed, composer_title,
                           fixture_environment, NoModelRequests, read_to_eof, read_until,
                           rendered_screen, set_size)

COMPOSER = composer_title("gpt-5.6-luna")

# A blank single-agent session has no rail; these sizes exercise its owned terminal geometry.
INITIAL_SIZE = (40, 120)
RESIZED = (30, 100)
REPAINT_PROBE_SIZE = (31, 101)
EXPECTED_ON_FULL_FRAME = (
    # The composer's top rule names the addressee; the conversation above it has no title.
    COMPOSER,
    # Not the agent rail: a fresh session has delegated nothing, and a roster of nobody is a
    # bordered box saying so in the column the conversation wanted.
    "~/",
)
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


def repaint(master, captured, markers=(), absent=(), exact_lines=()):
    for size in (REPAINT_PROBE_SIZE, RESIZED):
        start = len(captured)
        set_size(master, size)
        screen = await_screen(master, captured, size, markers, absent, start, exact_lines=exact_lines)
    return screen, bytes(captured[start:])


def check_drawer(master: int, captured: bytearray) -> None:
    """DRW-1/DRW-3: the chord pulls the Drawer open in a real terminal, and Escape returns it."""
    os.write(master, b"\x10")
    repaint(master, captured, ("Workspace", "> Configuration", "Permissions", "Esc close"))
    os.write(master, b"\x1b")
    repaint(master, captured, (COMPOSER,), ("Type to filter", "Esc close"))


def check_input_pointer(master: int, captured: bytearray) -> None:
    """COM-1/COM-2: real SGR placement, source copy and replacement over Chinese input."""
    row = RESIZED[0] - 3
    def report(button: int, column: int, release: bool = False) -> None:
        end = "m" if release else "M"
        os.write(master, f"\x1b[<{button};{column + 1};{row + 1}{end}".encode())

    report(0, 1)
    report(0, 1, True)
    os.write(master, b"\x1b[200~" + "中文abc".encode() + b"\x1b[201~")
    repaint(master, captured, ("中文abc",))
    report(0, 3)
    report(0, 3, True)
    os.write(master, b"X")
    painted, _ = repaint(master, captured, ("中X文abc",))
    bottom_rule = painted.splitlines()[row + 1]
    assert set(bottom_rule.strip()) == {"─"}, "composer bottom rule intact after Chinese input"
    start = len(captured)
    report(0, 1)
    report(32, 6)
    report(0, 6, True)
    expected = b"]52;c;" + base64.b64encode("中X文".encode())
    read_until(master, captured, lambda: expected in bytes(captured[start:]),
               description="composer source copy")
    # SEL-5: the actual event loop publishes a send receipt after the observed OSC write.
    repaint(master, captured, ("中X文abc", "Copy sent"))
    os.write(master, b"z")
    repaint(master, captured, ("zabc",))
    os.write(master, b"\x03")
    repaint(master, captured, (COMPOSER,), ("zabc",))


def check_newline(master: int, captured: bytearray) -> None:
    """COM-3: raw Ctrl-J creates two draft lines without a model request."""
    os.write(master, b"first\x0asecond")
    repaint(master, captured, ("first", "second"), exact_lines=("first", "second"))
    os.write(master, b"\x03")
    repaint(master, captured, (COMPOSER,), ("first", "second"))


def check_waiting_input(master: int, captured: bytearray) -> None:
    """IQU-4: with nothing waiting, `Alt-\u2191` disturbs neither the band nor the draft."""
    os.write(master, b"a draft")
    repaint(master, captured, ("a draft",), ("Waiting to send",))
    os.write(master, b"\x1b[1;3A")
    repaint(master, captured, ("a draft",), ("Waiting to send",), exact_lines=("a draft",))
    os.write(master, b"\x03")
    repaint(master, captured, (COMPOSER,), ("a draft",))


def check_model_menu(master: int, captured: bytearray) -> None:
    """MDL-2: opening, filtering and dismissing remain a blank conversation."""
    os.write(master, b"/model ")
    repaint(master, captured, ("Models", "current", "Enter confirm"))
    os.write(master, b"no-match")
    repaint(master, captured, ("No matching models.", "/model no-match"))
    os.write(master, b"\x1b")
    await_screen(master, captured, RESIZED, ("/model no-match",), ("Enter confirm",))
    repaint(master, captured, ("/model no-match",), ("Enter confirm",))
    os.write(master, b"\x15")
    repaint(master, captured, (COMPOSER,), ("/model",))


def check_effort(master: int, captured: bytearray) -> None:
    """EFF-1/EFF-2: a real command changes idle effort without a model request or journal."""
    os.write(master, b"/effort ")
    repaint(master, captured, ("Effort", "none", "low", "medium", "high", "xhigh", "max"))
    os.write(master, b"\x1b[C\r")
    repaint(master, captured, (COMPOSER, "low"), ("Enter confirm",))
    os.write(master, b"/effort none\r")
    repaint(master, captured, (COMPOSER, "none"), ("Enter confirm",))


def check_tree(master: int, captured: bytearray) -> None:
    """TRE-1/TRE-5: both aliases open one modal; hidden input stays untouched, even below min size."""
    for command in (b"/tree\r", b"/rewind\r"):
        os.write(master, command)
        await_screen(master, captured, RESIZED, ("Conversation tree",), complete=False)
        os.write(master, b"qzv\x03")
        for size in (REPAINT_PROBE_SIZE, RESIZED):
            start = len(captured)
            set_size(master, size)
            await_screen(master, captured, size, ("Conversation tree",), start=start, complete=False)
        os.write(master, b"\x1b")
        repaint(master, captured, (COMPOSER,), ("Conversation tree", "qzv"))
    os.write(master, b"/tree\r")
    await_screen(master, captured, RESIZED, ("Conversation tree",), complete=False)
    start = len(captured)
    set_size(master, (8, 40))
    await_screen(master, captured, (8, 40), start=start, complete=False)
    os.write(master, b"\x1b")
    repaint(master, captured, (COMPOSER,), ("Conversation tree",))


def run_smoke(model_url: str) -> int:
    if sys.argv[1:]:
        print("usage: smoke-tui.py", file=sys.stderr)
        return 2
    root = Path(__file__).resolve().parent.parent
    capture_dir = root / "target" / "smoke"
    capture_dir.mkdir(parents=True, exist_ok=True)
    child_env = fixture_environment()
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
allowed_reasoning_efforts = ["none", "low", "high", "max"]
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
""".replace("http://127.0.0.1:9/v1", model_url),
        encoding="utf-8",
    )
    child_env["PLEXMATON_HOME"] = str(config_root)
    skill_root = config_root / "skills" / "smoke-review"
    skill_root.mkdir(parents=True)
    (skill_root / "SKILL.md").write_text(
        "---\nname: smoke-review\ndescription: Review smoke fixture\n---\nCompletion-only fixture.\n",
        encoding="utf-8",
    )
    child_env["PLEXMATON_SMOKE_API_KEY"] = "fixture-only"
    default_sessions_root = config_root / "sessions"
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "plexmaton-cli", "--quiet"], cwd=root, check=True
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
    full_frame = b""
    failures: list[str] = []
    try:
        await_screen(master, captured, INITIAL_SIZE, EXPECTED_ON_FULL_FRAME)
        _, full_frame = repaint(master, captured, EXPECTED_ON_FULL_FRAME)
        click(master, CLICK_IN_RESIZED_COMPOSER, captured, cursor=True)
        repaint(master, captured, EXPECTED_ON_FULL_FRAME)
        on_chrome = click(master, CLICK_IN_STATUS, captured)
        on_transcript = click(master, CLICK_IN_TRANSCRIPT, captured, cursor=False)

        column, row = CLICK_IN_TRANSCRIPT
        os.write(master, f"\x1b[<0;{column + 1};{row + 1}m".encode())
        check_drawer(master, captured)
        check_input_pointer(master, captured)
        check_newline(master, captured)
        check_waiting_input(master, captured)
        check_effort(master, captured)
        check_model_menu(master, captured)
        check_tree(master, captured)

        question = "press Ctrl-D again to quit"
        armed_start = len(captured)
        os.write(master, b"\x04")
        read_until(master, captured,
                   lambda: collapsed(question.encode()) in collapsed(bytes(captured[armed_start:])),
                   description="armed quit question")
        # The status row must visibly return to its ordinary path without another key.
        read_until(master, captured,
                   lambda: question not in rendered_screen(bytes(captured[armed_start:]), RESIZED).splitlines()[-1]
                   and "~/" in rendered_screen(bytes(captured[armed_start:]), RESIZED).splitlines()[-1],
                   description="quit deadline expiry")
        repaint(master, captured, EXPECTED_ON_FULL_FRAME, (question,))
        assert process.poll() is None, "expiry must not exit"
        rearm_start = len(captured)
        # Both keys are queued together; the first must paint a NEW question after expiry.
        os.write(master, b"\x04\x04")
        read_until(master, captured, lambda: ALTERNATE_SCREEN_EXIT in captured,
                   description="alternate-screen release")
        exit_code = process.wait(timeout=3)
        read_to_eof(master, captured)
        assert collapsed(question.encode()) in collapsed(bytes(captured[rearm_start:])), "expired chord did not re-arm"
    except (subprocess.TimeoutExpired, TimeoutError, EOFError) as error:
        print(f"smoke: {error}", file=sys.stderr)
        return 1
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=3)
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
    if b"To continue this conversation, run:" in captured:
        failures.append("blank launch reported a nonexistent session")

    if failures:
        return 1

    return 0


def main() -> int:
    with NoModelRequests() as model:
        result = run_smoke(model.base_url)
    if result:
        return result
    print(
        f"smoke: painted the idle live runtime at {INITIAL_SIZE[0]}x{INITIAL_SIZE[1]}, "
        f"repainted on resize to {RESIZED[0]}x{RESIZED[1]}, routed an SGR click to the "
        "transcript and none to the status line, verified Ctrl-J draft lines and Copy sent feedback, "
        "left the draft and the absent waiting band untouched under Alt-\u2191, "
        "expired and re-armed the quit chord, and released "
        "mouse and focus reporting before the alternate screen, without creating an empty JSONL"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
