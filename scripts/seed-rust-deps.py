#!/usr/bin/env python3
"""Prewarm an absent target with conservative external Rust dependency artifacts.

Contract: macOS, Cargo 1.98, ordinary host debug builds, equal Cargo.lock files,
and a successful producer JSON log. Exclude local packages and the reverse
dependency closure of build scripts, native links and procedural macros. Hold
the producer's Cargo locks, clone into a private stage, and publish without
replacement. Cargo remains responsible for deciding whether each unit is fresh.
No compilation, binary rewriting, hardlinks, or byte-copy fallback.
"""

import argparse
from contextlib import contextmanager
import ctypes
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile


MAX_LOG_BYTES = 32 * 1024 * 1024


class Refused(Exception):
    """The request cannot be served inside the supported boundary."""


def build_events(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as log:
        if not stat.S_ISREG(os.fstat(log.fileno()).st_mode):
            raise Refused("Build log must be a regular file")
        contents = log.read(MAX_LOG_BYTES + 1)
    if len(contents) > MAX_LOG_BYTES:
        raise Refused("Build log exceeds 32 MiB")
    events = [json.loads(line) for line in contents.decode().splitlines() if line.strip()]
    if not all(isinstance(event, dict) for event in events):
        raise Refused("Build log records must be JSON objects")
    finished = [event for event in events if event.get("reason") == "build-finished"]
    if len(finished) != 1 or finished[0].get("success") is not True or events[-1] != finished[0]:
        raise Refused("Require one complete, successful Cargo JSON build log")
    return events


def eligible_packages(metadata):
    packages = {package["id"]: package for package in metadata["packages"]}
    blocked = set()
    for package_id, package in packages.items():
        source = package.get("source")
        kinds = {kind for target in package["targets"]
                 for kind in target["kind"] + target["crate_types"]}
        if (not source or not source.startswith(("registry+", "sparse+", "git+"))
                or package.get("links") or kinds & {"custom-build", "proc-macro"}):
            blocked.add(package_id)
    nodes = metadata.get("resolve", {}).get("nodes", [])
    if (not nodes or {node["id"] for node in nodes} != set(packages)
            or any(set(node["dependencies"]) - set(packages) for node in nodes)):
        raise Refused("Require a complete resolved Cargo metadata graph")
    while True:
        more = {node["id"] for node in nodes if set(node["dependencies"]) & blocked} - blocked
        if not more:
            return packages, set(packages) - blocked
        blocked.update(more)


def external_sources(metadata, workspaces):
    """Shared source IDs must also resolve to shared, checkout-independent files."""
    locations = {}
    for package in metadata["packages"]:
        if package.get("source"):
            manifest = Path(package["manifest_path"]).resolve(strict=True)
            if any(manifest.is_relative_to(workspace) for workspace in workspaces):
                raise Refused("External dependency sources must live outside both workspaces")
            locations[package["id"]] = package["manifest_path"]
    return locations


def regular_path(path, root, directory=False):
    """Reject escape and symlinks, including directory components under the root."""
    if root.is_symlink() or not path.is_absolute() or ".." in path.parts or not path.is_relative_to(root):
        raise Refused(f"Path escapes the selected target: {path}")
    current = root
    for part in path.relative_to(root).parts:
        current = current / part
        if current.is_symlink():
            raise Refused(f"Symlinks are not supported: {current}")
    mode = path.stat().st_mode
    if not (stat.S_ISDIR(mode) if directory else stat.S_ISREG(mode)):
        raise Refused(f"Unexpected file type: {path}")


def select_files(metadata, events, target):
    packages, eligible = eligible_packages(metadata)
    selected = {}
    accepted = set()
    excluded = set()
    units = set()

    def add(path, package_id):
        regular_path(path, target)
        relative = path.relative_to(target)
        previous = selected.setdefault(relative, package_id)
        if previous != package_id:
            raise Refused(f"Conflicting package ownership for {relative}")

    for event in events:
        if event.get("reason") != "compiler-artifact":
            continue
        package_id = event["package_id"]
        if package_id not in packages:
            raise Refused(f"Build log and metadata disagree: {package_id}")
        if package_id not in eligible:
            excluded.add(package_id)
            continue
        if event.get("manifest_path") != packages[package_id]["manifest_path"]:
            raise Refused("Build log refers to a different dependency source location")
        if (event["target"]["kind"] != ["lib"] or event.get("executable")
                or event["profile"]["test"]
                or not set(event["target"]["crate_types"]) <= {"lib", "rlib"}):
            raise Refused("Only ordinary external Rust library units are supported")
        filenames = [Path(name) for name in event["filenames"]]
        if not filenames or not any(path.suffix == ".rlib" for path in filenames):
            raise Refused("Capture cargo build or cargo test --no-run, not cargo check")
        hashes = set()
        name = event["target"]["name"]
        for path in filenames:
            match = re.fullmatch(r"lib" + re.escape(name) + r"-([0-9a-f]{16})\.(rlib|rmeta)", path.name)
            if path.parent != target / "debug/deps" or not match:
                raise Refused(f"Unsupported Cargo output layout: {path}")
            hashes.add(match[1])
            add(path, package_id)
        if len(hashes) != 1:
            raise Refused("One library unit must have one filename hash")
        unit_hash = hashes.pop()
        unit = packages[package_id]["name"] + "-" + unit_hash
        fingerprint = target / "debug/.fingerprint" / unit
        regular_path(fingerprint, target, directory=True)
        for path in fingerprint.iterdir():
            add(path, package_id)
        dep_info = target / "debug/deps" / f"{name}-{unit_hash}.d"
        if dep_info.exists() or dep_info.is_symlink():
            add(dep_info, package_id)
        accepted.add(package_id)
        units.add(unit)
    if not selected:
        raise Refused("No eligible dependency artifacts in this build")
    return selected, dict(packages=len(accepted), units=len(units), excluded_packages=len(excluded))


@contextmanager
def source_locks(target):
    """Cargo 1.98 uses flock; never create or copy a producer lock file."""
    descriptors = []
    try:
        for name in [".cargo-build-lock", ".cargo-lock", ".cargo-artifact-lock"]:
            path = target / "debug" / name
            regular_path(path, target)
            fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
            descriptors.append((fd, path))
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as error:
                raise Refused("Producer target is busy; retry when its Cargo command finishes") from error
        def verify():
            for fd, path in descriptors:
                if path.is_symlink() or os.fstat(fd).st_ino != path.stat().st_ino:
                    raise Refused("Producer lock file was replaced during seeding")

        yield verify
    finally:
        for fd, _ in reversed(descriptors):
            os.close(fd)


def identity(info):
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns


def same_inode(path, original):
    try:
        current = path.lstat()
        return (current.st_dev, current.st_ino) == (original.st_dev, original.st_ino)
    except FileNotFoundError:
        return False


def clone_file(source, destination):
    libc = ctypes.CDLL(None, use_errno=True)
    clone = libc.fclonefileat
    clone.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    clone.restype = ctypes.c_int
    fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode):
            raise Refused(f"Source is not a regular file: {source}")
        # Darwin AT_FDCWD is -2; the destination is an absolute path.
        if clone(fd, -2, os.fsencode(destination), 0):
            error = ctypes.get_errno()
            raise Refused(f"APFS clone failed ({os.strerror(error)}): {source}")
        after = os.fstat(fd)
        copied = destination.stat()
        if (identity(before) != identity(after) or identity(after) != identity(source.stat())
                or copied.st_ino == before.st_ino or copied.st_size != before.st_size):
            raise Refused(f"Source changed or clone was not independent: {source}")
        os.utime(destination, ns=(before.st_atime_ns, before.st_mtime_ns))
        return before.st_size
    finally:
        os.close(fd)


