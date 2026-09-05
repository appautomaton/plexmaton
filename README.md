# Plexmaton

Plexmaton is a Rust agentic harness with a Ratatui workspace, Responses, Chat Completions,
Anthropic Messages, Gemini GenerateContent, and native file/search/edit/command/skill tools.

## Development

User configuration and runtime state live in `~/.plexmaton/`. Optional project
`.plexmaton/config.toml` overrides `[active_model]` using user-defined provider/model names.
Credentials come from the named environment variable:

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
```

Set `PLEXMATON_HOME` for isolation. See [provider examples](examples/providers.toml) and
[request options](.agents/specs/provider-adapter.md#request-configuration).

The start directory is the native-tool root; file tools refuse absolute, parent-traversing, and
symlinked paths. Read and search run directly; create, edit, and command require **Allow Once** or
**Deny**. Commands are not OS-sandboxed and receive a credential-scrubbed environment.

```console
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- --ephemeral
```

Default sessions create JSONL on the first message; blank launches save nothing.
Exit prints a resume command for the selected saved session. `--ephemeral` disables persistence.
`plexmaton create work-01` reserves a name; `plexmaton resume work-01` restores history. Files are owner-only:
`PLEXMATON_HOME/sessions/<session-id>.jsonl` (ASCII letters, digits, `-`, `_`).
This build uses journal epoch `2026-09-05`; older epochs are refused, with no migration.

Skills are `<name>/SKILL.md` bundles, discovered in precedence order: project `.plexmaton/skills`,
project `.agents/skills`, then `PLEXMATON_HOME/skills`. The model sees summaries and loads instructions
or resources through `skill`. In the main input, `$` lists skills; Tab/Enter inserts `$name `,
Enter sends. Esc closes the list. Failures return drafts; saved content survives edits.
Skills never grant permissions. [Picker](.agents/specs/skill-picker.md) · [Format](.agents/specs/agent-skills.md).

`Ctrl-D` twice within one second quits. `Ctrl-C` clears a draft or interrupts its conversation;
`Esc` backs out one layer. `Ctrl-P` opens the palette: `/config` (alias `/settings`) shows the
resolved model configuration. Change `config.toml` and restart to apply settings.

`/new` starts an empty session. `/resume` opens history; `/continue`, `/sessions`, `/session` are aliases.
Arrows/Enter or click resumes; Esc cancels. Stop work and send or clear drafts before switching.

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

The example uses Bash, jq and a Nerd Font. See [footer protocol and limits](.agents/specs/status-line.md).

Pastel assistant Markdown supports code blocks and tables; chrome stays terminal-owned.
Streams coalesce; input bypasses their timer.
Drag across entries to copy plain text on release; the Copy icon keeps raw Markdown.
In-conversation approvals: Esc focuses input; Tab/click focuses the card.
See [keys](.agents/specs/interaction-routing.md#key-grammar) and
[copy](.agents/specs/selection-and-copy.md). Local macOS uses `pbcopy`; remote sessions use OSC 52.

Run `cargo test --workspace` and [quality gates](.agents/standards/quality-gates.md), including
offline terminal/footer smokes. Hooks: `git config core.hooksPath .githooks`.
