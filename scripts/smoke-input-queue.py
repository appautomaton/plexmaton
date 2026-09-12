#!/usr/bin/env python3
"""IQU-1/IQU-4: queue, withdraw and continue through a real PTY and paused loopback stream."""

import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile

from permission_fixture import PausedResponse, ScriptedProvider, response
from smoke_support import fixture_environment

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("permission_smoke", ROOT / "scripts/smoke-permissions.py")
journey = importlib.util.module_from_spec(spec)
spec.loader.exec_module(journey)

FIRST = "Inspect the queue fixture"
OLDER = "Keep this earlier message"
NEWEST = "$100 Return this exact message\nKeep 中文 and spacing intact"
SKILL_BODY = '---\nname: "100"\ndescription: Queue fixture skill\n---\nKeep FIXTURE_SKILL_MARKER intact.\n'
ALT_UP = b"\x1b[1;3A"


def saved_journal(home):
    paths = list((home / "sessions").glob("*.jsonl"))
    assert len(paths) == 1, paths
    return paths[0].read_bytes()


def capture_widths(terminal):
    """Capture actual single-agent frames, without a synthetic roster or Attention item."""
    output = ROOT / "target/smoke"
    output.mkdir(parents=True, exist_ok=True)
    for width, label in [(121, None), (120, "wide"), (88, "medium"), (60, "narrow")]:
        terminal.resize(width, "Waiting to send · 2", "Alt-↑")
        screen = terminal.wait("Responding", OLDER, absent=("Preparing text",))
        if label:
            (output / f"input-queue-{label}.txt").write_text(
                "\n".join(line.rstrip() for line in screen.splitlines()) + "\n")
    terminal.resize(120, "Waiting to send · 2", "Alt-↑")
    terminal.frame_start = len(terminal.capture)
    terminal.size = (12, 60)
    journey.smoke.set_size(terminal.master, terminal.size)
    short = terminal.wait("Message Plexmaton", "Responding", absent=("Waiting to send",))
    (output / "input-queue-short.txt").write_text(short)
    terminal.resize(120, "Waiting to send · 2", "Alt-↑")


def run_smoke(provider, paused):
    with tempfile.TemporaryDirectory(prefix="plexmaton-input-queue-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        home.mkdir()
        project.mkdir()
        skill = project / ".agents/skills/100"
        skill.mkdir(parents=True)
        (skill / "SKILL.md").write_text(SKILL_BODY)
        (home / "config.toml").write_text(f'''active_model = {{ provider = "fixture", model = "queue" }}
[providers.fixture]
base_url = "{provider.base_url}"
api_key_env = "PLEXMATON_QUEUE_FIXTURE_KEY"
api = "openai_chat_completions"
[providers.fixture.models.queue]
id = "fixture-only"
display_name = "QueueFixture"
context_window_tokens = 32768
max_output_tokens = 4096
output_reserve_tokens = 4096
''')
        environment = dict(fixture_environment(), PLEXMATON_HOME=str(home),
                           PLEXMATON_QUEUE_FIXTURE_KEY="fixture-only")
        with journey.Terminal(project, environment, "input-queue") as terminal:
            terminal.wait("Message Plexmaton")
            terminal.prompt(FIRST, "FIRST_STREAM", "Responding")
            terminal.prompt(OLDER, "Waiting to send · 1", OLDER)
            # A numeric skill needs its explicit picker binding; literal currency does not
            # activate it. The final request therefore proves the binding survived withdrawal.
            terminal.send(b"$", "$100", "Skills")
            terminal.send(b"\t", "$100", absent=("Tab/Enter insert",))
            # Ctrl-J inserts a real newline; Enter submits the complete two-line message.
            terminal.send(NEWEST.removeprefix("$100 ").replace("\n", "\x0a").encode() + journey.ENTER,
                          "Waiting to send · 2", "Return this exact message")
            capture_widths(terminal)
            before = saved_journal(home)
            terminal.send(b"DRAFT_KEEP", "DRAFT_KEEP", "Alt-↑ needs an empty draft")
            terminal.send(ALT_UP, "Waiting to send · 2", "DRAFT_KEEP", "Responding")
            assert saved_journal(home) == before, "occupied-draft refusal changed history"
            # Ctrl-C clears only this nonempty draft; it must not cancel the running response.
            terminal.send(b"\x03", "Waiting to send · 2", "takes back the last one", absent=("DRAFT_KEEP",))
            screen = terminal.send(ALT_UP, "Waiting to send · 1", "Responding",
                                   "Return this exact message", "Keep 中文 and spacing intact")
            composer = screen[screen.index("Message Plexmaton"):]
            # The terminal decoder retains a filler cell after wide glyphs; the request below
            # proves exact source bytes independently of their display-cell representation.
            assert all(line.replace(" ", "") in composer.replace(" ", "")
                       for line in NEWEST.splitlines()), composer
            assert "↳ " + OLDER in screen, "withdrew the oldest waiting message"
            assert saved_journal(home) == before, "withdrawal changed durable history"
            requests, errors = provider.snapshot()
            assert len(requests) == 1 and not errors, (requests, errors)
            initial_users = [message["content"] for message in requests[0]["messages"]
                             if message["role"] == "user"]
            assert initial_users[-1] == FIRST, initial_users

            # Completion is released by the observed withdrawal, never by a guessed delay.
            paused.release.set()
            terminal.wait("SECOND_DONE", "Return this exact message", absent=("Waiting to send",))
            requests, errors = provider.snapshot()
            assert len(requests) == 2 and not errors, (requests, errors)
            messages = requests[1]["messages"]
            assert [message["content"] for message in messages if message["role"] == "user"] == initial_users + [OLDER], messages
            assert any(message.get("content") == "FIRST_STREAM" for message in messages), messages
            assert b"Return this exact message" not in saved_journal(home)

            # Only an explicit resubmission makes the returned draft a third request.
            terminal.send(journey.ENTER, "THIRD_DONE", absent=("Waiting to send",))
            requests, errors = provider.snapshot()
            assert len(requests) == 3 and not errors, (requests, errors)
            users = [message["content"] for message in requests[2]["messages"] if message["role"] == "user"]
            skill_contexts = [text for text in users if text.startswith("Plexmaton activated skill context:\n")]
            assert len(skill_contexts) == 1, users
            activation = json.loads(skill_contexts[0].split("\n", 1)[1])
            assert activation["name"] == "100" and activation["instructions"] == "Keep FIXTURE_SKILL_MARKER intact.\n", activation
            assert [text for text in users if text not in skill_contexts] == initial_users + [OLDER, NEWEST], users
            terminal.quit()


def main():
    subprocess.run(["cargo", "build", "--locked", "-p", "plexmaton-cli", "--bin", "plexmaton", "--quiet"],
                   cwd=ROOT, check=True)
    paused = PausedResponse(response({"role": "assistant", "content": "FIRST_STREAM"}, "stop", "first"))
    with ScriptedProvider([paused,
                           response({"role": "assistant", "content": "SECOND_DONE"}, "stop", "second"),
                           response({"role": "assistant", "content": "THIRD_DONE"}, "stop", "third")]) as provider:
        run_smoke(provider, paused)
    print("input queue smoke passed: three local requests, draft guard, exact text/skill withdrawal and continuation")


if __name__ == "__main__":
    main()
