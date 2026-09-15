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
import os
from pathlib import Path
import subprocess
import tempfile

from provider_fixture import AddressedProvider, PausedResponse, calls, response, says
from smoke_support import DOWN, ENTER, ESC, ROOT, UP, Terminal, click, fixture_environment, sgr_press

ASK = "DELEGATE_ASK please have someone count the fixtures"
TASK = "COUNT_THE_FIXTURES in this project and report the number"
HISTORY_ANCHOR = "CHILD_HISTORY_LINE_02"
WORKING = "CHILD_WORKING counting them now\n" + "\n".join(
    f"CHILD_HISTORY_LINE_{index:02d}" for index in range(48)
)
REPORT = "CHILD_REPORT the project holds two fixtures"
WAITING = "MAIN_WAITING for the delegated answer"
SAW = "MAIN_SAW_THE_REPORT and agrees"
DONE = "CHILD_DONE"
RESTORED = "Conversation restored."
# Three names for two conversations: the roster and the inspector title say `Delegated 1`, a root
# entry addresses `delegated-1`, and the child's entries address the root as `agent-primary`.
CHILD, TARGET, ROOT_AGENT = "Delegated 1", "delegated-1", "agent-primary"
# The roster's first row, which is the only delegated session this journey creates.
ROSTER_ROW = (5, 1)
# What each conversation must show: the child's own work, and the root's task, letter and answer.
CHILD_SIDE = (CHILD, "send_mail · succeeded", f"sent to {ROOT_AGENT}", DONE)
ROOT_SIDE = (f"assigned to {TARGET}", f"received from {TARGET}", SAW)

STOP_ASK = "STOP_DELEGATE_ASK create a child and leave it working"
STOP_TASK = "STOP_CHILD_TASK keep the provider stream open"
STOP_CHILD_WORKING = "STOP_CHILD_WORKING first stream event"
STOP_CHILD_LATE = "STOP_CHILD_LATE_AFTER_STOP must never appear"
STOP_ROOT_INPUT = "STOP_ROOT_INPUT root remains responsive"
STOP_ROOT_ANSWER = "STOP_ROOT_ANSWER root answered after child Stop"


class QuietPausedResponse(PausedResponse):
    """Release a cancelled client without turning its expected broken pipe into a fixture error."""

    def write(self, stream):
        stream.write(self.prefix)
        stream.flush()
        assert self.release.wait(timeout=30), "paused fixture was not released"
        try:
            stream.write(self.remainder)
            stream.flush()
        except OSError:
            # Stop closes the provider stream. The late bytes are deliberately discarded by the
            # fixture so a socket teardown cannot hide the product-level stale-event assertion.
            pass


def paused_late_response():
    """Put a recognisable assistant delta after the first SSE event and its release barrier."""
    initial = response(
        {"role": "assistant", "content": STOP_CHILD_WORKING},
        "stop",
        "stopchild",
    )
    boundary = initial.index(b"\n\n") + 2
    late = {
        "id": "stopchild",
        "object": "chat.completion.chunk",
        "choices": [{"index": 0, "delta": {"content": STOP_CHILD_LATE}, "finish_reason": None}],
    }
    data = initial[:boundary] + b"data: " + json.dumps(late).encode() + b"\n\n" + initial[boundary:]
    return QuietPausedResponse(data)


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


def focus_primary(terminal):
    """Move focus to the root composer through its real pointer target."""
    at = (30, terminal.size[0] - 3)
    click(terminal.master, at, terminal.capture, cursor=True)
    os.write(terminal.master, sgr_press(*at)[:-1] + b"m")


def focus_primary_and_type(terminal, text):
    """Return focus to the root composer and leave a draft while the child provider is paused."""
    focus_primary(terminal)
    os.write(terminal.master, text.encode())
    return terminal.wait(text)


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


def both_at_three_widths(terminal, artifact="child"):
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
            write_frame(terminal, artifact, label, screen)
    write_frame(terminal, artifact, "narrow",
                terminal.resize(60, *CHILD_SIDE, absent=(SAW,)))
    write_frame(terminal, artifact, "narrow-root",
                close_child(terminal, *ROOT_SIDE, absent=(DONE,)))
    terminal.resize(120, *ROOT_SIDE, absent=(DONE,))


def write_frame(terminal, artifact, label, screen):
    (terminal.artifacts() / f"delegate-{artifact}-{label}.txt").write_text(
        "\n".join(row.rstrip() for row in screen.splitlines()) + "\n")


def journal_snapshot(home):
    """Exact durable bytes: passive browsing may acquire readers but must append no fact."""
    return {
        path.relative_to(home): path.read_bytes()
        for folder in ("sessions", "delegated-sessions", "collaborations")
        for path in sorted((home / folder).glob("*.jsonl"))
    }


