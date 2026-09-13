# Spec — Conversation tree

| Field | Value |
| --- | --- |
| Status | Implemented and locally verified; PR CI pending |
| Owns | Semantic tree projection, durable navigation, tree modal and draft return |
| Depends on | JRN-1–JRN-8, TIM-1, CPL-5, COM-3/COM-6, CMC-2, INV-1/INV-3/INV-6; [context epochs](../ui-ux.md#context-epochs-and-branch-selection) |
| Proven by | Agent, runtime, store, TUI and CLI tests below; real PTY smoke |

## Invariants

**TRE-1 — One action opens one modal.** `/tree` and `/rewind` resolve to the same typed action.
The `Conversation tree` modal owns its cursor and any enabled search input, blocking input below it while
producer events continue. Native frames are locally inspected; the user will test the interaction directly.

**TRE-2 — Tree rows are semantic projections.** Stable entry identities and parent relationships
define the tree; record order supplies chronology, not lexical IDs. Audit records are not messages;
previews and any enabled query results are bounded and truncation/partial availability is explicit.

**TRE-3 — Rewind preserves the original continuation.** Rewind creates and selects a fresh named
head in one journal mutation at a validated stable boundary. Selecting an existing head moves only
the durable selection; rename preserves selection identity and abandoning the selected head refuses.

**TRE-4 — Navigation commits before becoming visible.** Runtime admission revalidates origin,
revision, target and idle state; JRN-7 acknowledgement precedes projection replacement and draft
return. Busy/stale/refused navigation starts no work, and an uncertain write requires reopen.

**TRE-5 — Draft restoration preserves user input.** User targets return exact text and historical
explicit skill binding; resubmission follows normal current skill preparation. Like Pi, an existing
nonempty composer draft takes precedence; otherwise install the returned pair without concatenating.
Browsing, cancel and failed navigation preserve the original draft, binding and focus.

**TRE-6 — Selection follows identity and focus.** Hover and keyboard share a tree cursor separate
from transcript text selection. Filtering, folding, child dialogs and resize retain a valid selected
ID or a defined visible ancestor; empty/root results are explicit states, never negative indices.

**TRE-7 — Context follows destination ancestry.** A user target is before its turn; a completed
assistant target includes that output. A complete tool batch is indivisible. Rewind performs no
provider/tool replay, starts no turn, and applies CPL-5 at the destination rather than the source.

**TRE-8 — Labels and copy use semantic source.** Labels attach to stable node IDs without becoming
model messages or navigation endpoints. Copy obtains bounded exact source text, never the decorated,
shortened tree row. Label mutations must invalidate tree snapshots without breaking context accounting.

## Implementation map

Paths below are relative to the checkout. Backend and native boundaries are implemented. Core vocabulary never imports the journal, runtime or terminal.

| Owner | Files | Responsibility |
| --- | --- | --- |
| Shared vocabulary | Core `conversation_tree.rs`, `tree_snapshot.rs`, `tree_edit.rs`, `tree_source.rs` | Immutable origins, rows and addressed navigation/edit/copy requests |
| Journal | Agent `journal.rs`, `journal/heads.rs`, `journal/tree.rs`, `journal/tree_edit.rs`, `journal/tree_source.rs` | One durable selection; bounded semantic projection; annotations and exact source |
| Navigation planning | Agent `journal/navigation.rs`, `record/navigation.rs`, `turn/navigation.rs` | Canonical stable-boundary validation, destination projection and returned draft; no retry or turn start |
| Runtime | `runtime/tree_admission.rs`, `runtime/navigation.rs`, `runtime/tree_edit.rs`, `runtime/transition.rs` | Shared idle/origin fence; owned nonblocking commit; acknowledgement-gated receipts |
| UI state/rendering | `crates/plexmaton-tui/src/surface.rs`, `layout/registration.rs`, `render/mod.rs`, `workspace/hover.rs`, `workspace/pressed.rs` | One tree state and modal surface using native layout/hover/press machinery |
| Input/composition | `crates/plexmaton-tui/src/router.rs`, `state/composer_menu/grammar.rs`, `state/composer_menu.rs`; `crates/plexmaton-cli/src/input.rs`, `interaction.rs` | Alias-aware command parsing/completion, modal routing, runtime result conversion |

Do not add agent/runtime dependencies to TUI. HeadRevision and JournalSequence currently belong to
agent; convert to a small core-owned tree revision token at the boundary rather than importing them.
Existing `ConversationId`, `AgentId`, `ConversationEntryId` and `HeadName` remain identity owners.

## Data boundary

Use these logical fields; serialization belongs only where data is actually durable:

- `TreeOrigin`: conversation, displayed agent, selected head and a snapshot revision derived from
  the complete acknowledged journal sequence. This conservative token rejects stale trees even
  when only an audit/label changed. Do not invent a second mutable version counter in the UI.
- `TreeRow`: entry ID, nearest visible semantic parent ID, chronological ordinal, row kind,
  bounded preview, optional label, head markers, active-ancestry flag and typed navigation eligibility.
  Group assistant blocks and complete tool batches; map each displayed row back to a canonical
  boundary. Do not duplicate shared ancestors by walking every head independently.
- `TreeSnapshot`: origin, rows and heads; acquisition errors carry typed limit diagnostics. Build from acknowledged
  state on demand, not on every streaming delta. Presentation caches are disposable projections.
- `TreeNavigation`: origin plus `Rewind(entry_id)` or `SelectHead(name)`. Rewind resolves the actual
  boundary in the agent, never from caller-supplied role/parent data. Head management has explicit
  typed operations; do not parse display strings back into commands.
- Agent `TreeNavigationResult`: selected head, optional mutation sequence and returned text/skill
  pair. `Reaction`/`DispatchReport` carry a dedicated receipt beside `projection_reset`;
  `TreeEditResult` carries the new origin without a reset. Neither is an `UndeliveredInput`.

Snapshot acquisition is bounded to 64 active heads with at most 256 UTF-8 bytes per head name
(checked before cloning even for older journals), 16,384 unique ancestry entries, 65,536 scanned
journal records, 2,048 semantic rows, 512 UTF-8 bytes per preview including its truncation marker, and 256 KiB of aggregate preview
text. Crossing an acquisition limit returns a typed error and no partial snapshot; shortening an
individual preview sets its explicit truncation flag. Label and edited branch names are bounded to
256 UTF-8 bytes, reject whitespace-only text and control characters, and preserve accepted text.
`None` clears a label; equal metadata is a no-op. Query limits are needed only if search is included.
No silent partial tree or clipped rule that reads as complete. Start with bounded in-memory
projection and visible-row rendering; a database/index or async paging framework is not authorized
by hypothetical scale. Exact copy has an 8 MiB assembled-byte limit including separators and refuses
oversized or incomplete source rather than shortening it. Text/reasoning blocks remain in source
order; tool groups include each tool name, raw arguments and terminal output/failure, joined by
blank lines. Source bytes within each part remain unchanged. Opaque provider replay is excluded;
reused tool-call IDs are resolved by canonical assistant ancestry, never a global last match.

## Journal operations

`ForkAndSelectHead` is one journal mutation containing normal sequence
and record identity, expected source head/revision, fresh destination name and destination entry.
Validation checks source ownership/revision, name availability and stable target before changing
anything; reduction creates the new head and selects it together. Generate names from journal
sequence and check availability, never from a UI row number or wall-clock-only guess.

`SelectHead` names an existing destination, with expected selected origin and destination
revision. Runtime first compares TreeOrigin against current acknowledged state. Rename updates a
selected head in the same mutation; refuse abandoning a selected head until another is selected.
`SetEntryLabel` changes annotation metadata only, not model context, head revision or entry payload.

The selected head belongs to ConversationJournal and is reconstructed from records. `Record`
derives its head from that owner; `Record::from_journal` projects the durable selected head.

Old journals without selection records default to their existing main selection. Preserve healthy
old bytes/header and supported epochs under JRN-3; additive records do not justify header rewrites.
Test old healthy journals and active-head rename/abandon interactions explicitly. Blank automatic
sessions still materialize only on accepted input: opening an empty tree writes nothing and offers
no rewind target. Metadata edits on an empty semantic tree refuse before staging a write; this
preserves AutomaticJournal's bootstrap exception rather than widening it.

## Navigation state machine

| State/event | Required transition |
| --- | --- |
| Open tree | Snapshot acknowledged state; retain draft/binding/return focus; no model or journal effect |
| Fold/hover; search/filter if enabled | Update only tree presentation; stable selected identity and scroll |
| Close before admission | Dismiss and restore underlay/input; no navigation mutation |
| Enter while busy | Typed Busy; tree stays usable; do not interrupt live work implicitly |
| Enter stale/invalid target | Typed refusal; retain draft and selection where valid; refresh snapshot explicitly |
| Valid idle Enter | Stage one owned mutation and pending result; show pending state, not new context |
| Journal acknowledges | Publish replacement then returned draft; close overlay; normal submission remains a separate action |
| Definite/unknown write failure | Reuse JRN-7 freeze/reopen rules; publish no successful navigation and lose no draft |
| Close/interrupt/shutdown after admission | Accepted writes stay owned and joined; do not claim rollback. dismissal preserves the owned write |

Idle excludes an active agent turn, pending approval, agent/runtime queued input, model/tool work,
compaction, skill preparation and pending commit. The runtime owns one navigation-admission
predicate shared with metadata edits over these existing owners, also refusing shutdown,
journal-frozen state and an unconsumed tree receipt or projection reset.
`has_active_work()` alone is insufficient: a tool admission worker can finish while the agent
remains in `Turn::Working` awaiting approval. Combine it with the agent turn/queue state;
prove this gap with a pending-approval navigation refusal test in slice 10.3.
Selecting the already-current destination is a no-op. User rewind preserves the historical explicit
skill selection, including numeric names; it does not read current skill files until resubmission.
An unsent returned draft need not persist across process exit; the selected branch/context must.

## Native interaction

The tree fills the content rectangle above the existing Status/quit row. The Drawer can open
above it without losing the tree's cursor or draft. The single key/pointer grammar is owned by
[interaction routing](./interaction-routing.md#key-grammar): clicks select, Enter navigates,
child editors own their text, and dismissal never claims to roll back an admitted write.
Search and branch summaries are not exposed. The user chose the full-viewport modal, aliases,
close button, shared selection and Pi-style folding/labels; native visual evidence is below.

Pi's branch-summary choice is unresolved scope within this feature. It is model-generated context
from the old branch to the common ancestor, added at the destination; it is not a tree label or
an existing compaction checkpoint. Do not expose functional-looking summary controls until its
own prompt, persistence, cancellation and cache semantics are implemented and reviewed.

### Optional search

Rewind must be usable and shippable through browsing and selection alone. Include search only
when it is a small, well-tested extension of existing tree projection and input handling; omit it
if it needs separate infrastructure or substantial focus, state or performance machinery.
Rejected: requiring Pi search parity before rewind, because locating a target does not justify
delaying safe navigation or growing a second feature. A search field in the HTML fixture is not
a production requirement. If included, define scope/matching and prove TRE-1/TRE-2/TRE-6 for it;
search-only acceptance cases do not block a search-free delivery.
Keep the extension point in existing semantic snapshots, stable IDs and separated input state.
Use concrete Rust types/functions first; do not add an unused search trait, placeholder intent or
UI control. Introduce an abstraction only when an implemented boundary earns it under
[architecture](../standards/architecture.md#abstraction-discipline).

## Acceptance coverage

### Native validation

Actual native frames were locally inspected at 120×30, 88×30, 60×30 and 48×12, including the label editor and branch selector. No preview generator or design mockup is shipped. `scripts/smoke-tree.py` separately proves actual terminal switching with Chinese text, exact destination context, original-branch return, selected-head restart and one command effect across six loopback requests.

### Behavioral boundaries

The table defines coverage, not a claim that every case passes. The evidence table below owns
observed proofs and outstanding boundaries. Prefix proof tests with `tre_` and cite the invariant.

| Layer | Cases that must fail for a plausible broken implementation |
| --- | --- |
| Agent journal | Fork/select roundtrip; stale source; name collision; missing/foreign target; selected rename; selected abandon refusal; source path unchanged |
| Agent projection | User-before and completed-assistant-after; multi-block answers; complete versus interior parallel batch; failed/cancelled terminal; before/between checkpoints; root/empty tree; chronology independent of IDs |
| Store | Old epoch/header unchanged; new selection survives reopen; injected failed/partial write cannot report a half-completed fork/select |
| Runtime | Writer barrier pauses reset/draft; Busy in every owned-work state; no model/tool call on navigation; source unchanged; later submit exactly once; shutdown joins accepted navigation |
| Draft/skills | Existing draft wins; empty draft gets exact selected text/binding; no concatenation; numeric explicit skill; unavailable current skill handled only on later submit |
| TUI | Aliases and completion; mouse/key same selected ID; no hidden shortcuts; press/drag/resize target invalidation; child-dialog return; deep branch; min-size dismissal; search clear versus close and empty filter only if included |
| CLI fixture | Both commands → tree → rewind → edit/send → original branch → exit/resume; exact provider context and no spurious interrupted turn |

Use existing nearby tests before adding new infrastructure: `journal/projection/tests.rs`,
`journal/validation_tests.rs`, `record/retry.rs`, runtime journal-failure fixtures,
`workspace/retry/tests.rs`, `workspace/hover_tests.rs` and `workspace/pressed_tests.rs`.
Public wire/loopback fixtures prove integration; paid/live model calls are not tests.

## Evidence

| Invariant | Proven by |
| --- | --- |
| TRE-1 | `tre_1_tree_frames_keep_branch_copy_and_status_at_each_breakpoint`, `tre_1_rewind_alias_completion_inserts_a_command_without_running_it`, `tre_1_hidden_input_and_selection_shortcuts_preserve_exact_draft_and_skill`, `tre_1_global_quit_confirmation_rearms_after_interrupt_under_tree`, `tre_1_minimum_size_escape_closes_drawer_before_tree_and_blocks_hidden_input`; `scripts/smoke-tui.py` and `scripts/smoke-tree.py` |
| TRE-2 | `tre_2_all_heads_deduplicate_shared_ancestry_and_keep_chronology`, `tre_2_empty_root_and_exact_head_limit_are_explicit`, `tre_2_legacy_head_names_are_byte_bounded_before_snapshot_retention`, `tre_2_ancestry_and_scanned_record_limits_are_explicit`, `tre_2_node_preview_and_aggregate_bounds_are_typed_and_utf8_safe`, `tre_2_groups_assistant_blocks_and_parallel_tool_batch` |
| TRE-3 | `tre_3_fork_and_select_creates_and_selects_without_moving_source`, `tre_3_select_head_moves_only_durable_selection`, `tre_3_create_head_does_not_change_selection`, `tre_3_stale_source_revision_refuses_fork_and_select`, `tre_3_name_collision_refuses_fork_and_select`, `tre_3_missing_target_refuses_fork_and_select`, `tre_3_foreign_source_refuses_fork_and_select`, `tre_3_unstable_target_refuses_fork_and_select`, `tre_3_rename_of_selected_preserves_selection_identity`, `tre_3_abandon_selected_refuses`, `tre_3_old_journals_without_selection_records_default_to_main`, `tre_3_select_head_stale_destination_revision_refuses`, `tre_3_from_journal_projects_the_durable_selected_head`, `tre_3_fork_and_select_survives_reopen_without_rewriting_the_header`, `tre_3_partial_fork_and_select_write_does_not_select_or_create_the_destination`, `tre_3_rejected_fork_and_select_writes_nothing` |
| TRE-4 | `tre_4_navigation_is_acknowledgement_gated_cancellation_safe_and_report_owned`, `tre_4_pending_approval_refuses_navigation_without_flushing_or_writing`, `tre_4_queued_agent_input_refuses_navigation_and_remains_owned`, `tre_4_stale_and_invalid_navigation_refuse_without_writing`, `tre_4_navigation_write_failures_freeze_without_result_or_undelivered_input`, `tre_4_shutdown_joins_an_accepted_navigation_append`, `tre_4_8_metadata_ack_is_cancellation_safe_and_preserves_context`, `tre_4_8_metadata_write_failure_freezes_without_success_or_fake_input`; `tre_4_duplicate_metadata_enter_and_close_do_not_cancel_an_admitted_write`, `tre_4_5_report_preserves_drawer_and_rebases_its_return_focus`, `tre_4_8_edit_report_refreshes_acknowledged_metadata_without_reopening` |
| TRE-5 | Agent source pair: `tre_4_5_user_rewind_returns_exact_text_and_numeric_skill_without_effects`; `tre_4_5_report_restores_draft_only_after_ack_and_preserves_occupied_input`, `tre_4_5_failed_report_keeps_pending_tree_input` |
| TRE-6 | `tre_6_folding_an_ancestor_keeps_selection_on_a_visible_identity`, `tre_6_refresh_retains_valid_cursor_fold_and_view_mode`, `tre_6_pointer_and_keyboard_navigation_use_stable_rows_and_branches`, `tre_6_stationary_pointer_repeat_does_not_override_keyboard_cursor`, `tre_6_pointer_selects_a_branch_before_explicit_head_navigation`, `tre_6_tree_press_drag_resize_and_escape_never_release_activate` |
| TRE-7 | `tre_4_7_invalid_targets_refuse_atomically_with_typed_reasons`, `tre_4_7_foreign_agent_target_refuses_without_mutation`, `tre_7_assistant_rewind_keeps_the_complete_tool_batch`, `tre_7_rewinding_first_user_selects_root_before_any_request_atom`, `tre_7_rewind_projection_excludes_a_newer_source_checkpoint`, `tre_7_steering_rows_are_visible_but_not_rewindable`, `tre_7_checkpoint_row_is_visible_but_not_rewindable`; `scripts/smoke-tree.py` |
| TRE-8 | `tre_8_label_set_clear_and_noop_leave_context_and_accounting_identical`, `tre_3_8_head_edits_preserve_selection_and_history_and_refuse_selected_abandon`, `tre_8_invalid_metadata_targets_and_names_refuse_without_mutation`, `tre_8_empty_history_refuses_metadata_without_a_write`, `tre_8_snapshot_reads_the_authoritative_node_label`, `tre_8_tree_copy_reads_full_source_and_rejects_stale_or_foreign_rows`, `tre_8_assistant_copy_preserves_blocks_without_opaque_replay`, `tre_8_copy_capacity_is_exact_and_never_silently_truncates`, `tre_8_labels_revalidate_utf8_bounds_and_single_line_semantics`, `tre_3_8_labels_and_head_edits_reopen_without_rewriting_supported_headers`, `tre_8_partial_label_write_keeps_the_prior_annotation`; `tre_8_y_and_ctrl_y_return_the_same_workspace_copy_request`, `tre_8_label_editor_paints_its_caret_and_escape_returns_to_browsing`, `tre_8_copy_from_a_head_uses_its_marked_semantic_row` |
