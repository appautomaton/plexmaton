# Standard — Quality gates

| Field | Value |
| --- | --- |
| Trigger | A gate failed, you are setting up a clone, or you are about to claim a check passed |
| Owns | Which gates exist, what each one catches, and how to run them |

## The lanes

Workspace gates, run per change:

```console
cargo fmt --all --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Supply-chain and corpus lanes, which depend on the resolved graph or the documents rather than on
any single edit:

| Command | Catches |
| --- | --- |
| `cargo deny check` | Licences, advisories, and duplicate Ratatui/Crossterm generations |
| `cargo machete` | Dependencies that are declared but unused |
| `typos` | Prose and identifier spelling |
| `./scripts/check-file-length.sh` | Module sprawl in `crates/**/*.rs` |
| `./scripts/check-doc-budget.sh` | Documents that outgrew their layer. Reports only; never fails |
| `./scripts/smoke-tui.py` | Terminal lifecycle `TestBackend` cannot represent |

Policy is pinned in `rustfmt.toml`, `clippy.toml`, `deny.toml`, `_typos.toml`, and the root
`[workspace.lints]` table. Document budgets and their escape hatches live in
[`.agents/README.md`](../README.md).

## Sprawl guards

Guards exist at two levels, and both are thresholds for finding unseparated responsibilities, not
line-count style rules.

`too_many_lines` and `cognitive_complexity` work at function level and are the effective guard,
because a large file of small functions is usually fine while a long function never is.
`check-file-length.sh` adds a 400-line file-level sentinel measured above the first `#[cfg(test)]`
module, so inline tests do not count against the budget.

When either fires, split by responsibility and invariant. Raising the threshold is not the fix.

## Local setup

Commits run the fast gates through a repository-managed hook. Enable it once per clone:

```console
git config core.hooksPath .githooks
```

## The terminal smoke

`./scripts/smoke-tui.py` covers alternate-screen release, resize repaint, the quit key, and mouse
reporting in front of a real pseudo-terminal. Run it locally when changing the event loop, terminal
setup, layout classes, the quit binding, or surface kinds. It runs in CI.

Mouse reporting is checked here because neither half fits a cell buffer: enabling and releasing it
are byte sequences, and the click is sent as a real SGR report so crossterm's parser is on the path.
Release is asserted to happen *before* the alternate screen is handed back — the other order
switches the modes off on the terminal the user is now looking at.

Three properties of that boundary have already produced wrong evidence once, so they are worth
knowing before you touch it:

- A pseudo-terminal with no window size reports 0x0, and Ratatui then paints nothing. The script
  sets `TIOCSWINSZ` explicitly.
- Without its own session and controlling terminal, the child never receives `SIGWINCH`, so a
  resize is silently ignored.
- Ratatui emits only changed cells, so an incremental frame carries `1` rather than `attention 1`.
  The script forces one full repaint through a resize and asserts against that frame.
- An agent sandbox may refuse `pty.openpty` with "out of pty devices". That is the sandbox, not a
  defect; run the smoke outside it. `q` is the key it quits with, so a change to that binding
  changes this script in the same commit.

## Parallel checkouts

`.worktrees/<name>/` is the only place a worktree goes. It is the one ignored path, it belongs to
no vendor, and an agent harness that would default somewhere else is pointed here rather than
followed:

```console
git worktree add .worktrees/surfaces -b feat/surfaces   # then start the agent inside it
git worktree remove .worktrees/surfaces                 # never rm -rf; this takes target/ with it
```

Never put a worktree in `.agents/`, which is the tracked corpus (D-035).

**Never share `CARGO_TARGET_DIR` between worktrees.** It looks free — the checkouts differ by four
crates out of seventy-six — and it silently runs the wrong code. Two checkouts of this workspace
produce the same fingerprint for a member crate, so the second build overwrites the first's
artifact, and the first checkout's older source files then pass the freshness check against it.
Reproduce in under a minute:

```console
git worktree add --detach .worktrees/probe HEAD
printf '\n#[cfg(test)]\nmod probe { #[test] fn only_in_the_worktree() {} }\n' \
    >> .worktrees/probe/crates/plexmaton-core/src/lib.rs
(cd .worktrees/probe && CARGO_TARGET_DIR=/tmp/shared cargo test -p plexmaton-core --lib -- --list)
CARGO_TARGET_DIR=/tmp/shared cargo test -p plexmaton-core --lib -- --list
```

The second listing, run against the main checkout, contains `only_in_the_worktree`. This is
[cargo#12516](https://github.com/rust-lang/cargo/issues/12516), still open. A separate target
directory per worktree costs 245 MB and a 4-second cold build, and is the only safe answer.

## Claiming a result

Do not claim a check passed unless it was actually run in this workspace, in this state. Report
what changed, what was tested, and what remains unverified. Update phase evidence only when the
criterion is actually demonstrated — file presence, code volume, and a successful happy-path demo
are not evidence.
