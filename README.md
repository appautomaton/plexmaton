<div align="center">

# Plexmaton

**AI coding, clearly in view.**

[![CI](https://github.com/appautomaton/plexmaton/actions/workflows/ci.yml/badge.svg)](https://github.com/appautomaton/plexmaton/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-1.98.0-7b7df2?logo=rust)](rust-toolchain.toml)
[![Status](https://img.shields.io/badge/status-early_development-b76bd6)](.agents/roadmap.md)
[![App Automaton](https://img.shields.io/badge/by-App_Automaton-e2894f)](https://appautomaton.renocrypt.com/)

</div>

Plexmaton is an AI coding assistant for the terminal, built in Rust by App Automaton. Choose your model, follow the work, and return to saved conversations.

## Why Plexmaton?

- **Keep the work in view.** Streaming answers, tool activity and approvals share a responsive workspace with expandable details.
- **Choose your models.** Use OpenAI Responses, Chat Completions, Anthropic Messages or Gemini APIs.
- **Stay in control.** Approve an action once, remember a Session or Project permission, and review or revoke it in the Drawer.
- **Carry your context forward.** Saved conversations and automatic compaction preserve source history. Reusable `SKILL.md` instructions bring your workflows into the conversation.
- **Read comfortably.** Pastel Markdown, code, tables and native math stay visible during updates. Selection copies the representation you chose.

<p align="center">
<img src="crates/plexmaton-tui/frames/math/reply-88.svg" width="640" alt="Plexmaton terminal workspace displaying a formatted assistant response and native mathematical notation">
<br><sub>Workspace render preview. <a href=".agents/specs/math-layout.md">Native math support</a>.</sub>
</p>

## Get started

The current development target is **macOS on Apple Silicon**. Install the pinned Rust toolchain and ripgrep (`rg`).

```sh
git clone https://github.com/appautomaton/plexmaton.git
cd plexmaton
mkdir -p .local/plexmaton
cp examples/providers.toml .local/plexmaton/config.toml
```

Edit the copied configuration to select a provider and model you can access. Set the environment variable named by `api_key_env`, then launch:

```sh
PLEXMATON_HOME="$PWD/.local/plexmaton" cargo run -p plexmaton-cli --bin plexmaton
```

[Configuration reference](.agents/specs/provider-adapter.md#request-configuration)

The interface, tools and journals run locally. Model requests go to your configured endpoint. Commands are **not OS-sandboxed**. File tools stay within the workspace and refuse symlinks.

## Everyday controls

| Input | Action |
| --- | --- |
| `Ctrl-P` | Open the Drawer: configuration, conversations, permissions |
| `Ctrl-C` | Clear a draft or interrupt its conversation |
| `Ctrl-D` twice within one second | Quit |
| `Esc` | Back out one layer |
| `$` | Find and complete a skill |

The Drawer starts or resumes conversations and shows configuration and permissions. Rate limits offer Retry and Edit & retry.

[Full key guide](.agents/specs/interaction-routing.md#key-grammar) · [Skills](.agents/specs/agent-skills.md) · [Selection and copy](.agents/specs/selection-and-copy.md) · [Optional status line](.agents/specs/status-line.md)

## History and development

Conversations save on the first message; blank launches save nothing. Use `--ephemeral` to opt out. Exit prints a resume command. Journal epochs `2026-09-04` and `2026-09-05` remain readable without rewriting history.

One live agent is available today. Multi-agent collaboration is on the [roadmap](.agents/roadmap.md). Linux compatibility is not yet established.

Run `cargo test --workspace` and the [quality gates](.agents/standards/quality-gates.md). Tests use local model fixtures. Private state belongs in ignored `.local/`.

## More from App Automaton

[Website](https://appautomaton.renocrypt.com/) · [GitHub](https://github.com/appautomaton) · [Hugging Face](https://huggingface.co/appautomaton)

[pi-arcweld](https://github.com/appautomaton/pi-arcweld): a curated Pi coding workspace. [webmaton](https://github.com/appautomaton/webmaton): browser and web-research skills.
