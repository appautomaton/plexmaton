"""Owned, offline boundaries shared by the terminal smoke scripts.

A smoke script is a journey: what a person does at the terminal, and what must be on screen for
them. Everything a journey needs but does not decide lives here — the pseudo-terminal, the frame
reader, and the loopback tripwire — so a journey imports one module and no journey imports another.

Rejected: loading a sibling journey with `importlib.util.spec_from_file_location`, which is how
five of these scripts once reached `Terminal`. It executes the other journey's module body to
borrow its helpers, makes the helper's owner ambiguous, and charges every new journey the cost of
finding which script happens to hold what it needs.
"""
import errno
import fcntl
import os
import pty
import re
import select
import socket
import struct
import subprocess
import termios
import time
import unicodedata
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAX_CAPTURE_BYTES = 8 * 1024 * 1024
ALTERNATE_SCREEN_EXIT = b"\x1b[?1049l"
ANSI = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]")
WHITESPACE = re.compile(r"\s+")
# The pinned Ratatui Crossterm backend resets attributes after a completed draw, followed only
# by cursor controls. Readiness includes this boundary, not a partial matching caption.
FRAME_END = re.compile(rb"\x1b\[0m(?:\x1b\[(?:\?[0-9;]+[hl]|[0-9;]+H))*$")
UP, DOWN, ENTER, ESC = b"\x1b[A", b"\x1b[B", b"\r", b"\x1b"
# The `/permissions` row PER-11 seeds where CMD-7 reports it can fence a command. Short enough to
# survive the narrowest journey width: the full scope wraps, and a wrapped marker never matches.
SEEDED_COMMANDS = "Revoke Session: Commands"


def _read(master, sink, limit):
    try:
        chunk = os.read(master, 65536)
    except OSError as error:
        if error.errno == errno.EIO:  # PTY EOF on macOS/Linux.
            return False
        raise
    if not chunk:
        return False
    if len(sink) + len(chunk) > limit:
        raise AssertionError("terminal capture exceeded its byte bound")
    sink.extend(chunk)
    return True


def read_until(master, sink, ready, timeout=3.0, description="terminal state", limit=MAX_CAPTURE_BYTES):
    """Silence is not success: return only when the requested observable state exists."""
    deadline = time.monotonic() + timeout
    while not ready():
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([master], [], [], max(0, remaining))[0]:
            raise TimeoutError(f"timed out waiting for {description}")
        if not _read(master, sink, limit):
            raise EOFError(f"terminal closed before {description}")


def observe_for(master, seconds, sink):
    """A bounded negative-observation window, reserved for no-op and deadline assertions."""
    deadline = time.monotonic() + seconds
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([master], [], [], max(0, remaining))[0]:
            return
        if not _read(master, sink, MAX_CAPTURE_BYTES):
            return


def read_to_eof(master, sink, timeout=3.0):
    deadline = time.monotonic() + timeout
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([master], [], [], max(0, remaining))[0]:
            raise TimeoutError("terminal writers did not close")
        if not _read(master, sink, MAX_CAPTURE_BYTES):
            return


def fixture_environment():
    """Keep terminal/tool essentials, never inherited API credentials or a live tmux clipboard."""
    keys = ("PATH", "HOME", "LANG", "LC_ALL", "TMPDIR")
    env = {key: os.environ[key] for key in keys if key in os.environ}
    env.update(TERM="xterm-256color", COLORTERM="truecolor", SSH_TTY="/dev/plexmaton-smoke")
    return env


class NoModelRequests:
    """An owned loopback tripwire; nothing here can answer as a model or contact one."""

    def __enter__(self):
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            self.listener.bind(("127.0.0.1", 0))
            self.listener.listen(1)
            self.base_url = f"http://127.0.0.1:{self.listener.getsockname()[1]}/v1"
            return self
        except BaseException:
            self.listener.close()
            raise

    def __exit__(self, error_type, _error, _traceback):
        try:
            if error_type is None:
                assert not select.select([self.listener], [], [], 0)[0], "smoke attempted a model connection"
        finally:
            self.listener.close()


