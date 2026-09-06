# Spike — Kitty native text sizing

Read when evaluating real font scaling, script placement, or ML formula presentation.
Status: standalone source-linked transport verified 2026-09-05; live CLI verified and its
direct-Kitty appearance approved 2026-09-06. Broader terminal/font fidelity remains unproven.
[Phase 04](../../phases/phase-04-product-polish.md),
[math layout](../../specs/math-layout.md), [source comparison](../math-typesetting/README.md).

## Question and boundary

Can terminal text provide readable scripts and ML formulas without equation images? These
reviews use an isolated direct-Kitty window; the user's daily tmux path is not proven here.
MTH-2 owns production sizes and fidelity limits.

[preview.py](./preview.py) / [transport.py](./transport.py) own raw input, the alternate screen and
three-report OSC 66 probing. Fixed fixtures compare 1/2, 2/3 and 3/4 scripts in disjoint cell
rectangles, without arbitrary pixel placement.
[Protocol](https://sw.kovidgoyal.net/kitty/text-sizing-protocol/).

[ml_fixtures.py](./ml_fixtures.py) contains manually positioned transport fixtures and exact TeX,
not a renderer. Delete those scenes when Rust's public API reproduces them.

[reply.py](./reply.py) paginates the Rust public API's 61-formula attention reply, without another
parser/layout engine. Prose retains Markdown markers; this is not the production conversation.

[check_live.py](./check_live.py) runs the actual CLI/worker with one loopback reply, real mouse
copy, resize and overlays at 120/88/60. It whitelists the environment, disables host clipboard
writes, uses ephemeral state and owns the terminal, control socket and HTTP server through exit.

## ML corpus

| Fixture | Meaning and source | Typography exercised |
| --- | --- | --- |
| RLHF | Fixed-prompt expected reward with a sampled log-ratio KL penalty. Policy parameters are theta, reward parameters phi. This specializes the regularized objective in [InstructGPT, equation (2)](https://arxiv.org/html/2203.02155v1) with no pretraining term; it is not the PPO clipped surrogate | Expectation, nested policy index, Greek letters, named ref index, log, fraction and spanning brackets |
| Attention | Scaled dot-product attention, [Attention Is All You Need, equation (1)](https://arxiv.org/html/1706.03762v7) | Matrix transpose, softmax, fraction, root and dimension subscript |
| Matrix product | C = AB, with the full indexed sum and a 2-by-2 numerical example | Large summation with limits, paired matrix indices, aligned entries and full-height delimiters |

The numerical example yields rows (19, 22) and (43, 50). Space replaces A with the identity,
so C becomes B; this also exercises replacement of longer terminal content with shorter content.
Sources remain complete when scaling is unavailable. Unsupported glyphs or insufficient viewport
space are refused explicitly; the symbol set is printable ASCII plus the fixed reviewed math
characters, not arbitrary Unicode/font coverage.

## Reproduce

From the math-typesetting worktree, including from inside tmux:

```console
python3 .agents/spikes/kitty-text-sizing/launch_macos.py --page ml
```

For the full source-linked reply, prepare once, then launch the same owned terminal viewer:

```console
cargo run --offline --locked -p plexmaton-math --example native_preview -- target/math-review
python3 .agents/spikes/kitty-text-sizing/launch_macos.py --page reply --reply-directory target/math-review
```

The reply page uses j/k to move between complete-formula pages; q quits. It needs at least
60 × 40 cells and chooses a prepared 120/88/60-column layout that fits the window. Without proven
scaling, all original source, including delimiters, remains pageable without OSC 66 output.

This opens a separate Kitty instance using Menlo 15 and temporary configuration, cache and runtime
directories. It does not read the user's Kitty config, inherit tmux/API credentials, enable remote
control or change global font settings. The process owner waits for completion and joins termination;
the temporary directories are removed on exit. The window exits on q or after five minutes.
Use --page scripts for the size comparison. Within the window: m selects ML, s selects scripts,
Space replaces fixture content, r redraws, q quits. ML requires at least 60 by 43 cells.

Inside an existing terminal, the probe can also be run directly:

```console
python3 .agents/spikes/kitty-text-sizing/preview.py --page ml
```

It does not attempt tmux passthrough. If the measured capability is absent or unverified, it withholds
scaled runs and shows source. There is no environment-name-based capability assumption.

## Executed evidence

2026-09-05, macOS, installed Kitty 0.46.1, Menlo 15:

| Check | Observed result |
| --- | --- |
| Direct-Kitty probe | CPR positions (3,3), (3,5), (3,7): scaling supported |
| Script visual review | User inspected the actual roughly 88-column window and accepted the direction |
| ML 120/88/60 by 44 | Real Kitty character-state queries retained ref, softmax and complete numerical matrices at all three widths |
| ML replacement and redraw | Identity replacement removed old matrix values; a new frame was acknowledged |
| Source-linked complete reply | Eight pages at each of 120/88/60 × 44; all 61 occurrences covered. Every native character matched Kitty's character state in row order, normalizing Unicode composition and whitespace; redraw and clean exit acknowledged |
| Owned process exit | q closed the preview and Kitty exited with status zero |
| Unit and real-PTY suite | 19 tests passed, including real Rust preparation through reply encoding/pagination, invalid-output refusal, source fallback, fixed transport reservations, resize/replacement and cleanup on q, SIGTERM and probe timeout |
| Pixel capture | Dedicated-window screencapture failed. No retained pixel capture; character extraction does not prove glyph alignment or clipping |

Re-run the deterministic suite (PTY allocation may require leaving the agent sandbox):

```console
python3 -m unittest discover -s .agents/spikes/kitty-text-sizing -p 'test_*.py' -v
```

The explicit real-Kitty check opens and closes its own temporary window and control socket:

```console
python3 .agents/spikes/kitty-text-sizing/check_macos.py
```

For the production path (build the worktree binary first):

```console
python3 .agents/spikes/kitty-text-sizing/check_live.py --binary target/debug/plexmaton --output target/live-math-review.json
```

The 2026-09-06 run passed all three widths and exact OSC 52 copies, observed 548,922 terminal
bytes and clean exit, and made exactly one scripted request with no saved session. This is
character-state/transport evidence; the user separately approved the live review build's appearance.

Add `--reply-directory target/math-review --output target/math-review/kitty-reply-check.json` to
verify all reply pages and retain a new character-state report; the output path must not exist.

It checks terminal consumption, not pixel fidelity. Its isolated control socket is never the user's
live Kitty socket. The interactive launcher leaves remote control disabled.

Bounds: 96 probe-response bytes, 1.5-second probe deadline, 128 runs per fixed frame, at most 512
frames and a 1–600-second session deadline. Prepared reply files are bounded to one MiB each,
16,384 runs and 128 complete-formula pages; native frame output is bounded to 256 KiB. Tests
consume cleanup output before waiting for terminal drain;
macOS's independently reproduced kernel-owned PENDIN state is excluded from mode-setting equality.
Kitty reported an OpenGL copy fallback on this Mac; no performance claim follows from these runs.

## Remaining gates

Broader Unicode/font fidelity, real tmux/SSH sizing, true partial-multicell pixel display,
non-reflowing source reveal and saturated physical-terminal latency remain unproven. MTH-1–MTH-5
and PRE-1–PRE-4 own production selection, retained origins, visible clipping and process/cache
evidence. Running these reviews changes no notifications, persistent user configuration or dependency graph.
