# Workspace trust and permission modes

| Field | Value |
| --- | --- |
| Read when | Designing first-workspace confirmation, default access roots or permission modes |
| Status | Source comparison retained; implemented activation owned by PER-8 |
| Corpus | Revisions and sibling directories are pinned in [the spike](./README.md) |

## Three separate decisions

Workspace trust controls whether repository-owned configuration may activate integrations or affect
policy. Tool permission decides whether an admitted operation runs, prompts or is refused. OS
containment limits what the running process can actually access. A trust marker, remembered tool
grant and filesystem sandbox are not interchangeable.

Grok's `crates/codegen/xai-grok-workspace/src/folder_trust.rs:1` describes the config-execution gate:
when enabled, stored trust and the presence of repo-local execution configuration determine whether
to prompt. No such configuration means nothing to gate. Its local/dev build path disables the gate;
release behavior is separately feature-controlled. `trust.rs` stores decisions under the user home,
not in the checkout. `permission/resolution.rs:236` also gates project-sourced permission rules.
This is not a universal prompt before reading every project.

Kimi's `packages/agent-core-v2/src/workspace/workspaceTrust/workspaceTrust.ts` defines a distinct
workspace owner. Its first consumer gates project MCP config (`.mcp.json`, `.kimi-code/mcp.json`)
so repo configuration cannot start servers before trust. `workspaceTrustService.ts` writes the
marker outside the project, under Kimi's home; failed reads yield untrusted. The engine itself has
no interactive trust prompt. It reads the marker at initialization; another process's change is
observed only after restart in this source version.

## Tool policy categories

| Dimension | Grok Build source | Kimi `agent-core-v2` source |
| --- | --- | --- |
| Operation | `AccessKind`: Read, Grep, Edit, Bash, MCPTool, WebFetch, WebSearch, AgentMessage | Tool name plus execution metadata, file accesses and tool-provided rule matcher |
| Rule outcome | Allow / Ask / Deny; explicit rules resolve deny > ask > allow | approve / ask / deny; ordered policy chain, with configured deny before session grants before configured ask/allow |
| Mode | Default, acceptEdits, dontAsk, bypassPermissions and auto are parsed separately from sandbox profiles | manual / yolo / auto; plan and other harness constraints are separate veto listeners |
| Auto | Classifier path with safe fast paths, heuristic and LLM implementations; actual wiring selects the reviewer | `auto-mode-approve.ts` returns approve directly when mode is auto; earlier deny/guard decisions still apply |
| Reuse | Remembered project grant/deny state; command grant scope is verified against the operation | Session approval patterns fold from durable records; configured rules are supplied separately |
| Defaults | Resource-specific decisions, then rule/mode/grant handling; not merely one allow-everything flag | Read/Grep/Glob and helper tools are on the default-approve list after earlier gates; ordinary POSIX Write/Edit inside a Git workspace can also auto-approve |

Grok entrypoints: `permission/types.rs:113,328`, `permission/rules.rs:9`,
`permission/auto_mode/mod.rs:1,354` under `crates/codegen/xai-grok-workspace/src`.
Kimi entrypoints under `packages/agent-core-v2/src/agent/permissionPolicy`:
`types.ts`, `permissionPolicyService.ts`, and `policies/{auto-mode-approve,yolo-mode-approve,
default-tool-approve,git-cwd-write-approve,sensitive-file-access-ask,git-control-path-access-ask}.ts`.
Kimi's yolo node comes after more ask gates than its auto node. The names alone do not define the
policy; neither maps directly onto a Plexmaton mode yet.

## Containment is still independent

Codex's `codex-rs/linux-sandbox/README.md` documents a system-or-bundled bubblewrap path and explicit
fallback behavior. Grok's `crates/codegen/xai-grok-sandbox/src/lib.rs` instead describes `nono`
Landlock/Seatbelt enforcement, including per-child Linux network filtering, and also contains
bubblewrap-related handling. Its profiles include workspace, devbox, read-only, strict and off.
Neither stack was built, executed or benchmarked here; no relative-weight claim is established.

Kimi's inspected Bash tool uses its injected process runner. Its agent profile explicitly describes
the host as unsandboxed (`app/agentProfileCatalog/system.md:81` under `agent-core-v2/src`). The
permission chain and workspace path utilities do not establish an OS containment guarantee.
Likewise, Plexmaton's current command tool remains governed by CMD-2 and the command spec's
workspace-root limit; approval cannot confine a shell after launch.

## Plexmaton boundary

- Native read/search: the WFS-1 pinned workspace root. A separate scratch root needs its own
  admission/executor work.
- Native mutations: allow once or use the [session edit shortcut](./permission-state.md), backed by
  a scoped grant. P6 and APV-3 still apply.
- Shell: ask or reuse PER-4/PER-10 exact-command or literal-prefix grants. Do not promise its reads,
  writes or network are constrained to native file-tool roots.
- Workspace trust: the [skills integration](./config-ownership.md) adds narrow project model selection
  and skill reads. Neither grants execution capabilities. Project-provided permission Allow rules
  use PER-8’s exact-byte personal activation; project MCP/hooks remain future work.

First-workspace confirmation can show concrete roots and defaults, but it must not silently grant
unrestricted execution. Extra roots, trust, remembered permissions and revocation should remain
explicit policy operations; conversation rewind cannot undo current revocation. A first-workspace screen remains a design proposal; current controls and wording live in PER-7/PER-8.
