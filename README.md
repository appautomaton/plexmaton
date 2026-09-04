# Plexmaton

Plexmaton is an early-stage Rust agentic harness with a Ratatui workspace for steering and
observing agents.

The executable runs one real agent through an explicitly selected OpenAI-compatible Responses or
Chat Completions endpoint. It advertises native tools to read, search, create, and edit files and
run a foreground command.

## Workspace

- `plexmaton-core` / `plexmaton-agent`: semantic contracts and the provider-independent loop
- `plexmaton-provider` / `plexmaton-runtime`: wire codecs and owned live work
- `plexmaton-file-tools` / `plexmaton-command`: bounded native effects
- `plexmaton-sim` / `plexmaton-tui` / `plexmaton-cli`: fixtures, projection, and composition root

## Development

The normal configuration root is `~/.plexmaton/`; repositories are never searched for a
`.plexmaton/` directory. The file names the environment variable containing its credential:

```toml
# ~/.plexmaton/config.toml
active_provider = "local_luna"

[providers.local_luna]
kind = "openai_compatible"
protocol = "responses"
base_url = "http://127.0.0.1:8317/v1"
model = "gpt-5.6-luna"
api_key_env = "PLEXMATON_LOCAL_API_KEY"
reasoning_effort = "medium"
```

For isolated development, set `PLEXMATON_HOME` to a directory containing `config.toml`. Invalid
startup inputs fail before Plexmaton takes over the terminal.

The start directory is the native-tool root; file tools refuse absolute, parent-traversing, and
symlinked paths. Read and search run directly; create, edit, and command require **Allow Once** or
**Deny**. Commands are not OS-sandboxed and receive a credential-scrubbed environment.

```console
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- --ephemeral
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- create work-01
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- resume work-01
cargo fmt --all --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Launching without a session command creates an automatically named durable session and prints its
JSONL path and session ID on exit. `--ephemeral` is the explicit no-JSONL mode. `create` reserves a
new portable session name; `resume` requires that exact existing name. Durable sessions are
owner-only JSONL files under
`PLEXMATON_HOME/sessions/<session-id>.jsonl`. Names start with an ASCII letter or digit and then use
only letters, digits, `-`, or `_`.

Press `Ctrl-D` twice within one second to leave. `Ctrl-C` clears a non-empty draft; with an empty
draft it interrupts the focused conversation. `Esc` clears a selection, then closes the second
window, and never quits (INV-6, INV-7).
In a conversation, `Shift-↑` / `Shift-↓` selects semantic entries, `Ctrl-O` opens or closes retained
tool detail at the moving end, and `Ctrl-Y` copies producer source rather than painted cells. A
mouse drag copies on release; holding it at a conversation edge scrolls the selection into
off-screen entries, while terminal focus loss pauses without discarding it. A single click on a
foldable tool row selects and toggles the same detail.

Supply chain and prose, which depend on the resolved graph rather than on a single edit:

```console
cargo deny check
cargo machete
typos
./scripts/check-file-length.sh
./scripts/check-crate-graph.sh
./scripts/check-citations.sh
./scripts/check-doc-budget.sh
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
