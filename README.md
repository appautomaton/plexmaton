# Plexmaton

Plexmaton is an early-stage Rust agentic harness with a Ratatui workspace, OpenAI-compatible
Responses and Chat Completions, and native file/search/edit/command tools.

## Workspace

Semantic contracts and the provider-independent loop live in `core`/`agent`; wire codecs and live
work in `provider`/`runtime`; bounded effects in `file-tools`/`command`; fixtures, projection and
composition in `sim`/`tui`/`cli` (all prefixed `plexmaton-`).

## Development

The normal configuration root is `~/.plexmaton/`; repositories are never searched for a
`.plexmaton/` directory. The file names the environment variable containing its credential:

```toml
# ~/.plexmaton/config.toml
active_model = { provider = "local", model = "luna" }

[providers.local]
base_url = "http://127.0.0.1:8317/v1"
api_key_env = "PLEXMATON_LOCAL_API_KEY"
api = "openai_responses"

[providers.local.models.luna]
id = "gpt-5.6-luna"
display_name = "Luna"
reasoning_effort = "xhigh"
context_window_tokens = 272000
max_output_tokens = 128000
output_reserve_tokens = 16384

[providers.local.models.sol]
id = "gpt-5.6-sol"
display_name = "Sol"
api = "openai_chat_completions"
reasoning_effort = "high"
context_window_tokens = 272000
max_output_tokens = 128000
output_reserve_tokens = 32768
cost = { input = 0.2, output = 1.2, cache_read = 0.02, cache_write = 0.25 }
```

A route can own several models; a model may override `api`. `token_estimator` and `[...cost]` are
optional: an omitted estimator resolves to a versioned default; omitted pricing is unavailable.
For isolated development, point `PLEXMATON_HOME` at a directory containing `config.toml`.

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

Supply-chain and corpus gates:

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
