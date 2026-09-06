# Permission integration investigation

| Field | Value |
| --- | --- |
| Read when | Comparing the finite permission model with its production integration |
| Status | Production invariants and evidence promoted to PER-1–PER-10 |
| Basis | Revision and P1–P6 pinned in [the spike](./README.md) |

The finite model uses `ArgumentsId`, one native edit definition and opaque workspace identity. Its
15 cases distinguish call identity from a broader reusable scope and retain temporary grants across
Conversation switches. It assumes atomic acknowledgement and has no parser, filesystem checks,
worker cancellation or journal wire. Those limits define the experiment's evidence.

Production owners remain concrete:

| Boundary | Source under `crates/` | Contract |
| --- | --- | --- |
| Admitted subjects and evaluation | `plexmaton-agent/src/permissions/`; command/file catalogs | PER-2–PER-4/PER-10 and APV-1–APV-3 |
| Pending call and permission preparation | `plexmaton-agent/src/turn/permission.rs` | PER-5 |
| Shared Session and current dispatch | `plexmaton-runtime/src/runtime/permissions/` | PER-1/PER-5/PER-6 |
| Personal project transactions | `plexmaton-permission-store/src/` | PGR-1–PGR-5 |
| Shared rule grammar and controls | `plexmaton-cli/src/permission_config.rs`, `permission_controls.rs` | PER-7/PER-8 |
| Historical evidence | `plexmaton-agent/src/journal/` | PER-9 and JRN-7 |

[Permission policy](../../specs/permission-policy.md) and
[project storage](../../specs/project-permissions.md) own the production behavior and proofs.
[Configuration/skills](./config-ownership.md) preserves the discovery comparison; the
[parser comparison](./command-prefixes.md) records the literal-shell findings. Command containment,
MCP identities and multi-agent runtime handoff remain separate work.
