# Plexmaton

Plexmaton is an early-stage Rust agentic harness with a responsive Ratatui workspace for
steering and observing multiple agents.

The executable is still synthetic: it exercises the semantic event
boundary, deterministic multi-agent timeline, TUI reducer, rendering, and terminal event loop
without connecting to a model provider or tool runtime. A real producer is Phase 01's second stage.

## Workspace

- `plexmaton-core`: UI-facing semantic prototype contracts
- `plexmaton-sim`: deterministic synthetic scenarios, the test producer
- `plexmaton-tui`: explicit view state, reducer, surfaces, and rendering
- `plexmaton-cli`: terminal lifecycle and executable composition root

## Development

```console
cargo run -p plexmaton-cli
cargo fmt --all --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Press `Ctrl-D` twice to leave the synthetic TUI; the first press says so in the status line, and
any other key withdraws it. `Ctrl-C` clears the draft and never quits. `Esc` backs out one layer at
a time — a selection, then an open second window — and does not quit (INV-6, INV-7).

Supply chain and prose, which depend on the resolved graph rather than on a single edit:

```console
cargo deny check
cargo machete
typos
./scripts/check-file-length.sh
```

Terminal lifecycle cannot be proven by Ratatui's `TestBackend`. To exercise alternate-screen
entry and release, resize handling, and the quit key in front of a real pseudo-terminal:

```console
./scripts/smoke-tui.py
```

Enable the shared pre-commit hook once per clone:

```console
git config core.hooksPath .githooks
```

