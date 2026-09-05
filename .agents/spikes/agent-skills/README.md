# Agent Skills spike

| Field | Value |
| --- | --- |
| Status | Implemented and verified offline; live model behavior unverified |
| Read when | Revisiting skill roots, format compatibility or activation ownership |
| Question | How do portable skills and project settings fit the existing tool and journal boundaries? |
| Decision | Own a concrete skills adapter; reuse tool admission, request accounting and journal replay |
| Contract | [SKL-1–SKL-6](../../specs/agent-skills.md) |

## Evidence and selected design

The [local comparison](./sources.md) records six source revisions and the relevant implementation
paths. References were inspected, not modified or built. Production work starts from `13b5331`
in `.worktrees/agent-skills`, branch `spike/agent-skills`.

The [Agent Skills format](https://agentskills.io/specification) specifies `SKILL.md` with YAML
frontmatter and Markdown instructions; the
[client integration guide](https://agentskills.io/client-implementation/adding-skills-support)
treats discovery roots as client policy. Project `.agents/skills` interoperates with other clients;
project `.plexmaton/skills` and user `PLEXMATON_HOME/skills` provide Plexmaton's own scopes.
The spec owns precedence, bounds and invocation grammar rather than this report copying them.

Codex's physical-worktree configuration tests, and DSH/Kimi's native-plus-shared project skill
roots, demonstrate that project settings coexist with a user runtime root. The former blanket
project-directory prohibition had no separate technical justification in its introducing commit
`8342c34`. The roadmap now protects user ownership while SKL-1 implements narrow project selection.

Pi's small catalog and DSH's separate summary/body boundary informed the adapter. Plexmaton publishes
summaries in its existing `skill` tool definition, so the provider's request fingerprint and budget
already account for them. A second durable catalog is unnecessary: tool results retain model-loaded
content, and a typed journal atom retains explicitly loaded content separately from the user text.
All four codecs consume the same semantic activation.

`plexmaton-skills` owns discovery, format validation and exact content reads. It depends on the
existing file-tools confinement boundary rather than a framework or its own filesystem policy.
Runtime ownership covers async loading and shutdown; neither parser nor filesystem code enters the
semantic agent crate. Explicit edit/retry transfers the edited input to the same owned preparation
before any branch mutation, preserving both failed input and the previous branch.

YAML grammar is delegated to [yaml_serde](https://github.com/yaml/yaml-serde); a typed visitor avoids
materializing unknown metadata trees. The
[reference validator](https://github.com/agentskills/agentskills/blob/main/skills-ref/src/skills_ref/validator.py)
uses NFKC names, so Unicode normalization is also a focused dependency. Their audits live in
[the Rust standard](../../standards/rust.md), with exact resolution in `Cargo.lock`.

## Review witnesses

The implementation review caught and corrected derived-root symlinks, path-based directory
enumeration, user-only resource reads without an exact recorded source, interrupt queue saturation,
cancellation-cause loss and session-picker cancellation that did not reach scanning. Tests exercise
the corrected boundaries through real filesystem operations, agent transitions and owned loopback
HTTP. No test contacts a configured model service.

Rendered skill diagnostics were inspected at
[wide](../../../crates/plexmaton-tui/frames/skill-diagnostic-wide.txt),
[medium](../../../crates/plexmaton-tui/frames/skill-diagnostic-medium.txt), and
[narrow](../../../crates/plexmaton-tui/frames/skill-diagnostic-narrow.txt) widths. They use the real
TUI renderer with synthetic conversation events and show the notice, tool row and returned draft;
they are not evidence of live model quality.

## Reproduction and limits

Run from this worktree with its own `target/`:

```sh
cargo test -p plexmaton-skills
cargo test -p plexmaton-runtime --test skills
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/check-crate-graph.sh
./scripts/check-citations.sh
./scripts/check-doc-budget.sh
python3 scripts/smoke-tui.py
python3 scripts/smoke-statusline.py
```

The [stage record](../../phases/phase-04-product-polish.md) owns final gate receipts; the
[mechanism spec](../../specs/agent-skills.md) owns named proofs. Live provider compatibility,
realized performance and automatic compaction of active skills remain unverified. Installation,
watching, recursive/flat formats and arbitrary project provider definitions are outside this change.
The new typed activation uses journal epoch `2026-09-05`; earlier epochs have no migration reader.
