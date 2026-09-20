#!/usr/bin/env python3
"""Does cancellation still work through a sandbox-exec wrapper?

The command tool's contract requires group signalling to reach the shell and its
descendants, drains to terminate, and a launch failure to stay distinguishable
from a command exit. Inserting sandbox-exec puts one more process between the
runtime and the shell. This probe asks whether each of those still holds.

Every arm runs twice: once bare, once wrapped. A bare failure means the probe is
wrong, not the wrapper. Nothing outside a disposable temp directory is touched.

Run it outside any outer sandbox: a nested sandbox_apply is refused, and every
wrapped arm then fails for a reason that has nothing to do with this question.

    python3 .agents/spikes/permission-policy/seatbelt-lifecycle.py
"""

from __future__ import annotations

import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SANDBOX_EXEC = "/usr/bin/sandbox-exec"
SHELL = "/bin/sh"

# Same shape as the real profile would take: reads open, writes fenced to
# resolved roots. Signal behaviour should not depend on the profile, and an arm
# that proves it does is worth knowing about.
PROFILE = """(version 1)
(allow default)
(deny file-write* (subpath "/"))
(allow file-write* (subpath (param "ROOT")))
(allow file-write* (subpath (param "TMP")))
"""

# $$ inside `( ... )` is the invoking shell's pid, not the subshell's, so the
# descendant is a real `sh -c` child that records its own and then execs away.
SCRIPT = """
echo $$ > "$1/shell.pid"
sh -c 'echo $$ > "$0/child.pid"; exec sleep 300' "$1" &
wait
"""

# macOS ships no setsid(1), so the escaping descendant calls setsid(2) itself.
# The command spec already records that such a process outlives group
# signalling; this asks only whether the wrapper changes that.
ESCAPING_SCRIPT = """
echo $$ > "$1/shell.pid"
/usr/bin/python3 -c 'import os,sys,time
os.setsid()
open(sys.argv[1] + "/child.pid", "w").write(str(os.getpid()))
time.sleep(300)' "$1" &
wait
"""

BOUND = 5.0  # seconds a drain or a death may take before it counts as a failure

results: list[tuple[bool, str]] = []


def check(ok: bool, label: str) -> None:
    results.append((ok, label))
    print(f"  {'PASS' if ok else 'FAIL'}  {label}")


def launch(root: Path, tmp: Path, script: str, wrapped: bool) -> subprocess.Popen:
    argv = [SHELL, "-c", script, "sh", str(root)]
    if wrapped:
        argv = [
            SANDBOX_EXEC,
            "-p",
            PROFILE,
            "-D",
            f"ROOT={root}",
            "-D",
            f"TMP={tmp}",
            "--",
            *argv,
        ]
    # start_new_session mirrors how a runtime isolates the command's group, so
    # os.killpg below signals exactly what the runtime would signal.
    return subprocess.Popen(
        argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True
    )


def read_pid(path: Path) -> int | None:
    for _ in range(int(BOUND * 20)):
        try:
            text = path.read_text().strip()
            if text:
                return int(text)
        except (FileNotFoundError, ValueError):
            pass
        time.sleep(0.05)
    return None


