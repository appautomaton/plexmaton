# Plexmaton

Plexmaton is an early-stage Rust agentic harness with a Ratatui workspace for steering and
observing agents.

The executable runs one real agent through an explicitly selected OpenAI-compatible Responses or
Chat Completions endpoint. It advertises native tools to read, search, create, and edit files and
run a foreground command.

## Workspace

- `plexmaton-core`: semantic contracts shared by producers and projections
- `plexmaton-agent`: provider-independent turn, step, input, and approval state machine
- `plexmaton-provider`: bounded OpenAI-compatible request and SSE codecs
- `plexmaton-file-tools`: descriptor-rooted reads, searches, observations, and mutations
- `plexmaton-command`: bounded foreground Unix command execution
- `plexmaton-runtime`: owned HTTP and native-tool work, cancellation, and live composition
- `plexmaton-sim`: deterministic synthetic scenarios, the test producer
- `plexmaton-tui`: explicit view state, reducer, surfaces, and rendering
- `plexmaton-cli`: terminal lifecycle and executable composition root

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

The directory where `plexmaton` starts is the one canonical native-tool workspace. File tools
refuse absolute paths, parent traversal, and symlinks. Directory search re-enters the same
executable as an internal descriptor-rooted `rg` driver, and both search child paths receive a
cleared environment. Read and search run without approval; create, edit, and command calls wait for
an exact **Allow Once** or **Deny** decision. Commands are not OS-sandboxed; their cleared child
environment omits the selected key plus Plexmaton-private and credential-shaped variables.

```console
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton
cargo fmt --all --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Press `Ctrl-D` twice to leave the TUI; the first press says so in the status line, and
any other key withdraws it. `Ctrl-C` clears the draft, addresses the focused conversation's
interrupt, cancels and joins its live model or native-tool work, and never quits. `Esc` backs out
one layer at a time — a selection, then an open second window — and does not quit (INV-6, INV-7).

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

With a local profile and its named key environment variable already set, the opt-in live lane also
submits two safe tool-backed requests and waits for their streamed answers in the PTY:

```console
PLEXMATON_HOME=.local/plexmaton ./scripts/smoke-tui.py --live
```

Enable the shared pre-commit hook once per clone:

```console
git config core.hooksPath .githooks
```
