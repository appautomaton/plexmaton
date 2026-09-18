# Spec — Permission policy

| Field | Value |
| --- | --- |
| Status | Implemented locally; component, executable and three-width evidence reviewed |
| Owns | Reusable permission scope, precedence and coding Session/project authority |
| Proven by | Reducer, runtime, store, configuration, parser, TUI and executable proofs below |
| Depends on | [tool-admission](./tool-admission.md) APV-1–APV-6; [session-journal](./session-journal.md) JRN-7 |

## Invariants

**PER-1 — Session authority outlives a Conversation.** One explicit owner retains temporary
grants for the coding Session and physical workspace. Root and delegated child runtimes share that
owner; CHB-1 still bounds which tools a child can expose. Opening a new or saved conversation, head
selection and compaction do not reset it. Exit/restart creates fresh memory-only authority;
Conversation JSONL never restores a grant. Immutable snapshots are projections of this owner.

**PER-2 — Explicit rules precede memory.** Deny precedes Ask, then Allow or a remembered grant,
then capability fallback. Native read/search fallback allows unless an exact `native_inspection`
rule asks or denies them; native writes and process spawning ask. A reusable offer is valid only
when its addition authorizes the whole admitted operation; explicit Ask/Deny cannot be bypassed by
remembering it. APV-3 still applies.

**PER-3 — A preset names definitions.** The native inspection preset pins read and search
identities/revisions; the native file-change preset pins create and edit identities/revisions and
excludes agent-control/configuration paths and Git metadata. Neither a capability nor a similar
display name makes another tool a member. Typed permission subjects are issued by the trusted
catalog through its APV-1 ticket, never reconstructed from approval detail.

**PER-4 — Reuse retains scope and revision.** Exact command grants match exact source, definition,
revision and captured execution context, including the physical working directory, shell and
filtered environment. A changed context asks again. Mutations echo the reviewed owner/revision;
stale, foreign, capacity-exhausted and ineffective requests leave the prior state intact.

**PER-5 — A decision has a commit boundary.** The current pending Conversation/head/turn/call and
admitted arguments are checked before applying a user decision. JRN-7 acknowledges its audit
before dependent execution. Dispatch rechecks current policy authority; revoked or stale reusable
permissions cannot start an effect. Pending decisions remain visible until producer confirmation.
An interrupt or shutdown accepted before dispatch prevents a worker from starting, even while the
preceding audit is blocked. A failed audit does not implicitly revoke an already applied grant.

**PER-6 — Persistent project grants are personal policy.** Project grants belong under the
Plexmaton user root and bind physical project identity. They are separate from Conversation JSONL
and project-controlled configuration. Corrupt/torn records, stale revisions and failed/unknown
writes cannot authorize execution. A durable project grant is not rolled back if the subsequent
Conversation audit fails; the runtime reports the saved grant and that the tool did not run.
[Project storage](./project-permissions.md) PGR-1–PGR-5 owns its transaction protocol.
Cancellation also retains a completed Project worker receipt when another Conversation audit
fails before that worker is consumed. Feedback follows the affected call, changes no semantic
copy or item revision, and survives terminal release. A required invalid source returns typed
`Unavailable`, including for Allow once.
PER-8 defines configured authority and personal trust.

**PER-7 — Controls change grants, not parallel settings.** The native file-change setting derives
from its named grant; disabling it revokes that identity without promising that other sources
will stop allowing matching operations. Review and Back apply nothing, submission waits for
producer confirmation, and stale controls return the current view. Enabling a Session setting
before the first turn creates no Conversation JSONL. A completed change re-evaluates covered
waiting calls through PER-5. Controls sit where their lifetime is typed: the Session's grants
and the native preset are `/permissions` rows in the composer menu (CMC-3); Project grants and
configuration trust are the Drawer's Permissions page. A grant is offered in one place only, and
one acknowledged view lands in every open place.

**PER-8 — Configuration trust names exact bytes.** User rules are a startup snapshot. Both user
and project readers use one strict bounded declaration grammar compiled by the trusted catalog.
Project Ask/Deny apply without trust; Project Allow requires a personal trust record matching the
complete current file's SHA-256 fingerprint. Every project transaction refreshes the file before
mutation or execution. Changed bytes invalidate the reviewed revision; invalid sources return
Unavailable. Model and skill loading confer no trust. Trust review exposes every configured scope
in a scrollable view; confirmation echoes the fingerprint and current permission revision.

**PER-9 — Decision history is evidence, never authority.** Each admitted policy decision and
completed user choice records its call, definition/revision, observed Session/project revision,
command-context fingerprint and winning rule or grant scope before dependent effects. A choice
also retains its approval identity, reason and remembered grant identity. Replay validates call
ownership and lifecycle, adds no model content, and installs no permissions. This records the
reducer's decision; a later dispatch refusal remains a separate typed tool outcome. Configuration
fingerprints and context hashes retain provenance without copying whole policies or environment
values. At most one decision per queued/awaiting boundary is accepted.