def set_size(fd, size):
    rows, columns = size
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))


def sgr_press(column, row):
    """One left-button press, in the SGR encoding crossterm asks the terminal for."""
    return f"\x1b[<0;{column + 1};{row + 1}M".encode()


def cursor_visible(raw):
    return raw.rfind(b"\x1b[?25h") > raw.rfind(b"\x1b[?25l")


def click(master, at, sink, cursor=None):
    before = len(sink)
    os.write(master, sgr_press(*at))
    if cursor is None:
        observe_for(master, 0.05, sink)  # Explicit no-op observation, not transition readiness.
    else:
        read_until(master, sink, lambda: cursor_visible(bytes(sink)) == cursor
                   and FRAME_END.search(bytes(sink)) is not None
                   and (cursor or len(sink) > before),
                   description="pointer focus")
    return bytes(sink[before:])


def collapsed(raw):
    """Strips escape sequences and whitespace so cursor-move gaps do not break comparison."""
    return WHITESPACE.sub("", ANSI.sub("", raw.decode("utf-8", errors="replace")))


def rendered_screen(raw, size):
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


RULE_RUN = re.compile("─+")

# Rows a smoke terminal has. Thirty-three rather than thirty because the agents strip takes its
# rows from the top of the conversation, and these journeys assert that a long stretch of one is
# on screen at once — a height claim, which the strip changed the budget for.
SMOKE_ROWS = 33

# The width a journey runs at unless it says otherwise: one conversation, which is the shape most
# of a journey is about. Two columns are a width each journey sweeps to deliberately, not the
# default it happens to start in.
SMOKE_WIDTH = 95


def await_screen(master, captured, size, markers=(), absent=(), start=0, complete=True, exact_lines=()):
    def ready():
        screen = rendered_screen(bytes(captured[start:]), size)
        flat = collapsed(screen.encode())
        return (FRAME_END.search(bytes(captured[start:])) is not None
                and all(collapsed(marker.encode()) in flat for marker in markers)
                and all(collapsed(marker.encode()) not in flat for marker in absent)
                and all(any(row.strip(" │") == value for row in screen.splitlines()) for value in exact_lines)
                # A frame is complete once the composer's bottom rule is painted (ui-ux §input).
                # The rule is the longest run of "─" on its row, not the trailing one: with a
                # delegate's column open the rule ends where the primary's column does and the row
                # goes on with somebody else's border, so a trailing-run test never fires there.
                and (not complete or any(
                    max((len(run) for run in RULE_RUN.findall(row)), default=0) >= size[1] // 2
                    for row in screen.splitlines())))
    read_until(master, captured, ready, timeout=5,
               description=f"screen {size} with {markers!r} without {absent!r}")
    return rendered_screen(bytes(captured[start:]), size)


def composer_title(display_name):
    """What the composer's top rule says: the next message's model and, after it, its effort.

    A marker that the composer is on screen. The trailing separator keeps it from matching a status
    line that shows the same model name in another shape.
    """
    return f" {display_name} · "


