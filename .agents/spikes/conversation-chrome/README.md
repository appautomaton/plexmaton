# Spike — Conversation chrome

| Field | Value |
| --- | --- |
| Question | How the working agent, the user's turn and the mark should look, decided from rendered candidates |
| Feeds | [Phase 04](../../phases/phase-04-product-polish.md) stages 34 and 10, both complete |
| Method | HTML mockups in the palette, opened for the user; two kitty scripts for what a browser cannot show |

Every decision below was made by the user on a rendered frame, 2026-09-21 and 2026-09-22. The
generators are in [`mockups/`](./mockups/); each writes one HTML page to
`plexmaton-conversation-chrome/` under the system temporary directory, outside this corpus, and
`spin.py`, `mark-lively.py`, `mark-braille.py`, `mark-braille-frame.py`, `logo_term.py` and `cellsize.py` run in a terminal. Regenerate rather than trusting a description of them.

## What the user chose

| Surface | Chosen | Rejected on sight |
| --- | --- | --- |
| Activity line composition | mark, label, muted `· 32s · high effort`, `· quiet for 31s` once nothing has arrived; no token count | a red stall tell; estimated tokens |
| Activity mark | H: a dot grows into a circle, turns into a rounded square and a square, spins into a diamond and shrinks away through a smaller one, lingering on whole shapes, 1.6 s a cycle; chosen 2026-09-23 in kitty (`mark-lively.py`) after the user found N2 rigid and wanted it to grow from a point | N2, the circle, rounded square and square outline then filled, chosen 2026-09-22 and later found rigid; one size changing shape, and even steps retracing their path, likewise; a slice-by-slice sweep, which reads as progress; C2′ and C2″, which shook because mixed-size Unicode shapes centre differently; filled squares; braille; quarter circles; Claude Code's star |
| User message | band of lifted ground with a blue `›`, continuation indented | the accent bar; a bare prefix; a hairline bar; a band alone |
| Mark placement | a greeting: at launch onto an empty conversation it plays once at the centre, name beneath, and leaves after about 2.5 s: frame fades in for 0.25 s, the F3 cycle runs in 2 s with one 0.8 s sheen, frame and name fade out in 0.3 s; chosen 2026-09-23 after the user asked for it to be less aggressive and quicker | staying and moving until the first message, too insistent; 3.7 s, too slow; a header or drawer slot, not yet asked for |
| Mark drawing | braille, the way Grok draws its logo: two by four dots a cell, a rounded-square frame around a round centre as one small bitmap, square once corrected for the terminal's cell; chosen 2026-09-23 (`mark-braille.py`) | box drawing at three weights around a one-cell Nerd Font centre, which the user found too coarse to look good at any size |
| Mark motion | F3: the centre grows from a dot into a circle, becomes a rounded square and a square, turns into a diamond while the frame turns with it, and shrinks away, 3.2 s a cycle; Grok's sheen sweeps it every 4 s; chosen 2026-09-23 (`mark-braille-frame.py`) after the user asked for the outside to move as well as the inside | the frame breathing and drifting through palette slots (F1); frame and centre in counterpoint (F2); all of it at once (F4); K4's shape-only morph, rigid |
| Activity mark glyphs | the Material Design family the Nerd Font carries, one cell each, drawn together so they centre together | Unicode geometric shapes from mixed blocks, which jump between fallback fonts |

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
`radiobox-marked` F043E, `record-circle` F0FEC, `circle-double` F0E95, `rhombus` F070B,
`rhombus-medium` F0A10, `square-small` F0A15, `square-medium` F0A13. The product already draws its
header, tally and copy glyphs from this set.

Measured in Maple Mono NF CN, in units of a 1000-unit em on a 600-unit cell: the family's filled
shapes come at 168, 332 and 461 (the medium diamond), then 750 to 832, with nothing between, so a
mark grows in steps rather than smoothly. Its small and medium sizes and its squares share one
centre; the whole circle and diamond sit 41 units right of it, an offset nobody has seen move.
Maple Mono's own `·`, `•`, `▪`, `●` and `■` centre on the cell instead, 75 to 116 units left of
the family's, so they cannot join its cycle; `▢`, `◼` and `⬤` are not in the font and fall back.

## What the other CLIs do, read from their source

| | Claude Code 2.1.88 | Codex | Grok |
| --- | --- | --- | --- |
| Spinner | `· ✢ ✳ ✶ ✻ ✽` bounced, 12 frames at 120 ms; glyph and verb share one colour; a three-cell shimmer crosses the verb | none surveyed | braille, 8 frames at about 133 ms; pulsing `◆` while waiting on the user |
| Row text | one of 187 random verbs per request; elapsed and an estimated token count after 30 s; `thinking with X effort` | | fixed labels; phase timer after the label, turn timer and context tokens on the right |
| Stall | glyph and verb fade to red after 3 s without tokens | | phase timer never resets on payload churn |
| User turn | band, subtle `❯`, no hanging indent, text colour unchanged | band blended 12 % toward white from the real terminal background, dim `›`, two-space indent, never truncated | band, accent `❯`, indent, folds past three lines |
| Clock | one shared interval, halved when the window is blurred, off when scrolled away | | demand-driven; idle parks with no wakeups; every frame unit-tested to one column |
| Logo | | | braille art at seven or five rows by window height, none below; a sheen sweeps bottom-left to top-right in 1.3 s of every 4, over a slow pulse, redrawn at 12 fps |
