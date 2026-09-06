#!/usr/bin/env python3
"""Bounded POSIX grant-log experiment; not a production store or path-authority API."""

from contextlib import contextmanager
from dataclasses import dataclass
from enum import Enum
import fcntl
import json
import os
from pathlib import Path
import re
import stat
import sys
import time


MAX_LOG_BYTES = 64 * 1024
MAX_LINE_BYTES = 1024
MAX_RECORDS = 32
TOKEN = re.compile(r"[a-zA-Z0-9_-]{1,64}\Z")


class Failure(Enum):
    ABSENT = "absent"
    CORRUPT = "corrupt"
    BINDING = "binding_mismatch"
    STALE = "stale_version"
    INVALID = "invalid_mutation"
    NOT_GRANTED = "not_granted"
    NOT_WRITTEN = "not_written"
    UNKNOWN_WRITE = "unknown_write"
    CANCELLED = "cancelled"
    TIMEOUT = "timeout"
    LIMIT = "limit"
    IO = "io"
    AUDIT = "audit_failed"


class StoreFailure(Exception):
    def __init__(self, kind):
        self.kind = kind
        super().__init__(kind.value)


class Fault(Enum):
    NONE = "none"
    BEFORE_WRITE = "before_write"
    PARTIAL_WRITE = "partial_write"
    AFTER_WRITE = "after_write"


@dataclass(frozen=True)
class Version:
    store: str
    revision: int


@dataclass(frozen=True)
class Snapshot:
    version: Version
    active: dict
    issued: frozenset


def token(value):
    return type(value) is str and TOKEN.fullmatch(value) is not None


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON field")
        result[key] = value
    return result


