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
```

Launching without a session command creates an automatically named durable session and prints its
JSONL path and session ID on exit. `--ephemeral` is the explicit no-JSONL mode. `create` reserves a
new portable session name; `resume` requires that exact existing name. Durable sessions are
owner-only JSONL files under
`PLEXMATON_HOME/sessions/<session-id>.jsonl`. Names start with an ASCII letter or digit and then use
only letters, digits, `-`, or `_`.

`Ctrl-D` twice within one second quits. `Ctrl-C` clears a draft or interrupts its conversation;
`Esc` backs out one layer. `Ctrl-P` opens the palette: `config` and `settings`, with or without `/`,
open the same read-only provider/model/reasoning page. `Esc` restores the search; another closes it.
Change `config.toml` and restart to apply settings.

For the customizable pastel footer, add this to that user configuration:

```toml
[status_line]
command = "bash /absolute/path/to/plexmaton/examples/statusline-pastel.sh"
max_rows = 6
refresh_ms = 30000
```

The example needs Bash, jq and a Powerline-compatible font. It receives JSON snapshots on stdin;
editing the script requires no rebuild. Missing statistics are omitted. Quit/Ctrl-P hints occupy
the last terminal row. [Protocol, limits and configuration](.agents/specs/status-line.md).

Inputs support click-to-place, drag-to-select/copy, and typing or paste to replace selection. Arrows,
Home/End, word motion and line deletion preserve graphemes. Transcripts support selection, disclosure
and source copy. See the [key grammar](.agents/specs/interaction-routing.md#key-grammar)
and [selection contract](.agents/specs/selection-and-copy.md). Local macOS uses `pbcopy`; remote sessions use OSC 52.

Run `cargo test --workspace` and the [quality gates](.agents/standards/quality-gates.md).
`python3 scripts/smoke-tui.py` covers terminal/input lifecycle; `python3 scripts/smoke-statusline.py`
covers the configured footer without model calls. Enable hooks with `git config core.hooksPath .githooks`.
