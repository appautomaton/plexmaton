# Approval flow audit at b333364

| Field | Value |
| --- | --- |
| Read when | Comparing the pinned approval defects with PER-5’s regression evidence |
| Status | Pinned source audit retained; fixes and implemented flow owned by PER-5 |
| Base | `b333364` in the permission-policy worktree; 2026-09-05 local date |
| Contract | [tool admission](../../specs/tool-admission.md) APV-1–APV-6; [attention](../../specs/attention.md) ATT-1–ATT-3; [routing](../../specs/interaction-routing.md) INV-6/INV-10 |

## Audit boundary

This report and diagnostic describe `b333364`. [PER-5](../../specs/permission-policy.md) owns the
implemented approval journey and regression tests for the findings below. The runtime still has
one live agent; synthetic background interactions constrain Phase 03 without claiming delegation.

## Pinned path

```text
model tool call
  -> catalog admission
  -> ApprovalPolicy::decide
       Allow -> RunTool
       Forbidden -> refused result
       RequireApproval -> batch slot owns PendingApproval
  -> journaled AttentionRequested + AwaitingApproval tool status
  -> runtime append acknowledgement -> TUI projection
  -> ApprovalSubmission { to: AgentId, approval_id, AllowOnce | Deny }
  -> CLI AddressedInput -> LiveRuntime -> owning agent's Batch::resolve_approval
  -> journaled resolution/status -> append acknowledgement
       AllowOnce -> exact admitted call reaches executor checks
       Deny -> exact slot receives denied result
```

APV-5/APV-6 govern siblings and cancellation. A stale answer produces
`ApprovalDecisionRefusal::NotPending`.

### Source owners

All paths below are under `crates/`; function names identify the pinned implementation.

| Responsibility | Source |
| --- | --- |
| Capability decision | `plexmaton-agent/src/admission.rs`: `ApprovalPolicy::decide` |
| Pending call and answer | `plexmaton-agent/src/tools.rs`: `PendingApproval`, `Batch::resolve_approval`; `turn/batch.rs`: `admission_resolved`, `approval_decided` |
| Commit before execution | `plexmaton-runtime/src/runtime/transition.rs`: `begin_transition`, `apply_ready_reaction`; `runtime/tools.rs`: `start_execution` |
| Addressed input and feedback | `plexmaton-cli/src/main.rs`: `route_approval`, `dispatch_live`, `restore_undelivered` |
| Request projection and sequencing | `plexmaton-tui/src/state/ingest.rs`; `state/asking.rs`: `attention_listed`, `open_next_primary_approval`, `attend` |
| Presentation, geometry and input | `state/approval.rs`, `state/inspect.rs`, `content_approval.rs`, `render/mod.rs`, `layout/registration.rs`, `router.rs`, `workspace/approval_pointer.rs` under `plexmaton-tui/src` |

The audit isolated cross-conversation presentation identity and fixed two-choice assumptions.

## Pinned user journey

Primary approvals open above the composer with Deny selected. ATT-1 owns sequencing and Esc;
arrows/Enter or click answer, and Ctrl-O expands detail. Submission leaves the card pending.

Background requests share the internal queue and appear in Attention. Visiting opens the inspector
and modal approval; Esc dismisses it. `LiveRuntime::submit_selected` accepts only its one agent:
the background path is synthetic reference behavior.

## Reproduced gaps

[Diagnostic source](./approval-flow-probe.rs) drives the real `Workspace`, router, reducer and
Ratatui `TestBackend` with synthetic events and terminal input. It asserts observations of the
pinned implementation, not desired regression behavior. No provider or executor runs.

1. **The visible list and keyboard cursor disagree.** A pending primary request precedes a worker
   request internally. `attention_listed` shows only the worker, while movement/acknowledgement use
   the unfiltered queue. Enter visits the hidden primary. Rendering also derives the caret from
   that unfiltered cursor.
2. **One open card can displace another conversation's card.** Visiting the worker overwrites the
   single `ApprovalSurface.open`. Dismissing it does not reopen the pending primary request. That
   request remains unlisted and without an approval surface until another qualifying transition.
3. **Submission has no presentation state.** Repeated Enter before producer resolution yields the
   same submission twice. APV-4 prevents a second execution, but there is no submitting feedback.
   Source audit additionally finds `restore_undelivered` ignores `report.unresolved_approvals`, so
   the typed refusal has no normal UI feedback path. The probe does not execute the CLI report path.

All three observations reproduce at 120x24, 95x24 and 60x24. The pinned tests did not compose the
conflicting journeys. Frames inspected from this run:

| Observation | Wide | Medium | Narrow |
| --- | --- | --- | --- |
| Hidden cursor | [frame](./frames/attention-cursor-wide.txt) | [frame](./frames/attention-cursor-medium.txt) | [frame](./frames/attention-cursor-narrow.txt) |
| Primary displaced | [frame](./frames/displaced-primary-wide.txt) | [frame](./frames/displaced-primary-medium.txt) | [frame](./frames/displaced-primary-narrow.txt) |

To reproduce on the pinned base, copy the diagnostic to a new temporary `.rs` example under
`crates/plexmaton-tui/examples/`, then run
`cargo run -p plexmaton-tui --offline --locked --example <stem> -- <frame-directory>`.
Remove only that temporary example afterward; no production manifest edit is needed.

## Production result

PER-5 owns typed scope review, submission, refusal feedback, stable Attention navigation and
primary/background card restoration. Its evidence includes the real reducer/router and three-width
frames. PER-6 adds persistent-result feedback; PER-10 adds prefix scopes. The pinned probe asserts
the original defects and is not a current-worktree regression test.

## Validation and limits

This audit ran 18 existing tests: six agent lifecycle/admission/recovery tests, ten TUI approval
tests, the runtime tool-acknowledgement barrier and the CLI addressed-input mapping. All passed.
The diagnostic reproduced the three observations at every width on the pinned base. The
[view experiment](./approval-view.md) retains the user’s visual review; production evidence lives
in PER-5. Live background-agent behavior remains unvalidated.
