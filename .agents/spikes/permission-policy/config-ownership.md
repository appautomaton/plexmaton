# Configuration and skills coordination

| Field | Value |
| --- | --- |
| Read when | Extending project configuration, permission storage or skill-aware admission |
| Status | Configuration/skills comparison retained; permission behavior owned by PER-8 |
| Source | Skills commit `b3333645c5ac69de803cb94ee2c8e5aa46796f1f`, rebased onto main's rendering/math foundation |

The source commit establishes the shared discovery boundary. [PER-8](../../specs/permission-policy.md)
and [PGR-1–PGR-5](../../specs/project-permissions.md) own its implemented permission extension.

## Existing directory ownership

| Location | Owner and current behavior |
| --- | --- |
| `PLEXMATON_HOME/config.toml` | User settings and provider/model definitions; credential values still come from the named environment variables |
| `PLEXMATON_HOME/skills/` | User skill bundles across projects |
| `PLEXMATON_HOME/sessions/` | Runtime-owned session records |
| `<project>/.plexmaton/config.toml` | Project model selection and PER-8 permission rules |
| `<project>/.plexmaton/skills/` | Plexmaton project skill bundles |
| `<project>/.agents/skills/` | Shared project skill bundles; not a Plexmaton configuration or permission-state directory |

The user-root default is `~/.plexmaton`. Its override does not redirect project directories, and
project settings cannot redirect sessions or credentials. TOML is already used by both settings
readers. YAML frontmatter belongs to the skill bundle format and is unaffected by this choice.
Do not add another project settings file or local override tier merely to implement permissions.
An additional tier would need an actual use case and defined ownership.

## Project identity differs from execution root

`crates/plexmaton-cli/src/startup.rs:32` keeps two paths: canonical startup cwd for native tools,
and the result of `project_config::discover_project_root` for project settings and skills. Discovery
finds the nearest physical checkout, accepts linked-worktree gitfiles and falls back to cwd without
a Git marker. A gitfile's target is validated but never becomes the project root.

Permission configuration and the personal project store use that discovery result.
An exact command grant must still bind its actual cwd/root and environment. Starting from another
subdirectory can therefore use the same project namespace without inheriting an exact command grant
for a different cwd. A broad project mutation scope must be explicitly defined in project-relative
terms and may never widen the native executor's root. Project discovery is not file authority.

The finite permission model's `WorkspaceId` represents one admitted execution root; it does not
yet prove the separate project/store identity or nested-cwd cases. Keep that limitation explicit.

## Skill discovery and reads already have an owner

The concrete path is `startup.rs` → `NativeToolCatalog::with_skill_roots` →
`SkillCatalog::discover`. Discovery visits project `.plexmaton/skills`, project `.agents/skills`,
then user `skills`; the first validated name wins, with shadowing diagnostics. It retains bounded
metadata and descriptor-pinned source roots, not every skill body in model context.

`SkillCatalog::read` in `crates/plexmaton-skills/src/loading.rs` loads the selected body or a relative
resource on demand. It validates invocation policy, rechecks source metadata, rejects traversal and
symlink authority, applies byte limits and supports cancellation. A disappeared winning source is
an error, not permission to switch to another origin. Returned content carries source, canonical
location and digest.

Model calls use the `skill` tool adapter in `crates/plexmaton-runtime/src/native/skill.rs`, which
declares FileRead and enters ordinary admission/policy. Explicit `$name` input uses owned runtime
preparation and the same catalog reader before journaling activation. The picker projects metadata
and performs no file reads. These are distinct activation entrypoints; any future deny rule intended
to govern both must be checked at both relevant boundaries, not assumed to cover explicit input just
because it covers model tool calls. Loaded instructions and scripts confer no execution authority.

Permission subjects should reuse the selected skill origin and source identity; do not rediscover
directories in the policy evaluator or make generic file tools read all of the user's home. The
skill selection precedence is not permission-rule precedence. P3 still governs permission decisions.

## Settings and grant persistence

Both existing TOML readers use the same strict permission declaration grammar. PER-8 owns its
bounds, startup/refresh lifetime and exact-byte trust activation. PGR-5 owns the personal grant log;
Conversation JSONL is historical evidence under PER-9. Skill loading and model selection confer
no project Allow authority. Configuration write-back remains unimplemented.

## Implementation references

The [skills contract](../../specs/agent-skills.md) and [picker contract](../../specs/skill-picker.md)
own behavior and its evidence. Concrete readers are
[project settings](../../../crates/plexmaton-cli/src/project_config.rs),
[startup](../../../crates/plexmaton-cli/src/startup.rs),
[discovery](../../../crates/plexmaton-skills/src/discovery.rs),
[content loading](../../../crates/plexmaton-skills/src/loading.rs) and the
[runtime adapter](../../../crates/plexmaton-runtime/src/native/skill.rs).
