# Project permission storage investigation

| Field | Value |
| --- | --- |
| Read when | Comparing the POSIX experiment with the personal Rust permission store |
| Status | Storage decisions promoted to PGR-1–PGR-5 and PER-6/PER-8/PER-9 |
| Basis | [Finite model](./README.md), [configuration ownership](./config-ownership.md) |

The [file/process experiment](./store-experiment.md) exercises concurrent writers, stale revocation,
authorization ordering, reset, interrupted records, audit failure and bounded lock ownership.
Its 17 tests and seven mutation witnesses establish the candidate protocol on macOS. Fixture roots
are trusted and the log stores opaque scope keys; neither supplies production path authority.

[Personal project permissions](../../specs/project-permissions.md) owns the implemented physical
identity, private paths, complete validation, lock ordering, sync acknowledgement and resource
bounds. [Permission policy](../../specs/permission-policy.md) owns configuration trust, dispatch,
partial project-grant/Conversation-audit outcomes and historical decision evidence. Their evidence
tables cover real Rust file/process and runtime boundaries; the experiment's smaller limits and
lack of `fsync` do not describe the production store.

The format choice keeps user-edited rules in the existing TOML readers and machine-owned changes
in JSONL. A future configuration editor would need comment preservation and concurrent-edit
handling; parsing and serializing a complete file does not provide either. No such editor, repair
workflow, OS command containment or portable sandbox interface is delivered by this investigation.
