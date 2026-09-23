#!/usr/bin/env python3
"""The braille mark with the frame moving too, around the centre that grows and turns. Ctrl-C to stop."""
import math, sys, time

sys.path.insert(0, __import__("os").path.dirname(__file__))
base = __import__("mark-braille")  # noqa: E402

DRIFT = [(130, 180, 240), (139, 92, 246), (217, 70, 239), (139, 92, 246), (130, 180, 240), (120, 210, 205)]
CYCLE = 3.2
ease = lambda k: 0.5 - 0.5 * math.cos(math.pi * max(0.0, min(1.0, k)))


def drift(t, period=4.8):
    x = (t / period * len(DRIFT)) % len(DRIFT)
    i = int(x)
    return base.mix(DRIFT[i], DRIFT[(i + 1) % len(DRIFT)], x - i)


def phase(t):
    return (t % CYCLE) / CYCLE


def squareness(t):
    """0 while the centre is a circle, 1 once it has become a square (follows growing_core)."""
    p = phase(t)
    if p < 0.30:
        return 0.0
    if p < 0.48:
        return ease((p - 0.30) / 0.18)
    return 1.0 if p < 0.84 else 1.0 - ease((p - 0.84) / 0.16)


def turned(t):
    """0 upright, 1 turned a quarter of a right angle further (the diamond), following the centre."""
    p = phase(t)
    if p < 0.60:
        return 0.0
    if p < 0.78:
        return ease((p - 0.60) / 0.18)
    return 1.0 if p < 0.84 else 1.0 - ease((p - 0.84) / 0.16)


def frame(t, breathe=False, counterpoint=False, turn=False):
    """(inside-the-ring test, colour) for the frame at time t."""
    thickness = 0.075
    if breathe:
        thickness = 0.05 + 0.06 * (0.5 - 0.5 * math.cos(math.tau * t / 1.6))
    radius = 0.36
    if counterpoint:
        radius = 0.20 + 0.66 * squareness(t)  # square-ish around a circle, a circle around a square
    angle = turned(t) * math.pi / 4 if turn else 0.0
    scale = 1.0 / (1.0 + (math.sqrt(2) - 1.0) * math.sin(angle * 2) ** 2) if turn else 1.0
    half = 0.9 * scale

    def ring(x, y):
        c, s = math.cos(angle), math.sin(angle)
        x, y = x * c + y * s, -x * s + y * c
        return abs(base.rounded_box(x, y, half, min(radius, half))) < thickness
    return ring


def draw(mark, core, ring, frame_ink, shine=None):
    lines = []
    w, h = mark.cols * 2, mark.rows * 4
    cx, cy = w * mark.dw / 2, h * mark.dh / 2
    for row in range(mark.rows):
        line = ""
        for col in range(mark.cols):
            bits, frame_dots, core_dots = 0, 0, 0
            for dy in range(4):
                for dx in range(2):
                    px = ((col * 2 + dx + 0.5) * mark.dw - cx) / mark.half
                    py = ((row * 4 + dy + 0.5) * mark.dh - cy) / mark.half
                    on_ring, inside = ring(px, py), core(px, py)
                    if on_ring or inside:
                        bits |= base.DOTS[dy][dx]
                        frame_dots += on_ring
                        core_dots += inside and not on_ring
            ink = frame_ink if frame_dots >= core_dots else base.PURPLE
            if shine:
                ink = base.mix(ink, base.TEXT, shine(col, row, mark.cols, mark.rows))
            line += f"\x1b[38;2;{ink[0]};{ink[1]};{ink[2]}m{chr(0x2800 + bits)}" if bits else " "
        lines.append(line + "\x1b[0m")
    return lines


CANDIDATES = [
    ("F1  the frame breathes and drifts, with the sheen", lambda t: (frame(t, breathe=True), drift(t), base.grok_shine(t))),
    ("F2  inside and outside in counterpoint, with the sheen", lambda t: (frame(t, counterpoint=True), base.BLUE, base.grok_shine(t))),
    ("F3  they turn together, with the sheen", lambda t: (frame(t, turn=True), base.BLUE, base.grok_shine(t))),
    ("F4  all of it: breathe, drift, counterpoint, turn, sheen", lambda t: (frame(t, True, True, True), drift(t), base.grok_shine(t))),
]


def main():
    cell = base.cell_px()
    marks = [base.Mark(7, cell), base.Mark(5, cell)]
    print(f"\n  cell {cell[0]:.1f}×{cell[1]:.1f} px; marks {marks[0].cols}×7 and {marks[1].cols}×5 cells\n")
    height = 7 + 3
    sys.stdout.write("\x1b[?25l")
    start = time.time()
    try:
        while True:
            t = math.floor((time.time() - start) * base.FPS) / base.FPS
            core = base.growing_core(t)
            for name, make in CANDIDATES:
                ring, ink, shine = make(t)
                blocks = [draw(m, core, ring, ink, shine) for m in marks]
                for row in range(7):
                    small = blocks[1][row - 1] if 1 <= row <= 5 else " " * marks[1].cols
                    sys.stdout.write(f"    {blocks[0][row]}      {small}\x1b[K\n")
                name_row = f"\x1b[38;2;230;233;240m{'Plexmaton':^{marks[0].cols}}\x1b[0m"
                sys.stdout.write(f"    {name_row}      \x1b[38;2;142;162;196m{name}\x1b[0m\x1b[K\n\n\n")
            sys.stdout.write(f"\x1b[{height * len(CANDIDATES)}A")
            sys.stdout.flush()
            time.sleep(1 / base.FPS / 2)
    except KeyboardInterrupt:
        pass
    finally:
        sys.stdout.write(f"\x1b[{height * len(CANDIDATES)}B\x1b[?25h\x1b[0m\n")


if __name__ == "__main__":
    main()
