#!/usr/bin/env python3
"""TRE-1/TRE-4/TRE-5/TRE-7: native rewind, branch return and restart without effect replay."""

import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile

from permission_fixture import ScriptedProvider, command_turn, response
from smoke_support import fixture_environment

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("permission_smoke", ROOT / "scripts/smoke-permissions.py")
journey = importlib.util.module_from_spec(spec)
spec.loader.exec_module(journey)
FIRST, SECOND, EDITED = "TREE_FIRST 中文原始分支", "TREE_SECOND", "TREE_EDITED"
MAIN_CONT, RESUMED = "TREE_MAIN_CONT", "TREE_RESUMED"
EFFECT = "printf x >> tree-effect-count"
END, HOME = b"\x1b[F", b"\x1b[H"


class TreeTerminal(journey.Terminal):
    def wait(self, *markers, absent=()):
        # TRE-1: the opaque tree deliberately hides the composer's completion-rule sentinel.
        # Use the terminal's synchronized frame end plus explicit visible/absent state markers.
        return journey.smoke.await_screen(
            self.master, self.capture, self.size, markers, absent, self.frame_start, complete=False)


def requests(provider, count):
    captured, errors = provider.snapshot()
    assert not errors and len(captured) == count, (captured, errors)
    return captured


def users(request):
    return [message["content"] for message in request["messages"] if message["role"] == "user"]


def capture_widths(terminal):
    output = ROOT / "target/smoke"
    output.mkdir(parents=True, exist_ok=True)
    for width, label in [(121, None), (120, "wide"), (88, "medium"), (60, "narrow")]:
        screen = terminal.resize(width, "Conversation tree", SECOND)
        if label:
            (output / f"tree-{label}.txt").write_text(
                "\n".join(line.rstrip() for line in screen.splitlines()) + "\n")
    terminal.resize(120, "Conversation tree", SECOND)


def run_smoke(provider):
    with tempfile.TemporaryDirectory(prefix="plexmaton-tree-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        home.mkdir()
        project.mkdir()
        (home / "config.toml").write_text(f'''active_model = {{ provider = "fixture", model = "tree" }}
[providers.fixture]
base_url = "{provider.base_url}"
api_key_env = "PLEXMATON_TREE_FIXTURE_KEY"
api = "openai_chat_completions"
[providers.fixture.models.tree]
id = "fixture-only"
display_name = "TreeFixture"
context_window_tokens = 32768
max_output_tokens = 4096
output_reserve_tokens = 4096
''')
        environment = dict(fixture_environment(), PLEXMATON_HOME=str(home),
                           PLEXMATON_TREE_FIXTURE_KEY="fixture-only")
        effect = project / "tree-effect-count"
        with TreeTerminal(project, environment, "tree") as terminal:
            terminal.wait("Message Plexmaton")
            terminal.prompt("/tree", "Conversation tree")
            assert not list((home / "sessions").glob("*.jsonl")), "opening an empty tree wrote history"
            requests(provider, 0)
            terminal.send(journey.ESC, "Message Plexmaton", absent=("Conversation tree",))
            terminal.prompt(FIRST, "Approval required", EFFECT)
            terminal.send(b"1", "FIRST_DONE", absent=("Approval required",))
            assert effect.read_text() == "x"
            terminal.prompt(SECOND, "SECOND_DONE")
            baseline = users(requests(provider, 3)[2])
            assert baseline[-2:] == [FIRST, SECOND], baseline
            journal_paths = list((home / "sessions").glob("*.jsonl"))
            assert len(journal_paths) == 1, journal_paths
            journal = journal_paths[0]
            before = journal.read_bytes()

            terminal.prompt("/rewind", "Conversation tree", SECOND)
            screen = terminal.wait("Conversation tree", SECOND)
            assert " tools " not in screen, "intermediate tool steps leaked into the rewind list"
            assert "[−]" in screen and "Enter rewind" in screen, "displayed fold/action grammar missing"
            capture_widths(terminal)
            terminal.send(journey.ESC, "SECOND_DONE", "Message Plexmaton", absent=("Conversation tree",))
            assert journal.read_bytes() == before, "browsing/cancellation mutated history"
            terminal.prompt("/tree", "Conversation tree", SECOND)
            # The final pair is the second user and its assistant output; target by navigation,
            # not by a screen coordinate that changes across widths.
            terminal.send(END + journey.UP + journey.ENTER, "Message Plexmaton", SECOND,
                          absent=("Conversation tree", "SECOND_DONE"))
            requests(provider, 3)
            assert effect.read_text() == "x", "rewind replayed an effect"
            terminal.send(b"\x03" + EDITED.encode() + journey.ENTER, "EDITED_DONE")
            edited = requests(provider, 4)[3]
            assert users(edited) == baseline[:-1] + [EDITED], edited
            assert not any(message.get("content") == "SECOND_DONE" for message in edited["messages"]), edited

            terminal.prompt("/tree", "Conversation tree", "Messages")
            branches = terminal.send(b"b", "Branches", "main")
            assert all(not row.strip(" │") for row in branches.splitlines()[4:-3]), \
                "switching to branches left old message cells on screen"
            terminal.send(HOME + journey.ENTER, "SECOND_DONE", "Message Plexmaton",
                          absent=("Conversation tree", "EDITED_DONE"))
            terminal.prompt(MAIN_CONT, "MAIN_DONE")
            assert users(requests(provider, 5)[4]) == baseline + [MAIN_CONT]
            assert effect.read_text() == "x", "branch selection replayed an effect"
            # Exit with the non-default head selected: restoring main unconditionally must fail.
            # Reopening retains the previous browsing mode and stable cursor.
            terminal.prompt("/tree", "Conversation tree", "Branches", "main")
            terminal.send(END + journey.ENTER, "EDITED_DONE", "Message Plexmaton",
                          absent=("Conversation tree", "MAIN_DONE"))
            terminal.quit()

        with TreeTerminal(project, environment, "tree-resume", ("resume", journal.stem)) as terminal:
            terminal.wait("EDITED_DONE", "Message Plexmaton", absent=("MAIN_DONE", "SECOND_DONE"))
            requests(provider, 5)
            terminal.prompt(RESUMED, "RESUMED_DONE")
            assert users(requests(provider, 6)[5]) == baseline[:-1] + [EDITED, RESUMED]
            assert effect.read_text() == "x", "restart replayed an effect"
            terminal.quit()
        records = [json.loads(line) for line in journal.read_text().splitlines()]
        assert len(records) > 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true", help="use the current task's verified debug binary")
    arguments = parser.parse_args()
    if not arguments.skip_build:
        subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "plexmaton-cli",
                        "--bin", "plexmaton", "--quiet"], cwd=ROOT, check=True)
    replies = command_turn(EFFECT, "FIRST_DONE")
    for label in ["SECOND_DONE", "EDITED_DONE", "MAIN_DONE", "RESUMED_DONE"]:
        replies.append(response({"role": "assistant", "content": label}, "stop", label))
    with ScriptedProvider(replies) as provider:
        run_smoke(provider)
    print("tree smoke passed: six loopback requests, exact destination context, original branch, restart and one effect")


if __name__ == "__main__":
    main()
