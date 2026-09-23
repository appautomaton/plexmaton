#!/usr/bin/env python3
"""C made lively: growth, a turn and a sweep, from one Material Design family. Ctrl-C to stop.

Every glyph here shares the family's centring, and each frame lasts a whole number of the motion
clock's 67 ms phases; cycles are 24 phases (1.6 s) or 16 (1.07 s), both of which the product's
clock can drive unchanged.
"""
import sys, time

TEXT, BLUE, MUTED, RESET, REV = ("\x1b[38;2;230;233;240m", "\x1b[38;2;130;180;240m",
                                 "\x1b[38;2;142;162;196m", "\x1b[0m", "\x1b[7m")
PHASE_MS = 1000 / 15
G = dict(c_small="\U000F09DF", c_medium="\U000F09DE", circle="\U000F0765",
         square="\U000F0763", rounded="\U000F14FB",
         rhombus="\U000F070B", r_medium="\U000F0A10")
G.update({f"slice{n}": chr(0xF0A9D + n) for n in range(1, 9)})

SLICES = [(f"slice{n}", 1) for n in range(1, 9)]
CANDIDATES = [
    ("E    liked: C with pace", [("c_small", 3), ("c_medium", 1), ("circle", 5), ("rounded", 2),
                                  ("square", 5), ("rounded", 2), ("circle", 5), ("c_medium", 1)]),
    ("H    grow, turn, spin out  · 1.6 s", [("c_small", 3), ("c_medium", 1), ("circle", 6), ("rounded", 2),
                                             ("square", 5), ("rhombus", 4), ("r_medium", 3)]),
    ("K    spark                 · 1.6 s", [("c_small", 3), ("c_medium", 1), ("r_medium", 2), ("rhombus", 4),
                                             ("square", 4), ("rounded", 2), ("circle", 5), ("c_medium", 3)]),
    ("I    sweep                 · 1.6 s", [("c_small", 4), ("c_medium", 1), *SLICES, ("circle", 8), ("c_medium", 3)]),
    ("H'   grow, turn, spin out  · 1.07 s", [("c_small", 2), ("c_medium", 1), ("circle", 4), ("rounded", 1),
                                              ("square", 3), ("rhombus", 3), ("r_medium", 2)]),
    ("K'   spark                 · 1.07 s", [("c_small", 2), ("c_medium", 1), ("r_medium", 1), ("rhombus", 3),
                                              ("square", 3), ("rounded", 1), ("circle", 3), ("c_medium", 2)]),
    ("I'   sweep                 · 1.07 s", [("c_small", 2), ("c_medium", 1), *SLICES, ("circle", 4), ("c_medium", 1)]),
]


def frame_at(frames, phase):
    phase %= sum(hold for _, hold in frames)
    for glyph, hold in frames:
        if phase < hold:
            return glyph
        phase -= hold
    raise AssertionError("unreachable")


def strip():
    print(f"\n{TEXT}Frames in order, each in its own cell; the number is how many 67 ms phases it holds:{RESET}\n")
    for name, frames in CANDIDATES:
        cells = " ".join(f"{REV}{G[glyph]}{RESET}{MUTED}{hold}{RESET}" for glyph, hold in frames)
        print(f"  {MUTED}{name:38}{RESET} {cells}")
    print(f"\n{TEXT}As the activity line:{RESET}\n")


def animate():
    sys.stdout.write("\x1b[?25l")
    start = time.time()
    try:
        while True:
            elapsed_ms = (time.time() - start) * 1000
            phase = int(elapsed_ms // PHASE_MS)
            for name, frames in CANDIDATES:
                glyph = G[frame_at(frames, phase)]
                seconds = 12 + int(elapsed_ms // 1000)
                sys.stdout.write(f"  {BLUE}{glyph}{RESET} {TEXT}Thinking…{RESET}"
                                 f"{MUTED} · {seconds}s · max effort{RESET}      {MUTED}{name}{RESET}\x1b[K\n\n")
            sys.stdout.write(f"\x1b[{2 * len(CANDIDATES)}A")
            sys.stdout.flush()
            time.sleep(0.01)
    except KeyboardInterrupt:
        pass
    finally:
        sys.stdout.write(f"\x1b[{2 * len(CANDIDATES)}B\x1b[?25h{RESET}\n")


if __name__ == "__main__":
    for name, frames in CANDIDATES:
        assert sum(hold for _, hold in frames) in (16, 24), name
    strip()
    animate()
