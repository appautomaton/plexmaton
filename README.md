# Plexmaton

Plexmaton is an early-stage Rust agentic harness with a responsive Ratatui workspace for
steering and observing multiple agents.

The executable runs one real text-only agent through an explicitly selected OpenAI-compatible
Responses or Chat Completions endpoint. File and command tools are not advertised yet; the
deterministic simulator remains the test and measurement producer.

## Workspace

- `plexmaton-core`: semantic contracts shared by producers and projections
- `plexmaton-agent`: provider-independent turn, step, input, and approval state machine
- `plexmaton-provider`: bounded OpenAI-compatible request and SSE codecs
- `plexmaton-runtime`: owned HTTP stream, cancellation, and live agent composition
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
configuration or a missing key fails before Plexmaton takes over the terminal.

```console
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton
cargo fmt --all --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Press `Ctrl-D` twice to leave the TUI; the first press says so in the status line, and
any other key withdraws it. `Ctrl-C` clears the draft, addresses the focused conversation's
interrupt, cancels and joins its live model request, and never quits. `Esc` backs out one layer at
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

With a local profile and its named key environment variable already set, the opt-in live lane also
submits one request and waits for its streamed answer in the PTY:

```console
PLEXMATON_HOME=.local/plexmaton ./scripts/smoke-tui.py --live
```

Enable the shared pre-commit hook once per clone:

```console
git config core.hooksPath .githooks
```
