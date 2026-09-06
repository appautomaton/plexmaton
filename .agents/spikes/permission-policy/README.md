# Permission policy spike

| Field | Value |
| --- | --- |
| Status | Source comparison and finite experiments retained; production evidence in PER-1–PER-10 and PGR-1–PGR-5 |
| Read when | Extending approval scope, permission rules, durable grants or revocation |
| Question | How can routine work require fewer approvals while authority stays explicit, scoped and revocable? |
| Contract | [tool-admission](../../specs/tool-admission.md) APV-1–APV-6; [session-journal](../../specs/session-journal.md) JRN-1/JRN-2/JRN-7/JRN-8; [command-tool](../../specs/command-tool.md) CMD-1/CMD-2 |

## Corpus

Local source inspected 2026-09-05; reference suites/binaries were not run. Plexmaton base:
`b3333645c5ac69de803cb94ee2c8e5aa46796f1f`, branch `spike/permission-policy`.

| Directory | Revision | Source root for references below |
| --- | --- | --- |
| `claude-code` | `cc/2.1.88` mapped | `cc/2.1.88/src` |
| `codex` | `316795b3cf2a45e90d121d9f46499d4658b2645c` | `codex-rs` |
| `grok-build` | `72a61251fcffb464bcc687aeb5a998e5a98ec0c9` | `crates/codegen/xai-grok-workspace/src/permission` |
| `kimi-code` | `17dfd49768f753a4f0fe97d8e7d3317dab560575` | `packages/agent-core-v2/src/agent` |
| `dsh` | `47f943859bef60e4160492346772ded9b24f765a` | `packages/interaction` |
| `pi-arcweld/pi-mono` | `853a80d26c90a14c1886f0ebb8ffaae133ca2185` | `packages/coding-agent` |

Kimi's CLI/server manifests select `agent-core-v2`; the adjacent older engine is not the current
entrypoint. Claude Code is a mapped snapshot, not a full public Git source checkout.

## Comparison findings

| Source | Observed behavior and useful boundary |
| --- | --- |
| Claude | Tool-specific deny/ask checks precede broad automatic approvals; sandbox auto-allow is an explicit exception. `PermissionUpdate` separates rules, directories and modes. `utils/permissions/PermissionUpdate.ts:208` excludes session rules; only supported settings destinations persist |
| Codex | `core/src/tools/runtimes/unified_exec.rs:91` binds environment, executable/argv, cwd, TTY and sandbox/additional permissions. Session caching is distinct from durable prefix-rule amendments. Execution rules select Forbidden > Prompt > Allow (`execpolicy/src/policy.rs:402`) |
| Grok | Grants/denies persist at the discovered project root (`state.rs:124`). Remembered command labels must re-derive from the parsed command. Proposed rows are evaluated through real policy before display (`manager/bash_grants.rs:96,147`): truthful scope must also be effective |
| Kimi v2 | First-result chain puts configured deny before session grants, then configured ask/allow (`permissionPolicy/permissionPolicyService.ts:46`). Session approvals fold from persisted operations; supplied rules are transient (`permissionRules/permissionRulesOps.ts:46,57`) |
| DSH | Journal-folded approval policy and sandbox mode are independent knobs. Presets reject an unconfined shell (`permission-presets/src/index.ts:192`). Asked/decided events bracket a typed outcome (`user-approval/src/index.ts:257`) |
| pi | No core permission popups (`README.md:503`); `beforeToolCall` delegates to extensions (`src/core/agent-session.ts:490`). The regex confirmation example and sandbox execution extension are separate mechanisms |

Three failure-path lessons:

- Codex persists a prefix amendment before publishing live policy (`core/src/exec_policy.rs:447`); its
  session approval cache starts empty (`core/src/session/session.rs:1411`). Policy lifetime needs an
  explicit contract, independent of the approval label.
- Grok documents a read-modify-write race in merge-on-write; reset uses replacement
  (`state.rs:380–412`). Unioning stale allow sets is insufficient for revocation. A revisioned
  writer or equivalent transaction needs a concurrency proof.
