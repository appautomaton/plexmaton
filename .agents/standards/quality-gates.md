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
| `python3 -m unittest discover -s scripts/tests` | Gate boundary regressions and offline smoke-fixture ownership |
| `./scripts/check-crate-graph.sh` | A dependency arrow the design forbids: a runtime, a client or a terminal reachable from the loop or the vocabulary, and a producer reachable from the projection |
| `./scripts/check-citations.sh` | An `INV-4` or `INS-5` in code that resolves to nothing, and a spec naming a test that no longer exists |
| `./scripts/check-frames.sh` | A frame no document cites: evidence nobody can find again |
| `./scripts/check-doc-budget.sh` | Documents that outgrew their layer. Reports only; never fails |
| `./scripts/smoke-tui.py` | Terminal lifecycle `TestBackend` cannot represent |
| `python3 scripts/smoke-statusline.py` | Configured shell footer, three widths, last-row hints and cleanup in a real PTY; no model request |
| `python3 scripts/smoke-permissions.py` | Project trust, real command execution, a remembered prefix across restart, revoke/deny and three widths in a real PTY; eight bounded local fixture requests |
| `PLEXMATON_WRITE_FRAMES=1 cargo test -p plexmaton-tui frames` | Rewrites the frames under `crates/plexmaton-tui/frames/`; the diff is the review |
| `cargo run --release -p plexmaton-cli --bin plexmaton-measure` | What a frame costs. Reports only; its work counts are asserted by the test suite, and its timings belong to the machine that ran it ([frame-loop](../specs/frame-loop.md) FR-4) |

Policy is pinned in `rustfmt.toml`, `clippy.toml`, `deny.toml`, `_typos.toml`, and the root
`[workspace.lints]` table. Document budgets and their escape hatches live in
[`.agents/README.md`](../README.md).

## Sprawl guards

Function-level `too_many_lines` and `cognitive_complexity` identify unseparated responsibilities.
`check-file-length.sh` adds a 550-line file-level sentinel. Test-only files following the
workspace's `tests/`, `tests.rs`, `*_tests.rs`, or `test_support.rs` conventions are excluded; in a
mixed module, measurement stops above the trailing inline `#[cfg(test)] mod tests { ... }`.
Test-gated helpers outside that module still count. Discovery failure fails the gate.

Rejected: a 400-line sentinel and counting standalone tests, which forced mechanical splits
without finding a responsibility boundary. When a guard fires, split by responsibility rather
than raising it.

## Local setup

The executable and corpus gates require `rg` on PATH; script tests require Python 3 and Bash.
The configured-footer smoke also requires jq. CI installs Bash, ripgrep and jq explicitly.

The primary CI target is macOS Apple Silicon, matching local product development. One verification
job runs the same workspace, supply-chain and PTY gates; it does not package or deploy releases.
Actions use verified stable releases pinned to commit IDs.
Linux compatibility is not established: the current Bash grammar has a known native parser crash
on Linux ([upstream report](https://github.com/tree-sitter/tree-sitter-bash/issues/337)).
Passing macOS CI is not evidence of Linux support.

Commits run the read-only gates in parallel through a repository-managed hook, which reports its
time against a five-second budget. Enable it once per clone:

```console
git config core.hooksPath .githooks
```

The hook clears Git's repository-local environment first: an inherited `GIT_DIR` redirects even a
fixture's `git init`. Rejected: clippy and the workspace tests in the hook, which build the
workspace at two minutes a commit to repeat what the handoff run and CI already answered.

## The terminal smoke

`./scripts/smoke-tui.py` covers alternate-screen release, resize repaint, the quit key, and mouse
and focus reporting in front of a real pseudo-terminal. Run it locally when changing the event
loop, terminal setup, layout classes, the quit binding, or surface kinds. It runs in CI.

Input reporting is checked here because neither half fits a cell buffer: enabling and releasing
mouse and focus events are byte sequences, and the click is sent as a real SGR report so
crossterm's parser is on the path. Release is asserted to happen *before* the alternate screen is
handed back — the other order switches the modes off on the terminal the user is now looking at.

Terminal-boundary pitfalls:

- A pseudo-terminal with no window size reports 0x0, and Ratatui then paints nothing. The script
  sets `TIOCSWINSZ` explicitly.
- Without its own session and controlling terminal, the child never receives `SIGWINCH`, so a
  resize is silently ignored.
- Ratatui emits only changed cells, so an incremental frame carries `1` rather than `Agents · !1`.
  The script forces one full repaint through a resize and asserts against that frame.
- A normal smoke launch uses an isolated `PLEXMATON_HOME`; menus, draft edits and blank exit must
  create no JSONL or saved-session handoff. Test commands never contact a configured provider; see
  [testing](./testing.md) §Tier 5.
- Both smoke scripts use an owned loopback connection trap and a whitelisted child environment:
  no real credentials, proxy or live tmux clipboard. Blank launches assert zero JSONL, and the
  trap proves no model work without reading the journal.
- An agent sandbox may refuse `pty.openpty` with "out of pty devices". That is the sandbox, not a
  defect; run the smoke outside it. Two `Ctrl-D` presses inside the one-second window are how it
  quits; the script first lets one window expire, so a change to that chord changes this script in
  the same commit.

## Parallel checkouts

`.worktrees/<name>/` is the only place a worktree goes. It is the one ignored path, it belongs to
no vendor, and an agent harness that would default somewhere else is pointed here rather than
followed:

```console
git worktree add .worktrees/surfaces -b feat/surfaces   # then start the agent inside it
git worktree remove .worktrees/surfaces                 # never rm -rf; this takes target/ with it
```

Never put a worktree in `.agents/`, which is the tracked corpus.

After a move, rebuild packages whose fixtures embed old absolute paths (`cargo clean -p <package>`).

**Never share `CARGO_TARGET_DIR` between worktrees.** Sharing it can silently run the wrong code. Two checkouts of this workspace
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

The main checkout's listing contains `only_in_the_worktree`
([cargo#12516](https://github.com/rust-lang/cargo/issues/12516)). Use a separate target directory
per worktree.

## Claiming a result

Do not claim a check passed unless it was actually run in this workspace, in this state. Report
what changed, what was tested, and what remains unverified. Update phase evidence only when the
criterion is actually demonstrated — file presence, code volume, and a successful happy-path demo
are not evidence.
