# Phase 03 — Durable collaboration

| Field | Value |
| --- | --- |
| Status | Active; stage 7 slices 1–5 of 8 are complete; slice 6 is next; stage 8 remains prepared |
| Parent roadmap | [Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Inherits | Phase 02 journal recovery, providers, compaction, permissions and conversation trees. JRN-4/JRN-5 real CLI kill→resume and APV-6 pending-request readmission remain unproven |

## Outcome

The root delegates bounded work; a child runs the existing agent loop and returns typed mail.
Both conversations retain their own work, correspondence and identities. The user can inspect and
stop a child, and type into it only after explicit durable Handoff. Default resume starts only
the root and permits passive child-history inspection without executing providers or tools.
COL-3, CHB-1–CHB-3 and the [locked roadmap](../roadmap.md#locked) own authority and capability rules.

## Current evidence

The current task is `feat/phase-03-control` in `.worktrees/phase-03-control`. Stage 7.1–7.4 are
checkpointed locally through `149d5e0`; stage 7.5 is complete in the current change. Earlier
preparation is published on the task branch. The user
directed this same worktree to continue through the prepared Phase 03 plan end to end, without a PR,
merge or retirement between slices. The primary checkout remains on main; local commits may
checkpoint completed slices, while publication still requires instruction.

| Area | Evidence and limit |
| --- | --- |
| Delegation and mail | `scripts/smoke-delegate.py` uses the real binary, real runtime/storage and a loopback Chat Completions fixture. It drives delegate/send_mail, opens child work, checks both conversations at 120/95 columns and returns to the root after closing the child at 60 |
| Persistence | The smoke reads child JSONL and checks delegation/mail event kinds. Graceful root resume restores visible markers. This does not prove exact durable payloads, chronological equality, child reopening or process-kill recovery |
| Script infrastructure | Shared PTY/Terminal and provider fixtures replace cross-journey imports. Addressed replies match the final request message; the routing regression now compares response bodies |
| Ordered publication | The controlled old-code witness at `c965b28` published a delegated fact before the blocked user append was acknowledged. The repaired runtime refuses without numbering; the root collaboration component retains one typed projection bound to the exact root runtime while bounded owners hold later activity. Reopen rebuilds durable log/journal facts, and shutdown reports a transient runner outcome with no durable source. The runtime and CLI regressions prove consecutive exact-once success, cancelled-wait retention, both failure dispositions, conversation isolation and no pre-ack event/effect |
| Owned child Stop | Focused-child Ctrl-C now addresses the collaboration owner, never the root runtime. Stop admission is synchronous and cancellation-safe; accepted scheduling settles first, exact reports return through the child route, wakes cannot restart the stopped child, and repeated/missing/resumed requests remain typed control outcomes. The paused-provider PTY stops only the child, admits root input before releasing the provider barrier, completes that root request afterward, and rejects late child output from both screen and journal |
| Passive child resume | The normal replay route already projected valid child history before slice 3, but no executable opened it after restart. Pointer and keyboard now open that exact persisted child at 120/95/60 columns; at 60, scroll then close/reopen retains the same semantic first-visible history line before returning to Main. Provider request counts and the root, child and collaboration JSONL bytes remain unchanged across both resume journeys. Missing, locked and invalid child journals retain a selectable roster row with one bounded child-owned warning; invalid bytes remain untouched |
| Durable shared-entry placement | Successful collaboration tools and admitted recipient turns write validated reference-only session links after acknowledgement; task and mail bodies remain solely in the collaboration log. If an accepted ingress outlives its tool wait, the composition writes the same idempotent link through the caller's root or child journal owner. Selected branches place each side at its first durable link, while old or interrupted journals use a stable canonical suffix. Two real-file reopens preserve bytes and order; real passive child activation suppresses only its restored prefix and retains later recovery revisions; the PTY places task/mail among conversation work before the restoration confirmation |
| Execution after Handoff | The owner issues a process-local authenticated User input target for one canonical child; activation returns an exact runner-generation ticket and admits it through a separate one-slot runner lane only after the durable controller is User. Active Handoff stops Main work before transfer; idle transfer admits only the addressed child. Exact Handoff retry does not interrupt User work, Stop closes new input, cancelled waits and shutdown retain outcomes and drafts, and a reopened User child stays passive until explicit cold activation of its existing delegated journal. Reopen restores its history and read-only capability profile while stale targets, tickets and Main wakes/admissions fail |
| Validation | At unchanged `c965b28`, all seven executable journeys and all 41 Python tests passed locally on 2026-09-15. Stage 7.1 passed the 70-test runtime persistence module and its focused gates. Stage 7.2 passed 31 owned-scheduling tests, the 16-test Stop filter and all 124 then-current CLI binary tests. Stage 7.3 passed all 127 CLI binary tests, three focused Inspector keyboard/pointer/anchor tests, all 41 Python gate tests and the expanded real delegation journey. Stage 7.4 passed all 241 agent tests, all 231 runtime tests, every CLI target including 134 composition tests, all 41 Python tests, targeted Clippy, formatting and the expanded delegation PTY with ordered live/reopen task and mail rows. Stage 7.5 passed all 240 runtime tests, every CLI target including 134 composition tests, and all 41 Python gate tests, plus targeted Clippy, formatting, file-length, crate-graph, citation and frame gates. Its nine-test tier-2 module covers active/idle Handoff, exact retry, accepted-wait cancellation, failed input followed by Stop, cross-child Stop priority, canceled cold Handoff, retained terminal join order, strict history resume, stale target/ticket, passive reopen and explicit User activation. No branch CI result is recorded here |
| Active implementation lease | None. Slices 1–5 are complete; slice 6 has not started |
| Existing control backend | Main's four tools reach authenticated ingress; Handoff and Stop reach canonical owner settlement. The owned child actor has an authenticated post-Handoff User input lane and explicit cold activation. Control snapshots, product input routing and Handoff entries have no production projection |
| Native UI | Revisioned control/composer fixtures have native evidence in the [Kitty record](../spikes/kitty-native-preview/README.md). They prove presentation mechanisms, not production authority or final user acceptance |

Keep three facts separate: **Implemented** means the mechanism has its spec-named proof;
**Wired** means a production caller exists; **Accepted** means a specific executable scenario was
demonstrated. Missing journey coverage does not establish missing implementation. User visual
acceptance is a further, separately recorded condition.

## Execution sequence

The plans are the coordinator's execution packets. Load only the active plan and slice's routed
owners/tests, preserve existing work, and follow AGENTS.md for team roles and authorization.

| Stage | Purpose | State |
| --- | --- | --- |
| [7 — Product integration](../plans/phase-03-stage-07-product-integration.md) | Ordered publication; owned Stop; passive resume; durable entry placement; post-Handoff runtime ownership; control/input; canonical requests; complete journey and consistent display names | Active; slices 1–5 of eight complete; slice 6 is next |
| [8 — Recovery acceptance](../plans/phase-03-stage-08-recovery-acceptance.md) | Provisioning crash cuts; actual CLI kill→resume; pending approval readmission; final acceptance and closure | Prepared; four slices after stage 7 |

The previous backend work stays in its mechanisms. The former single unfinished activation/control
slice is decomposed by ownership boundary; no delivered foundation needs to be implemented again.
Execution follows the prepared slice order and coordinator protocol below.

## Coordinator protocol

The next coordinator reads this phase and the active plan, then only that slice's specs, named
owners and nearby tests. The user's assignment for this run is gpt-5.6-sol at xhigh as coordinator
and gpt-5.6-luna at max as implementation delegates. The coordinator owns decomposition, integration,
independent review, verification and task Git resources; Luna delegates may edit and test their
explicitly assigned slice scope under [AGENTS.md](../../AGENTS.md#working-discipline).
This run requires no continuing role from the planning session.

Keep one implementation slice in progress. Give each writer exclusive named files, and do not
edit those files concurrently. Independent read-only exploration may run alongside implementation;
additional writers require demonstrably disjoint ownership within the active slice. The coordinator
reviews the actual diff and test evidence before marking a slice complete; the implementer's
report alone is not verification. Delegate Git commits, branch switches and cleanup are not allowed.

Delegate packets contain the slice, role, read paths, exclusive edit scope, invariant, acceptance
checks and exclusions. Implementers return changed files, checks actually run, results and remaining
risks; read-only delegates return file/line findings and test seams. Do not copy trees/conversation
history or preload future slices/references. M/L describes scope, not a token or time budget.

For each slice: failing case → implementation → focused checks → targeted review → spec/status
update. Retain only command, tested revision/diff, result, artifact path and limits beside the slice;
logs belong in task target/. Update the [three status cells](../README.md#where-we-are).
When a plan is consumed, delete it and replace its phase row/link with the completed outcome;
the next plan uses this protocol, not a link into the deleted plan.

[Quality gates](../standards/quality-gates.md) owns checks and
[Rust builds](../standards/rust-builds.md) owns isolated artifacts. Verify the inherited baseline
once at execution start; expand checks for changed boundaries/failures. After execution is
authorized, continue routine green slices without another permission request. A concrete contract/
authority decision or external blocker gets evidence and a proposed resolution. Preserve existing
uncommitted work. Execution authorization alone does not authorize commit, push, PR or merge;
follow the user's explicit instruction for each. This planning task starts no code.

### Luna context and slot lifecycle

The Sol coordinator owns both context fit and delegate retirement. A slice is an acceptance unit,
not a promise that one Luna context can complete it; split L slices into bounded implementation
packets without weakening their final gate.

1. **Measure before dispatching work.** Inspect the actual runtime's model metadata, context
   telemetry and concurrent-agent limit. This preparation found Luna metadata with
   `context_window=272000` and `effective_context_window_percent=95` (258400 before used context),
   not a measured worker remainder. Recheck at run start; the
   [Luna API model ceiling](https://developers.openai.com/api/docs/models/gpt-5.6-luna) is not the
   local harness allocation. Spawn with minimal inherited context (`fork_turns="none"` in the
   current tool schema) and a self-contained packet. Each Luna reports its own available context
   after receiving that packet, using `get_context_remaining` or the runtime's equivalent. Never
   use Sol's remaining context as Luna's budget. If telemetry is unavailable, mark it unknown and
   use one small test/patch unit per delegate instead of assuming a whole L slice fits.
2. **Keep room to finish.** As a task policy, reserve at least 25% of that initial measured
   remainder for verification, failure diagnosis and handoff. This is a planning margin, not a
   model limit. Recheck at reading, patch and test milestones and before taking more work. If the
   next operation cannot fit above the reserve, or the runtime warns of compaction/context pressure,
   stop expanding scope and checkpoint. Do not rely on automatic compaction to preserve ownership
   or turn a nearly full thread into a fresh worker.
3. **Checkpoint before retirement.** Return changed files, completed work, actual test commands/
   results, unresolved hypotheses, the next atomic step, and owned process/session IDs. Finish the
   current atomic write and settle or stop only the delegate's own processes. The coordinator
   verifies the diff, records durable progress beside the active slice/spec, and preserves unfinished
   edits. A replacement receives this compact packet plus relevant paths, not the old transcript.
4. **Release every finished assignment.** On completion, cancellation, supersession or context
   rotation, collect the checkpoint and use the runtime's documented close/terminate operation
   when available. Do not park completed Luna threads for possible future work. For an unresponsive
   worker, use supported interruption/termination, then inspect files and owned processes before
   transferring edit ownership. Never discard unfinished work to make retirement look complete.
5. **Verify capacity was reclaimed.** Inventory agents before each spawn and after retirement;
   track ID, assignment, file ownership, state and context pressure in the coordinator's live
   working set. Count the coordinator when the runtime's limit does. Confirm that the old writer
   is inactive and check the runtime's slot accounting before dispatching a replacement. A final
   answer, an interrupt acknowledgement and a released slot are distinct facts. If closing is not
   exposed, follow the documented completion/interrupt behavior and report the limitation; do not
   invent a close call or claim interruption reclaimed capacity. If all slots remain occupied,
   collect/retire existing work before spawning more. No two workers inherit the same file lease.

The [OpenAI subagent guide](https://learn.chatgpt.com/docs/agent-configuration/subagents#managing-subagents)
describes stopping and closing agents; the running harness's actual tools determine which operation
Sol can invoke. Use fresh Luna assignments after retirement, with explicit file-ownership transfer.

## Mechanism routing

Read only the IDs a slice names. Architecture, testing, Git workflow and quality-gate standards
remain routed by [AGENTS.md](../../AGENTS.md#context-routing).

| IDs | Owner |
| --- | --- |
| COL | [Collaboration ledger](../specs/collaboration-ledger.md) |
| CIN | [Turn inclusion](../specs/collaboration-inclusion.md) |
| CHB | [Child bootstrap](../specs/delegated-bootstrap.md) |
| SCH | [Owned scheduling](../specs/owned-scheduling.md) |
| CTL | [Collaboration tools](../specs/collaboration-tools.md) |
| CMP | [Mail projection](../specs/collaboration-mail-projection.md) |
| CCV | [Child control view](../specs/child-control-view.md) |
| ENT | [Transcript entries](../specs/transcript-entry.md) |
| JRN | [Session journal](../specs/session-journal.md) |
| LIVE | [Live runtime](../specs/live-runtime.md) |
| INS | [Inspector](../specs/inspector.md) |
| INV | [Interaction routing](../specs/interaction-routing.md) |
| ATT | [Attention](../specs/attention.md) |
| APV | [Tool admission](../specs/tool-admission.md) |

## Exit gate

Close only after every required row below has current evidence and the native user-review boundary
is satisfied. The executable journey and focused mechanism tests complement each other.
A scope change needs the user's decision; green tests for a smaller journey cannot waive a row.

| Required outcome | Current gap | Closing slice |
| --- | --- | --- |
| Every published fact survives in order; shared entries keep their positions after restart | Controlled publication schedules and reference-only placement pass; no remaining graceful-restart gap | 7.1, 7.4 |
| Task, child work and mail remain readable in both conversations at all three widths | Baseline covers the initial round trip; task update and the complete control journey remain | 7.8 |
| Focused-child Stop settles only that child and root input stays responsive | The paused-provider PTY stops the focused child, accepts root input before the child handler is released, completes the root request afterward and admits no late child output; missing/resumed and repeated Stop remain root-safe typed outcomes | 7.2, 7.8 |
| Durable Handoff enables the exact child's input, preserves capabilities and refuses Main/stale authority | Settlement is wired; user dispatch, control snapshots and a Handoff entry are absent | 7.5, 7.6 |
| Default resume permits child browsing without work and restores selected history/control | Pointer and keyboard open the exact persisted child at 120/95/60 with no provider request or durable write; close/reopen retains its semantic reading anchor, and missing, locked and invalid journals remain selectable with explicit warnings. Process-kill equality remains | 7.3, 8.2 |
| Background requests have one canonical source, preserve focus, and resolve only through their owner | No production cross-session request source/projection; recovered requests stay inert until fresh admission is supported | 7.7, 8.2, 8.3 |
| Accepted mail/provisioning survive process death and retry once with exact identities | Real-file unit/component proofs exist; executable two-file and kill→resume witnesses remain | 8.1, 8.2 |
| Restored pending calls use current admission/policy without replaying effects | APV-6 is unproven; reconcile its restoration boundary with JRN-5 cancellation | 8.3 |
| Control remains exclusive, Stop progresses under backpressure, and provider attribution survives all four dialects | Reuse named spec evidence, then supply missing product-level witnesses; no live endpoint claim | 8.4 |
| Names, controls, focus and reading positions satisfy the rendered experience | Internal IDs appear in correspondence; production control frames and visual acceptance remain | 7.8, 8.4 |

## Outstanding facts and hypotheses

- **Observed drop:** a reported smoke run showed `[gap] resynchronized from 17 to 18` followed by
  `[drop] sequence 17: stale event sequence: expected 19, received 17`. The guard runs before
  touching the roster because Notices moves the hit rows. Twelve subsequent clean runs are only
  those runs' result. No controlled reproduction exists.
- **Controlled publication defect:** at pre-repair `c965b28`, a held user append followed by
  `project_delegated` published the later delegated envelope before acknowledgement. This proves
  the candidate mechanism defect, though it cannot establish that the earlier smoke trace used the
  same schedule. Stage 7.1 now refuses that projection without numbering it; the root collaboration
  component keeps one typed projection bound to the exact root runtime and applies it after
  acknowledgement. Durable facts replay after definite/uncertain failure, while shutdown reports a
  transient runner outcome that cannot replay. Rejected: an unbounded pending-commit suffix.
- **Stop, observed:** a focused child's Ctrl-C is routed through the collaboration owner, which
  retains accepted schedule/Stop settlement while the root loop continues. The paused-provider
  journey proves child-only cancellation, root input before fixture release, the later root answer
  and rejection of late child bytes from screen and journal. The footer still returns
  `Controller unavailable | Input locked`; slice 7.6 owns the authenticated control snapshot and
  visible `^C Stop` hint.
- **Resume, observed:** valid passive history projection was already wired; the missing evidence was
  the user route after restart. The expanded journey opens the same child by pointer and keyboard at
  all three widths, changes no provider count or durable byte, and retains a scrolled semantic anchor
  through close/reopen before returning to Main at 60. Storage and projection errors previously
  disappeared behind an empty-conversation placeholder; missing, locked and invalid journals now
  produce exact child-owned warnings without changing their evidence.
- **Placement, observed:** successful sender tools and recipient inclusions add session-local links
  to canonical collaboration references. Live projection waits for that session's acknowledged
  link; selected-branch replay inserts the same row there, and an unanchored legacy row remains in
  a stable collaboration-order suffix. An accepted ingress whose tool wait disappeared carries its
  authenticated caller and reference to the composition, which writes the link through that exact
  root or child journal owner. Real-file tests reopen twice without changing bytes; real child
  activation suppresses only its passive prefix and applies a same-item recovery revision once; the
  PTY keeps task and mail before the final restoration confirmation. Rejected: reconstructing
  chronology from wall-clock values or copying shared bodies into both journals.
- **Names:** root/child display labels already exist as Plexmaton and Delegated N. Correspondence
  currently leaks `agent-primary` and `delegated-N`. Stage 7.8 resolves display labels without
  changing durable identities or selectors; rendered copy remains subject to the UI/UX contract.
- **Approval recovery:** JRN-5 currently settles orphaned calls as interrupted; APV-6 requires fresh
  admission if a pending request is restored. Stage 8.3 must establish the supported flow, or
  bring a concrete contract decision to the user. Neither automatic effect replay nor silently
  treating cancellation as readmission is permitted.

## Deliberately not in Phase 03

These existing scope boundaries remain. Do not add unrelated work to a slice or silently defer an
exit row above.

| Deferred work | Owner / condition |
| --- | --- |
| Main-controlled child compaction/tree routing | Phase 04 product routing |
| CMP-2 queued/included presentation | Phase 04; the backend session join already exists |
| Mail-only wake of a sleeping recipient | A later phase if that product flow is needed |
| Collaboration tool invocation disclosure | Phase 04 entry disclosure |
| Native typed provider attribution beyond the current explicit encodings | Later provider hardening |
| Branch/export/delete reference retention | Required before exposing those operations together with collaboration; not a new export/delete project here |
| Power-loss durability | JRN-4 explicitly promises process-death durability only |

The quality-gates document budget is advisory; reconcile touched prose during final closure rather
than raising limits. Reference spikes are retained while their comparison evidence remains useful.
