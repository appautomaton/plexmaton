# Project grant-log experiment

| Field | Value |
| --- | --- |
| Read when | Implementing project grant transactions, revocation or interrupted-write recovery |
| Status | 17 POSIX file/process tests passed on macOS; production integration owned by PER-6 and PGR-1–PGR-5 |
| Contract | P1/P2/P4/P5 in [the spike](./README.md); [project storage proposal](./project-permissions.md) |

## Experiment

[Probe](./project_store_probe.py) and [tests](./test_project_store.py) use Python's standard library:
real JSONL files, POSIX `flock`, separate interpreter processes and owned temporary directories.
The experiment runs only inside owned temporary directories and contacts no model endpoint.

```sh
python3 -B -m unittest discover -s .agents/spikes/permission-policy -p 'test_project_store.py' -v
```

Each process opens the same stable lock file; readers take shared locks and mutations take exclusive
locks. The transaction reloads the bounded log and checks store identity plus revision before
appending. Grant identities cannot be reused after revocation. Scope values are opaque fixture keys,
not a production matcher or canonical argument encoding. The probe limits a log to 64 KiB, a line
to 1 KiB and mutations to 32. These are experiment bounds, not proposed product defaults.

Parent/child coordination uses readiness, contention and release markers with deadlines. Tests never
use elapsed sleep as proof that another process acquired a lock. Children have bounded collection
and are killed and reaped on failure; temporary files live through their owning test only.

## Evidence

| Behavior | Observed through real files/processes |
| --- | --- |
| Concurrent writers | Exactly one writer with a shared expected revision succeeds; explicit reevaluation lets the other append without losing the first grant |
| Revocation | A stale writer is refused; a fresh reader sees the revoke; the old grant identity cannot be reissued |
| Dispatch ordering | A revoke before authorization prevents the marker effect; an authorization before revoke may finish afterward |
| Lock ownership | A reader waits for an exclusive transaction; cancellation and exhausted waiting write nothing; killing the lock-owning process releases a waiting reader |
| Write uncertainty | A pre-write failure changes no bytes. A completed write followed by an error or process exit is present on reopen, despite no success reply |
| Torn records | Partial grant and revoke records make the entire grant view unavailable; no old-prefix authority is returned |
| Reset and validation | A fresh store identity rejects an old same-number revision. Malformed fields/records are corrupt; project mismatch and absent storage are separately typed |
| Resource bounds | Oversized input and an exhausted record budget refuse without appending; reading a missing store creates no empty directory |
| Two-file ordering | Failed session-audit creation leaves the project grant stored and creates no effect. The successful path creates both audit and effect |

All 17 cases passed. Seven disposable mutations each failed the intended assertion: removing the
revision check, ignoring store identity, omitting locks, salvaging a torn tail, reusing a revoked
grant ID, executing before audit, and undoing a project grant after audit failure. Mutation artifacts
were temporary and were removed after their processes completed.

## Decisions supported by the experiment

Keep the short transaction and version check; a process-local cache cannot be the project authority.
Treat a lost acknowledgement as an uncertain outcome that requires a fresh read, not as proof that
the grant was never written. A failed dependent audit starts no tool and does not compensate by
revoking a project grant another session may already have used.

Revocation governs later authorization decisions. It does not automatically cancel previously
authorized work or undo effects. Holding the project lock throughout a command would change this
contract and block revocation behind long-running tools; this experiment releases it at authorization.
PER-5/PER-6 own the production dispatch decision and cancellation boundary.

Do not reuse the session loader's valid-prefix recovery as permission recovery. A damaged suffix
may contain a revoke; dropping it can restore an old allow. The candidate reports a typed corrupt
source and leaves repair to a separately defined operation. PER-6 owns effective policy when the grant source is unavailable.

## Limits

This is a POSIX protocol experiment, not the production Rust store. It proves the exercised host
file/lock behavior, not Windows support, Linux-host validation or portable Rust locking details.
It assumes trusted temporary directory roots; parent-path replacement, root identity derivation,
network filesystems, deliberate lock-file replacement and hostile same-user processes are unproven.
There is no `fsync`, power-loss simulation or production repair/compaction strategy.

The second file represents an audit barrier, not `JournalFile`; the effect is a fixture marker,
not an agent tool. TOML identity/reload, full admitted call binding, worker queues, durable session
heads and UI publication are absent. The existing finite Rust model covers policy matching;
production composition and its tests live in [PER-6](../../specs/permission-policy.md) and
[PGR-1–PGR-5](../../specs/project-permissions.md).
