# Plan — Phase 04 stage 16, reasoning effort

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) stage 16 |
| Contract | [EFF-1–EFF-5](../specs/reasoning-effort.md), PRV-6, CMD-1/CMD-2, INV-11, FR-1/FR-3/FR-4, STL-3 |
| Status | Active; slice 4 of 4 implemented and locally verified; user terminal testing pending |

## Outcome

The executable exposes `/effort` over per-model declared choices, with the full spectrum,
disabled stops, shared RGB colors and a bounded four-shape max animation. The user approved the
visual direction and requested direct implementation in `feat/reasoning-effort`, with no further
previews. EFF-1–EFF-5 own the resulting behavior; the prototype is removed.

## Slices

1. **Configuration — locally verified.** PRV-6 validates `allowed_reasoning_efforts` and the
   configured default. Luna's authorized local configuration uses max and all six explicit levels.
2. **Design — accepted.** The user's corrections select colored vertical-only ticks, shared
   palette ownership and triangle, square, hexagon, circle; the pentagon is removed.
3. **Runtime — implemented.** EFF-1 admits changes only at an idle boundary, preserving active
   and queued work. Driver replacement recomputes the environment without rewriting history.
4. **Executable — implemented.** EFF-2–EFF-5 wire command, keyboard/pointer, confirmed composer
   colors, current effort reporting and one shared animation deadline for visible max labels.
   The status line retains its original script styling. Focused runtime, production-loop and TUI tests close code validation; the user performs
   final interactive testing with their existing resume command.

## Order and why

Model capabilities constrain choices; approved geometry constrains interaction; runtime admission
precedes publishing a confirmed effort. Presentation ticks never rerun a status script.

## Deliberately not in this plan

Changing effort during active work, durable Conversation overrides, saving defaults from the
selector, model switching, logo animation and network model discovery.