def publish(stage, destination):
    libc = ctypes.CDLL(None, use_errno=True)
    rename = libc.renamex_np
    rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(os.fsencode(stage), os.fsencode(destination), 0x00000004):  # RENAME_EXCL
        error = ctypes.get_errno()
        raise Refused(f"Cannot publish target without replacement: {os.strerror(error)}")


def seed(target, destination, selected, copier=clone_file, publisher=publish, verify_source=lambda: None):
    if destination.exists() or destination.is_symlink():
        raise Refused("Destination target must be absent; existing output is never removed")
    if target == destination or target.is_relative_to(destination) or destination.is_relative_to(target):
        raise Refused("Source and destination targets must not overlap")
    parent = destination.parent.stat()
    if target.stat().st_dev != parent.st_dev:
        raise Refused("Source and destination must be on the same clone-capable filesystem")
    stage = Path(tempfile.mkdtemp(prefix=".target-seed-", dir=destination.parent))
    stage_identity = stage.stat()
    result = None
    try:
        logical_bytes = 0
        snapshots = {}
        for relative in sorted(selected):
            source = target / relative
            regular_path(source, target)
            snapshots[source] = identity(source.stat())
            output = stage / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            logical_bytes += copier(source, output)
        for source, expected in snapshots.items():
            if identity(source.stat()) != expected:
                raise Refused(f"Source changed during seeding: {source}")
        for directory in sorted((p for p in stage.rglob("*") if p.is_dir()),
                                key=lambda p: len(p.parts), reverse=True):
            original = target / directory.relative_to(stage)
            regular_path(original, target, directory=True)
            info = original.stat()
            os.utime(directory, ns=(info.st_atime_ns, info.st_mtime_ns))
        verify_source()
        if not same_inode(destination.parent, parent) or not same_inode(stage, stage_identity):
            raise Refused("Destination parent or owned stage was replaced during seeding")
        result = dict(files=len(selected), logical_bytes=logical_bytes)
        publisher(stage, destination)
        return result
    except KeyboardInterrupt:
        if result is not None and same_inode(destination, stage_identity):
            return result  # Atomic publication committed; retain the complete target.
        raise
    finally:
        if same_inode(stage, stage_identity):
            shutil.rmtree(stage)


