# Shell prefix source comparison

| Field | Value |
| --- | --- |
| Read when | Comparing the parser/matcher choices behind PER-10 |
| Status | Source comparison complete; production implementation and evidence live in PER-10 |
| Contract | [Permission policy](../../specs/permission-policy.md) PER-4/PER-10; CMD-1/CMD-2 |

The approved approval journey remains **Allow once**, **Allow and remember…**, and **Deny**.
Remember opens the backend-issued scope and its available Session/Project lifetimes. Prefix
selection adds a concrete scope to that journey; it does not add a direct Always allow button or
an unreviewed rule editor. Production grammar, bounds, fallback and tests have one owner: PER-10.

## What the local sources establish

The source pins are the spike's corpus revisions. Their tests were read, not run in this audit.

| Source | Mechanism inspected |
| --- | --- |
| Codex `codex-rs/shell-command/src/bash.rs` | `try_parse_word_only_commands_sequence` walks a tree-sitter Bash tree, accepts listed literal syntax and operators, rejects parse errors, redirections and dynamic expansion, then preserves each command's argument vector |
| Codex `codex-rs/execpolicy/src/rule.rs` | `PrefixPattern::matches_prefix` compares argument tokens rather than raw string prefixes |
| Codex `codex-rs/core/src/exec_policy.rs` | `derive_requested_execpolicy_amendment_from_prefix_rule` validates a proposed prefix; `prefix_rule_would_approve_all_commands` evaluates the proposed rule against every parsed command before offering it |
| Grok `permission/manager/bash_grants.rs` | Prefix labels must re-derive from the parsed command. Ambiguous word joins cannot silently become different argument vectors; `always_allow_row_is_effective` checks that saving the displayed scope would actually allow the script |
| Grok `permission/bash_command_splitting.rs` | Tree-sitter parsing, source spans and wrapper/context handling are separate from choosing a highlighted prefix. Its accepted grammar is broader than Codex's and requires the surrounding policy checks |

Grok paths above are under `crates/codegen/xai-grok-workspace/src`. Its splitting function's prose
still says redirects are disallowed, while the current node/token allowlists include them; the
surrounding redirect checks matter. Copying a splitter without its enforcement would lose that
boundary.

## Decision and limits

Use the maintained Tree-sitter Bash grammar at the native command adapter, then lower a bounded
POSIX subset for the actual `/bin/sh` executor. The semantic agent receives complete immutable
literal operations; it neither imports the parser nor splits a display label. Literal parsing and
the choice of a useful suggestion remain separate responsibilities.

The comparison led to token-prefix matching, whole-request effectiveness and exact fallback for
unsupported syntax. The production tests additionally found a backslash-newline disagreement
between syntax-tree word nodes and `/bin/sh`; that gap now refuses reusable parsing. Generated
accepted quoted words are compared against a fixed `printf` in the executor dialect.

The initial suggestions cover `ls` and the specific Git subcommands listed in PER-10. A remembered
fetch prefix cannot cover a neighboring status command. One prefix may cover every operation in a
supported same-family sequence; partial grants are not combined. Compound requests do not receive
an automatic prefix suggestion. Explicit configuration can narrow an argv prefix, while the
approval UI currently chooses lifetime for the one issued scope.

No LLM, shell execution or executable-effect inference derives a production prefix. Scope is not
OS confinement, and the effects of hooks, configuration and executable children remain CMD-2's
boundary. External source tests were read, not run in this comparison.
