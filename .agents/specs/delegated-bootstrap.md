# Spec — Delegated child bootstrap

| Field | Value |
| --- | --- |
| Status | Implemented, wired and accepted. The real child stays passive across graceful and process-kill resume; an old approval cannot continue and fresh work uses current policy |
| Owns | Read-only child capability floor, shared coding Session policy, child journal placement and bound fresh/resumed runtime construction |
| Depends on | COL-3; CIN-2/CIN-4; APV-1–APV-3; JRN-3–JRN-5; PRV-6 |
| Proven by | Native catalog, real-file provenance/directory and direct runtime-constructor tests |

## Invariants

**CHB-1 — Child capabilities have a hard floor.** A V1 delegated child always has native
`read_file` and `search`, subject to the shared coding Session policy in PER-1. When the root binds
an authenticated collaboration owner, it adds only
the child-specific typed `send_mail` definition, whose recipient is fixed by canonical provenance.
Skills, file mutation, command execution, delegation, task control, permission memory and prompt
text cannot widen that set; execution rechecks the profile even for a call admitted by another
catalog.

**CHB-2 — Child construction is exact and closed.** Fresh and resumed child constructors require
one binding whose collaboration, delegation, worker Agent and child Conversation identities agree
before the runtime is exposed. The child mail catalog is crate-private and must carry the same full
canonical provenance as the runtime binding. They force CHB-1 and accept only a `ResolvedModel`, whose configured
reasoning effort has already passed model-local validation. Production construction installs the
root coding Session's permission owner before the child is exposed. Control is installed before any
bootstrap or recovery append; a fresh constructor refuses a journal that already has records.
Explicit User activation uses the same resumed constructor and fixed catalog after canonical User
control and the current owner's process-local target have both been validated. It requires the
existing delegated journal; activation cannot create missing child history.

**CHB-3 — Root resume never activates a child.** User-selectable root journals and delegated child
journals have separate owner-only directories that issue distinct, non-interconvertible runtime
tokens. Root constructors accept only a root token and delegated constructors accept only a child
token. Child history activates only through the delegated path and CHB-2; recovery dispatches no
provider or tool, and interrupted work needs a fresh explicit submission rather than automatic
restart. A process-dead approval has no decision route and cannot continue; a later explicit task
submission starts new model work under current policy. Explicit target resume may reconstruct a
canonical Main-controlled child but does not wake it.
User-controlled reconstruction requires a separately issued User target and explicit activation;
it likewise sends no input, provider request or tool effect.

## Model

```text
root selection ──▶ root conversations/ ──▶ user runtime

collaboration owner ──▶ delegated-sessions/ ──▶ storage-issued journal token
                                                └─ exact canonical binding
                                                   ├─ read/search + fixed-parent mail profile
                                                   └─ child runtime
```

Directory tokens close runtime routing, while COL-3 owns durable Controller and execution authority.
A raw path or journal cannot enter a public runtime constructor. Handoff changes input ownership and
does not change the CHB-1 profile.

## Evidence

[Named proofs](../evidence/delegated-bootstrap.md), one row an invariant.

## Integration boundary

[SCH-1–SCH-5](./owned-scheduling.md) validate canonical provenance, restore selected-branch
collaboration context and expose a bounded runner while keeping child history out of ordinary root
selection. The storage token proves directory origin; canonical provenance and COL-3 prove runtime
authority. Passive UI projection opens the validated child journal without constructing a runtime;
missing, writer-locked or invalid evidence keeps its roster row and projects one explicit warning.
Main-controlled child tree/compaction routing remains product work. Provider collaboration encoding
is bounded by PRV-1's four explicit dialects and makes no live-endpoint claim.
