# Phase 03 — Durable collaboration

| Field | Value |
| --- | --- |
| Status | Active; stage 7 and stage 8 recovery slices 1–3 are complete; final acceptance is next |
| Parent roadmap | [Roadmap](../roadmap.md) |
| Product contract | [UI/UX](../ui-ux.md) |
| Inherits | Phase 02 journal recovery, providers, compaction, permissions and conversation trees. APV-6 now permanently cancels process-dead approvals; new submissions use current policy |

## Outcome

The root delegates bounded work; a child runs the existing agent loop and returns typed mail.
Both conversations retain their own work, correspondence and identities. The user can inspect and
stop a child, and type into it only after explicit durable Handoff. Default resume starts only
the root and permits passive child-history inspection without executing providers or tools.
COL-3, CHB-1–CHB-3 and the [locked roadmap](../roadmap.md#locked) own authority and capability rules.

## Current evidence

The current task is `feat/phase-03-control` in `.worktrees/phase-03-control`. Stage 7 is
checkpointed locally through `a5cfdb2`; stage 8.1 is checkpointed through `c0133c8`, stage 8.2
through `b41ed4d`, and stage 8.3 is complete in the current change. Earlier
preparation is published on the task branch. The user
directed this same worktree to continue through the prepared Phase 03 plan end to end, without a PR,
merge or retirement between slices. The primary checkout remains on main; local commits may
checkpoint completed slices, while publication still requires instruction.

| Area | Evidence and limit |
| --- | --- |
| Delegation and mail | `scripts/smoke-delegate.py` uses the real binary, runtime/storage and a loopback Chat Completions fixture. It drives delegate/send_mail/Handoff, opens child work, sends a focused User child message, checks both conversations at 120/95 and returns to the root after closing the child at 60 |
| Persistence | Graceful root resume restores exact child history and User control without a request or write. The provisioning process witness kills after canonical creation, child-journal creation, runner registration and acknowledged mail, then reopens one exact passive child with its receipts/mail. The real CLI smoke separately kills while idle, during paused child work and after pending Handoff projection. Passive pointer, keyboard and repeated resume preserve exact bytes and normalized selected ancestry, task/control/correspondence, provider history and a synchronized tool-invocation trace. No provider or tool is replayed; explicit root continuation receives the same task/mail context, and the pending cut restores Main with one typed recovery suffix |
| Script infrastructure | Shared PTY/Terminal and provider fixtures replace cross-journey imports. Addressed replies match the final request message; the routing regression now compares response bodies |
| Ordered publication | The controlled old-code witness at `c965b28` published a delegated fact before the blocked user append was acknowledged. The repaired runtime refuses without numbering; the root collaboration component retains one typed projection bound to the exact root runtime while bounded owners hold later activity. Reopen rebuilds durable log/journal facts, and shutdown reports a transient runner outcome with no durable source. The runtime and CLI regressions prove consecutive exact-once success, cancelled-wait retention, both failure dispositions, conversation isolation and no pre-ack event/effect |
| Owned child Stop | Focused-child Ctrl-C now addresses the collaboration owner, never the root runtime. Stop admission is synchronous and cancellation-safe; accepted scheduling settles first, exact reports return through the child route, wakes cannot restart the stopped child, and repeated/missing/resumed requests remain typed control outcomes. The paused-provider PTY stops only the child, admits root input before releasing the provider barrier, completes that root request afterward, and rejects late child output from both screen and journal |
| Passive child resume | The normal replay route already projected valid child history before slice 3, but no executable opened it after restart. Pointer and keyboard now open that exact persisted child at 120/95/60 columns; at 60, scroll then close/reopen retains the same semantic first-visible history line before returning to Main. Provider request counts and the root, child and collaboration JSONL bytes remain unchanged across both resume journeys. Missing, locked and invalid child journals retain a selectable roster row with one bounded child-owned warning; invalid bytes remain untouched |
| Durable shared-entry placement | Successful collaboration tools and admitted recipient turns write validated reference-only session links after acknowledgement; task and mail bodies remain solely in the collaboration log. If an accepted ingress outlives its tool wait, the composition writes the same idempotent link through the caller's root or child journal owner. Selected branches place each side at its first durable link, while old or interrupted journals use a stable canonical suffix. Two real-file reopens preserve bytes and order; real passive child activation suppresses only its restored prefix and retains later recovery revisions; the PTY places task/mail among conversation work before the restoration confirmation |
| Execution after Handoff | The owner issues a process-local authenticated User input target for one canonical child; activation returns an exact runner-generation ticket and admits it through a separate one-slot runner lane only after the durable controller is User. Focused product input is retained synchronously before cold activation or journal progress, then settles through the owner activity branch so the terminal loop remains available. Immediate Stop owns and returns pre-activation input without starting a runner. Active Handoff stops Main work before transfer; idle transfer admits only the addressed child. Exact Handoff retry does not interrupt User work, Stop closes new input, cancelled waits and shutdown retain outcomes and drafts, and a reopened User child stays passive until explicit cold activation of its existing delegated journal. Production applies monotonic Main/pending/User snapshots, rolls failed transfer presentation forward to Main, anchors Handoff before the first User turn, routes focused input only to that child and restores exact refused drafts |
| Canonical background requests | The child journal remains the complete pending/resolved source. The collaboration log admits only bounded endpoint/Attention-ID references before root projection; references do not enter transcript or model context. Main-controlled live child approvals route through the exact owner and runner generation, while wrong, stale and passively reopened decisions remain inert. Passive restore joins the validated log and journal with no provider, tool or runner. Shutdown drains final producer resolutions before closing the collaboration writer, and a real recovered child plus full root/collaboration reopen proves a resolved request stays absent. The production child now requests `read_file` under the root coding Session's configured Ask rule. The product preserves root focus and opens that card only through user navigation at 120/95/60. After process death the old decision returns `NotPending`; a new explicit task runs under the reloaded Deny rule without reading the file |
| Complete product journey and names | The real binary performs one delegation, one exact task update and child continuation, typed mail, durable Handoff, focused User child input, passive pointer/keyboard resume, focused-child Stop with concurrent root continuation, actual process-kill recovery, and permanent cancellation of a process-dead child approval before new work uses current policy. Task, work, mail, control and return-to-root remain reachable at 120/95/60 through product controls. Correspondence and copied addressed text use `Plexmaton` / `Delegated N`; route IDs remain durable and absent on screen. Wide/medium/narrow checked-in frames and real-PTY artifacts were regenerated and inspected. Final Kitty acceptance remains the stage 8 gate |
| Validation | At unchanged `c965b28`, all seven executable journeys and all 41 Python tests passed locally on 2026-09-15. Stage 7.1 passed the 70-test runtime persistence module and its focused gates. Stage 7.2 passed 31 owned-scheduling tests, the 16-test Stop filter and all 124 then-current CLI binary tests. Stage 7.3 passed all 127 CLI binary tests, three focused Inspector keyboard/pointer/anchor tests, all 41 Python gate tests and the expanded real delegation journey. Stage 7.4 passed all 241 agent tests, all 231 runtime tests, every CLI target including 134 composition tests, all 41 Python tests, targeted Clippy, formatting and the expanded delegation PTY with ordered live/reopen task and mail rows. Stage 7.5 passed all 240 runtime tests, every CLI target including 134 composition tests, and all 41 Python gate tests, plus targeted Clippy, formatting, file-length, crate-graph, citation and frame gates. Its nine-test tier-2 module covers active/idle Handoff, exact retry, accepted-wait cancellation, failed input followed by Stop, cross-child Stop priority, canceled cold Handoff, retained terminal join order, strict history resume, stale target/ticket, passive reopen and explicit User activation. Stage 7.6 passed all 243 runtime tests, 500 TUI tests, every CLI target including 137 composition tests, all 41 Python gates, targeted Clippy and the expanded PTY with Main Handoff, child-only input/answer, passive User-control reopen and 120/95/60 frames. Stage 7.7 passed all 242 agent tests, all 244 runtime tests, every CLI target including 141 composition tests, all 41 Python gates, Clippy, formatting, file-length, crate-graph, citation and frame gates, the unchanged real delegation smoke, and a real-runner/full-reopen shutdown regression. Stage 7.8 passed all 500 TUI tests, every CLI target including 141 composition tests, all 41 Python gates and the expanded real-PTY journey with exact task-update identity, display labels, semantic scrolling and 120/95/60 saved frames. Stage 8.1 passed all 246 runtime unit tests, six provider-replay tests, five runtime skill tests and all 67 session-store tests; its real child-process witness passed all four forced-kill cuts. Stage 8.2 passed every CLI target (7 library, 141 binary, 23 measure, 4 journal projection, 14 render preparation and 4 startup tests), all 41 Python gates, workspace Clippy and the expanded real PTY with idle, paused-child and pending-Handoff process kills. Stage 8.3 passed the complete workspace test suite, all 41 Python tests, workspace Clippy, formatting, file-length, crate-graph, citation, frame and diff gates. Its real PTY proves Ask-before-death, inert old approval and fresh Deny-policy work at 120/95/60. Independent targeted review found no code blocker; its README wording correction is applied. No branch CI result is recorded here |
| Active implementation lease | None. Stage 8.3 is complete; stage 8.4 has not started |
| Existing control backend | Main's four tools, Handoff/Stop settlement, monotonic authenticated controller snapshots, child-only User input and distinct durable Handoff entries are wired. The real PTY accepts Main Handoff, focused child input and passive User-control resume |
| Native UI | Revisioned control/composer fixtures have native evidence in the [Kitty record](../spikes/kitty-native-preview/README.md); the authenticated product path has 120/95/60 real-PTY frames. Final native user acceptance remains separate |

Keep three facts separate: **Implemented** means the mechanism has its spec-named proof;
**Wired** means a production caller exists; **Accepted** means a specific executable scenario was
demonstrated. Missing journey coverage does not establish missing implementation. User visual
acceptance is a further, separately recorded condition.

## Execution sequence

The plans are the coordinator's execution packets. Load only the active plan and slice's routed
owners/tests, preserve existing work, and follow AGENTS.md for team roles and authorization.

| Stage | Purpose | State |
| --- | --- | --- |
| 7 — Product integration | Ordered publication; owned Stop; passive resume; durable entry placement; post-Handoff runtime ownership; control/input; canonical requests; complete journey and consistent display names | Complete; all eight slices verified |
| [8 — Recovery acceptance](../plans/phase-03-stage-08-recovery-acceptance.md) | Provisioning crash cuts; actual CLI kill→resume; permanent cancellation of process-dead approvals with fresh-policy resubmission; final acceptance and closure | Active; slices 1–3 complete, slice 4 next |

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
| Task, child work and mail remain readable in both conversations at all three widths | The complete task-update/mail/Handoff/User-input journey passes at 120/95/60, including semantic scrolling and returning to root at 60 | 7.8 |
| Focused-child Stop settles only that child and root input stays responsive | The paused-provider PTY stops the focused child, accepts root input before the child handler is released, completes the root request afterward and admits no late child output; missing/resumed and repeated Stop remain root-safe typed outcomes | 7.2, 7.8 |
| Durable Handoff enables the exact child's input, preserves capabilities and refuses Main/stale authority | Main/pending/User snapshots, exact draft return, distinct rows, child-only input and passive reopen pass at tier 2 and in the real PTY | 7.5, 7.6 |
| Default resume permits child browsing without work and restores selected history/control | Pointer and keyboard open the exact persisted child at 120/95/60 with no request or write; close/reopen retains its semantic reading anchor, and missing, locked and invalid journals remain selectable with explicit warnings. Actual idle and paused-work process kills preserve exact bytes and normalized selected ancestry/control/correspondence; repeated resume stays inert | 7.3, 8.2 |
| Background requests have one canonical source, preserve focus, and resolve only through their owner | Canonical live routing, passive replay and graceful-shutdown resolution pass. The CHB-1 production `read_file` request uses the shared coding Session policy; real PTY Ask and process-dead inert states preserve focus and require user navigation at 120/95/60 | 7.7, 8.2, 8.3 |
| Accepted mail/provisioning survive process death and retry once with exact identities | A real child process is killed after canonical creation, child-journal creation, runner registration and acknowledged mail; every cut reopens one delegation, preserves exact retry receipts/mail, starts no passive work and explicitly recovers exactly one child | 8.1 |
| Process-dead pending calls cannot continue; new explicit work uses current admission/policy without replaying effects | The old approval returns `NotPending` with no provider/tool/durable effect; a later explicit task creates a new model turn whose `read_file` is denied by the freshly loaded policy | 8.3 |
| Control remains exclusive, Stop progresses under backpressure, and provider attribution survives all four dialects | Reuse named spec evidence, then supply missing product-level witnesses; no live endpoint claim | 8.4 |
| Names, controls, focus and reading positions satisfy the rendered experience | Production labels, control/input frames and reading anchors pass at 120/95/60; final Kitty user acceptance remains | 7.8, 8.4 |

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
  and rejection of late child bytes from screen and journal. The authenticated Main snapshot keeps
  input locked and shows the existing focused `^C Stop` hint.
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
- **Names, observed:** correspondence rows and their copied source resolve the roster labels
  `Plexmaton` and `Delegated N`; durable IDs and selectors remain unchanged and absent on screen.
- **Provisioning death, observed:** a child process holds the real collaboration writer and is
  killed after canonical creation, child-journal creation, runner registration and acknowledged
  mail. Each reopen retains the same opaque target and exact retry receipts, reconstructs no work
  passively, and repeated explicit recovery returns one canonical child journal and runner. The
  exhaustive uncertain-write, corruption and mismatched-constructor refusals remain separate
  lower-tier proofs because the process witness adds only the boundary they cannot exercise.
- **CLI process death, observed:** the real terminal process is killed while both conversations are
  idle, during a paused child request and after pending Handoff is projected but before admission.
  Passive pointer, keyboard and repeated resume preserve the durable prefix and request history;
  transient child bytes disappear, explicit root continuation sees the same task/mail, and pending
  Handoff reopens under Main with one typed `process_died` recovery suffix. JRN-4's lower tiers own
  incomplete-tail permutations rather than duplicating them in this product journey.
- **Approval recovery, observed:** the user chose permanent cancellation for process-dead
  approvals. Recovery settles the old call as `process_died`; its approval ID returns `NotPending`
  and cannot execute. A new explicit task starts a new model turn and call under current policy.
  The production child shares the root coding Session owner and requests its CHB-1 `read_file` tool.
  The real 120/95/60 PTY changes `native_inspection` from Ask to Deny across restart, preserves the
  old inert presentation without focus movement, reads no protected file content, and records only
  the new forbidden result and subsequent typed mail after resubmission.

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
