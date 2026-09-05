# Plexmaton

Plexmaton is an early-stage Rust agentic harness with a Ratatui workspace, OpenAI-compatible
Responses and Chat Completions, and native file/search/edit/command tools.

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

A route can own several models; each may override `api`. Estimation defaults when omitted;
omitted pricing is unavailable. Set `PLEXMATON_HOME` for isolated development.

The start directory is the native-tool root; file tools refuse absolute, parent-traversing, and
symlinked paths. Read and search run directly; create, edit, and command require **Allow Once** or
**Deny**. Commands are not OS-sandboxed and receive a credential-scrubbed environment.

```console
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- --ephemeral
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- create work-01
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- resume work-01
```

Default sessions are durable; exit prints the JSONL path and ID. `--ephemeral` disables persistence.
`create` reserves a name; `resume` restores history. Files are owner-only:
`PLEXMATON_HOME/sessions/<session-id>.jsonl` (ASCII letters, digits, `-`, `_`).

`Ctrl-D` twice within one second quits. `Ctrl-C` clears a draft or interrupts its conversation;
`Esc` backs out one layer. `Ctrl-P` opens the palette: `/config` (alias `/settings`) shows the
resolved model configuration. Change `config.toml` and restart to apply settings.

`/resume` opens searchable history; `/continue`, `/sessions` and `/session` are aliases.
Use arrows and Enter or click. Escape cancels; loading never starts a model request.
Stop active work and send or clear drafts before switching.

Unanswered rate limits offer **Retry** / **Edit & retry** beside the failed message, not in the
palette. Click a button, or press `r` / `e` while the primary transcript has navigation focus.
Retry reuses the question; editing preserves the old branch. `Esc` cancels editing.

For the pastel footer, add:

```toml
[status_line]
command = "bash /absolute/path/to/plexmaton/examples/statusline-pastel.sh"
max_rows = 6
refresh_ms = 30000
```

The example needs Bash, jq and a Nerd Font. It receives JSON snapshots on stdin;
editing the script requires no rebuild. Context appears only after API-reported usage; missing
statistics are omitted. Quit/Ctrl-P hints occupy the last terminal row.
[Protocol, limits and configuration](.agents/specs/status-line.md).

Inputs support pointer selection and grapheme-safe editing. Hover a message for its upper-right
Nerd Font Copy icon; plain clicks do not copy. Transcript drags select entries; tools disclose
retained detail. See [keys](.agents/specs/interaction-routing.md#key-grammar) and
[copy](.agents/specs/selection-and-copy.md). Local macOS uses `pbcopy`; remote sessions use OSC 52.

Run `cargo test --workspace` and the [quality gates](.agents/standards/quality-gates.md).
`python3 scripts/smoke-tui.py` covers terminal/input lifecycle; `python3 scripts/smoke-statusline.py`
covers the configured footer without model calls. Enable hooks with `git config core.hooksPath .githooks`.