def line(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


class ProjectGrantLog:
    """One trusted temporary directory, one stable lock file, one bounded JSONL source."""

    def __init__(self, directory, project):
        if not token(project):
            raise ValueError("invalid fixture project identity")
        self.directory = Path(directory)
        self.project = project
        self.path = self.directory / "permissions.jsonl"
        self.lock_path = self.directory / "permissions.lock"

    @contextmanager
    def transaction(self, *, exclusive, timeout=3, cancelled=None, waiting=None):
        deadline = time.monotonic() + timeout
        flags = os.O_RDWR | os.O_CREAT | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK
        try:
            descriptor = os.open(self.lock_path, flags, 0o600)
        except FileNotFoundError as error:
            raise StoreFailure(Failure.ABSENT) from error
        except OSError as error:
            raise StoreFailure(Failure.IO) from error
        acquired = False
        try:
            self._regular_private(descriptor)
            mode = fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH
            announced = False
            while True:
                if cancelled is not None and cancelled():
                    raise StoreFailure(Failure.CANCELLED)
                try:
                    fcntl.flock(descriptor, mode | fcntl.LOCK_NB)
                    acquired = True
                    break
                except BlockingIOError:
                    if waiting is not None and not announced:
                        waiting()
                        announced = True
                    if time.monotonic() >= deadline:
                        raise StoreFailure(Failure.TIMEOUT)
                    time.sleep(0.005)
            yield
        finally:
            # Explicit unlock avoids extending ownership through an inherited descriptor.
            if acquired:
                fcntl.flock(descriptor, fcntl.LOCK_UN)
            os.close(descriptor)

    @staticmethod
    def _regular_private(descriptor):
        metadata = os.fstat(descriptor)
        if (not stat.S_ISREG(metadata.st_mode)
                or metadata.st_uid != os.getuid()
                or metadata.st_mode & 0o077):
            raise StoreFailure(Failure.CORRUPT)

    def initialize(self, store):
        if not token(store):
            raise ValueError("invalid fixture store identity")
        self.directory.mkdir(mode=0o700, exist_ok=True)
        with self.transaction(exclusive=True):
            encoded = line({"kind": "header", "project": self.project, "store": store})
            descriptor = os.open(
                self.path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW,
                0o600,
            )
            try:
                if os.write(descriptor, encoded) != len(encoded):
                    raise StoreFailure(Failure.UNKNOWN_WRITE)
            finally:
                os.close(descriptor)

    def read(self, **control):
        with self.transaction(exclusive=False, **control):
            return self._load()

    def _load(self):
        try:
            descriptor = os.open(self.path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
        except FileNotFoundError as error:
            raise StoreFailure(Failure.ABSENT) from error
        except OSError as error:
            raise StoreFailure(Failure.IO) from error
        try:
            self._regular_private(descriptor)
            with os.fdopen(descriptor, "rb", closefd=False) as file:
                data = file.read(MAX_LOG_BYTES + 1)
        finally:
            os.close(descriptor)
        if len(data) > MAX_LOG_BYTES:
            raise StoreFailure(Failure.LIMIT)
        if not data or not data.endswith(b"\n"):
            raise StoreFailure(Failure.CORRUPT)
        lines = data.splitlines()
        if len(lines) > MAX_RECORDS + 1:
            raise StoreFailure(Failure.LIMIT)
        records = []
        try:
            for encoded in lines:
                if not encoded:
                    raise StoreFailure(Failure.CORRUPT)
                if len(encoded) + 1 > MAX_LINE_BYTES:
                    raise StoreFailure(Failure.LIMIT)
                value = json.loads(encoded, object_pairs_hook=unique_object)
                if type(value) is not dict:
                    raise ValueError("record must be an object")
                records.append(value)
        except (ValueError, UnicodeError) as error:
            raise StoreFailure(Failure.CORRUPT) from error
        header = records[0]
        if (set(header) != {"kind", "project", "store"} or header["kind"] != "header"
                or not token(header["store"]) or not token(header["project"])):
            raise StoreFailure(Failure.CORRUPT)
        if header["project"] != self.project:
            raise StoreFailure(Failure.BINDING)
        snapshot = Snapshot(Version(header["store"], 0), {}, frozenset())
        for record in records[1:]:
            snapshot = self._reduce(snapshot, record)
        return snapshot

    @staticmethod
    def _reduce(snapshot, record):
        kind = record.get("kind")
        keys = {"kind", "revision", "grant"} | ({"scope"} if kind == "grant" else set())
        if (kind not in ("grant", "revoke") or set(record) != keys
                or type(record.get("revision")) is not int
                or record["revision"] != snapshot.version.revision + 1
                or not token(record.get("grant"))):
            raise StoreFailure(Failure.CORRUPT)
        active = dict(snapshot.active)
        issued = set(snapshot.issued)
        grant = record["grant"]
        if kind == "grant":
            if grant in issued or not token(record.get("scope")):
                raise StoreFailure(Failure.CORRUPT)
            active[grant] = record["scope"]
            issued.add(grant)
        else:
            if grant not in active:
                raise StoreFailure(Failure.CORRUPT)
            del active[grant]
        return Snapshot(Version(snapshot.version.store, record["revision"]), active, frozenset(issued))

    def mutate(self, expected, mutation, *, fault=Fault.NONE, **control):
        with self.transaction(exclusive=True, **control):
            current = self._load()
            if current.version != expected:
                raise StoreFailure(Failure.STALE)
            if current.version.revision >= MAX_RECORDS:
                raise StoreFailure(Failure.LIMIT)
            record = dict(mutation, revision=current.version.revision + 1)
            try:
                updated = self._reduce(current, record)
            except StoreFailure as error:
                raise StoreFailure(Failure.INVALID) from error
            encoded = line(record)
            if len(encoded) > MAX_LINE_BYTES or self.path.stat().st_size + len(encoded) > MAX_LOG_BYTES:
                raise StoreFailure(Failure.LIMIT)
            if fault is Fault.BEFORE_WRITE:
                raise StoreFailure(Failure.NOT_WRITTEN)
            try:
                descriptor = os.open(self.path, os.O_WRONLY | os.O_APPEND | os.O_CLOEXEC | os.O_NOFOLLOW)
            except OSError as error:
                raise StoreFailure(Failure.NOT_WRITTEN) from error
            try:
                self._regular_private(descriptor)
                output = encoded[:len(encoded) // 2] if fault is Fault.PARTIAL_WRITE else encoded
                try:
                    written = os.write(descriptor, output)
                except OSError as error:
                    raise StoreFailure(Failure.UNKNOWN_WRITE) from error
                if written != len(encoded) or fault is Fault.AFTER_WRITE:
                    raise StoreFailure(Failure.UNKNOWN_WRITE)
            finally:
                os.close(descriptor)
            return updated

    def authorize(self, expected, grant, scope, **control):
        with self.transaction(exclusive=False, **control):
            current = self._load()
            if current.version != expected:
                raise StoreFailure(Failure.STALE)
            if current.active.get(grant) != scope:
                raise StoreFailure(Failure.NOT_GRANTED)
            # This decision is the linearization point; it does not undo later effects on revoke.
            return current.version


def wait_for_marker(path, timeout=3):
    deadline = time.monotonic() + timeout
    while not Path(path).exists():
        if time.monotonic() >= deadline:
            raise StoreFailure(Failure.TIMEOUT)
        time.sleep(0.005)


def run_job(job):
    """Fixed fixture protocol for independent processes, not a model-visible tool."""
    store = ProjectGrantLog(job["directory"], job["project"])
    expected = Version(job["store"], job["revision"])
    control = {"timeout": job.get("timeout", 3)}
    if "cancel" in job:
        control["cancelled"] = lambda: Path(job["cancel"]).exists()
    if "waiting" in job:
        control["waiting"] = lambda: Path(job["waiting"]).touch()
    if "ready" in job:
        Path(job["ready"]).touch()
    if "start" in job:
        wait_for_marker(job["start"])
    operation = job["operation"]
    if operation == "read":
        snapshot = store.read(**control)
        return {"status": "ok", "store": snapshot.version.store,
                "revision": snapshot.version.revision, "active": snapshot.active}
    if operation == "hold":
        with store.transaction(exclusive=True, **control):
            Path(job["locked"]).touch()
            wait_for_marker(job["release"])
        return {"status": "ok"}
    if operation in ("mutate", "grant_then_audit", "crash_after_commit"):
        snapshot = store.mutate(expected, job["mutation"], fault=Fault(job.get("fault", "none")), **control)
        if operation == "crash_after_commit":
            os._exit(91)
        if operation == "grant_then_audit":
            try:
                with open(job["audit"], "xb") as audit:
                    audit.write(line({"store": snapshot.version.store, "revision": snapshot.version.revision}))
            except OSError as error:
                raise StoreFailure(Failure.AUDIT) from error
            store.authorize(snapshot.version, job["mutation"]["grant"], job["mutation"]["scope"], **control)
            Path(job["effect"]).write_text("executed\n")
        return {"status": "ok", "revision": snapshot.version.revision}
    if operation == "authorize":
        store.authorize(expected, job["grant"], job["scope"], **control)
        if "authorized" in job:
            Path(job["authorized"]).touch()
            wait_for_marker(job["continue"])
        Path(job["effect"]).write_text("executed\n")
        return {"status": "ok"}
    raise ValueError("unknown fixture operation")


if __name__ == "__main__":
    try:
        result = run_job(json.loads(Path(sys.argv[1]).read_text()))
    except StoreFailure as error:
        result = {"status": error.kind.value}
    print(json.dumps(result, sort_keys=True), flush=True)
