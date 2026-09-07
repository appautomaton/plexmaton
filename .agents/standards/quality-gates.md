# Standard — Quality gates

| Field | Value |
| --- | --- |
| Trigger | Selecting or running checks, fixing a gate, setting up clone hooks or worktree builds, or reporting results |
| Owns | Which gates exist, what each one catches, and how to run them |

## The lanes

For workflow performance changes, read the [CI speed baseline](../spikes/ci-speed/README.md).

Select checks per [AGENTS.md](../../AGENTS.md#working-discipline). CI runs on pull requests,
pushes to `main`, or manual dispatch, not feature-branch pushes alone.

CI's Rust gates:

```console
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

For local compile-only feedback, use `cargo check -p <crate> --all-targets --locked`. CI uses
Clippy for compilation; these are alternative checks, not two required runs.

Supply-chain and corpus lanes, selected by dependency or document changes:

| Command | Catches |
| --- | --- |
| `cargo deny check` | Licences, advisories, and duplicate Ratatui/Crossterm generations |
| `cargo machete` | Dependencies that are declared but unused |
| `typos` | Prose and identifier spelling |
| `./scripts/check-file-length.sh` | Module sprawl in `crates/**/*.rs` |
| `python3 -m unittest discover -s scripts/tests` | Gate boundary regressions and offline smoke-fixture ownership |
| `./scripts/check-crate-graph.sh` | A dependency arrow the design forbids: a runtime, a client or a terminal reachable from the loop or the vocabulary, and a producer reachable from the projection |
| `./scripts/check-citations.sh` | Duplicate invariant IDs/prefix owners, missing source citations or proof functions, and broken inline local links/headings |
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

The gates require ripgrep, Python 3 and Bash; the hook uses Perl for timing and the footer smoke
needs jq. CI installs Bash, ripgrep and jq explicitly.

CI runs two macOS jobs concurrently: static/supply-chain/Python checks, and Rust/PTY verification.
Checks omit target artifacts from caching; `verify` retains its existing target-cache key for
Clippy, tests and smokes. The final `macOS Apple Silicon` check runs on Ubuntu and passes only if
both macOS jobs succeed. CI does not package or deploy releases.
Actions use verified stable releases pinned to commit IDs.
Linux compatibility is not established: the current Bash grammar has a known native parser crash
on Linux ([upstream report](https://github.com/tree-sitter/tree-sitter-bash/issues/337)).
Passing macOS CI is not evidence of Linux support.

The hook runs read-only gates in parallel, then reports document budgets and total time against
a five-second target. Cargo graph and smoke checks use `--locked` to preserve dependency choices.
Enable the hook once per clone:

```console
git config core.hooksPath .githooks
```

The hook clears Git's repository-local environment first: an inherited `GIT_DIR` redirects even a
fixture's `git init`. Rejected: clippy and the workspace tests in the hook, which build the
workspace at two minutes a commit to repeat what CI already checks.

## The terminal smoke

`./scripts/smoke-tui.py` covers alternate-screen release, resize repaint, quitting, and mouse/focus
reporting through a real PTY. Use CI unless a terminal or interaction change needs local
reproduction or terminal-specific evidence.

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

[Git workflow](./git-workflow.md) owns checkout placement, task branches and retirement.
Read [Rust builds](./rust-builds.md) before building in a task worktree or changing Cargo profiles,
artifact paths, caches or concurrency. It owns build isolation even when a harness supplies its
own worktree defaults.

## Claiming a result

Report local and CI results with their revision, plus pending, failed, or unrun checks.
Passing CI does not cover subsequent edits.
Update phase evidence only when the criterion is demonstrated; file presence, code volume, and
happy-path demos do not suffice.
