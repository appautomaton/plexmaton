# Plexmaton

Plexmaton is an early-stage Rust agentic harness with a responsive Ratatui workspace for
steering and observing multiple agents.

The current Phase 00 executable is intentionally synthetic: it exercises the semantic event
boundary, deterministic multi-agent timeline, TUI reducer, rendering, and terminal event loop
without connecting to a model provider or tool runtime.

## Workspace

- `plexmaton-core`: UI-facing semantic prototype contracts
- `plexmaton-sim`: deterministic Phase 00 scenarios
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

Press `q`, `Esc`, or `Ctrl-C` to leave the synthetic TUI.

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

