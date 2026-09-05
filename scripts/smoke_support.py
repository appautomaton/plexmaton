"""Owned, offline boundaries shared by the terminal smoke scripts."""
import os
import errno
import select
import socket
import time

MAX_CAPTURE_BYTES = 8 * 1024 * 1024


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
