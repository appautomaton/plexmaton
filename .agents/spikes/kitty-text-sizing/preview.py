#!/usr/bin/env python3
"""Bounded, direct-terminal OSC 66 review. No images or live user config.

Run inside a terminal: python3 preview.py [--seconds 300] [--report new-report.jsonl]
Fixed fixtures isolate transport; the reply page consumes Rust-prepared native geometry.
"""
import argparse
from contextlib import contextmanager
from enum import Enum
import json
import os
from pathlib import Path
import select
import signal
import sys
import termios
import time
import tty


from transport import Align, Capability, Canvas, ENTER, LEAVE, PROBE, classify, probe
import ml_fixtures
from reply import Reply


class Page(Enum):
    SCRIPTS = "scripts"
    ML = "ml"
    REPLY = "reply"

def paired(canvas, x, y, fraction, variant, base="x"):
    canvas.add(x, y, base, scale=(2, 1, 2, Align.CENTER))
    upper, lower = (("n+1", "ij") if variant == 0 else ("k", "p")) if base == "x" else ("max", "total")
    n, d = fraction
    canvas.add(x + 2, y, upper, scale=(1, n, d, Align.BOTTOM))
    canvas.add(x + 2, y + 1, lower, scale=(1, n, d, Align.TOP))


def frame(columns, rows, capability, variant=0, redraw=0, page=Page.SCRIPTS):
    if page is Page.REPLY:
        raise ValueError("source-linked replies use Reply.frame with prepared geometry")
    canvas = Canvas(columns, rows)
    minimum_rows = 43 if page is Page.ML else 34
    if columns < 60 or rows < minimum_rows:
        # Tiny windows show an explicit refusal rather than dropping terms from a formula.
        message = f"Need 60 x {minimum_rows} cells"
        if columns >= len(message):
            canvas.add(0, 0, message, color=93)
        return canvas
    canvas.add(2, 1, "Plexmaton / native text sizing experiment", color=94)
    canvas.add(2, 2, f"{capability.value} / {columns}x{rows} / frame {redraw}", color=90)
    if capability is not Capability.SCALED:
        canvas.add(2, 5, "Scaled fixtures withheld: capability was not proven.", color=93)
        if page is Page.ML:
            row = 7
            for name, source in ml_fixtures.SOURCES.items():
                canvas.add(2, row, name + " / TeX source", color=96)
                row += 1
                width = min(columns - 4, 120)
                for start in range(0, len(source), width):
                    canvas.add(2, row, source[start:start + width])
                    row += 1
                row += 1
        else:
            canvas.add(2, 7, r"Exact source: x_{ij}^{n+1}   T_{total}^{max}")
            canvas.add(2, 9, "Try a direct Kitty window, without tmux.", color=90)
    elif page is Page.ML:
        ml_fixtures.paint(canvas, variant)
    else:
        canvas.add(2, 4, "1. Single scripts within one terminal row", color=96)
        canvas.add(4, 6, "x")
        canvas.add(5, 6, "ij", scale=(1, 2, 3, Align.BOTTOM))
        canvas.add(11, 6, "+ y")
        canvas.add(14, 6, "max", scale=(1, 2, 3, Align.BOTTOM))
        canvas.add(23, 6, "+ a")
        canvas.add(26, 6, "n+1", scale=(1, 2, 3, Align.TOP))
        canvas.add(2, 8, "2. Paired scripts / centered base / two-row band", color=96)
        canvas.add(22, 9, r"x_{ij}^{n+1}" if variant == 0 else "x_p^k", color=90)
        canvas.add(39, 9, r"T_{total}^{max}", color=90)
        for y, fraction in [(11, (1, 2)), (14, (2, 3)), (17, (3, 4))]:
            canvas.add(4, y, f"{fraction[0]}/{fraction[1]} script size", color=95)
            paired(canvas, 22, y, fraction, variant)
            paired(canvas, 39, y, fraction, variant, "T")
        canvas.add(2, 20, "3. Nested subscript / x_{i_0}", color=96)
        canvas.add(22, 22, "x", scale=(2, 1, 2, Align.CENTER))
        canvas.add(24, 23, "i", scale=(1, 2, 3, Align.TOP))
        canvas.add(25, 23, "0", scale=(1, 1, 2, Align.BOTTOM))
        canvas.add(2, 25, "4. Ordinary-cell reference / three-row band", color=90)
        canvas.add(23, 27, "n + 1")
        canvas.add(22, 28, "x")
        canvas.add(23, 29, "ij")
    canvas.add(2, rows - 3, "m ML / s scripts | Space replace | r redraw | q quit", color=93)
    canvas.add(2, rows - 2, "Fixed transport fixtures. No TeX parser or images.", color=90)
    return canvas


