#!/usr/bin/env python3
"""Phase 03's journey: delegate, read the child's own work, take its answer, and restart.

Every collaboration mechanism has its own unit evidence. This script is the acceptance evidence:
what a person does in the real binary, and what has to be on screen for them. A mechanism can be
specified, implemented, tested, green, and called from production, and still show the user nothing;
only this says otherwise.

Passing here is acceptance evidence and nothing else. It does not say a mechanism is wired, and
failing to appear here does not say one is unwired — `handoff` is called from production today and
has no step below. Phase 03's exit gate is this script passing with Stop, Handoff and reopening a
resumed child's conversation among its steps.
"""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile

from provider_fixture import AddressedProvider, calls, says
from smoke_support import ESC, ROOT, Terminal, click, fixture_environment

ASK = "DELEGATE_ASK please have someone count the fixtures"
TASK = "COUNT_THE_FIXTURES in this project and report the number"
WORKING = "CHILD_WORKING counting them now"
REPORT = "CHILD_REPORT the project holds two fixtures"
WAITING = "MAIN_WAITING for the delegated answer"
SAW = "MAIN_SAW_THE_REPORT and agrees"
DONE = "CHILD_DONE"
# Three names for two conversations: the roster and the inspector title say `Delegated 1`, a root
# entry addresses `delegated-1`, and the child's entries address the root as `agent-primary`.
CHILD, TARGET, ROOT_AGENT = "Delegated 1", "delegated-1", "agent-primary"
# The roster's first row, which is the only delegated session this journey creates.
ROSTER_ROW = (5, 1)
# What each conversation must show: the child's own work, and the root's task, letter and answer.
CHILD_SIDE = (CHILD, WORKING, DONE)
ROOT_SIDE = (f"assigned to {TARGET}", f"received from {TARGET}", SAW)


def script():
    """Each reply is addressed to the request that earns it; the two runners interleave freely.

    The root and the child never share a cue: a cue is either prose this journey wrote or a
    tool-call identity the fixture minted, and the runtime chooses neither.
    """
    return [
        # The root is asked for something it does not do itself.
        (ASK, calls("mainask", ("delegate", {"task": TASK}))),
        # `delegate` returned a handle. The root says what it is doing and stops.
        ("call_DELEGATE_mainask", says(WAITING)),
        # The child receives the task as its own work, narrates it, and mails the result back.
        (TASK, calls("childwork", ("send_mail", {"summary": REPORT, "artifacts": []}), text=WORKING)),
        # The child's letter was accepted; its turn ends.
        ("call_SEND_MAIL_childwork", says(DONE)),
        # CMP-1: the root admits its own turn to read the inbox, and answers with the letter in it.
        (REPORT, says(SAW)),
    ]


def configure(home, provider):
    home.mkdir()
    (home / "config.toml").write_text(f'''active_model = {{ provider = "fixture", model = "delegate" }}
[providers.fixture]
base_url = "{provider.base_url}"
api_key_env = "PLEXMATON_DELEGATE_FIXTURE_KEY"
api = "openai_chat_completions"
[providers.fixture.models.delegate]
id = "fixture-only"
display_name = "DelegateFixture"
context_window_tokens = 32768
max_output_tokens = 4096
output_reserve_tokens = 4096
''')
    return dict(fixture_environment(), PLEXMATON_HOME=str(home),
                PLEXMATON_DELEGATE_FIXTURE_KEY="fixture-only")


def records(path):
    return [json.loads(line) for line in path.read_text().splitlines()]


def one(folder):
    paths = list(folder.glob("*.jsonl"))
    assert len(paths) == 1, (folder, paths)
    return paths[0]


def open_child(terminal, *markers):
    """Select the delegated session on the roster, which is what puts it on screen (INS-4).

    The row is clicked rather than reached with `Tab`: focus cycles through whatever surfaces exist
    at that moment, and a wait between the keys is impossible because focus is drawn in colour
    alone, so every marker that could end the wait is on screen before the key is read. A press
    names the row instead of counting hops to it.
    """
    click(terminal.master, ROSTER_ROW, terminal.capture)
    return terminal.wait(CHILD, *markers)


def no_dropped_events(terminal):
    """The projection must publish every delegated fact, in order.

    A drop is reported on screen as a `[drop] ... stale event sequence` notice, and the notice
    panel it opens also moves everything below it. Asserted by name so the run says what happened
    instead of failing later on a moved row.
    """
    assert b"stale event sequence" not in bytes(terminal.capture), \
        "the projection dropped a delegated event as stale"


