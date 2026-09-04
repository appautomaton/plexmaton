# Spec — Session timing

| Field | Value |
| --- | --- |
| Status | TIM-1 implemented; TIM-2–TIM-5 unproven until Phase 02 stage 2 slice 3 |
| Owns | Durable turn chronology, model-request attempt timing and immutable provider usage |
| Depends on | JRN-1/JRN-5/JRN-7, LOOP-1/LOOP-4/LOOP-6, PRV-1/PRV-5 and LIVE-1/LIVE-3/LIVE-4/LIVE-5 |
| Proven by | TIM-1 evidence below; remainder unproven |

## Invariants

**TIM-1 — User and turn time name semantic boundaries.** Every claimed user item retains its
accepted wall time; an initial item atomically starts a turn with stable item/turn identities, exact
text and opened wall time. One terminal fact records its typed outcome and either known completion
wall time or separately named recovery-observed wall time. These boundary facts are the durable
lifecycle authority and project the starting/terminal agent status. Idle emits no time record;
sequence and ancestry, never time, decide order.

Intra-turn status facts may still project current work, but cannot independently start or finish a
turn; no separate terminal status append follows `TurnFinished`. Recovery of every valid prefix is
therefore idempotent around both boundaries.

**TIM-2 — Dispatch authorization and measured execution are distinct.** A durable pre-effect fact
names the attempt and its pre-append authorization wall time; JRN-7 acknowledges it before network
work. The adapter samples wall and monotonic clocks immediately before HTTP dispatch. One immutable
terminal fact says whether dispatch occurred and retains measurements only when it did; process
death may leave only authorization. Rejected: treating journal latency as API latency.

**TIM-3 — Request accounting is immutable and correlated.** A stable request-attempt identity links
one typed owner, exact semantic-prefix boundary, request-environment fingerprint, provider/profile,
terminal outcome and exact provider-reported usage. The fingerprint covers instructions and tools
without copying them into the journal. Missing usage remains unavailable and a retry is distinct.
Turn totals fold only reachable agent-step attempts; whole-session incurred usage folds every
unique agent-step and compaction attempt once, never once per head.

**TIM-4 — Timing cannot perturb model context or cache ancestry.** Canonical audit facts correlate
to a semantic path but neither advance its head nor project to a `RequestItem`. Equal semantic
ancestry and request environment encode byte-identically with or without timing inspection. A
checkpoint begins a cache epoch; rewind and head selection only select an existing identity. Audit
record IDs, journal sequence, wall time and head revision are excluded from that identity.

**TIM-5 — Partial timing stays honest.** Cancellation and typed model failure end an owned attempt
as not-dispatched or dispatched according to the effect boundary. Process death may leave durable
authorization without a terminal attempt; recovery reports it as outcome unknown and never
fabricates dispatch, duration or usage.

## Request timing state

```text
Authorized { authorized_at, semantic_boundary, request_environment }
    ├── NotDispatched { outcome }
    └── Dispatched {
          dispatched_at,
          headers_after_ms?,
          first_output_after_ms?,
          terminal_after_ms,
          outcome,
          usage
        }
```

An attempt owner is `AgentStep { turn_id, step_id }` or
`Compaction { compaction_id, source_boundary }`. Compaction attempts have their own usage total and
enter whole-session incurred cost, but never inflate a turn or serve as an agent-request usage
anchor. Slice 3 fixes both wire variants before the compaction orchestrator consumes the latter.

`Authorized.authorized_at` is observed before its record is appended; it is not a claim about when
the append was acknowledged. The fact also records the semantic-prefix boundary and
request-environment fingerprint; its terminal fact refers only to that identity. `NotDispatched`
is restricted to cancellation or typed preparation/encoding failure before `.send()`. The existing
durable cumulative `TurnUsageUpdated` payload is retired when this lands. A UI event with that name
may remain as a projection of immutable per-attempt usage, never as a second journal authority.

All dispatched offsets are integer milliseconds from one adapter-owned `Instant` sampled
immediately before `.send()`. Present milestones satisfy `headers ≤ first_output ≤ terminal`.
Headers may be absent only when no response arrived; first output may be absent. First output means
the first non-empty text or reasoning delta, complete tool call, or opaque replay item; usage and
stop markers do not count. Milestones accumulate only in the owned request task and enter the
journal together in its terminal fact, never as streaming records. The current transport has no
typed timeout outcome, so this spec does not invent one under `ModelError::Transport`.

Accepted time travels process-locally with queued next-turn input and next-step steering and becomes
durable only when the matching LOOP-6 boundary claims it. It is retained on both initial and
steering user items; queueing alone creates no journal fact.

Durable wall observations use validated `UnixMillis(u64)` and monotonic offsets use checked
`ElapsedMillis(u64)`; both serialize as integer milliseconds. The owned composition/runtime samples
wall time through an injected clock, and the HTTP adapter owns each request `Instant`. Agent,
journal reduction and replay receive those values and never read a clock, so deterministic tests
use a fake source and loading cannot invent new chronology.

Wall values establish chronology only and are never subtracted to claim turn, tool or provider
duration; elapsed values come only from one monotonic `Instant`. Tool/approval timing is outside
these slices: call-to-result wall distance includes admission, approval, scheduling and ordered
finalization. Provider response/trace IDs are deferred diagnostics and, if later retained under
PRV-4, never become recovery or cache identity.

## Journal and tree placement

The initial user/turn fact and claimed steering are semantic entries and advance one checked head.
`TurnFinished`, request authorization and request terminal facts are typed `JournalRecord`s outside
the `SessionEntry` tree. They advance only the global journal sequence and correlate through stable
turn, step, attempt and semantic-boundary identities; head rename cannot change their ownership.
For one selected head, a projector admits audit facts only when their owning `TurnId` starts on its
path and their semantic boundary is on that path. Whole-session cost instead folds every unique
attempt once.

`CreateHead` and `MoveHead` may target only a stable semantic boundary: no `TurnStarted` on its path
lacks a canonical `TurnFinished`, and no context atom is split. Checked semantic appends are the
owned live-progress exception; abandoning a partial live head is refused, while rename preserves
its ownership. Rewinding to a user item moves the new head to the boundary before that turn and
returns the exact text as a user-owned draft; explicit resubmission creates a fresh global
`TurnId`. Exit before resubmission cannot look like crashed work. Two heads extended from one
completed boundary own distinct later turns and attempts without a speculative `BranchId`.

## Evidence

| Invariant | Proven by |
| --- | --- |
| TIM-1 | `tim_1_turn_boundaries_are_durable_and_terminal_time_does_not_advance_the_head`, `tim_1_queued_turn_and_steering_keep_their_original_accepted_time`, `tim_1_every_live_turn_terminal_path_has_a_typed_outcome`, `tim_1_unscoped_user_and_lifecycle_payloads_change_nothing`, `cancelled_submit_behind_an_older_commit_keeps_its_arrival_time_and_text`, `tim_1_turn_chronology_reopens_from_jsonl_without_entering_model_context`, `tim_1_jsonl_rejects_timeless_user_and_unscoped_lifecycle_records` |
| TIM-2 | Unproven |
| TIM-3 | Unproven |
| TIM-4 | Unproven; chronology isolation: `tim_1_sibling_heads_project_only_their_own_later_turns_and_terminals` |
| TIM-5 | Unproven |
