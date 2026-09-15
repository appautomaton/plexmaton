# Phase 03 — Durable collaboration

| Field | Value |
| --- | --- |
| Status | Active; completion plans prepared for stages 7–8. Implementation awaits user instruction |
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

The current task is `feat/phase-03-control` in `.worktrees/phase-03-control`, based on
`b722eb8`. Its inherited uncommitted changes are scripts, tests, CI and documentation.
The primary checkout remains on main. Recheck refs/status before starting execution.

| Area | Evidence and limit |
| --- | --- |
| Delegation and mail | `scripts/smoke-delegate.py` uses the real binary, real runtime/storage and a loopback Chat Completions fixture. It drives delegate/send_mail, opens child work, checks both conversations at 120/95 columns and returns to the root after closing the child at 60 |
| Persistence | The smoke reads child JSONL and checks delegation/mail event kinds. Graceful root resume restores visible markers. This does not prove exact durable payloads, chronological equality, child reopening or process-kill recovery |
| Script infrastructure | Shared PTY/Terminal and provider fixtures replace cross-journey imports. Addressed replies match the final request message; the routing regression now compares response bodies |
| Validation | Last reported locally: seven journeys and 41 Python tests passed, plus citations/frames/typos. CI invocation is present in the working-tree workflow; there is no CI result for these uncommitted changes. Revalidate the inherited baseline once at execution start |
| Existing control backend | Main's four tools reach authenticated ingress; Handoff reaches canonical owner settlement. The owned child actor survives Stop/Handoff, but has no user-submission lane. Control snapshots and Handoff entries have no production projection |
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
| [7 — Product integration](../plans/phase-03-stage-07-product-integration.md) | Ordered publication; owned Stop; passive resume; durable entry placement; post-Handoff runtime ownership; control/input; canonical requests; complete journey and consistent display names | Prepared; eight remaining slices; none started |
| [8 — Recovery acceptance](../plans/phase-03-stage-08-recovery-acceptance.md) | Provisioning crash cuts; actual CLI kill→resume; pending approval readmission; final acceptance and closure | Prepared; four slices after stage 7 |

The previous backend work stays in its mechanisms. The former single unfinished activation/control
slice is decomposed by ownership boundary; no delivered foundation needs to be implemented again.
This preparation task ends with plans and document validation, then waits for instruction.

## Coordinator protocol

The next coordinator reads this phase and the active plan, then only that slice's specs, named
owners and nearby tests. Follow [AGENTS.md](../../AGENTS.md#working-discipline): the coordinator
owns edits; gpt-5.6-luna at max explores bounded distinct angles, and gpt-5.6-sol at high reviews.
Delegates are read-only. A coordinator may itself be gpt-5.6-luna at max; this plan requires no
continuing role from the planning session.

Delegate packets contain the slice, question, read paths, invariant and expected evidence.
Return file/line findings and test seams, not copied trees or conversation history. Do not preload
future slices/reference repositories. Read M/L as scope, not a promised token or time budget.

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
uncommitted work; commit, push, PR and merge remain unauthorized. This planning task starts no code.

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
| Every published fact survives in order; shared entries keep their positions after restart | One observed drop; durable placement appends correspondence after the restoration confirmation | 7.1, 7.4 |
| Task, child work and mail remain readable in both conversations at all three widths | Baseline covers the initial round trip; task update and the complete control journey remain | 7.8 |
| Focused-child Stop settles only that child and root input stays responsive | Ctrl-C is inferred from source to hit the root runtime and propagate WrongAgent; no executable reproduction | 7.2, 7.8 |
| Durable Handoff enables the exact child's input, preserves capabilities and refuses Main/stale authority | Settlement is wired; user dispatch, control snapshots and a Handoff entry are absent | 7.5, 7.6 |
| Default resume permits child browsing without work and restores selected history/control | Root markers restore; the documented child-open failure remains untested by the journey | 7.3, 8.2 |
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
- **Unproven cause:** a reaction waits in `pending_commit` while `project_delegated` numbers and
  queues another event. Nothing ties this candidate path to that trace. Stage 7.1 must construct
  the interleaving before choosing its repair. Rejected: repairing a supposed returned-reaction
  publication path that no longer exists.
- **Stop, read from source:** a focused child's Ctrl-C is routed through the root
  `LiveRuntime::submit`; WrongAgent propagates out of input dispatch. The journey never sends
  that key. The footer returns `Controller unavailable | Input locked` before it could draw
  `^C Stop`, because production supplies no control snapshot.
- **Resume:** the existing record reports a populated roster but no child window after Down/Enter.
  Child journals are read during restore; their existence is not proof of a usable surface.
  Stage 7.3 owns reproduction and repair.
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
