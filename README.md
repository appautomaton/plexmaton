# Plexmaton

A Rust/Ratatui coding harness with four provider dialects and native file/search/edit/command/skill tools.

## Development

State/config: `~/.plexmaton/`; keys use environment variables.
Project `.plexmaton/config.toml` selects a user-defined provider/model via `[active_model]`.

```toml
# ~/.plexmaton/config.toml
active_model = { provider = "local", model = "luna" }

[providers.local]
base_url = "http://127.0.0.1:8317/v1"
api_key_env = "PLEXMATON_LOCAL_API_KEY"
api = "openai_responses"

[providers.local.models.luna]
id = "gpt-5.6-luna"
reasoning_effort = "xhigh"
context_window_tokens = 272000
max_output_tokens = 128000
output_reserve_tokens = 16384
compaction_keep_recent_tokens = 20000
```

`PLEXMATON_HOME` isolates state. [Provider examples](examples/providers.toml) ·
[Options](.agents/specs/provider-adapter.md#request-configuration).

```console
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton
PLEXMATON_HOME=.local/plexmaton cargo run -p plexmaton-cli --bin plexmaton -- --ephemeral
```

Files stay under the start directory, without symlinks. Reads/searches allow; writes/commands ask:
**Allow once**, **Allow and remember…**, or **Deny**. Session grants survive
`/new`/resume until exit; Project grants survive restarts. `/permissions` manages grants, native
presets and project trust. [Rules and prefixes](.agents/specs/permission-policy.md#configuration).
Commands use a credential-scrubbed environment, not an OS sandbox.

Conversations save owner-only JSONL on first message; blank launches save nothing. Exit prints a
resume command. `--ephemeral` disables persistence. `plexmaton create work-01` / `plexmaton resume work-01`
create/restore
`PLEXMATON_HOME/sessions/<session-id>.jsonl` (ASCII letters, digits, `-`, `_`).
Journal epoch: `2026-09-05`; older epochs are refused, with no migration.
Automatic [compaction](.agents/specs/compaction.md) preserves original history and the current skill invocation.

Skills are `<name>/SKILL.md` bundles in project `.plexmaton/skills`, project `.agents/skills`, then
`PLEXMATON_HOME/skills`, in precedence order. The model sees summaries and loads content through
`skill`. In the main input, `$` lists skills; Tab/Enter completes, Enter sends, Esc closes the list.
Failures return drafts. Skills never grant permissions.
[Picker](.agents/specs/skill-picker.md) · [Format](.agents/specs/agent-skills.md).

`Ctrl-D` twice within one second quits. `Ctrl-C` clears a draft or interrupts its conversation;
`Esc` backs out one layer. `Ctrl-P` opens the palette. `/config` (alias `/settings`) shows resolved
settings; restart after configuration edits. `/new` starts an empty Conversation; `/resume` opens
history (aliases: `/continue`, `/sessions`, `/session`). Stop work and send or clear drafts before switching.

Rate limits offer **Retry** / **Edit & retry**: click or press `r` / `e` with transcript focus.
Editing preserves the old branch; Esc cancels.

Pastel Markdown supports code and tables. Native math needs verified text sizing (direct Kitty);
tmux/unverified terminals show labelled source. Formula clicks copy delimited TeX; drags select
whole formulas. [Limits](.agents/specs/math-layout.md).
Drag across entries to copy plain text; the Copy icon keeps Markdown. Tool clicks expand/collapse
without selecting. Approval cards stay in the conversation; Esc focuses input, Tab/click focuses the card.
[Keys](.agents/specs/interaction-routing.md#key-grammar) · [Copy](.agents/specs/selection-and-copy.md).

Copy uses local `pbcopy` or remote OSC 52: 8 MiB cap, no truncation, cancellation on exit.
Clipboard and [render preparation](.agents/specs/render-preparation.md) do not hold input.

Optional [pastel footer](examples/statusline-pastel.sh): Bash, jq, Nerd Font.
[Configuration](.agents/specs/status-line.md).

Run `cargo test --workspace` and [quality gates](.agents/standards/quality-gates.md), including
offline terminal smokes. Hooks: `git config core.hooksPath .githooks`.
Private state belongs in ignored `.local/`; fixtures, frames and `Cargo.lock` are tracked.
