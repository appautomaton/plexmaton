"""Measure three profiles in fresh targets; temporarily change one codegen attribute."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--jobs", type=int, required=True)
args = parser.parse_args()
if args.jobs < 1:
    parser.error("--jobs must be positive")

root = Path.cwd()
if not (root / ".git").is_file():
    raise SystemExit("Run from a task worktree root, not the primary checkout.")
source = root / "crates/plexmaton-tui/src/theme.rs"
original = source.read_bytes()
needle = b"    pub fn style(&self, role: Role) -> Style {"
if original.count(needle) != 1:
    raise SystemExit("The edit fixture changed; update the measurement explicitly.")
edited = original.replace(needle, b"    #[inline(never)]\n" + needle)
if subprocess.run(["git", "diff", "--quiet", "HEAD", "--", str(source)]).returncode:
    raise SystemExit("Preserve existing theme.rs edits before measuring.")
base = root / "target/profile-measure"
if base.exists():
    raise SystemExit("target/profile-measure already exists; use a fresh task worktree.")
base.mkdir(parents=True, exist_ok=False)
results = []

def size(path):
    return int(subprocess.check_output(["du", "-sk", str(path)], text=True).split()[0]) * 1024

def build(label, target, env, stage):
    cmd = ["cargo", "test", "-p", "plexmaton-tui", "--lib", "--no-run", "--offline", "--locked", "--jobs", str(args.jobs), "--target-dir", str(target)]
    log = base / f"{label}-{stage}.log"
    print(f"{label}: {stage}", flush=True)
    started = time.monotonic()
    with log.open("w") as output:
        subprocess.run(cmd, cwd=root, env=env, stdout=output, stderr=subprocess.STDOUT, check=True, timeout=600)
    return {"seconds": round(time.monotonic() - started, 3), "target_bytes": size(target)}

try:
    for label, debug, incremental in [("full-incremental", "2", "1"), ("lines-incremental", "line-tables-only", "1"), ("lines-no-incremental", "line-tables-only", "0")]:
        source.write_bytes(original)
        env = os.environ.copy()
        target = base / label
        env.update(CARGO_PROFILE_DEV_DEBUG=debug, CARGO_PROFILE_TEST_DEBUG=debug, CARGO_INCREMENTAL=incremental, CARGO_BUILD_BUILD_DIR=str(target))
        row = {"case": label, "debug": debug, "incremental": incremental == "1"}
        row["cold"] = build(label, target, env, "cold")
        source.write_bytes(edited)
        row["edit"] = build(label, target, env, "edit")
        results.append(row)
        (base / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(json.dumps(row), flush=True)
finally:
    source.write_bytes(original)

print("Source restored; results:", base / "results.json", flush=True)
