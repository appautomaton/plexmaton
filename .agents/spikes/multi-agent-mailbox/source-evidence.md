# Multi-agent source evidence

| Field | Value |
| --- | --- |
| Read when | Checking the mailbox spike comparison against source |
| Scope | Source inspection only; no live endpoint or reference-harness durability proof |
| Corpus | Exact remote revisions and inspection limits in [the spike](./README.md#corpus) |

Paths below are relative to the named source repository at the pinned remote revision. Inspection
used `git show` and `git grep` without changing a reference checkout.

## Ranking

Grok Build is the primary reference for execution ownership; Codex V2 is the primary reference for
independent sessions and typed provenance. Kimi Code and DeepSeek Harness contribute focused
late-callback, retry and cancellation cases. None has Plexmaton's durable single-controller handoff,
so [COL-3](../../specs/collaboration-ledger.md) remains normative.

## Codex

`codex-rs/protocol/src/protocol.rs` constructs `ResponseItem::AgentMessage`, whose typed wire value is
in `protocol/src/models.rs`. `core/src/session/rollout_reconstruction.rs` reconstructs agent mail;
`codex-api/src/common.rs` serializes response items on both transports. This proves a distinct
semantic representation, not live provider acceptance.

`core/src/session/handlers.rs` enqueues into `core/src/session/input_queue.rs`; inclusion is recorded
later by the Session. Pending delivery remains process-local, and `core/src/session/turn_suspension.rs`
drops such accepted input during handoff. No durable controller record equivalent to COL-3 was found.

`core/src/agent/control/execution.rs` uses non-cloneable `AgentExecutionGuard` ownership;
`control/residency.rs` separates held reservations from committed slots; `control/legacy.rs` persists
lifecycle state before shutdown. These support the reservation/permit split, while Codex's shared
`AgentControl` supports one owner across multiple child turns. Normal V2 spawn/resume skips the
legacy completion watcher; terminal events notify the parent through the Session.

The latest inspected commit adds a disabled-by-default `send_message_to_user_async` feature for root
agents while excluding subagents. It is useful evidence for keeping user communication and agent
collaboration distinct; it does not implement child input control.

## Grok Build

Under `crates/codegen/xai-grok-tools/src/implementations/grok_build/`,
`task/active_message.rs` defines `Open`, `Claimed`, `Committed` and `Revoked`. Admission claims the
lease, performs a synchronous insertion, then commits; revocation succeeds only from `Open`.
Settlement that cannot prove admitted or rejected becomes `AdmissionUncertain`.

`task/coordinator/active_message.rs` retains in-flight leases and owned semaphore permits for both
active and spawn-ready queued work. Finalization waits until every admission settles.
`task/coordinator.rs` parks child terminal output behind that boundary, and an active child generation
rejects late completion for a reused identity. The shell-side `session/message_delivery.rs` rechecks
target, IDs, operation and coordinator-issued authorization before delivery.

This is the strongest model for `Main -> Releasing -> User/Frozen`, bounded reservations and stale
work. It is not durable handoff: committed insertion is persisted later. Background completion may
become internally attributed synthetic user-role content in
`xai-grok-sampling-types/src/conversation/chat_completions.rs`, which Plexmaton rejects.

## Kimi Code

`packages/agent-core-v2/test/agent/llmRequester/llmRequesterService.test.ts` proves that retry
notification occurs before resending a repaired projection and before each indefinite-retry backoff.
Its turn-machine regression discards an interrupted attempt stream when the service retries below
the turn, so only the later attempt's tool identity reaches completion.

This supports invalidating callbacks below a replaced attempt. It does not establish durable
Conversation control, writer exclusion or handoff recovery.

## DeepSeek Harness

Under `packages/core/agent-loop/tests/`, `cancel.spec.ts` covers parking queued work, clearing a
latched wake, reset after a cancelled turn and cancellation during error recovery.
`coverage-edges.spec.ts` ignores retry actions returned after abort and prevents recovery retries when
cancellation races the waterfall. `loop.spec.ts` abandons a live row when durable assistant
settlement is rejected and settles failed attempts before retry; `inbox.spec.ts` clears both pending
lists as durable cancellations.

`packages/core/agent-loop/src/agent.ts` resets the abort controller so an old wake latch becomes
stale; `assistant-stream.ts` emits an explicit abandoned terminal frame. These are useful Slice 3
cancellation cases, but their inbox/abort state does not replace Plexmaton's collaboration log or
permit-retained writer authority.

## Applied boundary

Plexmaton adapts Grok's explicit admission/finalization states without copying a cloneable delivery
wrapper or synthetic wake message. It adapts Codex's independent session and typed provenance while
making Handoff durable. Kimi and DeepSeek cases constrain stale callbacks and cancellation.

A resolved `TurnAdmission` remains inspectable data. The collaboration file/control owner reserves
the one Main execution slot before input and binds it to an exact admission. When controller routing
is attached, the runtime refuses direct user input under Main control and retains the resulting
permit through session/model work. Any unknown collaboration append freezes authority until reopen;
a live reservation or permit retains the physical file lock. The bounded asynchronous collaboration
owner, authenticated ingress and product bootstrap remain unproven.
