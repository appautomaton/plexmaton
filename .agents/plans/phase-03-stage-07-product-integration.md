# Phase 03, stage 7 — Product integration

| Field | Value |
| --- | --- |
| Phase | [Phase 03](../phases/phase-03-collaboration.md) |
| Contract | Roadmap locked typed mail and single controller; COL-1–COL-5, CIN-1–CIN-4, CHB-1–CHB-3, SCH-1–SCH-5, CMP-1 and UI/UX control/mail/Attention sections |
| Status | Slices 1–5 complete; Slice 6 control UI built and the roster/Attention redesign reviewed and landed, production activation blocked |

## Outcome

One authenticated Main composition root owns collaboration mutations, child construction, runner
updates and teardown. Native delegation/mail/Handoff operations derive authorship from that owner
capability, preserve exact input across cancellation and refuse unsupported child providers before
durable creation. Product read models join canonical collaboration and selected recipient-session facts
without inventing model inclusion or Attention. Native UI fixtures can validate presentation and
input routing before provider activation; only a production provider that safely represents typed
collaboration context can close the end-to-end integration gate.

## Slices

1. **Mutation ownership (complete).** Retain accepted regular admission and Handoff preflight/results across
   caller cancellation, expose exact settlement, and include it in joined shutdown. Disposable
   projection/control queries may be recomputed; they never mutate canonical state.
2. **Typed native grammar (complete).** Define bounded delegation, mail, task-update and Handoff requests whose
   arguments contain no author, endpoint or durable retry identity. Main receives all four tools;
   a child receives only typed `send_mail`, without write/shell/delegation/control capabilities.
3. **Authenticated owner ingress (complete).** Bind parsed operations to a bounded command lane. Derive Main
   and child identity from owner-held capabilities, assign retry identities inside the owner, and
   keep production provider refusal ahead of delegation creation.
4. **Root composition service (complete backend boundary).** Authenticate Main from the exact tool
   ingress and unique instance token carried by its user-owned root runtime. Preflight deterministic provider, credential,
   child-profile and runner-capacity failures before identity allocation. Admit canonical creation
   before creating or resuming the child journal, bind the exact child capability, register one
   bounded runner and route content-free wake, Stop and Handoff. Cancellation retains the exact
   command; explicit resume reuses canonical provenance without automatically waking a child.
   A post-canonical failure returns the exact target for explicit recovery without another
   creation or automatic wake; post-build registration failure shuts down its caller-owned runtime
   before returning. Process death during the two-file provisioning window remains unproven.
5. **Session-aware read model (complete backend boundary).** Join CMP-1 mail with exact-runtime-sealed selected recipient-session CIN-2 facts before
   reporting queued versus included. Reconcile the locked Attention source before projecting
   background requests; do not infer them from mail or task state. The mail join is implemented;
   an active owned child serves it through a separate bounded inspection lane. Attention still has
   no canonical action-required source and therefore remains unprojected.
6. **Control UI, provider activation and native frames (in progress).** Complete these parts in order:
   - **Control presentation and routing (implemented; visual acceptance pending).**
     [CCV-1–CCV-4](../specs/child-control-view.md) now supply typed control facts to the real Workspace
     renderer. Ten focused tests cover the control, input and preservation boundaries, including
     hidden selection/drag settlement and short-height composer geometry. Kitty readbacks cover
     four states at 120, 88 and 60 columns plus explicit entered-child input at each width; see the
     [native record](../spikes/kitty-native-preview/README.md#evidence-and-limits).
     Normal terminal restoration was checked and the final targeted Sol review found no blocker. This part grants
     no runtime authority and does not prove production Stop, Handoff, mail or Attention.
   - **Production activation.** Enable execution only after a provider has a native typed collaboration
     field. Connect inspection, Stop, Handoff, composer control and passive projections to the real
     root service, then verify the production journey and native frames at three widths.
   Review any interaction contract change with the user after they have seen its native frame.

## Order and why

Mutation retention closes the authority hole before native tools can reach it. Typed schemas fix
what authenticated ingress accepts; the root service then supplies real facts for read-side
composition. Native control UI now uses the proven backend vocabulary while its fixture remains
explicitly separate from production authority. Provider activation still gates the complete journey.

## Deliberately not in this plan

No recursive delegation, child write/shell/delegation capabilities, automatic control transfer,
synthetic user turns, automatic post-restart wake, commit, PR or merge. The Attention and roster
layout redesign has since been reviewed and built: the user reviewed a computed mockup of every
state at four widths and directed the change, so it is no longer the unreviewed redesign this
excluded. The [Kitty preview](../spikes/kitty-native-preview/README.md)
owns the native demonstration method and its limits.
