# Spec — Motion

| Field | Value |
| --- | --- |
| Status | MOT-1 and MOT-3 implemented for the effort rail; MOT-2 unproven until the activity line moves |
| Owns | When anything in the workspace moves, what a moving frame may change, and the one clock it runs on |
| Depends on | FR-1 and FR-3 in [frame-loop](./frame-loop.md); EFF-4 in [reasoning-effort](./reasoning-effort.md) as the first thing that moves |
| Proven by | `plexmaton-tui::workspace` effort tests below |

## Invariants

**MOT-1 — One visible-only motion clock.** The workspace owns one deadline at fifteen phases a
second over a 32 s cycle, and every moving cell reads its phase; nothing that moves owns a timer of
its own. Each moving thing answers only whether it is visible now. Absence of any visible moving
cell disarms the deadline, so an idle workspace wakes for nothing; a late wake lands on the current
phase and counts the next tick from itself, queuing no missed frames. Rejected: a clock per moving
thing, which is how the effort rail began and would have given the activity line a second one.

**MOT-2 — A motion frame is one cell.** Every glyph in a motion sequence has display width one, and
a test proves it for each sequence, so a frame can never widen or reflow a row. Unproven: the
effort markers are checked; no other sequence exists yet.

**MOT-3 — Motion changes presentation only.** A phase change invalidates the painted frame and never
the semantic revision or layout (FR-1), so replaying a journal reproduces the same rows whatever the
clock was doing.

## Grammar

The phase is a count, not a time: renderers derive their frame from it, so two moving things on
screen stay in step. The effort rail's marker completes a cycle every 1.6 s (EFF-4).
