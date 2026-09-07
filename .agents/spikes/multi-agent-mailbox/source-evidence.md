# Multi-agent source evidence

| Field | Value |
| --- | --- |
| Read when | Checking the mailbox spike comparison against source |
| Scope | Source inspection only; no live endpoint or production durability proof |
| Corpus | Revisions and inspection limits in [the spike](./README.md#corpus) |

Paths and line numbers refer to those pinned revisions. Codex paths start at `codex-rs/`;
Grok paths use the prefix stated below. Inspect pinned content with `git show <revision>:<path>`
when a local reference checkout has advanced.

## Codex

`protocol/src/protocol.rs:862` constructs `ResponseItem::AgentMessage`; the serde variant is in
`protocol/src/models.rs:973`. Contextual fragment helpers construct the text, but their assistant
role is not the final V2 atom. `core/src/session/rollout_reconstruction.rs:347` reconstructs typed
mail. `codex-api/src/common.rs:274,327` serializes response items on HTTP and WebSocket transports.
This proves the source representation, not live endpoint acceptance or other provider dialects.

`core/src/session/handlers.rs:79` enqueues into the process-local queue in
`core/src/session/input_queue.rs:122`; `core/src/session/mod.rs:3549` records included mail later.
`core/src/session/turn_suspension.rs:96` explicitly drops process-local accepted input on handoff.
There is no explicit mailbox capacity bound. `core/src/tools/handlers/multi_agents_v2/wait.rs:67`
waits for caller activity and returns no child output.

Normal V2 spawn and resume skip the legacy completion watcher
(`core/src/agent/control/spawn.rs:769,1244`); terminal events notify the parent through
`core/src/session/mod.rs:2084`. The legacy watcher discards send errors, but its existence does
not prove duplicate completion on the normal V2 path.

## Grok Build

Paths here are under `crates/codegen/`. In `xai-grok-tools/src/implementations/grok_build/`,
`task/active_message.rs:150-233` defines `Open`, `Claimed`, `Committed`, `Revoked`. Admission CASes
Open to Claimed, runs a synchronous insertion closure, then stores Committed. Revocation succeeds
only from Open; settlement checks the lease rather than trusting an affirmative reply. Unprovable
settlement becomes `AdmissionUncertain`. `task/coordinator/active_message.rs:58-155` finalizes only
after in-flight admissions settle; uncertainty forces a failed/cancelled child result
(`task/coordinator.rs:876`). This bounds admission races, not crash recovery.

`xai-grok-shell/src/session/acp_session_impl/prompt_queue.rs:102` inserts into `pending_inputs`;
`parent_message.rs:289-362` in that directory persists at a later safe point. Committed is therefore
not a disk acknowledgement. `xai-grok-shell/src/session/message_delivery.rs:157-206` checks target,
content and IDs, operation, and authorization against the coordinator-issued grant. The coordinator
also checks ownership, active state, workflow exclusion and cancellation before delivery
(`xai-grok-tools/src/implementations/grok_build/task/coordinator/active_message.rs:542`).

`xai-grok-shell/src/agent/subagent/spawn.rs:309-401` requires background eligibility and a surfaceable
result, then checks cancellation, feature enablement, prior waiter delivery, explicit kill, active
goal loop and parent channel liveness. These suppress unwanted, duplicate or undeliverable wakes.
The prompt path at line 441 retains `SubagentCompleted` internally; provider conversion in
`xai-grok-sampling-types/src/conversation/chat_completions.rs:67` produces user-role content.
Foreground waiting buys direct output in the originating tool call, at the cost of holding that
call; synthetic wake reuses prompt scheduling but loses the distinct actor role on the wire.

Coordinator ingress/events and session commands use unbounded channels. Independent limits are
not all fail-fast: `task/admission.rs:6-30` queues at the default 32-child concurrency limit.
`task/coordinator_state.rs:17-55` caps completed retention at 1024 and active admissions at 64/8;
message text is capped at 32 KiB. Model completion output is uncapped in `task/mod.rs:601-615`.
These task paths are under `xai-grok-tools/src/implementations/grok_build/`.