@contextmanager
def terminal(fd, output):
    """Own raw mode and alternate screen through normal exit, errors and termination."""
    if not os.isatty(fd) or not os.isatty(output.fileno()):
        raise ValueError("run the preview in a real terminal")
    previous = termios.tcgetattr(fd)
    old_handler = signal.getsignal(signal.SIGTERM)

    def terminate(_number, _frame):
        raise KeyboardInterrupt

    try:
        signal.signal(signal.SIGTERM, terminate)
        tty.setraw(fd)
        output.write(ENTER)
        output.flush()
        yield
    finally:
        try:
            output.write(LEAVE)
            output.flush()
        finally:
            termios.tcsetattr(fd, termios.TCSADRAIN, previous)
            signal.signal(signal.SIGTERM, old_handler)


def run(seconds, report, page=Page.SCRIPTS, reply_directory=None):
    fd, output = sys.stdin.fileno(), sys.stdout.buffer
    started = time.monotonic()
    count, variant, old_size = 0, 0, None
    capability = Capability.UNVERIFIED
    reply = Reply(reply_directory) if reply_directory is not None else None
    reply_page = 0

    def record(event, **fields):
        if report is not None:
            report.write(json.dumps({"event": event, **fields}) + "\n")
            report.flush()

    try:
        with terminal(fd, output):
            size = os.get_terminal_size(fd)
            if size.columns >= 8 and size.lines >= 5:
                capability, response = probe(fd, output)
                record("probe", capability=capability.name, response_hex=response.hex())
            dirty = True
            while time.monotonic() - started < seconds:
                size = os.get_terminal_size(fd)
                if dirty or size != old_size:
                    if count >= 512:
                        record("frame_limit")
                        break
                    count += 1
                    if page is Page.REPLY:
                        if reply is None:
                            raise ValueError("reply page requires Rust-prepared --reply-directory")
                        canvas = reply.frame(min(size.columns, 512), min(size.lines, 512), capability, reply_page, count)
                    else:
                        canvas = frame(min(size.columns, 512), min(size.lines, 512), capability, variant, count, page)
                    output.write(canvas.wire())
                    output.flush()
                    record("frame", columns=size.columns, rows=size.lines, variant=variant, number=count, page=page.value, reply_page=reply_page)
                    old_size, dirty = size, False
                if not select.select([fd], [], [], 0.1)[0]:
                    continue
                keys = os.read(fd, 64)
                if not keys or b"q" in keys or b"\x03" in keys:
                    break
                if b" " in keys:
                    variant = 1 - variant
                    dirty = True
                if b"r" in keys:
                    dirty = True
                if b"j" in keys or b"k" in keys:
                    reply_page += keys.count(b"j") - keys.count(b"k")
                    dirty = True
                if b"m" in keys or b"s" in keys:
                    page = Page.ML if b"m" in keys else Page.SCRIPTS
                    dirty = True
    except KeyboardInterrupt:
        record("interrupted")
    finally:
        record("closed", frames=count)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=300, help="automatic exit deadline, 1..600")
    parser.add_argument("--page", choices=[page.value for page in Page], default="scripts")
    parser.add_argument("--report", type=Path, help="new JSONL lifecycle report; never overwrites")
    parser.add_argument("--reply-directory", type=Path, help="Rust native_preview output directory")
    args = parser.parse_args()
    if not 1 <= args.seconds <= 600:
        parser.error("--seconds must be in 1..600")
    if args.report is None:
        run(args.seconds, None, Page(args.page), args.reply_directory)
    else:
        with args.report.open("x", encoding="utf-8") as report:
            run(args.seconds, report, Page(args.page), args.reply_directory)


if __name__ == "__main__":
    main()
