# Spec — Reasoning effort

| Field | Value |
| --- | --- |
| Status | Implemented locally; physical-terminal appearance awaits user testing |
| Owns | Conversation effort selection, its RGB projection and its visible-only animation |
| Depends on | PRV-6, CMC-1/CMC-2, INV-11, FR-1/FR-3/FR-4, STL-3 |
| Proven by | Runtime, TUI and production-loop tests below |

## Invariants

**EFF-1 — An effort change has one idle boundary.** The runtime validates the addressed agent,
declared model capabilities and idle state, then replaces its driver/model/request environment
atomically without a journal or network effect. Running turns, approvals, queued input, compaction,
persistence failure and shutdown refuse the change; historical request records remain untouched.

**EFF-2 — The selector keeps the full spectrum.** `none`, `low`, `medium`, `high`, `xhigh`, `max`
stay in canonical positions; unavailable stops are dark gray and cannot be selected. Arrows skip
disabled stops, pointer movement or a matching press/release previews a level (INV-3), Enter asks the runtime to apply
it, and Escape cancels; only runtime acceptance updates the composer's confirmed effort.

**EFF-3 — Effort has one RGB palette.** `theme/effort.rs` owns the selector and composer rule/label
colors: gray, pastel orange, cyan, green, then static and animated pastel rainbow. The rail is
neutral and its vertical-only ticks are colored. Composer rules stay static; the composer's max
letters and the selector's selected max marker/letters share one color phase. Xhigh stays static.
The status script owns its original styling.

**EFF-4 — Motion has a bounded visible owner.** Visible max labels share one 67 ms deadline;
late wakes coalesce and absence of a visible max label disarms it. Animation changes three composer
foreground cells and, while selected, four selector cells; its marker cycles triangle, square,
hexagon and circle in one cell every 1.6 s. Covered, collapsed, clipped or retry-editing composer
labels own no wake. Animation changes neither semantic revision nor layout; FR-3 owns output.

**EFF-5 — Configuration remains the default.** A confirmed override lasts while that Conversation
is open; new, resume and restart use the configured model default. Status snapshots read the current
runtime model's effort value; no animation tick runs or restyles the status script.

## Grammar

`/effort` followed by Enter opens the selector. Typing `/effort ` also opens it; trailing text
filters selectable levels without removing disabled stops from the spectrum. `/effort high` then
Enter applies the matching level, and `/effort default` resets to the provider default. Left/Right
or Up/Down change the preview, Enter confirms, and Escape cancels without clearing the draft.
An active press is cancelled before Escape closes the selector. Invalid or unknown choices do not
submit a message. The configured `allowed_reasoning_efforts` list is PRV-6's; absence is unknown
capabilities, not an inferred full list. Saving a new configuration default remains a file edit.

## Evidence

[Named proofs](../evidence/reasoning-effort.md), one row an invariant.
