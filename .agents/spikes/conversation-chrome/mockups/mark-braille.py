#!/usr/bin/env python3
"""The Plexmaton mark drawn in braille, the way Grok draws its logo. Ctrl-C to stop.

Each cell is a 2x4 dot matrix, so the mark is a small bitmap: a rounded-square frame around a
circular centre, corrected for this terminal's cell so it comes out square. Animated at 15 frames
a second, the product's motion clock.
"""
import fcntl, math, struct, sys, termios, time

BLUE, PURPLE, TEXT, MUTED, GROUND = (130, 180, 240), (139, 92, 246), (230, 233, 240), (142, 162, 196), (17, 19, 28)
DOTS = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]]  # [row][column] bit of each braille dot
FPS = 15


def cell_px():
    try:
        rows, cols, width, height = struct.unpack("HHHH", fcntl.ioctl(sys.stdout.fileno(), termios.TIOCGWINSZ, b"\0" * 8))
        if width and height and cols and rows:
            return width / cols, height / rows
    except OSError:
        pass
    return 9.0, 20.0


def mix(a, b, k):
    return tuple(round(x + (y - x) * max(0.0, min(1.0, k))) for x, y in zip(a, b))


def rounded_box(x, y, half, radius):
    qx, qy = abs(x) - half + radius, abs(y) - half + radius
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - radius


def superellipse(x, y, size, exponent, turn):
    c, s = math.cos(turn), math.sin(turn)
    x, y = x * c + y * s, -x * s + y * c
    if size <= 0:
        return 1.0
    return (abs(x / size) ** exponent + abs(y / size) ** exponent) ** (1 / exponent) - 1


class Mark:
    def __init__(self, rows, cell):
        cw, ch = cell
        self.rows = rows
        self.cols = max(3, round(rows * ch / cw)) | 1
        self.dw, self.dh = cw / 2, ch / 4  # one dot, in pixels
        self.half = min(self.cols * cw, rows * ch) / 2  # half the block's shorter side, in pixels

    def draw(self, core, shine=None):
        """core(x, y) -> inside? in units of the block's half-size; shine(col, row) -> 0..1."""
        lines = []
        w, h = self.cols * 2, self.rows * 4
        cx, cy = w * self.dw / 2, h * self.dh / 2
        for row in range(self.rows):
            line = ""
            for col in range(self.cols):
                bits, frame_dots, core_dots = 0, 0, 0
                for dy in range(4):
                    for dx in range(2):
                        px = ((col * 2 + dx + 0.5) * self.dw - cx) / self.half
                        py = ((row * 4 + dy + 0.5) * self.dh - cy) / self.half
                        ring = abs(rounded_box(px, py, 0.9, 0.36)) < 0.075
                        inside = core(px, py)
                        if ring or inside:
                            bits |= DOTS[dy][dx]
                            frame_dots += ring
                            core_dots += inside and not ring
                ink = BLUE if frame_dots >= core_dots else PURPLE
                if shine:
                    ink = mix(ink, TEXT, shine(col, row, self.cols, self.rows))
                glyph = chr(0x2800 + bits)
                line += f"\x1b[38;2;{ink[0]};{ink[1]};{ink[2]}m{glyph}" if bits else " "
            lines.append(line + "\x1b[0m")
        return lines


def still_core(x, y):
    return math.hypot(x, y) < 0.32


def grok_shine(t):
    """Grok's sheen: a raised-cosine band sweeps bottom-left to top-right, then rests; a slow pulse."""
    band, cycle, sweep, strength, pulse = 0.38, 4.0, 0.32, 0.55, 0.06
    p = (t % cycle) / cycle
    position = -band + min(p / sweep, 1.0) * (1 + 2 * band)
    breathe = pulse * (0.5 - 0.5 * math.cos(math.tau * t / 5.0))

    def at(col, row, cols, rows):
        diag = (col + (rows - 1 - row)) / (cols + rows)
        d = abs(diag - position)
        return breathe + (strength * 0.5 * (1 + math.cos(math.pi * d / band)) if d < band else 0.0)
    return at


def growing_core(t):
    """H made smooth: a dot grows into a circle, turns into a rounded square and a square, spins
    into a diamond and shrinks away; 3.2 s a cycle, lingering on the whole shapes."""
    p = (t % 3.2) / 3.2
    ease = lambda k: 0.5 - 0.5 * math.cos(math.pi * max(0.0, min(1.0, k)))
    if p < 0.18:
        size, exponent, turn = 0.04 + 0.30 * ease(p / 0.18), 2.0, 0.0
    elif p < 0.30:
        size, exponent, turn = 0.34, 2.0, 0.0
    elif p < 0.48:
        k = ease((p - 0.30) / 0.18)
        size, exponent, turn = 0.34 - 0.04 * k, 2.0 + 6.0 * k, 0.0
    elif p < 0.60:
        size, exponent, turn = 0.30, 8.0, 0.0
    elif p < 0.78:
        k = ease((p - 0.60) / 0.18)
        size, exponent, turn = 0.30 + 0.04 * k, 8.0, k * math.pi / 4
    elif p < 0.84:
        size, exponent, turn = 0.34, 8.0, math.pi / 4
    else:
        k = ease((p - 0.84) / 0.16)
        size, exponent, turn = 0.34 - 0.30 * k, 8.0, math.pi / 4
    return lambda x, y: superellipse(x, y, size, exponent, turn) < 0


CANDIDATES = [
    ("B1  still", lambda t: (still_core, None)),
    ("B2  still, with Grok's sheen", lambda t: (still_core, grok_shine(t))),
    ("B3  the centre grows and turns (H, smooth)", lambda t: (growing_core(t), None)),
    ("B4  grows and turns, with the sheen", lambda t: (growing_core(t), grok_shine(t))),
]


def main():
    cell = cell_px()
    marks = [Mark(7, cell), Mark(5, cell)]
    print(f"\n  cell {cell[0]:.1f}×{cell[1]:.1f} px; marks {marks[0].cols}×7 and {marks[1].cols}×5 cells\n")
    height = 7 + 3
    sys.stdout.write("\x1b[?25l")
    start = time.time()
    try:
        while True:
            t = time.time() - start
            t = math.floor(t * FPS) / FPS
            for name, make in CANDIDATES:
                core, shine = make(t)
                blocks = [m.draw(core, shine) for m in marks]
                for row in range(7):
                    big = blocks[0][row]
                    small_row = row - 1
                    small = blocks[1][small_row] if 0 <= small_row < 5 else " " * marks[1].cols
                    sys.stdout.write(f"    {big}      {small}\x1b[K\n")
                name_row = f"\x1b[38;2;{TEXT[0]};{TEXT[1]};{TEXT[2]}m{'Plexmaton':^{marks[0].cols}}\x1b[0m"
                sys.stdout.write(f"    {name_row}      \x1b[38;2;{MUTED[0]};{MUTED[1]};{MUTED[2]}m{name}\x1b[0m\x1b[K\n\n\n")
            sys.stdout.write(f"\x1b[{height * len(CANDIDATES)}A")
            sys.stdout.flush()
            time.sleep(1 / FPS / 2)
    except KeyboardInterrupt:
        pass
    finally:
        sys.stdout.write(f"\x1b[{height * len(CANDIDATES)}B\x1b[?25h\x1b[0m\n")


if __name__ == "__main__":
    main()
