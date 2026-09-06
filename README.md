# Plexmaton

Plexmaton is a Rust agentic harness with a Ratatui workspace, Responses, Chat Completions,
Anthropic Messages, Gemini GenerateContent, and native file/search/edit/command tools.

## Development

Configuration lives in `~/.plexmaton/`, never a repository's `.plexmaton/`; credentials use env vars:

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
compaction_keep_recent_tokens = 20000
```

Set `PLEXMATON_HOME` for isolation.

[Provider examples](examples/providers.toml) and [request options](.agents/specs/provider-adapter.md#request-configuration)
cover native endpoints, instructions, optional reasoning and cache hints.

File tools stay under the start directory and refuse symlinks. Read/search run directly;
create/edit/command require **Allow Once** or **Deny**. Commands use a credential-scrubbed
environment, not an OS sandbox.

```console
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- --ephemeral
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- create work-01
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- resume work-01
```

Default sessions create JSONL on the first message; blank launches save nothing.
Exit prints a resume command for the selected saved session. `--ephemeral` disables persistence.
`create` reserves a name; `resume` restores history. Files are owner-only:
`PLEXMATON_HOME/sessions/<session-id>.jsonl` (ASCII letters, digits, `-`, `_`).

Automatic [compaction](.agents/specs/compaction.md) preserves original history.

`Ctrl-D` twice within one second quits. `Ctrl-C` clears a draft or interrupts its conversation;
`Esc` backs out one layer. `Ctrl-P` opens the palette: `/config` (alias `/settings`) shows the
resolved model configuration. Change `config.toml` and restart to apply settings.

`/new` starts an empty session. `/resume` opens history; `/continue`, `/sessions`, `/session` are aliases.
Arrows/Enter or click resumes; Esc cancels. Stop work and send or clear drafts before switching.

Unanswered rate limits offer **Retry** / **Edit & retry** beside the failed message, not in the
palette. Click a button, or press `r` / `e` while the primary transcript has navigation focus.
Retry reuses the question; editing preserves the old branch. `Esc` cancels editing.

Pastel footer:

```toml
[status_line]
command = "bash /absolute/path/to/plexmaton/examples/statusline-pastel.sh"
max_rows = 6
refresh_ms = 30000
```

The example needs Bash, jq and a Nerd Font; script edits need no rebuild.
Context requires API-reported usage; unknown statistics stay hidden.
[Protocol, limits and configuration](.agents/specs/status-line.md).

Assistant Markdown supports code and tables.
Native LaTeX needs verified text sizing (tested in direct Kitty).
tmux and unverified terminals show labelled source. Click a formula to copy its exact delimited
TeX; drags select whole formulas. [Limits](.agents/specs/math-layout.md).
Drag across entries to copy plain text on release; the Copy icon keeps raw Markdown.
Click tool rows to expand/collapse without selecting; drag or use the keyboard to select.
In-conversation approvals: Esc focuses input; Tab/click focuses the card.
See [keys](.agents/specs/interaction-routing.md#key-grammar) and
[copy](.agents/specs/selection-and-copy.md). Local macOS uses `pbcopy`; remote sessions use OSC 52.
Clipboard helpers do not hold input; the latest copy wins. Allocation cap: 8 MiB, no truncation.
Exit cancels pending delivery.

See [quality gates](.agents/standards/quality-gates.md),
[background preparation](.agents/specs/render-preparation.md) and `plexmaton-measure`.
