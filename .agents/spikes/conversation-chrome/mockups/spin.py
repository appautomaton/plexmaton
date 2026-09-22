#!/usr/bin/env python3
"""Run this in a kitty tab (not inside the Claude session): python3 /tmp/plexmaton-activity-line/spin.py
Shows the square/circle cycles in your real terminal font. Ctrl-C to stop; it stops by itself after 20 s."""
import sys, time
BLUE, PURPLE, MUTED, TEXT, RESET, REV = "\x1b[38;2;130;180;240m", "\x1b[38;2;139;92;246m", "\x1b[38;2;142;162;196m", "\x1b[38;2;230;233;240m", "\x1b[0m", "\x1b[7m"
CYCLES = [
  ("C2   ring → rounded frame → square, mixed blocks", "∘◦○◯▢□◻□▢◯○◦", 110),
  ("C2'  same idea, Geometric Shapes block only",      "▫◻□▢◯○◦○◯▢□◻", 110),
  ("C2'' three sizes only",                             "◦○◯▢□▢◯○", 130),
  ("C3   logo core opening in the square (filled)",   "▪◼■◘◙◘■◼", 120),
]
def strip():
    print(f"{TEXT}Each glyph in its own cell, reverse video shows the cell box so you can judge centring:{RESET}\n")
    for name, g, _ in CYCLES:
        cells = "".join(f"{REV}{c}{RESET} " for c in g)
        print(f"  {MUTED}{name:52}{RESET} {cells}")
    print()
def animate(seconds=20.0):
    sys.stdout.write("\x1b[?25l")
    t0 = time.time()
    try:
        while time.time() - t0 < seconds:
            t = (time.time() - t0) * 1000
            for name, g, ms in CYCLES:
                i = int(t // ms) % len(g)
                k = (t / 1600) % 1; k = k * 2 if k < 0.5 else 2 - k * 2
                r, gg, b = (round(130 + (139 - 130) * k), round(180 + (92 - 180) * k), round(240 + (246 - 240) * k))
                col = f"\x1b[38;2;{r};{gg};{b}m"
                sys.stdout.write(f"  {col}{g[i]}{RESET} {TEXT}Thinking…{RESET} {MUTED}· 32s · high effort{RESET}   {MUTED}{name}{RESET}\x1b[K\n")
            sys.stdout.write(f"\x1b[{len(CYCLES)}A")
            sys.stdout.flush()
            time.sleep(0.033)
    except KeyboardInterrupt:
        pass
    finally:
        sys.stdout.write(f"\x1b[{len(CYCLES)}B\x1b[?25h{RESET}\n")
if __name__ == "__main__":
    strip()
    if "--static" not in sys.argv:
        animate()
