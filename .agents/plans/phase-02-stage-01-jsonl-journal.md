# Plan — Phase 02 stage 1, canonical JSONL journal

| Field | Value |
| --- | --- |
| Phase | [Phase 02 — Durable sessions and context](../phases/phase-02-durable-sessions.md) §scope 1–2 |
| Contract | PRV-3/PRV-4, ENT-1/ENT-3, LOOP-2/LOOP-4 and APV-6 |
| Status | Active; slice 1 of 6 done; slice 2 ready |
| Blocked | None |

## Outcome

One typed append log is the session source. Its immutable entry graph and named heads rebuild the
model record and the visible conversation; one per-session JSONL file preserves that log across a
normal exit and recovers its longest valid prefix after an incomplete final write. The live CLI can
create and resume a session without adding a database, executing replayed work, or introducing a
second transcript.

## Constraints established before implementation

- JSONL is the canonical session store, not an export beside another authority. One physical line
  is one complete typed mutation; a header version selects an explicit decoder. Unknown kinds,
  versions, sequence gaps, parent references and head revisions are typed errors, never skipped.
- An entry append names its parent, target head and expected head revision in the same record, so
  advancing that head is one mutation. Creating, moving, renaming and abandoning heads remain their
  own single-record mutations because they need not create conversation content.
- The writer has one owner and a bounded input queue. It appends before applying the same mutation
  to its in-memory reducer. An ordinary write error returns ownership of user text and becomes
  visible; the TUI loop never performs filesystem work.
- Loading accepts the longest valid prefix. A valid final JSON value without `\n` gains one; an
  incomplete final line is isolated and reported; corruption earlier in the file stops recovery at
  that point rather than guessing around broken ancestry. An open final turn becomes interrupted.
- Replay is reduction only. Historical tool calls and outcomes are data; no projector invokes a
  model, tool, approval policy or filesystem effect. Provider-required call/result pairing is
  rebuilt from typed state, never patched silently by a codec.
- Provider replay remains codec-tagged, bounded and redacted under decoding errors and Debug.
  A lossless test path includes it; ordinary presentation never does.
- Same typed records produce the same projections regardless of whether they arrived live or from
  disk. File position, vector index, timestamps and formatted error strings are not identities.

## Slices

1. **Journal vocabulary and reducer.** Add stable session, record, entry and head identities; a
   typed record enum; immutable parent-linked entries; monotonic sequence; fresh-name head creation;
   and revision-checked append and existing-head mutations. The version boundary remains with Slice
   3's typed file header.
   Keep this slice in memory and independent of files. Give
   `ProviderReplay` a validated lossless storage representation without weakening its redacted
   `Debug`. *Closes when* every record JSON-round-trips, invalid ancestry/order/revision is refused,
   replaying the same records twice yields equal graph/head state, and no secret appears in Debug or
   decoding errors.
2. **Two pure projections.** Derive ordered `RequestItem`s and `SessionEventEnvelope`s from one head
   path, including text, reasoning, tool lifecycle, usage and notices. A recovered unfinished turn
   has an explicit projection and no unmatched provider call. *Closes when* the canonical live
   fixture produces the current model request and TUI state from journal records alone, shuffled
   branches cannot alter another head, and replay touches no effect boundary.
3. **JSONL file adapter.** Add one concrete storage module with a typed header, exact one-line codec,
   serialized append owner and bounded commands. Load validates and reduces incrementally; fork
   stages a complete sibling then renames it. *Closes when* temp-directory tests cover create,
   append, reopen, valid missing newline, incomplete tail isolation, middle corruption, write
   failure and two attempted writers, with no test touching the real `PLEXMATON_HOME`.
4. **Make the journal authoritative.** Replace `Record`'s independent item/event counters with the
   journal reducer and projections. Transient step assembly remains, but final facts enter once and
   both consumers derive from them. *Closes when* existing agent/provider/TUI fixtures remain equal,
   deleting either projection and rebuilding it changes nothing, and no reconciliation path exists.
5. **Runtime ownership and submission failure.** The live runtime owns the journal writer and waits
   for append completion before starting model/tool work that depends on it. A failed user-message
   append returns the exact draft and emits a typed visible failure. *Closes when* injected write
   failure loses no input, cancelling a caller leaves the write owned, shutdown drains or reports
   every accepted record, and the TUI loop contains no filesystem call.
6. **Create, resume and recovery journey.** Resolve session paths under `PLEXMATON_HOME/sessions`,
   add the first explicit create/resume CLI surface, and drive a real saved conversation through
   exit and reopen. *Closes when* normal resume, a torn final record and an unfinished final turn are
   read through the real composition root; the screen names recovery once; the full workspace gates
   pass; and this consumed plan is deleted or the next stage plan replaces it.

## Order, and why

The record grammar lands before files because an inspectable file with unstable semantics is not a
session format. Both projections land before persistence so load and live traffic have one reducer
to exercise. The concrete JSONL adapter then earns the I/O seam; only after it works does the current
`Record` hand over authority. Runtime integration follows authority, and the executable journey
closes the stage.

Rejected: SQLite and redb before a measured index/query need; a live JSONL mirror beside another
authority; treating a malformed middle line as safe to skip; replay that re-executes tools; and a
provider codec that invents missing results.

## Deliberately not in this plan

Context token estimation, compaction, cache epochs, physical branch garbage collection, persistent
permission grants, additional provider dialects and MCP. They consume this journal in later Phase
02 stages. Multi-agent mail and delegation remain Phase 03.
