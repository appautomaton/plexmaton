# Spike — Conversation chrome

| Field | Value |
| --- | --- |
| Question | How the working agent, the user's turn and the mark should look, decided from rendered candidates |
| Feeds | [stage 34](../../plans/phase-04-stage-34-conversation-chrome.md) and [stage 10](../../plans/phase-04-stage-10-branding.md) plans |
| Method | HTML mockups in the palette, opened for the user; two kitty scripts for what a browser cannot show |

Every decision below was made by the user on a rendered frame, 2026-09-21 and 2026-09-22. The
generators are in [`mockups/`](./mockups/); each writes one HTML page to
`plexmaton-conversation-chrome/` under the system temporary directory, outside this corpus, and
`spin.py`, `logo_term.py` and `cellsize.py` run in a terminal. Regenerate rather than trusting a description of them.

## What the user chose

| Surface | Chosen | Rejected on sight |
| --- | --- | --- |
| Activity line composition | mark, label, muted `· 32s · high effort`, `· quiet for 31s` once nothing has arrived; no token count | a red stall tell; estimated tokens |
| Activity mark | N2, the Nerd Font's circle, rounded square and square, outline then filled: chosen 2026-09-22 in a kitty window beside the Unicode cycles; the user asked for the filled half to keep it simple yet ever changing | C2′ and C2″, which shook in the user's terminal because mixed-size Unicode shapes centre differently; filled squares; braille; quarter circles; Claude Code's star |
| User message | band of lifted ground with a blue `›`, continuation indented | the accent bar; a bare prefix; a hairline bar; a band alone |
| Mark placement | centred in the empty conversation, name beneath, gone with the first message | a header or drawer slot, not yet asked for |
| Mark motion | K4 as the base: frame breathes between weights and drifts between palette slots; centre morphs circle, ring, rounded square, square, solid and outline | K1 to K3 as they stood |
| Mark glyphs | the Material Design family the Nerd Font carries, one cell each, drawn together so they centre together | Unicode geometric shapes from mixed blocks, which jump between fallback fonts |

The activity indicator and the mark are two things: the row has one cell, the empty conversation
has a block. They share the clock and the glyph family, not the geometry.

## Measured

A block of cells is square only for one cell shape, so the width must come from the terminal.

| Font | Cell | Odd blocks nearest square |
| --- | --- | --- |
| Sarasa Term SC Nerd, plus kitty's `modify_font cell_height 3px` at 12 pt | 0.50 em × 1.25 em, about 8 × 23 px | 9×3, 15×5 |
| JetBrains Mono, from the fontsource CDN file | 0.60 em × 1.32 em | 11×5 exactly, 7×3 and 15×7 within 6 % |
| Maple Mono NF CN Medium, the user's kitty font since 2026-09-22 | 0.60 em × 1.32 em; CJK 1.20 em, two cells | 11×5 exactly, 7×3 and 15×7 within 6 %; every glyph below present |

kitty answers `CSI 16 t` with the cell in pixels; `cellsize.py` shows the reply and the sizes.

Material Design glyphs used, all present at one advance in Sarasa Term SC Nerd and in Symbols Nerd
Font Mono: `circle-small` F09DF, `circle-medium` F09DE, `circle` F0765, `circle-outline` F0766,
`square` F0763, `square-outline` F0764, `square-rounded` F14FB, `square-rounded-outline` F14FC,
`radiobox-marked` F043E, `record-circle` F0FEC, `circle-double` F0E95. The product already draws
its header, tally and copy glyphs from this set.

## What the other CLIs do, read from their source

| | Claude Code 2.1.88 | Codex | Grok |
| --- | --- | --- | --- |
| Spinner | `· ✢ ✳ ✶ ✻ ✽` bounced, 12 frames at 120 ms; glyph and verb share one colour; a three-cell shimmer crosses the verb | none surveyed | braille, 8 frames at about 133 ms; pulsing `◆` while waiting on the user |
| Row text | one of 187 random verbs per request; elapsed and an estimated token count after 30 s; `thinking with X effort` | | fixed labels; phase timer after the label, turn timer and context tokens on the right |
| Stall | glyph and verb fade to red after 3 s without tokens | | phase timer never resets on payload churn |
| User turn | band, subtle `❯`, no hanging indent, text colour unchanged | band blended 12 % toward white from the real terminal background, dim `›`, two-space indent, never truncated | band, accent `❯`, indent, folds past three lines |
| Clock | one shared interval, halved when the window is blurred, off when scrolled away | | demand-driven; idle parks with no wakeups; every frame unit-tested to one column |
