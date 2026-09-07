# AGENTS.md — Plexmaton

Plexmaton is an early-stage Rust agentic harness with a Ratatui UI, explicit session/context state, multiple provider dialects, and durable tools and sessions. Asynchronous multi-agent collaboration is planned. Preserve that direction without speculative compatibility or abstraction.

Inherit `../AGENTS.md` relative to the primary checkout; the stricter applicable rule wins. This is the only project document loaded every turn. Task-specific rules load through the table below; [`.agents/README.md`](./.agents/README.md) owns the corpus structure.

## Context routing

Read only what the active task needs. Do not preload all phases, reference repositories, or broad source trees.

| When you are about to… | Read |
| --- | --- |
| start or resume work on an open phase | its file in [`phases/`](./.agents/phases/); when none is open, read the roadmap before creating one |
| ask what the product is, or plan beyond this phase | [`roadmap.md`](./.agents/roadmap.md) |
| design, implement, or review code | [`standards/architecture.md`](./.agents/standards/architecture.md) |
| implement or review a named mechanism | `.agents/specs/<mechanism>.md` — cite its invariant in the test that proves it |
| start a stage big enough to have an order | `.agents/plans/`, shaped as [`.agents/README.md`](./.agents/README.md) §plans says — slice it before writing code; delete the plan when consumed |
| change interaction, layout, focus, attention, or copy behaviour | the relevant sections of [`ui-ux.md`](./.agents/ui-ux.md) |
| write, change, or delete a test | [`standards/testing.md`](./.agents/standards/testing.md) |
| organize a module, or add/upgrade/remove a dependency | [`standards/rust.md`](./.agents/standards/rust.md) |
| start or resume development; create, merge, or retire a branch/worktree | [`standards/git-workflow.md`](./.agents/standards/git-workflow.md) |
| select, run, or fix gates; set up clone hooks or worktree builds | [`standards/quality-gates.md`](./.agents/standards/quality-gates.md) |
| write or reorganize a document | [`.agents/README.md`](./.agents/README.md) |
| compare against a third-party implementation | `.references/`, which is gitignored and absent in a fresh clone. Treat its absence as normal |

A change is not done until the documents it invalidates are rewritten in the same change:

| When you change… | Update |
| --- | --- |
| a mechanism's behaviour | its spec: the invariant, and the evidence table |
| a rule the contract states | [`ui-ux.md`](./.agents/ui-ux.md), and only with the user's agreement: the contract is theirs, and an agent proposes a change with a rendered frame they have seen |
| a key binding, or what the executable does or how it runs | the spec's grammar, `scripts/smoke-tui.py` where it drives the key, and the root `README.md` |
| a phase's or a plan's status | the roadmap's row for that phase, the phase file's status, the plan's status; three cells, one fact each |
| a dependency | [`standards/rust.md`](./.agents/standards/rust.md) §audited foundation |

**Cite, don't restate.** Reference invariant IDs such as `INS-5` instead of making another copy of the rule.

## Working discipline

The primary checkout stays on `main`; all development, including docs and CI, happens on a task
branch in `.worktrees/<name>/` under the primary checkout, unless the user explicitly requests an
exception. Each task's primary agent owns its branch, worktree and cleanup. After its authorized
merge, that agent syncs `main` and retires the task's worktree and local/remote branches without another
cleanup request, following [Git workflow](./.agents/standards/git-workflow.md). Preserve unfinished
work; report a blocked cleanup instead of calling it complete. New work gets a new branch.

The primary agent owns all edits, integration, and verification. Delegates are read-only and
return evidence.

**Codex only:** explore with `gpt-5.6-luna` at `max` effort; use 2–4 distinct angles for substantial
work, as slots permit. Review with `gpt-5.6-sol` at `high` effort, one targeted reviewer by default.

Before changing code: read routed docs and nearby tests; check repository status, preserve unrelated changes, and identify the contract and lowest meaningful test tier.

While changing code: keep one coherent scope; test behavior and relevant failure and cancellation paths. Use focused local checks and prefer GitHub CI for broad builds and suites on feature branches. Expand or repeat checks only for new changes, failures, or specific risks.

Before handoff: inspect the diff for dependency, generated-file, snapshot, or formatting churn; report changes, validation, and unverified work.

For layout, copy, focus, attention, or interaction changes, inspect wide, medium, and narrow rendered frames; attach them to the stage record unless the user reviewed them. String assertions prove mechanisms, not the experience.

Claim only observed passes for the current code; identify local, CI, and pending checks. Do not commit, publish, merge, install globally, or mutate live user configuration unless explicitly requested. Commit messages follow Conventional Commits.

## Documenting work

Keep roadmap, code, tests, comments, and user-facing behavior aligned. Document ownership is in [`.agents/README.md`](./.agents/README.md).

- **Rewrite in place.** Replace outdated statements; no correction notes, superseded markers, or appended history. Git holds the history.
- **One name per thing.** Use `ui-ux.md` vocabulary in code and on screen; rename all three together.
- **Record contested choices.** Once a choice survives use, add a `Rejected:` sentence beside the rule naming the alternative and why it lost. Uncontested choices need no rejection note.
- **An invariant with no test is marked unproven**, never left reading as fact.
- **Link to the owner.** Expand docs only when needed; promote findings only when they change a durable invariant or system boundary.
