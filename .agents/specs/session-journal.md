# Spec — Session journal

| Field | Value |
| --- | --- |
| Status | Structural reducer implemented; file storage and live projections remain unproven |
| Owns | Stable session records, immutable entry ancestry, named-head revisions and their lossless wire form |
| Depends on | PRV-3/PRV-4 for model replay, ENT-1/ENT-3 for transcript identity and pure reduction |
| Proven by | `plexmaton-agent::journal` tests |

## Invariants

**JRN-1 — One record is one complete mutation.** An entry append carries its parent, target head and
expected head revision together; applying it advances that head or changes nothing. Head creation
requires a fresh never-used name; movement, rename and abandonment are likewise revision-checked
records. Sequence, record and entry identities never repeat.

**JRN-2 — Reduction is deterministic and typed.** The same ordered records build equal entries,
heads and paths. A sequence gap, unknown parent/head, stale revision or reused/retired identity is a
typed refusal which mutates nothing.

**JRN-3 — The wire is lossless without weakening opaque replay.** Every record has one tagged JSON
form and decodes through the constructors that protect identities and `ProviderReplay`. Lossless
serialization includes the exact replay payload; `Debug` and decoding errors reveal no payload.

**JRN-4 — File recovery keeps a valid prefix.** Unproven until Stage 1 slice 3: the JSONL adapter
repairs a complete final value missing its newline, isolates an incomplete tail, and refuses to
guess past earlier corruption.

**JRN-5 — Replay performs no effects.** Unproven until Stage 1 slice 2: walking a head and deriving
model/UI projections cannot invoke a provider, tool, approval policy or filesystem operation.

## Model

```text
JournalRecord ──▶ SessionJournal
                     ├─ entries: EntryId → { parent, payload }
                     ├─ heads: HeadName → { target, revision }
                     └─ ordered records

AppendEntry { head, expected_revision, entry.parent_id }
CreateHead | MoveHead | RenameHead | AbandonHead
```

`main` exists at revision zero and points to no entry in a new session. Abandoned and renamed-away
head names remain retired so a stale command cannot become valid after a name is reused.

## Failure modes

| Situation | Response |
| --- | --- |
| Record sequence is not exactly next | Typed unexpected-sequence refusal; no state changes |
| Append parent differs from the named head | Typed parent mismatch; no implicit branch |
| Head revision changed after a record was prepared | Typed stale-revision refusal |
| Parent or head target is unknown | Typed missing-identity refusal |
| Record, entry or retired head name is reused | Typed duplicate/retired refusal |
| Opaque replay exceeds its byte bound during decode | Decode fails before a journal can retain it |

## Evidence

| Invariant | Proven by |
| --- | --- |
| JRN-1 | `jrn_1_append_and_head_mutations_form_one_checked_tree` |
| JRN-2 | `jrn_2_invalid_records_change_nothing`, `jrn_2_the_same_records_build_equal_journals_and_paths`, `jrn_2_each_head_mutation_rejects_a_stale_revision`, `jrn_2_each_head_mutation_rejects_a_missing_head`, `jrn_2_an_unknown_append_parent_is_a_missing_entry`, `jrn_2_head_names_are_never_reused` |
| JRN-3 | `jrn_3_every_record_round_trips_and_debug_redacts_replay`, `jrn_3_every_model_item_variant_round_trips_inside_an_append`, `jrn_3_decoding_rechecks_identity_and_replay_bounds` |
| JRN-4 | Unproven until Stage 1 slice 3 |
| JRN-5 | Unproven until Stage 1 slice 2 |