def visible_history_anchor(screen):
    """The first semantic history line in the viewport, independent of border geometry."""
    prefix = "CHILD_HISTORY_LINE_"
    for row in screen.splitlines():
        start = row.find(prefix)
        if start >= 0:
            return row[start:start + len(HISTORY_ANCHOR)]
    raise AssertionError("the resumed child viewport has no history anchor")


def assert_ordered(screen, *markers):
    """Require one visible conversation to retain semantic first-appearance order."""
    positions = [screen.index(marker) for marker in markers]
    assert positions == sorted(positions), (markers, positions)


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
            root_screen = terminal.wait(f"received from {TARGET}", "CHILD_REPORT", SAW)
            assert_ordered(
                root_screen,
                ASK,
                "delegate · succeeded",
                f"assigned to {TARGET}",
                WAITING,
                f"received from {TARGET}",
                SAW,
            )
            # Checked before the roster is touched: a drop opens a notice panel that moves every
            # row below it, and the click that opens the child would then miss for a reason the
            # failure does not name.
            no_dropped_events(terminal)
            # CCV-1: the child's own conversation — its prose, its tool, its letter — under the
            # roster name, on the same entry grammar the primary uses.
            open_child(terminal, *CHILD_SIDE)
            both_at_three_widths(terminal)
            journal = one(home / "sessions")
            no_dropped_events(terminal)
            terminal.quit()

        assert len(records(one(home / "delegated-sessions"))) > 1, "the child kept no journal"
        kinds = {record["event"]["kind"] for record in records(one(home / "collaborations"))
                 if "event" in record}
        assert {"delegation_created", "mail_accepted"} <= kinds, kinds
        durable_before_resume = journal_snapshot(home)

        # CHB-3/INS-1/INS-6: pointer selection opens the exact restored child at every responsive
        # width, then closes it at the narrow width and returns the root without starting work.
        with Terminal(project, environment, "delegate", "resume-pointer",
                      ("resume", journal.stem)) as terminal:
            root_screen = terminal.wait(
                f"assigned to {TARGET}", f"received from {TARGET}", SAW, RESTORED
            )
            assert_ordered(
                root_screen,
                ASK,
                "delegate · succeeded",
                f"assigned to {TARGET}",
                WAITING,
                f"received from {TARGET}",
                SAW,
                RESTORED,
            )
            requests_before, errors_before = provider.snapshot()
            open_child(terminal, *CHILD_SIDE)
            both_at_three_widths(terminal, "resumed-pointer")
            # INS-6: a semantic viewport anchor belongs to the child, not the transient window.
            # Park inside the long restored message, close while the Inspector owns focus, then
            # reopen by pointer at the same width and require the same first visible history line.
            open_child(terminal, *CHILD_SIDE)
            terminal.send(ENTER, "Controller unavailable", "Input locked", *CHILD_SIDE)
            oldest = terminal.send(
                UP * 100, f"assigned by {ROOT_AGENT}", "CHILD_WORKING"
            )
            assert_ordered(oldest, f"assigned by {ROOT_AGENT}", "CHILD_WORKING")
            newest = terminal.send(
                DOWN * 100, "send_mail · succeeded", f"sent to {ROOT_AGENT}", DONE
            )
            assert_ordered(newest, "send_mail · succeeded", f"sent to {ROOT_AGENT}", DONE)
            terminal.resize(60, *CHILD_SIDE)
            parked_screen = terminal.send(UP * 12, HISTORY_ANCHOR)
            write_frame(terminal, "resumed-pointer", "anchor-parked", parked_screen)
            parked = visible_history_anchor(parked_screen)
            assert parked == HISTORY_ANCHOR, (parked, HISTORY_ANCHOR)
            focus_primary(terminal)
            terminal.send(ESC, *ROOT_SIDE, absent=(DONE,))
            reopened = terminal.send(b"\t" + DOWN, CHILD, parked)
            write_frame(terminal, "resumed-pointer", "anchor-reopened", reopened)
            assert visible_history_anchor(reopened) == parked, \
                "the resumed child's reading anchor moved across close/reopen"
            close_child(terminal, *ROOT_SIDE, absent=(DONE,))
            terminal.resize(120, *ROOT_SIDE, absent=(DONE,))
            requests_after, errors_after = provider.snapshot()
            assert requests_after == requests_before, "passive child browsing contacted the provider"
            assert not errors_before and not errors_after, (errors_before, errors_after)
            no_dropped_events(terminal)
            terminal.quit()
        assert journal_snapshot(home) == durable_before_resume, \
            "pointer browsing appended a durable fact"

        # A fresh resume begins on the roster. Down opens the same persisted child by keyboard;
        # Enter explicitly moves into the read-only window, and every width retains its work.
        with Terminal(project, environment, "delegate", "resume-keyboard",
                      ("resume", journal.stem)) as terminal:
            terminal.wait(f"assigned to {TARGET}", f"received from {TARGET}", SAW, RESTORED)
            requests_before, errors_before = provider.snapshot()
            terminal.send(DOWN, *CHILD_SIDE)
            terminal.send(ENTER, "Controller unavailable", "Input locked", *CHILD_SIDE)
            terminal.widths("resumed-keyboard", *CHILD_SIDE)
            requests_after, errors_after = provider.snapshot()
            assert requests_after == requests_before, "keyboard browsing contacted the provider"
            assert not errors_before and not errors_after, (errors_before, errors_after)
            no_dropped_events(terminal)
            terminal.quit()
        assert journal_snapshot(home) == durable_before_resume, \
            "keyboard browsing appended a durable fact"


