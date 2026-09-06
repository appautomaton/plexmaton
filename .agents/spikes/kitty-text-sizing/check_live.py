#!/usr/bin/env python3
"""Explicit owned-Kitty check of the real CLI with one local, scripted model response.

No user configuration, session, credentials or clipboard is used. Not a headless test target.
The terminal's byte dump and character state complement, but do not prove, pixel-level typography.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

from launch_macos import KITTY, launch_configuration


def wait_for(predicate, description):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.03)
    raise TimeoutError(description)


class Response(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        self.connection.settimeout(2)
        length = int(self.headers.get("Content-Length", "0"))
        if not 0 < length <= 2 * 1024 * 1024 or self.path != "/v1/chat/completions":
            self.send_error(400)
            return
        request = json.loads(self.rfile.read(length))
        self.server.requests.append(request)
        if len(self.server.requests) != 1 or request["model"] != "fixture":
            self.send_error(400)
            return
        events = [
            {"id": "native-math-fixture", "choices": [{"index": 0, "delta": {"role": "assistant", "content": self.server.reply}, "finish_reason": None}]},
            {"id": "native-math-fixture", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 8, "completion_tokens": 3000, "total_tokens": 3008}},
        ]
        payload = b"".join(b"data: " + json.dumps(event).encode() + b"\n\n" for event in events) + b"data: [DONE]\n\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def check(binary):
    root = Path(__file__).resolve().parents[3]
    document = json.loads((root / "crates/plexmaton-math/fixtures/attention-derivatives.json").read_text())
    # Fixture ranges are UTF-8 byte offsets, not Python character offsets.
    original = document["text"].encode()
    formula = original[document["math"][0]["start"]:document["math"][0]["end"]].decode()
    server = HTTPServer(("127.0.0.1", 0), Response)
    server.reply, server.requests = document["text"], []
    worker = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.05})
    worker.start()
    try:
        with tempfile.TemporaryDirectory(prefix="plexmaton-live-math-") as temporary:
            directory = Path(temporary)
            home = directory / "app"
            home.mkdir()
            (home / "config.toml").write_text(f'''active_model = {{ provider = "fixture", model = "fixture" }}
[providers.fixture]
base_url = "http://127.0.0.1:{server.server_port}/v1"
api_key_env = "PLEXMATON_MATH_FIXTURE_KEY"
api = "openai_chat_completions"
[providers.fixture.models.fixture]
id = "fixture"
reasoning_effort = "none"
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
''')
            command, env = launch_configuration(directory, 180, "reply")
            command = command[:command.index(sys.executable)]
            address = "unix:" + str(directory / "control")
            capture = directory / "terminal.ansi"
            command[1:1] = ["--listen-on", address, "--dump-bytes", str(capture)]
            command[command.index("allow_remote_control=no")] = "allow_remote_control=socket-only"
            # Empty permission set prevents OSC 52 from touching the host's clipboard.
            command.extend(["--override", "clipboard_control=", str(binary), "--ephemeral"])
            env.update(PLEXMATON_HOME=str(home), PLEXMATON_MATH_FIXTURE_KEY="fixture-only",
                       SSH_TTY="isolated-math-fixture")
            kitten = KITTY.with_name("kitten")

            def raw():
                if not capture.exists():
                    return b""
                if capture.stat().st_size > 16 * 1024 * 1024:
                    raise ValueError("terminal trace exceeded 16 MiB")
                return capture.read_bytes()

            def remote(*arguments):
                result = subprocess.run([str(kitten), "@", "--to", address, "--use-password", "never", *arguments],
                                        env=env, capture_output=True, text=True, timeout=5)
                if result.returncode:
                    if arguments[0] not in ("ls", "get-text"):
                        raise RuntimeError(f"Kitty command failed: {arguments[0]}: {result.stderr[:1000]}")
                    return ""
                if len(result.stdout) > 128 * 1024:
                    raise ValueError("terminal state exceeded its bound")
                return result.stdout

            def screen():
                return remote("get-text", "--match", "id:1", "--extent", "screen")

            def send(text):
                remote("send-text", "--match", "id:1", text)

            snapshots = []
            with subprocess.Popen(command, env=env, cwd=directory, stdin=subprocess.DEVNULL,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as child:
                try:
                    text = wait_for(lambda: (text if "Message Plexmaton" in (text := screen()) and "Type a message" in text else None), "ready agent and composer")
                    row = next(row for row, line in enumerate(text.splitlines()) if "Type a message" in line)
                    send(f"\\x1b[<0;5;{row + 1}M\\x1b[<0;5;{row + 1}m")
                    send("fixture")
                    wait_for(lambda: "fixture" in screen(), "fixture text entered in focused composer")
                    send(r"\r")
                    wait_for(lambda: b"n=7:d=10" in raw(), "worker-prepared native math in real CLI output")
                    for columns in (120, 88, 60):
                        remote("resize-os-window", "--match", "id:1", "--width", str(columns + 2), "--height", "45")

                        def resized():
                            data = remote("ls")
                            if not data:
                                return False
                            windows = json.loads(data)
                            window = windows[0]["tabs"][0]["windows"][0]
                            return window.get("columns") == columns

                        wait_for(resized, f"{columns}-column terminal resize")
                        # Kitty's new dimensions can precede the application's resized frame and
                        # its owned preparation. Scroll only after current-width content is drawn;
                        # a wheel over an unresolved viewport can otherwise leave tail-follow on.
                        def prepared_frame():
                            text = screen()
                            complete = any(line == "─" * columns for line in text.splitlines())
                            return complete and "Preparing text…" not in text and (
                                "Attention" in text or "Softmax alone" in text
                            )

                        wait_for(prepared_frame, f"prepared {columns}-column application frame")
                        # Scroll the actual conversation to its first complete formula.
                        send(r"\x1b[<64;5;6M" * 180)
                        text = wait_for(lambda: (text if "Attention" in (text := screen()) and "softmax" in text else None),
                                        f"native attention formula at {columns}")
                        if "\\frac" in text or "Math source" in text or "Math preparation" in text:
                            raise AssertionError("the application displayed source or a refusal instead of native math")
                        lines = text.splitlines()
                        row, line = next((row, line) for row, line in enumerate(lines) if "Attention" in line)
                        column = line.index("Attention")
                        before = len(raw())
                        # Real SGR reports enter Crossterm's sole input path; source comes from the painted map.
                        send(f"\\x1b[<0;{column + 1};{row + 1}M\\x1b[<0;{column + 1};{row + 1}m")

                        def copied():
                            matches = re.findall(rb"\x1b\]52;c;([A-Za-z0-9+/=]*)\x1b\\", raw()[before:])
                            return base64.b64decode(matches[-1]).decode() if matches else None

                        value = wait_for(copied, "formula click copy through the production output owner")
                        if value != formula:
                            raise AssertionError("native formula click did not retain the complete original delimiters")
                        snapshots.append({"columns": columns, "text": text, "copied": value})
                        send(r"\x10")
                        wait_for(lambda: "Type to filter" in screen(), "Drawer over native math")
                        send(r"\x1b")
                        wait_for(lambda: "Type to filter" not in screen(), "Drawer dismissal")
                    send(r"\x04\x04")
                    child.wait(timeout=8)
                    if child.returncode != 0:
                        raise RuntimeError("Kitty/CLI did not exit cleanly")
                    captured = raw()
                    if b"\x1b[?1049l" not in captured or b"\x1b[?2026l" not in captured:
                        raise AssertionError("terminal restoration was not observed")
                    if list(home.rglob("*.jsonl")):
                        raise AssertionError("ephemeral native review wrote a session")
                    if len(server.requests) != 1:
                        raise AssertionError("unexpected scripted model request count")
                    return {"snapshots": snapshots, "scripted_requests": 1, "terminal_bytes": len(captured), "clean_exit": True}
                except Exception:
                    print(f"fixture requests: {len(server.requests)}; child: {child.poll()}; trace head: {raw()[:180]!r}", file=sys.stderr)
                    print("current isolated screen:\n" + screen(), file=sys.stderr)
                    raise
                finally:
                    if child.poll() is None:
                        child.terminate()
                        try:
                            child.wait(timeout=3)
                        except subprocess.TimeoutExpired:
                            child.kill()
                            child.wait(timeout=3)
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=3)
        if worker.is_alive():
            raise RuntimeError("scripted response server did not join")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if args.output.exists():
        parser.error("output must not already exist")
    result = check(binary)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(f"Live native math verified at 120/88/60; {result['terminal_bytes']} terminal bytes; clean exit")


if __name__ == "__main__":
    main()
