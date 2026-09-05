"""Review transport for Rust-prepared full replies; no TeX parsing or formula placement here."""
import json
from pathlib import Path
import textwrap
import unicodedata

from transport import Capability


class WireFrame:
    def __init__(self, data):
        self.data = data

    def wire(self):
        return self.data


def native_run(run, x, y):
    text = run["text"]
    if not text or len(text.encode()) > 8192 or any(unicodedata.category(c) in ("Cc", "Cf", "Cs") for c in text):
        raise ValueError("invalid native text")
    columns, rows = run["columns"], run["rows"]
    if not (type(columns) is int and 0 < columns <= 512 and type(rows) is int and rows in (1, 2)):
        raise ValueError("invalid reservation")
    style = {"roman": "0", "italic": "0;3", "bold": "0;1", "bold_italic": "0;1;3"}[run["style"]]
    paint = run["paint"]
    if paint["kind"] == "rgb":
        rgb = [paint[key] for key in ("red", "green", "blue")]
        if any(type(c) is not int or not 0 <= c <= 255 for c in rgb):
            raise ValueError("invalid native paint")
        style += ";38;2;" + ";".join(map(str, rgb))
    elif paint != {"kind": "inherit"}:
        raise ValueError("unknown native paint")
    payload = text.encode()
    scale = run["scale"]
    if scale in ("script", "script_script"):
        if rows != 1 or columns > 7:
            raise ValueError("fractional run exceeds the OSC 66 reservation")
        n, d = (7, 10) if scale == "script" else (1, 2)
        alignment = {"top": 0, "bottom": 1, "center": 2}[run["align"]]
        payload = f"\x1b]66;s=1:n={n}:d={d}:v={alignment}:w={columns};".encode() + payload + b"\x07"
    elif scale == "large":
        if rows != 2 or columns % 2 or not 0 < columns // 2 <= 7:
            raise ValueError("large run exceeds the OSC 66 reservation")
        payload = f"\x1b]66;s=2:w={columns // 2};".encode() + payload + b"\x07"
    elif scale != "full" or rows != 1:
        raise ValueError("unknown native scale")
    return f"\x1b[{y + 1};{x + 1}H\x1b[{style}m".encode() + payload


class Reply:
    def __init__(self, directory):
        self.documents = {}
        for width in (120, 88, 60):
            with (Path(directory) / f"reply-{width}.json").open("rb") as source:
                raw = source.read(1024 * 1024 + 1)
            if len(raw) > 1024 * 1024:
                raise ValueError("prepared reply exceeds one MiB")
            document = json.loads(raw)
            if len(document["source"].encode()) > 65536 or any(unicodedata.category(c) in ("Cc", "Cf", "Cs") and c not in "\n\r\t" for c in document["source"]):
                raise ValueError("unsafe or oversized review source")
            if document["width"] != width or len(document["runs"]) > 16384 or len(document["formulas"]) != 61:
                raise ValueError("unexpected review document")
            if not 0 < len(document["pages"]) <= 128:
                raise ValueError("review page count exceeded")
            self.documents[width] = document

    def frame(self, columns, rows, capability, page, redraw):
        if columns < 60 or rows < 40:
            return WireFrame(b"\x1b[2J\x1b[HNeed 60 x 40 cells" if columns >= 18 else b"\x1b[2J")
        width = next(width for width in (120, 88, 60) if width <= columns)
        document = self.documents[width]
        wire = bytearray(b"\x1b[?2026h\x1b[0m\x1b[2J")
        if capability is not Capability.SCALED:
            lines = [part for line in document["source"].split("\n") for part in (textwrap.wrap(line, width - 4, replace_whitespace=False, drop_whitespace=False) or [""])]
            pages = (len(lines) + 35) // 36
            page %= pages
            for y, line in enumerate(lines[page * 36:(page + 1) * 36], 2):
                wire.extend(f"\x1b[{y + 1};3H".encode() + line.encode())
            heading = f"Source only / {capability.value}"
        else:
            pages = len(document["pages"])
            page %= pages
            start, end = (document["pages"][page][key] for key in ("start", "end"))
            occupied = set()
            for run in document["runs"]:
                if run["y"] < start or run["y"] >= end:
                    continue
                x, y = run["x"] + 2, run["y"] - start + 2
                if x < 2 or y < 2 or x + run["columns"] > width - 2 or y + run["rows"] > 38:
                    raise ValueError("prepared run crosses the review viewport")
                cells = {(cx, cy) for cy in range(y, y + run["rows"]) for cx in range(x, x + run["columns"])}
                if cells & occupied:
                    raise ValueError("prepared native reservations overlap")
                occupied.update(cells)
                wire.extend(native_run(run, x, y))
            heading = f"Plexmaton / RaTeX native text / {width}x{rows}"
        wire.extend(f"\x1b[1;3H\x1b[0;94m{heading}".encode())
        wire.extend(f"\x1b[{rows};3H\x1b[0;93mPage {page + 1}/{pages} | j/k page | q quit | frame {redraw}".encode())
        wire.extend(b"\x1b[0m\x1b[?2026l")
        if len(wire) > 256 * 1024:
            raise ValueError("review frame exceeded 256 KiB")
        return WireFrame(bytes(wire))
