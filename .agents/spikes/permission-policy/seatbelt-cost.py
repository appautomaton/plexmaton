#!/usr/bin/env python3
"""What one `sandbox-exec` wrapper costs per command on this host.

The spike records that startup and per-call overhead were never measured, here or in any compared
source. This measures the only thing that is actually in question for a per-command wrapper: the
extra wall time between deciding to run a command and the command having run.

It is not a benchmark of confinement in production. It measures one host, one profile shape, short
commands, cold caches only for the first iteration. Run it and read the numbers beside the profile
they came from; do not carry them to another machine.
"""

import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# The shape the probe beside this file proves correct: a write fence with two allowed roots,
# a denied control subpath, and no network.
PROFILE = """(version 1)
(allow default)
(deny file-write*)
(allow file-write* (literal "/dev/null"))
(allow file-write* (subpath (param "ROOT")) (subpath (param "SCRATCH")))
(deny file-write* (subpath (param "CONTROL")))
(deny network*)
"""

ENVIRONMENT = {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}
RUNS = 60


def bare(command, _root, _scratch):
    return ["/bin/sh", "-c", command]


def confined(command, root, scratch):
    return [
        "/usr/bin/sandbox-exec",
        "-p",
        PROFILE,
        "-D",
        f"ROOT={root}",
        "-D",
        f"SCRATCH={scratch}",
        "-D",
        f"CONTROL={root}/.git",
        "/bin/sh",
        "-c",
        command,
    ]


def measure(build, command, root, scratch):
    samples = []
    for _ in range(RUNS):
        argv = build(command, root, scratch)
        start = time.perf_counter()
        finished = subprocess.run(
            argv, env=dict(ENVIRONMENT, HOME=str(scratch), TMPDIR=str(scratch)),
            capture_output=True, check=False,
        )
        samples.append((time.perf_counter() - start) * 1000.0)
        if finished.returncode != 0:
            raise SystemExit(
                f"{command!r} failed under {build.__name__}: "
                f"{finished.stderr.decode(errors='replace')[:200]}"
            )
    return samples


def report(label, bare_ms, confined_ms):
    b, c = statistics.median(bare_ms), statistics.median(confined_ms)
    print(
        f"{label:<22} bare {b:6.1f} ms   confined {c:6.1f} ms   "
        f"added {c - b:6.1f} ms  ({c / b:.1f}x)"
    )


def main():
    if sys.platform != "darwin":
        raise SystemExit("this probe measures macOS sandbox-exec; nothing to measure here")
    if not Path("/usr/bin/sandbox-exec").exists():
        raise SystemExit("/usr/bin/sandbox-exec is absent")

    with tempfile.TemporaryDirectory(prefix="plexmaton-seatbelt-cost-") as folder:
        # Resolved, not as handed out: on macOS the temp root arrives as /var/... while the kernel
        # matches /private/var/..., and an unresolved `subpath` silently grants nothing. Any
        # writable root reaching a profile has to be resolved first — including a user's own.
        fixture = Path(folder).resolve()
        root = fixture / "project"
        scratch = fixture / "scratch"
        (root / ".git").mkdir(parents=True)
        scratch.mkdir()

        print(f"macOS sandbox-exec, {RUNS} runs per arm, median wall time per invocation\n")
        for label, command in [
            ("trivial (true)", "true"),
            ("one write", f"printf x > {root}/probe"),
            ("small pipeline", "printf 'a\\nb\\nc\\n' | /usr/bin/sort | /usr/bin/head -1"),
        ]:
            report(
                label,
                measure(bare, command, root, scratch),
                measure(confined, command, root, scratch),
            )
    print(
        "\nOne host, short commands. The added milliseconds are what a per-command wrapper costs\n"
        "before the command's own work begins; a command that takes a second pays it once."
    )


if __name__ == "__main__":
    main()