def alive(pid: int) -> bool:
    """Only valid for processes this probe does not own: an unreaped child of
    ours stays answerable as a zombie."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def died_within(pid: int, bound: float) -> bool:
    deadline = time.monotonic() + bound
    while time.monotonic() < deadline:
        if not alive(pid):
            return True
        time.sleep(0.05)
    return not alive(pid)


def reap(proc: subprocess.Popen) -> None:
    """Leave nothing behind when an assertion fails mid-probe."""
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    try:
        proc.communicate(timeout=BOUND)
    except (subprocess.TimeoutExpired, ValueError):
        proc.kill()


def signal_arm(root: Path, tmp: Path, sig: signal.Signals, wrapped: bool) -> None:
    where = "wrapped" if wrapped else "bare"
    name = sig.name
    for stale in ("shell.pid", "child.pid"):
        (root / stale).unlink(missing_ok=True)

    proc = launch(root, tmp, SCRIPT, wrapped)
    shell_pid = read_pid(root / "shell.pid")
    child_pid = read_pid(root / "child.pid")

    if shell_pid is None or child_pid is None:
        check(False, f"{where}/{name}: command started and recorded its pids")
        err = b""
        try:
            _, err = proc.communicate(timeout=1.0)
        except subprocess.TimeoutExpired:
            pass
        if err.strip():
            print(f"        stderr: {err.decode(errors='replace').strip()[:160]}")
        reap(proc)
        return
    check(True, f"{where}/{name}: command started and recorded its pids")
    if wrapped:
        print(f"        launched={proc.pid} shell={shell_pid} descendant={child_pid}")

    os.killpg(proc.pid, sig)

    # Reap our own child first: a zombie answers kill(pid, 0) and would read as
    # alive. The drain is the half that hangs when a surviving descendant holds
    # the pipe, so time it here and treat a timeout as the failure it is.
    drained = True
    started = time.monotonic()
    try:
        proc.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        drained = False
        proc.kill()
        proc.communicate()
    elapsed = time.monotonic() - started
    check(drained, f"{where}/{name}: output drain reached EOF in {BOUND:.0f}s")
    if drained:
        print(f"        drain returned after {elapsed:.2f}s")

    check(proc.poll() is not None, f"{where}/{name}: launched process exited")
    check(died_within(shell_pid, BOUND), f"{where}/{name}: shell died")
    check(died_within(child_pid, BOUND), f"{where}/{name}: descendant died")
    reap(proc)


def escape_arm(root: Path, tmp: Path, wrapped: bool) -> bool:
    """Returns whether the escaping descendant survived. Not a pass/fail on its
    own: the contract already admits it. The pass is bare and wrapped agreeing."""
    for stale in ("shell.pid", "child.pid"):
        (root / stale).unlink(missing_ok=True)
    proc = launch(root, tmp, ESCAPING_SCRIPT, wrapped)
    child_pid = read_pid(root / "child.pid")
    if child_pid is None:
        reap(proc)
        raise RuntimeError("escaping descendant never recorded its pid")
    os.killpg(proc.pid, signal.SIGTERM)
    reap(proc)
    survived = not died_within(child_pid, 1.5)
    try:
        os.kill(child_pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    return survived


def launch_failure_arm(root: Path) -> None:
    """A rejected profile must not look like a command that ran and did nothing."""
    marker = root / "never.txt"
    marker.unlink(missing_ok=True)
    proc = subprocess.Popen(
        [
            SANDBOX_EXEC,
            "-p",
            "(this is not a profile)",
            "--",
            SHELL,
            "-c",
            f'echo ran > "{marker}"',
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    _, err = proc.communicate(timeout=BOUND)
    check(not marker.exists(), "launch failure: command body never ran")
    check(proc.returncode != 0, f"launch failure: nonzero exit ({proc.returncode})")
    check(bool(err.strip()), "launch failure: wrapper wrote a diagnosable stderr")
    if err.strip():
        print(f"        stderr: {err.decode(errors='replace').strip()[:120]}")


def main() -> int:
    if sys.platform != "darwin":
        print("darwin only")
        return 0
    if not Path(SANDBOX_EXEC).exists():
        print(f"missing {SANDBOX_EXEC}")
        return 1

    # Resolved, because an unresolved subpath is accepted and grants nothing.
    workdir = Path(tempfile.mkdtemp(prefix="plexmaton-lifecycle.")).resolve()
    tmproot = Path(tempfile.gettempdir()).resolve()
    try:
        print("\nSignal delivery through the wrapper")
        for wrapped in (False, True):
            for sig in (signal.SIGTERM, signal.SIGKILL):
                signal_arm(workdir, tmproot, sig, wrapped)

        print("\nA descendant that leaves the process group")
        bare = escape_arm(workdir, tmproot, wrapped=False)
        wrapped = escape_arm(workdir, tmproot, wrapped=True)
        check(
            bare == wrapped,
            f"wrapper does not change the known escape (bare survived={bare}, "
            f"wrapped survived={wrapped})",
        )

        print("\nLaunch failure stays distinguishable")
        launch_failure_arm(workdir)
    finally:
        shutil.rmtree(workdir, ignore_errors=True)

    passed = sum(1 for ok, _ in results if ok)
    print(f"\n{passed}/{len(results)} checks passed")
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
