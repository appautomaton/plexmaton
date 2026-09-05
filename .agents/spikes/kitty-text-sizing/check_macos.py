#!/usr/bin/env python3
"""Explicit real-Kitty check of the fixed ML fixtures; never part of headless unit discovery.

Checks the terminal's character state, not pixels. Visual acceptance remains a separate gate.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time
import unicodedata

from launch_macos import KITTY, launch_configuration


def wait_for(predicate, description):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.03)
    raise TimeoutError("missing experiment state: " + description)


def verify_page_text(document, bounds, screen):
    runs = sorted((run for run in document["runs"] if bounds["start"] <= run["y"] < bounds["end"]),
                  key=lambda run: (run["y"], run["x"]))
    normalize = lambda text: "".join(unicodedata.normalize("NFC", text).split())
    expected = normalize("".join(run["text"] for run in runs))
    actual = normalize("".join(screen.splitlines()[2:38]))
    if expected != actual:
        raise AssertionError("real Kitty lost or reordered native characters on this page")


def check(directory, reply_directory=None):
    directory = Path(directory)
    address = "unix:" + str(directory / "control")
    report = directory / "lifecycle.jsonl"
    command, env = launch_configuration(directory, 120, "reply" if reply_directory is not None else "ml")
    if reply_directory is not None:
        command.extend(["--reply-directory", str(Path(reply_directory).resolve())])
    command[1:1] = ["--listen-on", address]
    command[command.index("allow_remote_control=no")] = "allow_remote_control=socket-only"
    command.extend(["--report", str(report)])
    kitten = KITTY.with_name("kitten")
    snapshots = []

    def records():
        if not report.exists():
            return []
        raw = report.read_text()
        if len(raw) > 128 * 1024:
            raise ValueError("experiment report exceeded its bound")
        # A record is ready only after the writer has published its terminating newline.
        return [json.loads(line) for line in raw.split("\n")[:-1]]

    def remote(*arguments):
        result = subprocess.run(
            [str(kitten), "@", "--to", address, "--use-password", "never", *arguments],
            env=env, capture_output=True, text=True, timeout=5, check=True,
        )
        if len(result.stdout) > 64 * 1024:
            raise ValueError("terminal reply exceeded its bound")
        return result.stdout

    def screen():
        return remote("get-text", "--match", "id:1", "--extent", "screen")

    with subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL) as child:
        try:
            wait_for(lambda: any(r["event"] == "frame" for r in records()), "initial frame")
            if records()[0].get("capability") != "SCALED":
                raise RuntimeError("real Kitty did not prove text scaling")
            for columns in (120, 88, 60):
                # The 8-point padding consumes two columns and one row with the fixture font.
                remote("resize-os-window", "--match", "id:1", "--width", str(columns + 2), "--height", "45")
                wait_for(lambda: any(r.get("columns") == columns and r.get("rows") == 44 for r in records()),
                         f"{columns}x44 layout")
                text = wait_for(lambda: (text if f"{columns}x44" in (text := screen()) else None),
                                f"{columns}-column terminal consumption")
                if reply_directory is None:
                    if "19  22" not in text or "43  50" not in text or "softmax" not in text or "ref" not in text:
                        raise AssertionError("the terminal lost a complete ML fixture")
                    snapshots.append({"columns": columns, "rows": 44, "text": text})
                else:
                    document = json.loads((Path(reply_directory) / f"reply-{columns}.json").read_text())
                    page_count = len(document["pages"])
                    current = records()[-1]["reply_page"]
                    remote("send-text", "--match", "id:1", "k" * (current % page_count) + "r")
                    for page, bounds in enumerate(document["pages"]):
                        marker = f"Page {page + 1}/{page_count}"
                        text = wait_for(lambda: (value if marker in (value := screen()) else None), marker)
                        verify_page_text(document, bounds, text)
                        indices = [index for index, formula in enumerate(document["formulas"])
                                   if bounds["start"] <= formula["y"] < bounds["end"]]
                        if page == 0 and ("Attention" not in text or "softmax" not in text or "√" not in text):
                            raise AssertionError("the real terminal lost the engine-generated attention formula")
                        snapshots.append({"columns": columns, "rows": 44, "page": page + 1,
                                          "formula_indices": indices, "text": text})
                        if page + 1 < page_count:
                            remote("send-text", "--match", "id:1", "j")
                    seen = [index for snapshot in snapshots if snapshot["columns"] == columns for index in snapshot["formula_indices"]]
                    if seen != list(range(61)):
                        raise AssertionError("full-reply paging lost or repeated a formula")
            if reply_directory is not None:
                previous = max(r.get("number", 0) for r in records())
                remote("send-text", "--match", "id:1", "r")
                wait_for(lambda: any(r.get("number", 0) > previous for r in records()), "reply redraw")
                remote("send-text", "--match", "id:1", "q")
                if child.wait(timeout=5) != 0 or records()[-1]["event"] != "closed":
                    raise AssertionError("reply preview did not clean up")
                return {"capability": records()[0], "frames": snapshots, "closed": records()[-1], "pixel_capture": "not checked"}
            remote("send-text", "--match", "id:1", " ")
            replaced = wait_for(lambda: (text if "1  0" in (text := screen()) else None), "identity replacement")
            if "19  22" in replaced or "43  50" in replaced:
                raise AssertionError("the terminal retained stale matrix text")
            previous = max(r.get("number", 0) for r in records())
            remote("send-text", "--match", "id:1", "r")
            wait_for(lambda: any(r.get("number", 0) > previous for r in records()), "explicit redraw")
            remote("send-text", "--match", "id:1", "q")
            if child.wait(timeout=5) != 0:
                raise RuntimeError("Kitty did not exit cleanly with the preview")
            if records()[-1]["event"] != "closed":
                raise AssertionError("preview did not acknowledge terminal cleanup")
            return {"capability": records()[0], "frames": snapshots, "replacement_cleared": True,
                    "closed": records()[-1], "pixel_capture": "not checked"}
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=3)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reply-directory", type=Path)
    parser.add_argument("--output", type=Path, help="new retained JSON character-state report")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="plexmaton-kitty-check-", dir="/tmp") as directory:
        result = check(directory, args.reply_directory)
        if args.output is None:
            print(json.dumps(result, indent=2))
        else:
            with args.output.open("x", encoding="utf-8") as output:
                json.dump(result, output, indent=2)
            print(f"Verified {len(result['frames'])} real Kitty frames; report: {args.output}")
