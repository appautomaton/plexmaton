# AGENTS.md source comparison

| Field | Value |
| --- | --- |
| Read when | Revisiting AGI-1–AGI-5 discovery, prompt placement or lifecycle |
| Question | Which instruction-file behavior fits the existing request environment? |
| Method | Read-only local source/test comparison on 2026-09-07; no reference-harness execution |
| Decision owner | [Agent instructions](../../specs/agent-instructions.md) |

Source paths below are relative to the sibling checkout named in the first column, beneath the
primary checkout's parent. References are optional local material, not build dependencies.

| Checkout and revision | Discovery and prompt | Lifecycle and limits |
| --- | --- | --- |
| `codex` · `9f70e348e0227980de97e361cce830236fb18317` | `codex-rs/core/src/agents_md.rs`, `context/user_instructions.rs`: global then Git-root to cwd; override/default/configured fallback; user context | `agents_md_manager.rs`, `context/world_state/agents_md.rs`: cached environment snapshot, cold-resume reconciliation against persisted context; project content truncated at a shared 32 KiB default |
| `grok-build` · `72a61251fcffb464bcc687aeb5a998e5a98ec0c9` | `crates/codegen/xai-grok-agent/src/prompt/agents_md.rs`: global/compatibility roots then Git-root to cwd; ordered named/rules files; synthetic user reminder | `xai-grok-shell/src/session/acp_session.rs` beneath `crates/codegen`: prompt snapshot and history persisted; full unbounded file reads; fail-soft omissions |
| `pi-arcweld/pi-mono` · `9767ba275f3e9a5ee0f5c5342249b629ab1b2282` | `packages/coding-agent/src/core/resource-loader.ts`, `system-prompt.ts`: global then filesystem-root to cwd; filename fallbacks and worktree duplicate suppression; system prompt | `agent-session.ts`, `sdk.ts` beside them: current files on new session/resume and explicit reload; unbounded reads, fail-soft errors |
| `kimi-code` · `f12d59e089e2531a33fbca30b26ffeabd5862b45` | `packages/agent-core-v2/src/agent/profile/context.ts`: brand/generic global then Git-root to cwd; native/shared files; system-prompt reference-data framing | `workspaceInstructionsService.ts`, `agentsMdReminderService.ts` under `packages/agent-core-v2/src`: watched snapshot but bound prompt stays frozen; nested tool access prompts a file read; 32 KiB warning retains full text |

All four implement instruction-file loading, but none supplies a universal contract to copy.
Codex and Kimi annotate authority explicitly. Pi's broader ancestry needs special handling for
worktrees. Grok's `xai-grok-tools/src/types/agents_md_tracker.rs` has lazy-loading tests, but no
production registration or `check_path` call was found; its registry registers the separate Cursor
rules tracker. File presence and documentation do not prove that executable behavior.

The source tests consulted were Codex `core/src/agents_md_tests.rs` and
`core/tests/suite/agents_md.rs`, Grok's tests within `prompt/agents_md.rs`, Pi
`packages/coding-agent/test/resource-loader.test.ts`, and Kimi
`packages/agent-core-v2/test/agent/profile/context.test.ts` plus reminder/profile-binding tests.

Plexmaton reuses its physical project-root boundary, pinned reader and immutable request environment.
It borrows root-to-cwd ordering and user-role attribution, makes incomplete loads explicit, and
uses model-directed nested reads. AGI-1–AGI-5 own the precise decisions and implementation proof;
this comparison establishes neither live model compliance nor external compatibility.
