"""Bounded OSC 66 transport and disjoint cell reservations for the native-text spike."""
from enum import Enum, IntEnum
import math
import os
import re
import select
import time


class Capability(Enum):
    SCALED = "scaled text supported"
    WIDTH_ONLY = "width only; scaling unavailable"
    UNSUPPORTED = "OSC 66 unsupported"
    UNVERIFIED = "unverified response"


class Align(IntEnum):
    TOP = 0
    BOTTOM = 1
    CENTER = 2


CPR = re.compile(rb"\x1b\[([1-9][0-9]{0,3});([1-9][0-9]{0,3})R")
MATH_SYMBOLS = frozenset("θπφβ−∼√Σ⎡⎢⎣⎤⎥⎦─│⎛⎜⎝⎞⎟⎠·×")
ENTER = b"\x1b[?1049h\x1b[?25l\x1b[0m\x1b[2J\x1b[H"
LEAVE = b"\x1b[?2026l\x1b[0m\x1b[?25h\x1b[?1049l"
PROBE = b"\x1b[3;3H\x1b[6n\x1b]66;w=2; \x07\x1b[6n\x1b]66;s=2; \x07\x1b[6n"


def classify(data):
    """Only three complete, plausible CPRs establish capability; silence never does."""
    if len(data) > 96:
        raise ValueError("probe response exceeded 96 bytes")
    matches = list(CPR.finditer(data))
    if len(matches) != 3 or b"".join(m.group() for m in matches) != data:
        return Capability.UNVERIFIED
    positions = [tuple(map(int, m.groups())) for m in matches]
    if positions == [(3, 3), (3, 5), (3, 7)]:
        return Capability.SCALED
    if positions == [(3, 3), (3, 5), (3, 6)]:
        return Capability.WIDTH_ONLY
    if positions == [(3, 3), (3, 3), (3, 3)]:
        return Capability.UNSUPPORTED
    return Capability.UNVERIFIED


def probe(fd, output):
    output.write(PROBE)
    output.flush()
    data = bytearray()
    deadline = time.monotonic() + 1.5
    while len(list(CPR.finditer(data))) < 3:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([fd], [], [], remaining)[0]:
            break
        chunk = os.read(fd, 97 - len(data))
        if not chunk:
            raise EOFError("terminal closed during capability probe")
        if b"\x03" in chunk or b"q" in chunk:
            raise KeyboardInterrupt
        data.extend(chunk)
        if len(data) > 96:
            raise ValueError("probe response exceeded 96 bytes")
    return classify(bytes(data)), bytes(data)


class Canvas:
    """A small display list with disjoint, explicit cell reservations; no overprinting."""

    def __init__(self, columns, rows):
        if not 1 <= columns <= 512 or not 1 <= rows <= 512:
            raise ValueError("preview dimensions must be in 1..512")
        self.columns, self.rows = columns, rows
        self.occupied = set()
        self.commands = []

    def add(self, x, y, text, *, scale=None, color=39):
        if not text or len(text) > 120 or any(not (32 <= ord(c) <= 126 or c in MATH_SYMBOLS) for c in text):
            raise ValueError("fixture text must use the bounded, reviewed symbol set")
        if color not in (39, 90, 93, 94, 95, 96):
            raise ValueError("unknown fixture color")
        width, height, payload = len(text), 1, text.encode("utf-8")
        if scale is not None:
            s, n, d, alignment = scale
            if not (all(type(value) is int for value in (s, n, d))
                    and 1 <= s <= 7 and 0 < n < d <= 15 and isinstance(alignment, Align)):
                raise ValueError("invalid scaled-text metadata")
            # Conservative packing for the fixed one-cell symbol set, not general Unicode.
            units = math.ceil(len(text) * n / d)
            if units > 7:
                raise ValueError("fixture must be split before exceeding OSC 66 width")
            width, height = s * units, s
            payload = f"\x1b]66;s={s}:n={n}:d={d}:v={alignment.value}:w={units};{text}\x07".encode()
        if x < 0 or y < 0 or x + width > self.columns or y + height > self.rows:
            raise ValueError("fixture exceeds the viewport")
        cells = {(cx, cy) for cy in range(y, y + height) for cx in range(x, x + width)}
        if cells & self.occupied:
            raise ValueError("scaled text reservations overlap")
        if len(self.commands) >= 128:
            raise ValueError("fixture display list exceeded its bound")
        self.occupied.update(cells)
        self.commands.append(f"\x1b[{y + 1};{x + 1}H\x1b[{color}m".encode() + payload)

    def wire(self):
        # Full-region repaint is deliberate in this experiment: stale multicells must be erased.
        return b"\x1b[?2026h\x1b[0m\x1b[2J" + b"".join(self.commands) + b"\x1b[0m\x1b[?2026l"