def stop_script(paused):
    """The child response has one visible prefix and one late marker behind the pause barrier."""
    return [
        (STOP_ASK, calls("stopask", ("delegate", {"task": STOP_TASK}))),
        ("call_DELEGATE_stopask", says("STOP_ROOT_WAITING child is running")),
        (STOP_TASK, paused),
        (STOP_ROOT_INPUT, says(STOP_ROOT_ANSWER)),
    ]


def run_stop_smoke(provider, paused):
    """SCH-2/SCH-4/INV-7: Stop the focused child at a paused provider boundary, then continue Main."""
    with tempfile.TemporaryDirectory(prefix="plexmaton-delegate-stop-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        project.mkdir()
        environment = configure(home, provider)

        with Terminal(project, environment, "delegate", "stop") as terminal:
            terminal.wait("Message Plexmaton")
            terminal.prompt(STOP_ASK, "STOP_ROOT_WAITING", CHILD)
            # Selecting then entering the row is the real open/focus path; Ctrl-C then resolves
            # the Inspector conversation rather than the primary runtime (INV-7).
            open_child(terminal, STOP_CHILD_WORKING)
            terminal.send(ENTER, "Controller unavailable", "Input locked", STOP_CHILD_WORKING)
            requests, errors = provider.snapshot()
            assert len(requests) == 3 and not errors, (requests, errors)
            assert STOP_CHILD_LATE.encode() not in bytes(terminal.capture)

            # The old route sends this addressed interrupt through Main and exits with WrongAgent.
            # The fixed route settles the child while leaving the PTY and root runtime alive.
            terminal.send(
                b"\x03",
                f"{CHILD} · Idle",
                "Controller unavailable",
                STOP_CHILD_WORKING,
            )
            assert terminal.process.poll() is None, "focused-child Stop exited the root session"
            # Root input is accepted before the paused handler is released. The HTTP server cannot
            # serve the next request until release, so the draft itself is the pre-release witness;
            # the answer below proves the same root request completes afterward.
            focus_primary_and_type(terminal, STOP_ROOT_INPUT)
            requests, errors = provider.snapshot()
            assert len(requests) == 3 and not errors, (requests, errors)
            release_start = len(terminal.capture)
            paused.release.set()
            terminal.send(ENTER, STOP_ROOT_ANSWER, absent=(STOP_CHILD_LATE,))
            assert STOP_CHILD_LATE.encode() not in bytes(terminal.capture[release_start:])
            no_dropped_events(terminal)
            terminal.quit()

        requests, errors = provider.snapshot()
        assert len(requests) == 4 and not errors, (requests, errors)
        journals = list((home / "delegated-sessions").glob("*.jsonl"))
        assert journals and all(STOP_CHILD_LATE.encode() not in path.read_bytes() for path in journals)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true", help="use the current task's verified debug binary")
    arguments = parser.parse_args()
    if not arguments.skip_build:
        subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "plexmaton-cli",
                        "--bin", "plexmaton", "--quiet"], cwd=ROOT, check=True)
    with AddressedProvider(script()) as provider:
        run_smoke(provider)
    paused = paused_late_response()
    with AddressedProvider(stop_script(paused)) as provider:
        run_stop_smoke(provider, paused)
    print("delegate smoke passed: one delegation; the child's own work and letter; both "
          "conversations at 120 and 95, the child alone at 60, and the root readable there once "
          "the child is closed; no dropped event; a durable ledger and child journal; passive "
          "pointer and keyboard resume at all three widths with durable task/mail placement, "
          "a final restoration confirmation, no request or durable write; and "
          "focused-child Stop through a paused provider with root continuation")


if __name__ == "__main__":
    main()