**PER-10 — Prefix authority covers complete literal operations.** A maintained parser at command
admission lowers only bounded complete literal sequences, preserving argv boundaries, source spans
and PER-4 context. Prefix Allow/grants must cover every command; prefix Ask/Deny applies to any
covered command before exact or reusable Allow. Unsupported syntax never matches a prefix.
Suggestions name meaningful floors (`ls`, `git fetch`), never discard leading options or wrappers,
and are offered only when effective for the whole call. Exact fallback explains why no prefix is
offered. This classifies syntax and scope, not executable effects or OS confinement (CMD-2).
A scope the user cannot read cannot be granted: a clipped one disables remembered grants for both
keyboard and pointer while Back stays usable, and a scope must reach a successfully delivered frame
before a separate press can confirm it. Which of the card's parts gives way first is ui-ux's.

## Evidence

[Named proofs](../evidence/permission-policy.md), one row an invariant.

## Bounds and ownership

The Session holds at most 128 user rules and 128 temporary grants; the current project configuration
has its own 128-rule bound. Command subjects retain at most 24 KiB and native paths at most 4 KiB.
The runtime shares the Session owner explicitly across Conversation replacement. A poisoned owner
fails closed. No mutable process global or journal replay path creates authority.
PGR-1–PGR-5 defines project storage.

## Configuration

Both user `config.toml` under PRV-6's root and project `.plexmaton/config.toml` accept:

```toml
[[permissions.rules]]
action = "allow"
match = { kind = "native_file_changes" }

[[permissions.rules]]
action = "ask"
match = { kind = "native_inspection" }

[[permissions.rules]]
action = "ask"
match = { kind = "exact_command", source = "git fetch origin" }

[[permissions.rules]]
action = "allow"
match = { kind = "command_prefix", arguments = ["git", "fetch"] }
```

Actions are `allow`, `ask`, or `deny`. Match kinds name the native inspection or file-change
preset, exact shell source or a literal argv prefix. Unknown fields, more than 128 rules, and
commands outside CMD-1 bounds refuse the complete source. The catalog compiles definitions and
command context; configuration cannot supply either.
Project files retain SKL-1's 64 KiB complete-read bound. User rules load once per coding Session;
restart reloads them. Project rules refresh under the personal store lock before controls and dispatch.

The Drawer's Permissions page reviews and revokes Session/Project grants, controls the Session native preset, and
reviews project rules. In project review, Up/Down or the wheel scroll complete escaped scopes;
Enter or the visible Continue action opens activation confirmation. Back is selected initially.
Esc returns one page. Activation applies only to the reviewed SHA-256 fingerprint and permission
revision. Withdrawal remains available if the project file changed or disappeared. Trust and grants
live outside Conversation JSONL; no authority is recovered from a historical decision.

## Literal command grammar

The command worker parses with tree-sitter Bash and lowers a POSIX subset for `/bin/sh -c`:
complete simple literal commands joined by `;`, newline, `&&` or `||`. Quotes, concatenation and
literal escapes preserve argument boundaries, including empty arguments. Variable/command
expansion, globbing, redirects, pipelines, background jobs, control flow and alias manipulation
have no reusable parse. A backslash-newline gap between word nodes is rejected because the parser
can split argv where `/bin/sh` joins it. Quoted data is decoded from its complete source span.

One Allow rule or grant must cover the entire sequence; separate partial grants are not combined.
A restrictive prefix applies to any literal sibling in that complete parse. These matchers do not
infer commands inside unsupported dynamic syntax. Exact permissions still match exact source and
PER-4 context regardless of parser availability. A wrapper is never peeled and a leading option
never discarded: `git -C ../other fetch` cannot match `["git", "fetch"]`.

Suggestions are deliberately small: `ls`, or `git` followed immediately by `fetch`, `status`,
`diff`, `log` or `show`. They apply only to one simple command. Other commands and compounds offer
exact reuse with an explanatory note. Explicit configuration can choose a narrower literal argv
prefix, including a quoted argument such as `["git", "fetch", "team origin"]`; the UI currently
chooses lifetime for the one backend-issued offer, without a prefix editor.

Source, parse, lowered tree and persisted prefix are each bounded separately rather than from a
shared pool, and `plexmaton-command/src/{admission,prefix}.rs` hold the values. The parse carries a
wall-clock deadline as well, which is the non-obvious one: a node or depth bound caps how large a
parse becomes and says nothing about how long it takes. That deadline and owner cancellation are
both checked at the pinned engine's progress callbacks, and a knowable callback cadence is part of
why the engine is pinned. Parser and tree are local to one retained admission worker, with no cache
and no detached work, so nothing accumulates across admissions.

Capacity, parser failure and unsupported syntax produce typed exact fallback. No shell execution,
expansion, PATH lookup or external process derives permission tokens. Differential tests use only a
fixed `printf` and quoted generated data as the `/bin/sh` oracle.