def cargo_metadata(worktree):
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env["RUSTUP_AUTO_INSTALL"] = "0"
    version = subprocess.check_output(["cargo", "--version"], cwd=worktree, env=env,
                                      text=True, timeout=30).split()
    if len(version) < 2 or not version[1].startswith("1.98."):
        raise Refused("This seeder supports Cargo 1.98 only; use ordinary Cargo for other versions")
    result = subprocess.run(["cargo", "metadata", "--format-version=1", "--offline", "--locked"],
                            cwd=worktree, env=env, text=True, capture_output=True, timeout=60)
    if result.returncode:
        raise Refused(f"Cargo metadata failed: {result.stderr.strip()}")
    metadata = json.loads(result.stdout)
    if Path(metadata["workspace_root"]).resolve() != worktree:
        raise Refused(f"Expected a Cargo workspace root: {worktree}")
    default = worktree / "target"
    if (Path(metadata["target_directory"]).resolve() != default
            or Path(metadata.get("build_directory", default)).resolve() != default):
        raise Refused("Require worktree-local default target and build directories")
    return metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path, help="Completed producer workspace")
    parser.add_argument("--build-log", required=True, type=Path, help="Successful Cargo JSON stdout")
    parser.add_argument("--destination", required=True, type=Path, help="New workspace with no target")
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise Refused("APFS dependency seeding is supported on macOS only")
    source = args.source.resolve(strict=True)
    destination = args.destination.resolve(strict=True)
    target = source / "target"
    output = destination / "target"
    if source == destination or target.is_symlink() or output.exists() or output.is_symlink():
        raise Refused("Require distinct workspaces, a real producer target and an absent destination target")
    lockfiles = {path: path.read_bytes() for path in [source / "Cargo.lock", destination / "Cargo.lock"]}
    if len(set(lockfiles.values())) != 1:
        raise Refused("Cargo.lock differs; build the new dependency graph with ordinary Cargo")
    events = build_events(args.build_log)
    # Fail promptly if Cargo is active; query metadata outside the target locks to
    # avoid reversing Cargo's package-cache/target-lock acquisition order.
    with source_locks(target):
        pass
    metadata = cargo_metadata(source)
    consumer = cargo_metadata(destination)
    roots = [source, destination]
    if external_sources(metadata, roots) != external_sources(consumer, roots):
        raise Refused("External package identities or source locations differ; use ordinary Cargo")
    with source_locks(target) as verify_locks:
        def verify():
            verify_locks()
            if any(path.read_bytes() != contents for path, contents in lockfiles.items()):
                raise Refused("Cargo.lock changed during seeding")

        files, summary = select_files(metadata, events, target)
        summary.update(seed(target, output, files, verify_source=verify))
    print(json.dumps({"target": str(output), **summary}, sort_keys=True))


def interrupted(_signal, _frame):
    raise KeyboardInterrupt


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupted)
    try:
        main()
    except (Refused, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"Dependency seeding refused: {error}", file=sys.stderr)
        sys.exit(2)
    except KeyboardInterrupt:
        print("Dependency seeding cancelled; unpublished staging was removed; any committed target is complete and retained", file=sys.stderr)
        sys.exit(130)