class Terminal:
    """One real `plexmaton` in front of a pseudo-terminal, named by the journey that drives it.

    Artifacts land in `target/smoke/<journey>-<name>`, so a failure names the journey that produced
    it. Rejected: one shared prefix for every journey, which filed the tree and model captures
    under `permissions-`.
    """

    def __init__(self, project, environment, journey, name, arguments=(), composer=None):
        self.project, self.environment = project, environment
        self.composer = composer
        self.journey, self.name = journey, name
        self.arguments = tuple(arguments)
        self.size = (SMOKE_ROWS, SMOKE_WIDTH)
        self.capture = bytearray()
        self.frame_start = 0

    def __enter__(self):
        self.master, slave = pty.openpty()
        try:
            set_size(slave, self.size)
            self.process = subprocess.Popen(
                [str(ROOT / "target/debug/plexmaton"), *self.arguments], cwd=self.project, env=self.environment,
                stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
                preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0))
        except BaseException:
            os.close(self.master)
            raise
        finally:
            os.close(slave)
        return self

    def artifacts(self):
        output = ROOT / "target/smoke"
        output.mkdir(parents=True, exist_ok=True)
        return output

    def __exit__(self, *_error):
        try:
            output = self.artifacts()
            (output / f"{self.journey}-{self.name}.raw").write_bytes(self.capture)
            (output / f"{self.journey}-{self.name}.txt").write_text(
                rendered_screen(bytes(self.capture[self.frame_start:]), self.size))
        finally:
            try:
                if self.process.poll() is None:
                    self.process.kill()
            finally:
                os.close(self.master)
            self.process.wait(timeout=10)

    def wait(self, *markers, absent=(), complete=True):
        try:
            return await_screen(
                self.master,
                self.capture,
                self.size,
                markers,
                absent,
                self.frame_start,
                complete=complete,
            )
        except (EOFError, TimeoutError) as error:
            screen = rendered_screen(bytes(self.capture[self.frame_start :]), self.size)
            raise type(error)(
                f"{error}; process exit: {self.process.poll()}\nlast rendered screen:\n{screen}"
            ) from error

    def send(self, keys, *markers, absent=()):
        os.write(self.master, keys)
        return self.wait(*markers, absent=absent)

    def resize(self, width, *markers, absent=()):
        self.frame_start = len(self.capture)
        self.size = (SMOKE_ROWS, width)
        set_size(self.master, self.size)
        return self.wait(*markers, absent=absent)

    def widths(self, name, *markers):
        for width, label in [(121, None), (120, "two-columns"), (95, "one-column"), (60, "narrow")]:
            screen = self.resize(width, *markers)
            if label:
                # PRE-1: resize may first publish placeholders. Review the settled frame,
                # and fail if the owned preparation never supplies it.
                screen = self.wait(*markers, absent=("Preparing text",))
                (self.artifacts() / f"{self.journey}-{name}-{label}.txt").write_text(
                    "\n".join(row.rstrip() for row in screen.splitlines()) + "\n")
        self.resize(SMOKE_WIDTH, *markers)

    def prompt(self, message, *markers, absent=()):
        at = (5, self.size[0] - 3)
        click(self.master, at, self.capture, cursor=True)
        os.write(self.master, sgr_press(*at)[:-1] + b"m")
        return self.send(message.encode() + ENTER, *markers, absent=absent)

    def restore_command_approvals(self):
        """Take PER-11's seeded confined-command grant back, so a command asks again.

        A coding Session starts holding that grant wherever CMD-7 can fence a command, which is
        what makes routine work quiet. A journey whose subject is the approval itself revokes the
        row first: the question returns because the bound that replaced it is gone, which is the
        same route the owner has. Where no fence exists the grant was never seeded and the
        question is already there, so this changes nothing and the journey reads the same on
        every host.

        Rejected: a flag that skips seeding for tests. An invisible default cannot be revoked
        from `/permissions`, so a journey using one would prove a path the owner does not have.
        """
        screen = self.prompt("/permissions", "Session permissions", "Session grants last until")
        if SEEDED_COMMANDS in screen:
            self.send(DOWN, f"> {SEEDED_COMMANDS}")
            self.send(ENTER, "Revoke this permission?", "> Back")
            self.send(UP, "> Revoke permission")
            self.send(ENTER, "Permission updated", absent=(SEEDED_COMMANDS,))
        self.send(ESC, self.composer, absent=("Enter review",))
        return self.send(b"\x15", self.composer, absent=("/permissions",))

    def quit(self):
        self.send(b"\x04", "press Ctrl-D again to quit")
        os.write(self.master, b"\x04")
        read_until(self.master, self.capture, lambda: ALTERNATE_SCREEN_EXIT in self.capture,
                   description=f"{self.journey} terminal release")
        # Restoration and the durable-conversation handoff may still be writing. Keep draining
        # the PTY until EOF before joining; waiting first can hold a terminal drain on macOS.
        read_to_eof(self.master, self.capture)
        assert self.process.wait(timeout=3) == 0
