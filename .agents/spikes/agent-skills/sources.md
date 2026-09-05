# Agent Skills source comparison

Read this when choosing Plexmaton's skill roots, metadata contract, loading boundary, invocation
semantics, or session representation. This is a bounded comparison of local snapshots, not a claim
about current upstream behavior or the Agent Skills standard. Paths are relative to
`/Users/ac/dev/agents/coding`.

| Source | Local revision |
| --- | --- |
| Pi | `853a80d26c90a14c1886f0ebb8ffaae133ca2185` |
| Grok Build | `72a61251fcffb464bcc687aeb5a998e5a98ec0c9` |
| DSH | `47f943859bef60e4160492346772ded9b24f765a` |
| Kimi Code | `17dfd49768f753a4f0fe97d8e7d3317dab560575` |
| Codex | `316795b3cf2a45e90d121d9f46499d4658b2645c` |
| pi_agent_rust | `b7b5988b3a4ee83cb2baae24c6a44fb182c68e58` |

## Observed implementations

| Source | Roots and discovery | Metadata and loading | Invocation, trust, and durability |
| --- | --- | --- | --- |
| Pi | User `~/.pi/agent/skills` is collected before project `.pi/skills`; first name wins and explicit paths follow. It accepts root flat Markdown and recursive `SKILL.md`, stops below a skill root, and honors ignore files. `pi-arcweld/pi-mono/packages/coding-agent/src/core/skills.ts:163-272,407-505`. | The catalog retains name, description, path, source, and a model-invocation flag. It validates 64-character kebab names and 1,024-character descriptions, but warns on most bad names; a missing description rejects. The body remains on disk. `skills.ts:67-127,277-380`. | The model uses the ordinary read tool; `/skill:name` expands explicitly. `disable-model-invocation` hides catalog entries. Untrusted project resources are omitted by `packages/coding-agent/src/core/resource-loader.ts:380-404`. No skill-specific replay contract was traced. |
| Grok Build | Scope order is local, repo, user, server, bundled, plugin. Dynamic discovery walks accessed-file ancestors deepest first through `.grok/skills`, `.agents/skills`, and optionally `.claude/skills`; recursion is bounded and sorted. `grok-build/crates/codegen/xai-grok-tools/src/implementations/skills/types.rs:3-33`; `.../skills/discovery.rs:112-145,824-921`. | It repairs YAML, normalizes names, may derive a description from a 2 KiB body peek, and parses `when-to-use`, paths, arguments, metadata, allowed tools, model, effort, and invocation flags. Discovery usually keeps metadata only; invocation reads the body. `discovery.rs:465-569,672-812`; `.../skills/skill.rs:455-522`. | User slash and model paths share XML rendering. The named tool implementation had moved elsewhere according to `skill.rs:36-44`, so full session behavior was not established. Parsed `allowed-tools` proves metadata support, not permission enforcement or elevation. |
| DSH | Rank order is project `.dsh/skills`, project `.agents/skills`, custom, user DSH, user Agents, bundled. Lower rank wins within a layer; the nearest scoped registry layer wins across layers. It scans direct bundles and flat files. `dsh/packages/skill/skill-filesystem/src/index.ts:241-260`; `dsh/docs/subsystems/skills.md:13-17,64-85`. | Providers list summaries and separately `get` complete definitions; bodies are reread rather than registry-cached. Local parsing requires name and description and recognizes canonical kebab model/user invocation fields. `skills.md:92-178,190-195`; `skill-filesystem/src/index.ts:793-835,992-1007`. | Bounded name/description catalogs and explicit user invocation both load bodies on demand under separate policies. Catalog and explicit-invocation context are typed durable messages. `dsh/packages/skill/tool-skill/src/index.ts:27-57,127-251`. `trustedHost` chooses a read route; it is not a tool-permission grant. |
| Kimi Code | Brand roots precede generic roots: user Kimi skills then user `.agents/skills`; project `.kimi-code/skills` then project `.agents/skills`. Discovery is first-win by root order. Session contributions replace the base in priority order. `kimi-code/packages/agent-core-v2/src/app/skillCatalog/skillRoots.ts:18-48`; `.../session/sessionSkillCatalog/skillCatalogService.ts:106-118`. | Discovery retains complete bodies. Directory skills require frontmatter; flat files may derive metadata. Kimi adds prompt/inline/flow types, parameter expansion, diagrams, plugins, and opt-in qualified subskills. `.../app/skillCatalog/parser.ts:50-110`; `.../fileSkillDiscovery.ts:41-127`. | User and model activations have typed origins. A `skill.activate` fact is recorded; replay reapplies it without activation events or telemetry. `.../agent/skill/skillService.ts:1-11,46-114`. No skill-specific permission elevation was found. |
| Codex | User and project config are separate provenance layers; project overrides user and session flags override project. Skills combine config-layer, plugin, extra, and `.agents/skills` roots from project root through cwd. `codex/codex-rs/config/src/config_layer_source.rs:15-50`; `codex/codex-rs/ext/skills/src/host_roots.rs:28-70,73-184`. | Catalog entries have opaque authorities, packages, and main resources; bodies are read through the owner. Enabled and prompt-visible are separate. The host parser projects name, description, and short description. `codex-rs/ext/skills/src/catalog.rs:6-18,42-80,171-190,251-263`; `codex-rs/skills/src/parser.rs:4-92`. | Catalogs are bounded replaceable world-state sections; explicit mentions load typed context fragments. `codex-rs/ext/skills/src/world_state.rs:68-120`; `.../extension.rs:355-500`. Source authority chooses the read route; invocation policy is not filesystem or tool authorization. |

Codex demonstrates checkout-local configuration without conflating it with user runtime state.
Nearest `.codex/config.toml` wins. A linked worktree reads configuration values from the physical
worktree's `.codex` directories, while Codex applies separate shared-repository behavior to hooks.
The latter is product-specific and need not be copied.
`codex/codex-rs/core/src/config/config_loader_tests.rs:2893-3054`.

The pi_agent_rust snapshot adds a lowest-precedence managed tier. A `managed: true` marker is
required before mutation, and mutations enter an audit ledger, preventing agent-authored skills
from shadowing or modifying user work. This informs later authoring rather than initial discovery.
`plexmaton/.references/pi_agent_rust/src/skills_managed.rs:1-13,24-28,156-233`.

## Application and limits

The [mechanism spec](../../specs/agent-skills.md) owns Plexmaton's implemented roots, precedence,
metadata and activation behavior. These reference implementations provide comparisons, not binding policy.

No reference repository was changed or built; its test suite was not run. Grok startup
discovery and Pi/Grok replay behavior remain partially traced.
