#!/usr/bin/env python3
"""Disposable macOS write-boundary probe; not a production sandbox profile."""

import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile


PROFILE = """(version 1)
(allow default)
(deny file-write*)
(allow file-write* (literal "/dev/null"))
(allow file-write* (subpath (param "ROOT")) (subpath (param "SCRATCH")))
(deny file-write* (subpath (param "CONTROL")))
(deny network*)
"""


def run(argv, root, scratch):
    # All commands are fixed, synchronous fixtures with tiny output. No host config.
    environment = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(scratch),
        "TMPDIR": str(scratch),
        "LC_ALL": "C",
    }
    with subprocess.Popen(
        argv, cwd=root, env=environment, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
    ) as child:
        try:
            stdout, stderr = child.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate(timeout=2)
            raise
        return child.returncode, stdout, stderr


def require(condition, label, result=None):
    if not condition:
        raise AssertionError(f"{label}: {result!r}")
    print(f"PASS {label}", flush=True)


def main():
    if sys.platform != "darwin":
        raise SystemExit("This probe requires macOS; no unsandboxed fallback.")
    with tempfile.TemporaryDirectory(prefix="plexmaton-seatbelt-", dir="/private/tmp") as name:
        fixture = Path(name).resolve()
        root, scratch, outside = (fixture / part for part in ("project", "scratch", "outside"))
        for directory in (root, scratch, outside, root / ".git"):
            directory.mkdir()
        control = root / ".git"
        (outside / "read.txt").write_text("synthetic outside content\n")
        (root / "escape").symlink_to(outside, target_is_directory=True)
        prefix = [
            "/usr/bin/sandbox-exec", "-D", f"ROOT={root}",
            "-D", f"SCRATCH={scratch}", "-D", f"CONTROL={control}", "-p", PROFILE,
        ]

        result = run([*prefix, "/usr/bin/true"], root, scratch)
        require(result[0] == 0, "sandbox startup", result)

        # A denied operation is meaningful only when the parent can do it.
        for directory in (outside, control):
            target = directory / "parent-control.txt"
            result = run(["/bin/sh", "-c", 'printf fixture > "$1"', "probe", str(target)], root, scratch)
            require(result[0] == 0 and target.read_text() == "fixture", f"parent can write {directory.name}", result)
            target.unlink()

        for label, target, permitted in (
            ("project write", root / "ok.txt", True),
            ("scratch write", scratch / "ok.txt", True),
            ("outside write denied", outside / "blocked.txt", False),
            ("control write denied", control / "blocked.txt", False),
            ("symlink escape denied", root / "escape" / "escaped.txt", False),
        ):
            result = run([*prefix, "/bin/sh", "-c", 'printf fixture > "$1"', "probe", str(target)], root, scratch)
            require((result[0] == 0) == permitted and target.exists() == permitted, label, result)

        target = outside / "nested.txt"
        result = run([
            *prefix, "/bin/sh", "-c", '/bin/sh -c \'printf fixture > "$1"\' probe "$1"',
            "probe", str(target),
        ], root, scratch)
        require(result[0] != 0 and not target.exists(), "descendant write denied", result)

        result = run([*prefix, "/bin/cat", str(outside / "read.txt")], root, scratch)
        require(result[0] == 0 and result[1] == b"synthetic outside content\n", "write fence permits outside reads", result)

        # Owned loopback listener, no external address or model service.
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            listener.settimeout(2)
            listener.bind(("127.0.0.1", 0))
            listener.listen(2)
            connect = ["/usr/bin/nc", "-z", "-w", "1", "127.0.0.1", str(listener.getsockname()[1])]
            result = run(connect, root, scratch)
            require(result[0] == 0, "parent loopback control", result)
            with listener.accept()[0]:
                pass
            result = run([*prefix, *connect], root, scratch)
            require(result[0] != 0, "child loopback connection denied", result)

        target = root / "must-not-run.txt"
        result = run([
            "/usr/bin/sandbox-exec", "-p", "(invalid-profile)",
            "/bin/sh", "-c", 'printf fixture > "$1"', "probe", str(target),
        ], root, scratch)
        require(result[0] != 0 and not target.exists(), "bad profile never runs command", result)


if __name__ == "__main__":
    main()