def close_child(terminal, *markers, absent=()):
    """DRW-3, one layer per Escape: the first leaves the window, the second clears the selection."""
    terminal.send(ESC, CHILD)
    return terminal.send(ESC, *markers, absent=absent)


def both_at_three_widths(terminal):
    """Both conversations at three widths, including what the narrowest one has to give up.

    Docking is width-dependent, so the same markers at every width would prove neither side. At 120
    and 95 the root's task, letter and answer stay beside the child's. At 60 the child takes the
    column and the root is off screen — and closing it at that same width must hand the root's
    three entries back, or the narrow layout would be a place where delegated work is unreadable.

    The sweep opens on a width nobody asked for: a resize to the width already set sends no
    `SIGWINCH`, nothing repaints, and the frame this reads from would be empty.
    """
    for width, label in [(121, None), (120, "wide"), (95, "medium")]:
        screen = terminal.resize(width, *CHILD_SIDE, *ROOT_SIDE)
        if label:
            write_frame(terminal, label, screen)
    write_frame(terminal, "narrow", terminal.resize(60, *CHILD_SIDE, absent=(SAW,)))
    write_frame(terminal, "narrow-root", close_child(terminal, *ROOT_SIDE, absent=(WORKING,)))
    terminal.resize(120, *ROOT_SIDE, absent=(WORKING,))


def write_frame(terminal, label, screen):
    (terminal.artifacts() / f"delegate-child-{label}.txt").write_text(
        "\n".join(row.rstrip() for row in screen.splitlines()) + "\n")


def run_smoke(provider):
    with tempfile.TemporaryDirectory(prefix="plexmaton-delegate-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        project.mkdir()
        environment = configure(home, provider)
        (project / "first").write_text("first fixture\n")
        (project / "second").write_text("second fixture\n")

        with Terminal(project, environment, "delegate", "round-trip") as terminal:
            terminal.wait("Message Plexmaton")
            # CTL-1: one model-visible call creates a real child, and the roster names it.
            terminal.prompt(ASK, WAITING, CHILD)
            # ENT-1: the root's own transcript holds the task it handed out, addressed to the child.
            terminal.wait(f"assigned to {TARGET}", "COUNT_THE_FIXTURES")
            # CMP-1: the answer arrives as mail the root reads in a turn it admits itself.
            terminal.wait(f"received from {TARGET}", "CHILD_REPORT", SAW)
            # Checked before the roster is touched: a drop opens a notice panel that moves every
            # row below it, and the click that opens the child would then miss for a reason the
            # failure does not name.
            no_dropped_events(terminal)
            # CCV-1: the child's own conversation — its prose, its tool, its letter — under the
            # roster name, on the same entry grammar the primary uses.
            open_child(terminal, f"assigned by {ROOT_AGENT}", WORKING, "send_mail · succeeded",
                       f"sent to {ROOT_AGENT}", DONE)
            both_at_three_widths(terminal)
            journal = one(home / "sessions")
            no_dropped_events(terminal)
            terminal.quit()

        assert len(records(one(home / "delegated-sessions"))) > 1, "the child kept no journal"
        kinds = {record["event"]["kind"] for record in records(one(home / "collaborations"))
                 if "event" in record}
        assert {"delegation_created", "mail_accepted"} <= kinds, kinds

        # CHB-3: a default resume wakes only the root, and what it delegated is still in the
        # conversation. Reopening the child's own conversation is a step of this journey that does
        # not pass yet: the roster lists it with its counts, and `Down` and `Enter` on that row
        # paint nothing at all, so there is no way to read the history the journal still holds.
        with Terminal(project, environment, "delegate", "resume", ("resume", journal.stem)) as terminal:
            terminal.wait(f"assigned to {TARGET}", f"received from {TARGET}", SAW)
            no_dropped_events(terminal)
            terminal.quit()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true", help="use the current task's verified debug binary")
    arguments = parser.parse_args()
    if not arguments.skip_build:
        subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "plexmaton-cli",
                        "--bin", "plexmaton", "--quiet"], cwd=ROOT, check=True)
    with AddressedProvider(script()) as provider:
        run_smoke(provider)
    print("delegate smoke passed: one delegation; the child's own work and letter; both "
          "conversations at 120 and 95, the child alone at 60, and the root readable there once "
          "the child is closed; no dropped event; a durable ledger and child journal; and resume")


if __name__ == "__main__":
    main()
