# Spec — Personal project permission store

| Field | Value |
| --- | --- |
| Status | Implemented locally; store, runtime, configuration and executable evidence reviewed |
| Owns | Physical identity, bounded personal storage and cross-process transactions |
| Depends on | [permission-policy](./permission-policy.md) PER-4–PER-6 |
| Proven by | Store process/fault tests, runtime PER-6 tests and PER-8/PER-10 executable journey |

## Invariants

**PGR-1 — Physical project identity.** The namespace binds the discovered canonical project path,
its device and inode. Each transaction revalidates the pinned project and personal-store paths;
replaced roots, linked permission paths and non-private child directories cannot supply authority.

**PGR-2 — One transaction order.** Readers, mutations and dispatch refreshes take a bounded,
cancellable exclusive lock on one stable file. Mutations compare both store identity and sequence;
reset creates a fresh identity, so old revisions cannot match again.

**PGR-3 — Validate the whole source.** Every record, sequence, grant identity and constructor must
validate within explicit bounds before any snapshot is returned. A malformed, unsupported or torn
source is a typed failure, including a complete final JSON value without its newline.
Rejected: Conversation valid-prefix recovery, because a discarded suffix may contain a revoke.

**PGR-4 — Acknowledgement follows persistence.** Mutations acknowledge only after file sync;
initialization/reset also sync the containing directory. A failed/unknown write returns no new
authority; mutation consumes the transaction, preventing reuse of an older snapshot after failure.

**PGR-5 — Personal policy remains separate.** Grants, revokes and exact project-config trust live
under `PLEXMATON_HOME/projects/<physical-key>/permissions.jsonl`, outside Conversation history and
project configuration. An initialized stable lock plus a missing log is corruption, never absence.

## Storage

The stable `permissions.lock` is never replaced. Its bounded initialization marker distinguishes an
absent, never-written source from a deleted log; it holds no grants. `permissions.jsonl` begins with
format 1, physical project identity and a fresh store identity, followed by grant/revoke/trust records.
A grant identity cannot be reused after revocation within that store. Trust records name an exact
SHA-256 configuration fingerprint and confer no authority through loaded skills or model selection.

Limits: 128 active grants, 4096 mutations, 192 KiB per encoded record, 16 MiB per whole source.
Project offers require room for a maximum-sized grant as well as free count and revision capacity.
Exhaustion refuses mutation; no automatic compaction or repair restores an older allow. Reset is
explicit and accepts only a healthy source with its current revision. Cancellation before writing
applies nothing; cancellation after writing begins cannot roll back a committed grant.

Private project directories use mode 0700; regular, singly-linked log/lock files use 0600 and the
current effective UID. Files open relative to pinned descriptors with no symlink following and
nonblocking type validation. Local Unix filesystems are the target; hostile same-user mutation,
network filesystem lock semantics and power-loss behavior beyond host sync guarantees are unproven.

## Evidence

[Named proofs](../evidence/project-permissions.md), one row an invariant.
