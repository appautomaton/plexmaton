# Where tool permission is actually enforced

| Field | Value |
| --- | --- |
| Read when | Changing who may run a tool, what a grant covers, or whether a call is asked about at all |
| Status | Draft. Source claims verified in this checkout; no measurement taken, no decision made |
| Basis | [The spike](./README.md) pinned the comparison; [sandbox-boundary](./sandbox-boundary.md) pinned the containment options. This is the third angle: our own enforcement |

The spike asked how routine work can need fewer approvals while authority stays explicit. It
compared other harnesses and modelled the policy. It did not audit where this harness decides. That
is the gap, and the answer is not where the corpus says it is.

## The rule the code states, and the place it does not hold

`crates/plexmaton-agent/src/tools.rs:27` is the governing sentence:

> Which tool. Routing, never a trust decision: what a call is allowed to do is its declared effect,
> and a name is a label the model chose.

True of `ApprovalPolicy` and `PermissionSnapshot`. Not true of the highest-stakes boundary in the
product — whether a delegated child may write files or run commands at all:

```rust
// crates/plexmaton-runtime/src/native.rs:92-104
fn allows(self, name: &str) -> bool {
    match self {
        Self::Full | Self::MainCollaboration => true,
        Self::ReadOnly => matches!(name, READ_TOOL_NAME | SEARCH_TOOL_NAME),
        Self::ChildCollaboration => matches!(
            name, READ_TOOL_NAME | SEARCH_TOOL_NAME | crate::SEND_MAIL_TOOL_NAME),
    }
}
```

gating admission before any capability is read:

```rust
// crates/plexmaton-runtime/src/native.rs:304-306
let name = request.requested().name.as_str();
if !self.profile.allows(name) { return ... request.refuse(AdmissionRefusal::UnknownTool) ... }
```

CHB-1 states the outcome — a child's set cannot be widened, and execution rechecks the profile — and
never states the mechanism. `NativeToolProfile` appears nowhere in `.agents/`. Anyone auditing this
system the way the spike did, by reading the permission specs and `plexmaton-agent`, concludes the
engine is capability-driven and is right about the engine and wrong about what protects a child.

Two more name-keyed gates sit beside it, also unnamed in the corpus: `is_collaboration_tool_name`
(`crates/plexmaton-runtime/src/collaboration_tools.rs`) and the skill dispatcher's own name check.
The file-tool and command catalogs route by name and then decide by declared effect, which is the
honest case the sentence describes; these three decide.

## What the user cannot say

`ApprovalDecision` (`crates/plexmaton-core/src/permissions.rs`) is `AllowOnce`, `AllowAndRemember`,
`Deny`. There is no `DenyAndRemember` — verified absent. Deny wins every precedence race in the
system and is the only rule shape the product's own interface cannot produce; it exists solely for
someone who hand-edits configuration. A user who has just refused something cannot say "and stop
asking", and there is no view that shows the rules they wrote by hand once those are loaded.

## The engine that never runs

`ApprovalPolicy::new(approval_required, forbidden)` is the only constructor that gives the
capability tiers content. Both non-test-file call sites — `admission.rs:497` and `turn/mod.rs:2169`
— fall after `#[cfg(test)]`. Production builds `ApprovalPolicy::default()`, whose `approval_required`
and `forbidden` are empty and whose only live field is `fallback_ask`. The capability-Forbidden and
capability-Ask tiers are documented, unit-tested, and unreachable from a running Plexmaton; the job
they describe is done instead by the rule-based Deny/Ask path. Two mechanisms, one job, one of them
never invoked.

## What the comparison adds that this spike did not already hold

[The spike's table](./README.md) recorded Claude's sandbox auto-allow as "an explicit exception".
Re-reading the same corpus against the composition question shows it is not an exception anywhere —
it is the shared rule, named in code in three independent implementations:

| Source | The named site |
| --- | --- |
| Claude | `sandbox.autoAllowBashIfSandboxed`, default **true**; a sandboxed call "skips the ask rule" |
| grok-build | `should_auto_allow_bash() = AUTO_ALLOW_BASH && is_active()`, consumed to short-circuit to Allow |
| Codex | under the default approval policy, a restricted sandbox returns `Decision::Allow` rather than `Prompt`, with the reason written beside it: let the sandbox enforce without a user prompt |

And the converse, which is the part that matters for us: the two sources with no containment —
kimi-code and this harness — are the two where turning the questions off leaves nothing underneath.
Codex says so directly at its own `Never` policy: it allows the command *relying on the sandbox for
protection*. We have no such sentence to write, because
[the shell is unconfined](./sandbox-boundary.md) and the file tools' `openat`/`NOFOLLOW` pinning
covers only themselves.

## Already answered; do not re-derive

- Containment mechanisms, their platform gaps, the candidate insertion point and the deprecation of
  `sandbox-exec`: [sandbox-boundary](./sandbox-boundary.md), with a macOS probe beside it.
- A delegated child sharing the root's exact Session owner: recorded as the design in
  [permission-state](./permission-state.md) and CHB-1/CHB-2, not a defect to rediscover.
- Cross-harness precedence, lifetime and failure-path lessons: [the spike](./README.md).

Three agent sweeps were spent re-deriving most of that list before this file was opened. The corpus
held it.

## Signal this still lacks

- **Barely a baseline.** One real session read from its journal — 17 turns, 26 tool-bearing records
  — carries exactly one approval cycle (`require_approval` → `awaiting_approval` → `approval` with
  its id, four `append_entry` records). Counted by reading the JSONL, so it is reproducible, and it
  is one point of the wrong shape: that session was conversation and reading, and approval is
  triggered by writes and commands. A baseline worth optimising against needs sessions that do the
  work approval exists for, and nothing yet records the rate as a fact rather than an archaeology.
- ~~No cost.~~ Measured on macOS: a constant ~6 ms per invocation, under one percent of a
  command taking a second. [sandbox-boundary](./sandbox-boundary.md) holds the table and the
  resolved-path constraint that running it exposed. Linux and Windows remain unmeasured.
- **No contradiction inventory.** The three findings above were found by reading; whether more spec
  sentences describe mechanisms the code does not use is unknown.

## What to decide next

[Next decisions](./next-decisions.md) holds the criteria a design is judged on, the hypothesis on
the table, and the four forks — so this file can stay what is true, and that one can stay what is
open.