- Kimi's absent approval service returns approved (`toolApproval/toolApprovalService.ts:132`); DSH returns
  unavailable. Plexmaton retains APV-4/APV-6. Codex `never`, Claude `dontAsk`, and DSH `never`
  reject unresolved asks: no prompts and unrestricted execution are different policies.

Follow-ups: [trust/modes](./trust-and-modes.md), [optional sandbox](./sandbox-boundary.md),
[integration](./integration.md), [store experiment](./store-experiment.md),
[approval flow](./approval-flow.md).

## Finite model semantics

P1–P6 name the finite experiment’s oracle. Production contracts and UI grammar live in the specs:

**P1 — Conversation replay cannot restore authority.** Coding-session grants belong to runtime
memory; project grants come from the project store. Journal approvals are history, not reusable
grants. Revocation names a stable grant ID; regrant uses a new ID. Rejected: durable
conversation-scoped grants, which make temporary consent survive resume and duplicate persistence.

**P2 — Scope names identity and lifetime.** Session means the ongoing coding period; Conversation
means the history selected by `/new` or resume. Temporary grants bind the coding session and root,
survive conversation changes, and expire when Plexmaton exits. Project grants persist for the
physical checkout under `PLEXMATON_HOME`. The finite model does not exercise production owner handoff or physical identity.

**P3 — Default-mode precedence is deny > explicit ask > allow.** Distinguish a default approval
requirement from a user's rule demanding a prompt every time. Remembering may satisfy the former;
one-call approval satisfies either. Offer a remembered scope only when it would actually suppress
the prompt. Rejected: session memory overriding explicit ask rules; Kimi makes that different choice.

**P4 — Approval pins a call.** Bind answers and permits to call/session/head identity, canonical
subject, definition/environment identity and policy revision. Cancellation, changed arguments or
stale policy invalidate the answer. Recheck at dispatch; consume one-call permission once, using
the existing pending owner (APV-4/APV-6).

**P5 — Effects follow accepted decisions and required writes.** Memory changes require the current
policy revision. Persistent changes and required call audit/lifecycle writes precede dependent
publication or execution (JRN-7); failed or uncertain writes block work. Session grants need no
journal grant record. The finite model assumes atomic acceptance; production storage evidence is in PGR-1–PGR-5.

**P6 — Scope describes admitted operations.** Subjects come from admission; broad edits exclude
control/config paths and cannot relax APV-3. The finite model matches full scripts; production
exact and prefix scopes are owned by [PER-4/PER-10](../../specs/permission-policy.md).
The shell is unconfined (CMD-2); a workspace-write preset needs executor containment. Matching
commands cannot establish their actual effects.

## Finite experiment

[Model](./policy-model.rs) and [tests](./policy-tests.rs): 15 P1–P6 cases cover precedence, revocation,
scope, call identity and coding-session expiry. The trace is in memory, not the conversation JSONL.
All pass with warnings rejected; conversation changes retain grants while restart and root changes
refuse temporary reuse. Production handoff evidence is in PER-5.

The experiment uses finite identities and at most 32 policy records. It does not exercise storage,
OS/path checks, async handoff, shell parsing, MCP or UI. Production evidence lives in the permission
specs; no live model was used for this experiment.

Run from the worktree; build artifacts stay outside the source tree:

```sh
spike_build_dir=$(mktemp -d /tmp/plexmaton-permission.XXXXXX)
trap 'rm -rf "$spike_build_dir"' EXIT
rustc --edition 2024 --test -D warnings .agents/spikes/permission-policy/policy-model.rs \
  -o "$spike_build_dir/policy-model-tests"
"$spike_build_dir/policy-model-tests"
```

## Production work

[Phase 02](../../phases/phase-02-durable-sessions.md) records the completed local stage and gates.
[Permission policy](../../specs/permission-policy.md) owns the production invariants;
the finite model is comparison evidence. OS containment and MCP remain separate work.
