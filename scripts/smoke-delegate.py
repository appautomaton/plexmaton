#!/usr/bin/env python3
"""Phase 03's journey: delegate, read the child's own work, take its answer, and restart.

Every collaboration mechanism has its own unit evidence. This script is the acceptance evidence:
what a person does in the real binary, and what has to be on screen for them. A mechanism can be
specified, implemented, tested, green, and called from production, and still show the user nothing;
only this says otherwise.

Passing here is acceptance evidence and nothing else. It does not say a mechanism is wired, and
failing to appear here does not say one is unwired. Phase 03's exit gate is this script passing
with Stop, Handoff, focused child input and reopening a resumed child's conversation among its
steps.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import threading
import time

from provider_fixture import AddressedProvider, PausedResponse, asked, calls, response, says
from smoke_support import (
    DOWN,
    ENTER,
    ESC,
    ROOT,
    Terminal,
    click,
    collapsed,
    fixture_environment,
    observe_for,
    read_to_eof,
    rendered_screen,
    sgr_press,
)

ASK = "DELEGATE_ASK please have someone count the fixtures"
TASK = "COUNT_THE_FIXTURES in this project and report the number"
HISTORY_PREFIX = "CHILD_HISTORY_LINE_"
WORKING = "CHILD_WORKING counting them now\n" + "\n".join(
    f"CHILD_HISTORY_LINE_{index:02d}" for index in range(48)
)
REPORT = "CHILD_REPORT the project holds two fixtures"
WAITING = "MAIN_WAITING for the delegated answer"
SAW = "MAIN_SAW_THE_REPORT and agrees"
DONE = "CHILD_DONE"
RESTORED = "Conversation restored."
CHILD, TARGET, ROOT_AGENT = "Delegated 1", "Delegated 1", "Plexmaton"
INTERNAL_NAMES = ("agent-primary", "delegated-1", "child-runtime")
# The roster's first row, which is the only delegated session this journey creates.
ROSTER_ROW = (5, 1)
# What each conversation must show: the child's own work, and the root's task, letter and answer.
CHILD_SIDE = (CHILD, "send_mail · succeeded", f"sent to {ROOT_AGENT}", DONE)
ROOT_SIDE = (f"assigned to {TARGET}", f"received from {TARGET}", SAW)

HANDOFF_ASK = "HANDOFF_ASK transfer the delegated child to me"
HANDOFF_DONE = "HANDOFF_DONE user control is ready"
CHILD_INPUT = "USER_CHILD_INPUT answer this child directly"
CHILD_ANSWER = "USER_CHILD_ANSWER received only by the child"
HANDOFF_ROW = "handoff · Controller: User"
UPDATE_ASK = "UPDATE_TASK_ASK continue the delegated child with a revised task"
UPDATED_TASK = "RETAIN_TOTAL_2_FIXTURES"
UPDATE_DONE = "UPDATE_TASK_DONE revised and scheduled once"
UPDATE_CHILD_ACK = "CHILD_UPDATE_ACK retained the verified total"
USER_CHILD_SIDE = (CHILD, HANDOFF_ROW, CHILD_INPUT, CHILD_ANSWER)
USER_ROOT_SIDE = (HANDOFF_DONE,)

STOP_ASK = "STOP_DELEGATE_ASK create a child and leave it working"
STOP_TASK = "STOP_CHILD_TASK keep the provider stream open"
STOP_CHILD_WORKING = "STOP_CHILD_WORKING first stream event"
STOP_CHILD_LATE = "STOP_CHILD_LATE_AFTER_STOP must never appear"
STOP_ROOT_INPUT = "STOP_ROOT_INPUT root remains responsive"
STOP_ROOT_ANSWER = "STOP_ROOT_ANSWER root answered after child Stop"

APPROVAL_ASK = "APPROVAL_DELEGATE_ASK create a child that requests inspection"
APPROVAL_TASK = "APPROVAL_CHILD_TASK inspect the first fixture"
APPROVAL_WAITING = "APPROVAL_ROOT_WAITING for the child inspection"
APPROVAL_UPDATE_ASK = "APPROVAL_UPDATE_ASK submit new work after process recovery"
APPROVAL_UPDATED_TASK = "APPROVAL_UPDATED_TASK inspect under the current policy"
APPROVAL_UPDATE_DONE = "APPROVAL_UPDATE_DONE new work submitted"
APPROVAL_POLICY_DENIED = "APPROVAL_POLICY_DENIED current rules refused the new read"
APPROVAL_REPORT = "APPROVAL_CHILD_REPORT current policy was applied"
APPROVAL_CHILD_DONE = "APPROVAL_CHILD_DONE"
APPROVAL_ROOT_SAW = "APPROVAL_ROOT_SAW fresh policy result"

KILL_ASK = "KILL_DELEGATE_ASK create a recoverable child"
KILL_TASK = "KILL_CHILD_TASK retain this task through process death"
KILL_WAITING = "KILL_ROOT_WAITING for durable mail"
KILL_CHILD_WORK = "KILL_CHILD_WORK completed before process death"
KILL_REPORT = "KILL_CHILD_REPORT durable correspondence"
KILL_CHILD_DONE = "KILL_CHILD_DONE mail acknowledged"
KILL_ROOT_SAW = "KILL_ROOT_SAW durable correspondence"
KILL_UPDATE_ASK = "KILL_UPDATE_ASK revise the child task before process death"
KILL_UPDATED_TASK = "KILL_UPDATED_TASK survive the process boundary"
KILL_UPDATE_DONE = "KILL_UPDATE_DONE task acknowledged"
KILL_CHILD_PAUSED = "KILL_CHILD_PAUSED transient prefix"
KILL_CHILD_LATE = "KILL_CHILD_LATE must not survive"
KILL_ROOT_CONTINUE = "KILL_ROOT_CONTINUE use the recovered collaboration context"
KILL_ROOT_ANSWER = "KILL_ROOT_ANSWER recovered context retained"
KILL_HANDOFF_ASK = "KILL_HANDOFF_ASK begin a transfer and pause before acknowledgement"
RECOVERY_WARNING = "The previous turn didn't finish."
KILL_ROOT_MARKERS = (
    KILL_ASK,
    f"assigned to {TARGET}",
    f"received from {TARGET}",
    KILL_ROOT_SAW,
    KILL_UPDATE_ASK,
    KILL_UPDATED_TASK,
    KILL_UPDATE_DONE,
)
KILL_IDLE_ROOT_MARKERS = KILL_ROOT_MARKERS[:4]
KILL_CHILD_MARKERS = (
    f"assigned by {ROOT_AGENT}",
    KILL_CHILD_WORK,
    "send_mail · succeeded",
    f"sent to {ROOT_AGENT}",
    KILL_CHILD_DONE,
    KILL_UPDATED_TASK,
)
KILL_IDLE_CHILD_MARKERS = KILL_CHILD_MARKERS[:5]
KILL_REQUEST_CUES = (
    KILL_ASK,
    "call_DELEGATE_killmain",
    KILL_TASK,
    "call_SEND_MAIL_killchild",
    KILL_REPORT,
    KILL_UPDATE_ASK,
    "call_UPDATE_TASK_killtaskupdate",
    KILL_UPDATED_TASK,
    KILL_ROOT_CONTINUE,
    KILL_HANDOFF_ASK,
)
KILL_CHILD_REQUEST_CUES = {
    KILL_TASK,
    "call_SEND_MAIL_killchild",
    KILL_UPDATED_TASK,
}


class QuietPausedResponse(PausedResponse):
    """Release a cancelled client without turning its expected broken pipe into a fixture error."""

    def __init__(self, data):
        super().__init__(data)
        self.finished = threading.Event()

    def write(self, stream):
        try:
            stream.write(self.prefix)
            stream.flush()
            assert self.release.wait(timeout=30), "paused fixture was not released"
            try:
                stream.write(self.remainder)
                stream.flush()
            except OSError:
                # Stop or process death closes the provider stream. The late bytes are discarded
                # so socket teardown cannot hide the product-level stale-event assertion.
                pass
        finally:
            self.finished.set()


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


def paused_text_response(prefix, late, identity):
    """Expose one transient delta, then hold the terminal event behind an owned barrier."""
    initial = response({"role": "assistant", "content": prefix}, "stop", identity)
    boundary = initial.index(b"\n\n") + 2
    event = {
        "id": identity,
        "object": "chat.completion.chunk",
        "choices": [{"index": 0, "delta": {"content": late}, "finish_reason": None}],
    }
    data = initial[:boundary] + b"data: " + json.dumps(event).encode() + b"\n\n" + initial[boundary:]
    return QuietPausedResponse(data)


def delegated_target(body):
    """Read the opaque target from Main's earlier successful delegation result."""
    for message in reversed(body.get("messages") or []):
        content = message.get("content")
        if not isinstance(content, str):
            continue
        try:
            result = json.loads(content)
        except json.JSONDecodeError:
            continue
        if isinstance(result, dict) and result.get("status") == "delegated":
            target = result.get("target")
            assert isinstance(target, str) and target, result
            return target
    raise AssertionError("the request has no earlier delegated target")


def handoff_reply(body):
    return calls("handoff", ("handoff", {"target": delegated_target(body)}))


def update_task_reply(body):
    return calls(
        "taskupdate",
        ("update_task", {"target": delegated_target(body), "task": UPDATED_TASK}),
    )


def kill_update_task_reply(body):
    return calls(
        "killtaskupdate",
        ("update_task", {"target": delegated_target(body), "task": KILL_UPDATED_TASK}),
    )


def approval_update_task_reply(body):
    return calls(
        "approvaltaskupdate",
        ("update_task", {"target": delegated_target(body), "task": APPROVAL_UPDATED_TASK}),
    )


class CollaborationProvider(AddressedProvider):
    """Allows addressed replies to derive the opaque selector from their exact request."""

    @staticmethod
    def payload(entry):
        data = entry[1]
        return b"dynamic handoff reply" if callable(data) else data

    def choose(self, body):
        question = asked(body)
        for index, (cue, data) in enumerate(self.responses):
            if index not in self.answered and cue in question:
                self.answered.add(index)
                return data(body) if callable(data) else data
        raise AssertionError(f"no scripted reply is addressed to {question[:200]}")


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
        # Main updates the canonical task; the owner schedules that exact child once.
        (UPDATE_ASK, update_task_reply),
        ("call_UPDATE_TASK_taskupdate", says(UPDATE_DONE)),
        (UPDATED_TASK, says(UPDATE_CHILD_ACK)),
        # Main's model uses the opaque selector from its own earlier tool result.
        (HANDOFF_ASK, handoff_reply),
        ("call_HANDOFF_handoff", says(HANDOFF_DONE)),
        # After acknowledged transfer, the user's focused child composer owns this turn.
        (CHILD_INPUT, says(CHILD_ANSWER)),
    ]


def kill_resume_script(paused):
    """One killed child request, one explicit root continuation and one pending Handoff cut."""
    return [
        (KILL_ASK, calls("killmain", ("delegate", {"task": KILL_TASK}))),
        ("call_DELEGATE_killmain", says(KILL_WAITING)),
        (
            KILL_TASK,
            calls(
                "killchild",
                ("send_mail", {"summary": KILL_REPORT, "artifacts": []}),
                text=KILL_CHILD_WORK,
            ),
        ),
        ("call_SEND_MAIL_killchild", says(KILL_CHILD_DONE)),
        (KILL_REPORT, says(KILL_ROOT_SAW)),
        (KILL_UPDATE_ASK, kill_update_task_reply),
        ("call_UPDATE_TASK_killtaskupdate", says(KILL_UPDATE_DONE)),
        (KILL_UPDATED_TASK, paused),
        (KILL_ROOT_CONTINUE, says(KILL_ROOT_ANSWER)),
        (KILL_HANDOFF_ASK, handoff_reply),
    ]


def approval_recovery_script():
    """A restored approval stays cancelled; fresh submitted work uses the current child policy."""
    read = {"path": "first", "offset": None, "limit": None}
    return [
        (APPROVAL_ASK, calls("approvalmain", ("delegate", {"task": APPROVAL_TASK}))),
        ("call_DELEGATE_approvalmain", says(APPROVAL_WAITING)),
        (APPROVAL_TASK, calls("approvaloldread", ("read_file", read))),
        (APPROVAL_UPDATE_ASK, approval_update_task_reply),
        ("call_UPDATE_TASK_approvaltaskupdate", says(APPROVAL_UPDATE_DONE)),
        (APPROVAL_UPDATED_TASK, calls("approvalfreshread", ("read_file", read))),
        (
            "call_READ_FILE_approvalfreshread",
            calls(
                "approvalmail",
                ("send_mail", {"summary": APPROVAL_REPORT, "artifacts": []}),
                text=APPROVAL_POLICY_DENIED,
            ),
        ),
        ("call_SEND_MAIL_approvalmail", says(APPROVAL_CHILD_DONE)),
        (APPROVAL_REPORT, says(APPROVAL_ROOT_SAW)),
    ]


def configuration_source(provider, inspection_policy=None):
    source = f'''active_model = {{ provider = "fixture", model = "delegate" }}
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
'''
    if inspection_policy is not None:
        source += f'''\n[[permissions.rules]]
action = "{inspection_policy}"
match = {{ kind = "native_inspection" }}
'''
    return source


def configure(home, provider, inspection_policy=None):
    home.mkdir()
    (home / "config.toml").write_text(configuration_source(provider, inspection_policy))
    return dict(fixture_environment(), PLEXMATON_HOME=str(home),
                PLEXMATON_DELEGATE_FIXTURE_KEY="fixture-only")


def records(path):
    return [json.loads(line) for line in path.read_text().splitlines()]


def one_event(entries, kind):
    matches = [
        record["event"]
        for record in entries
        if record.get("event", {}).get("kind") == kind
    ]
    assert len(matches) == 1, (kind, matches)
    return matches[0]


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
    os.write(terminal.master, sgr_press(*ROSTER_ROW)[:-1] + b"m")
    return terminal.wait(CHILD, *markers)


def click_narrow_agents(terminal, *markers):
    """Open the Narrow full-region navigator through its visible collapsed handle."""
    at = (terminal.size[1] - 6, 0)
    click(terminal.master, at, terminal.capture)
    os.write(terminal.master, sgr_press(*at)[:-1] + b"m")
    return terminal.wait("┌ Agents", *markers, absent=("Agents ^B",), complete=False)


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


def scroll_child_until(terminal, upward, markers, limit=40):
    """Find named semantic rows by bounded user scrolling instead of assuming viewport geometry."""
    event = f"\x1b[<{64 if upward else 65};41;6M".encode()
    wanted = [collapsed(marker.encode()) for marker in markers]
    for _ in range(limit + 1):
        screen = rendered_screen(bytes(terminal.capture[terminal.frame_start:]), terminal.size)
        flat = collapsed(screen.encode())
        if all(marker in flat for marker in wanted):
            return screen
        os.write(terminal.master, event)
        observe_for(terminal.master, 0.02, terminal.capture)
    screen = rendered_screen(bytes(terminal.capture[terminal.frame_start:]), terminal.size)
    raise AssertionError(f"scroll never exposed {markers!r} at {terminal.size}:\n{screen}")


def task_update_at_three_widths(terminal, artifact):
    """The revised task and its exact child continuation stay reachable at every width."""
    for width, label in [(121, None), (120, "wide"), (95, "medium"), (60, "narrow")]:
        terminal.resize(width, UPDATE_CHILD_ACK)
        screen = scroll_child_until(
            terminal, True, (f"assigned by {ROOT_AGENT} · {UPDATED_TASK}",)
        )
        if label:
            write_frame(terminal, artifact, label, screen)
        scroll_child_until(terminal, False, (UPDATE_CHILD_ACK,))
    terminal.resize(120, UPDATE_CHILD_ACK)


def root_task_update_at_three_widths(terminal, artifact):
    """The root's side of the revised task remains readable in every conversation region."""
    markers = (f"assigned to {TARGET}", UPDATED_TASK, UPDATE_DONE)
    for width, label in [(121, None), (120, "wide"), (95, "medium"), (60, "narrow")]:
        if width == 60:
            terminal.resize(width, UPDATE_DONE)
            screen = scroll_child_until(
                terminal, True, (f"assigned to {TARGET} · {UPDATED_TASK}", UPDATE_DONE)
            )
        else:
            screen = terminal.resize(width, *markers)
        if label:
            write_frame(terminal, artifact, label, screen)
        if width == 60:
            scroll_child_until(terminal, False, (UPDATE_DONE,))
    terminal.resize(120, *markers)


def root_history_at_three_widths(terminal, artifact):
    """The root's task, incoming letter, and answer remain readable at each product width."""
    markers = (f"assigned to {TARGET}", f"received from {TARGET}", SAW)
    for width, label in [(121, None), (120, "wide"), (95, "medium"), (60, "narrow")]:
        if width == 60:
            terminal.resize(width, SAW)
            screen = scroll_child_until(terminal, True, markers)
        else:
            screen = terminal.resize(width, *markers)
        if label:
            write_frame(terminal, artifact, label, screen)
        if width == 60:
            scroll_child_until(terminal, False, (SAW,))
    terminal.resize(120, *markers)


def child_history_at_three_widths(terminal, artifact):
    """The child's task, work, and mail remain reachable before returning to its current tail."""
    for width, label in [(121, None), (120, "wide"), (95, "medium"), (60, "narrow")]:
        terminal.resize(width, *USER_CHILD_SIDE)
        oldest = scroll_child_until(
            terminal, True, (f"assigned by {ROOT_AGENT}", "CHILD_WORKING")
        )
        mail = scroll_child_until(
            terminal, False, ("send_mail · succeeded", f"sent to {ROOT_AGENT}", DONE)
        )
        if label:
            write_frame(terminal, f"{artifact}-history", label, oldest)
            write_frame(terminal, f"{artifact}-mail", label, mail)
        scroll_child_until(terminal, False, USER_CHILD_SIDE)
    terminal.resize(120, *USER_CHILD_SIDE)


def no_dropped_events(terminal):
    """The projection must publish every delegated fact, in order.

    A drop is reported on screen as a `[drop] ... stale event sequence` notice, and the notice
    panel it opens also moves everything below it. Asserted by name so the run says what happened
    instead of failing later on a moved row.
    """
    assert b"stale event sequence" not in bytes(terminal.capture), \
        "the projection dropped a delegated event as stale"


def no_internal_names(terminal):
    """Correspondence uses roster labels while durable routing identities stay off screen."""
    screen = bytes(terminal.capture)
    for name in INTERNAL_NAMES:
        assert name.encode() not in screen, f"internal agent identity reached the screen: {name}"


def close_child(terminal, *markers, absent=()):
    """Close the selected child and return to the root conversation in one Escape rung."""
    return terminal.send(ESC, *markers, absent=absent)


def both_at_three_widths(terminal, artifact="child"):
    """Current child and root outcomes at three widths, including the narrow return path.

    Root and child history have their own semantic-scroll sweeps. Here, 120 and 95 keep the root's
    final Handoff result beside the child's User turn. At 60 the child takes the full region and the
    root is off screen; closing it at that width must hand the root result back.

    The sweep opens on a width nobody asked for: a resize to the width already set sends no
    `SIGWINCH`, nothing repaints, and the frame this reads from would be empty.
    """
    for width, label in [(121, None), (120, "wide"), (95, "medium")]:
        screen = terminal.resize(width, *USER_CHILD_SIDE, *USER_ROOT_SIDE)
        if label:
            write_frame(terminal, artifact, label, screen)
    write_frame(terminal, artifact, "narrow",
                terminal.resize(60, *USER_CHILD_SIDE, absent=(SAW,)))
    write_frame(terminal, artifact, "narrow-root",
                close_child(terminal, *USER_ROOT_SIDE, absent=(CHILD_ANSWER,)))
    terminal.resize(120, *USER_ROOT_SIDE, absent=(CHILD_ANSWER,))


def stop_hint_at_three_widths(terminal):
    """Focused Main control shows Stop beside the capability line at every supported width."""
    for width, label, hint in [
        (121, None, True),
        (120, "wide", True),
        (95, "medium", True),
        (60, "narrow", True),
    ]:
        screen = terminal.resize(width, "Controller: Main", STOP_CHILD_WORKING)
        screen = terminal.wait(
            "Controller: Main", STOP_CHILD_WORKING, absent=("Preparing text",)
        )
        assert ("^C Stop" in screen) == hint, (width, screen)
        assert "Read-only files | No shell" in screen, (width, screen)
        if label:
            write_frame(terminal, "stop-main", label, screen)
    terminal.resize(120, "Controller: Main", STOP_CHILD_WORKING, "^C Stop")


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


def invocation_snapshot(path):
    """Debug-binary execution witness, bounded independently of durable exact-retry effects."""
    if not path.exists():
        return []
    invocations = path.read_text().splitlines()
    assert len(invocations) <= len(KILL_REQUEST_CUES), invocations
    assert set(invocations) <= {"delegate", "send_mail", "update_task", "handoff"}, invocations
    return invocations


IDENTITY_FIELDS = {
    "agent",
    "session_id",
    "record_id",
    "id",
    "parent",
    "parent_id",
    "entry_id",
    "agent_id",
    "item_id",
    "turn_id",
    "attempt_id",
    "call_id",
    "artifact_id",
    "attention_id",
    "delegation",
    "collaboration",
    "conversation",
    "item",
    "semantic_boundary",
    "session",
    "turn",
}
TARGET_ID = re.compile(r"target-v1-[0-9a-f]{64}")
LOOPBACK_URL = re.compile(r"http://127\.0\.0\.1:[0-9]+")
GENERATED_ID = re.compile(
    r"\b(?:agent|collaboration-item|conversation|session|turn)-[0-9a-f][0-9a-f-]{15,}\b"
)


def normalize_fixture(value, identities, key=None):
    """Keep semantic structure while replacing run-specific identity and timing values."""
    if isinstance(value, dict):
        return {
            field: normalize_fixture(item, identities, field)
            for field, item in value.items()
            if field != "replay"
        }
    if isinstance(value, list):
        return [normalize_fixture(item, identities, key) for item in value]
    if isinstance(value, str) and key in IDENTITY_FIELDS:
        if value not in identities:
            digest = hashlib.sha256(value.encode()).hexdigest()[:12]
            identities[value] = f"<{key}:{digest}>"
        return identities[value]
    if key and (key.endswith("_at") or key.endswith("_unix_ms")):
        return "<time>"
    if key == "fingerprint":
        return "<fingerprint>"
    if isinstance(value, str):
        value = LOOPBACK_URL.sub("<loopback>", value)
        value = TARGET_ID.sub("<target>", value)

        def replace_generated(match):
            generated = match.group(0)
            if generated not in identities:
                digest = hashlib.sha256(generated.encode()).hexdigest()[:12]
                identities[generated] = f"<generated:{digest}>"
            return identities[generated]

        return GENERATED_ID.sub(replace_generated, value)
    return value


def journal_projection(entries, identities):
    """Reduce the selected head and its exact immutable ancestry from durable mutations."""
    heads = {"main": None}
    selected = "main"
    nodes = {}
    for record in entries:
        kind = record.get("kind")
        if kind == "append_entry":
            entry = record["entry"]
            nodes[entry["id"]] = {
                "sequence": record["sequence"],
                "id": entry["id"],
                "parent_id": entry.get("parent_id"),
                "type": entry["payload"]["type"],
            }
            heads[record["head"]] = entry["id"]
        elif kind == "create_head":
            heads[record["head"]] = record.get("at")
        elif kind == "move_head":
            heads[record["head"]] = record.get("to")
        elif kind == "rename_head":
            heads[record["renamed"]] = heads.pop(record["head"])
            if record["head"] == selected:
                selected = record["renamed"]
        elif kind == "abandon_head":
            heads.pop(record["head"])
        elif kind == "fork_and_select_head":
            heads[record["destination"]] = record.get("at")
            selected = record["destination"]
        elif kind == "select_head":
            selected = record["destination"]

    ancestry = []
    cursor = heads[selected]
    while cursor is not None:
        node = nodes[cursor]
        ancestry.append(node)
        cursor = node["parent_id"]
    ancestry.reverse()
    return {
        "selected_head": selected,
        "selected_ancestry": normalize_fixture(ancestry, identities),
    }


def file_manifest(path, session, identities):
    decoded = records(path)
    header, entries = decoded[0], decoded[1:]
    manifest = {
        "format": header["format"],
        "schema": header["schema"],
        "records": normalize_fixture(entries, identities),
    }
    if session:
        manifest.update(journal_projection(entries, identities))
    return manifest


def collaboration_projection(entries, identities):
    """Summarize canonical tasks, control, correspondence and admitted cross-file items."""
    delegations = {}
    ordered = []
    correspondence = []
    admissions = []
    for record in entries:
        event = record.get("event")
        if event is None:
            continue
        kind = event["kind"]
        if kind == "delegation_created":
            delegation = event["delegation"]
            delegations[delegation] = {
                "delegation": delegation,
                "delegator": event["delegator"],
                "worker": event["worker"],
                "task_history": [event["task"]],
                "controller": "Main",
            }
            ordered.append(delegation)
        elif kind == "task_updated":
            delegations[event["delegation"]]["task_history"].append(event["task"])
        elif kind == "handoff_completed":
            delegations[event["delegation"]]["controller"] = "User"
        elif kind == "mail_accepted":
            mail = event["mail"]
            correspondence.append({
                "sequence": record["sequence"],
                "id": mail["id"],
                "from": mail["from"],
                "to": mail["to"],
                "summary": mail["summary"],
                "artifacts": mail["artifacts"],
            })
        elif kind == "turn_admitted":
            admission = event["admission"]
            admissions.append({
                "sequence": record["sequence"],
                "boundary": admission["boundary"],
                "items": admission["items"],
                "previous": admission["previous"],
            })
    return normalize_fixture(
        {
            "delegations": [delegations[delegation] for delegation in ordered],
            "correspondence": correspondence,
            "admissions": admissions,
        },
        identities,
    )


def semantic_manifest(home):
    """Reviewable durable projections with stable identities, order and selected branches."""
    identities = {}
    root = file_manifest(one(home / "sessions"), True, identities)
    child = file_manifest(one(home / "delegated-sessions"), True, identities)
    collaboration_path = one(home / "collaborations")
    collaboration = file_manifest(collaboration_path, False, identities)
    collaboration["projection"] = collaboration_projection(
        records(collaboration_path)[1:], identities
    )
    return {"root": root, "child": child, "collaboration": collaboration}


def provider_manifest(requests, errors):
    """Keep request ownership and tool shape without copying whole provider payloads."""
    history = []
    answered = set()
    for sequence, request in enumerate(requests, start=1):
        question = asked(request)
        cues = [cue for cue in KILL_REQUEST_CUES if cue not in answered and cue in question]
        assert len(cues) == 1, (sequence, cues, question)
        cue = cues[0]
        answered.add(cue)
        messages = request.get("messages") or []
        tools = [
            tool["function"]["name"]
            for tool in request.get("tools") or []
            if tool.get("type") == "function"
        ]
        history.append({
            "sequence": sequence,
            "actor": "child" if cue in KILL_CHILD_REQUEST_CUES else "root",
            "cue": cue,
            "last_role": messages[-1]["role"],
            "message_count": len(messages),
            "available_tools": tools,
            "prior_tool_calls": sum(len(message.get("tool_calls") or []) for message in messages),
        })
    return {
        "requests": history,
        "root_requests": sum(item["actor"] == "root" for item in history),
        "child_requests": sum(item["actor"] == "child" for item in history),
        "errors": list(errors),
    }


def screen_manifest(screen, markers):
    positions = {marker: screen.index(marker) for marker in markers}
    ordered = [marker for marker, _ in sorted(positions.items(), key=lambda item: item[1])]
    return {"order": ordered, "count": {marker: screen.count(marker) for marker in markers}}


def assert_recovery_prefix(before, after):
    """Only the killed root turn may append typed recovery; all prior semantics stay exact."""
    assert before["child"] == after["child"], "pending Handoff changed passive child history"
    assert before["collaboration"] == after["collaboration"], \
        "pending Handoff became durable across process death"
    assert before["root"]["selected_head"] == after["root"]["selected_head"]
    prior = before["root"]["records"]
    recovered = after["root"]["records"]
    assert recovered[:len(prior)] == prior, "root recovery changed the durable prefix"
    suffix = recovered[len(prior):]
    interruptions = [
        record for record in suffix
        if record.get("kind") == "append_entry"
        and record["entry"]["payload"]["type"] == "turn_interrupted_by_recovery"
    ]
    cancelled_calls = [
        record for record in suffix
        if record.get("kind") == "append_entry"
        and record["entry"]["payload"]["type"] == "tool_call_changed"
        and record["entry"]["payload"].get("status") == "cancelled"
        and record["entry"]["payload"].get("outcome")
        == {"status": "cancelled", "reason": "process_died"}
        and record["entry"]["payload"].get("item_revision") == 2
    ]
    finished = [
        record for record in suffix
        if record.get("kind") == "turn_finished"
        and record["fact"].get("outcome") == "process_died"
    ]
    assert len(suffix) == 3, suffix
    assert len(interruptions) == 1, suffix
    assert len(cancelled_calls) == 1, suffix
    assert len(finished) == 1, suffix


def write_manifest(name, manifest):
    output = ROOT / "target/smoke"
    output.mkdir(parents=True, exist_ok=True)
    (output / f"delegate-{name}.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    )


def wait_for_path(path, terminal, timeout=10):
    deadline = time.monotonic() + timeout
    while not path.exists():
        assert terminal.process.poll() is None, "binary exited before the process-cut marker"
        assert time.monotonic() < deadline, f"timed out waiting for {path.name}"
        observe_for(terminal.master, 0.01, terminal.capture)
    return path.read_text().splitlines()


def kill_terminal(terminal):
    process_group = os.getpgid(terminal.process.pid)
    assert process_group == terminal.process.pid, "terminal binary does not own its process group"
    os.killpg(process_group, signal.SIGKILL)
    # A dense final frame can still be queued on the PTY. Drain it while the killed process closes
    # its slave; waiting without reading can leave macOS reporting the process as exiting forever.
    read_to_eof(terminal.master, terminal.capture, timeout=10)
    status = terminal.process.wait(timeout=1)
    assert status != 0, "process-cut binary exited successfully instead of being killed"


def binary_contains(marker):
    with (ROOT / "target/debug/plexmaton").open("rb") as binary:
        previous = b""
        while chunk := binary.read(1024 * 1024):
            data = previous + chunk
            if marker in data:
                return True
            previous = data[-len(marker):]
    return False


def visible_history_anchor(screen):
    """The first semantic history line in the viewport, independent of border geometry."""
    for row in screen.splitlines():
        start = row.find(HISTORY_PREFIX)
        if start >= 0:
            return row[start:start + len(HISTORY_PREFIX) + 2]
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
            root_history_at_three_widths(terminal, "root-history")
            # CTL-1: the existing target updates its canonical task and schedules that child once.
            requests_before_update, errors_before_update = provider.snapshot()
            focus_primary_and_type(terminal, UPDATE_ASK)
            updated_root = terminal.send(ENTER, UPDATE_DONE, UPDATED_TASK)
            assert_ordered(
                updated_root, UPDATE_ASK, "update_task · succeeded", UPDATED_TASK, UPDATE_DONE
            )
            open_child(terminal, UPDATED_TASK, UPDATE_CHILD_ACK)
            task_update_at_three_widths(terminal, "task-update")
            close_child(terminal, *ROOT_SIDE, UPDATED_TASK, UPDATE_DONE)
            root_task_update_at_three_widths(terminal, "root-task-update")
            requests_after_update, errors_after_update = provider.snapshot()
            assert len(requests_after_update) == len(requests_before_update) + 3, \
                "task update did not schedule the exact child once"
            assert not errors_before_update and not errors_after_update, \
                (errors_before_update, errors_after_update)
            # COL-3/CCV-1–CCV-4: Main's real tool transfers control, then the focused child input
            # reaches that child's provider and leaves the primary composer as a return target.
            focus_primary_and_type(terminal, HANDOFF_ASK)
            terminal.send(ENTER, HANDOFF_DONE, HANDOFF_ROW)
            open_child(terminal, *CHILD_SIDE, HANDOFF_ROW, "Controller: User")
            terminal.send(
                ENTER,
                UPDATED_TASK,
                UPDATE_CHILD_ACK,
                HANDOFF_ROW,
                "Controller: User",
                "Message Plexmaton",
                "to return",
            )
            terminal.send(
                CHILD_INPUT.encode() + ENTER,
                *USER_CHILD_SIDE,
                "Controller: User",
                "Message Plexmaton",
                "to return",
            )
            terminal.widths(
                "handoff-user",
                *USER_CHILD_SIDE,
                "Controller: User",
                "Message Plexmaton",
                "to return",
            )
            close_child(
                terminal,
                *ROOT_SIDE,
                HANDOFF_DONE,
                HANDOFF_ROW,
                absent=(CHILD_INPUT, CHILD_ANSWER),
            )
            # Checked before the roster is touched: a drop opens a notice panel that moves every
            # row below it, and the click that opens the child would then miss for a reason the
            # failure does not name.
            no_dropped_events(terminal)
            no_internal_names(terminal)
            # CCV-1/ENT-1: the child's task, work, letter and current User turn remain reachable.
            open_child(terminal, *USER_CHILD_SIDE)
            child_history_at_three_widths(terminal, "live")
            both_at_three_widths(terminal)
            journal = one(home / "sessions")
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()

        assert len(records(one(home / "delegated-sessions"))) > 1, "the child kept no journal"
        collaboration_records = records(one(home / "collaborations"))
        kinds = {record["event"]["kind"] for record in collaboration_records if "event" in record}
        assert {"delegation_created", "task_updated", "mail_accepted", "handoff_completed"} \
            <= kinds, kinds
        creation = one_event(collaboration_records, "delegation_created")
        update = one_event(collaboration_records, "task_updated")
        assert update["delegation"] == creation["delegation"], (creation, update)
        assert update["task"] == UPDATED_TASK, update
        durable_before_resume = journal_snapshot(home)

        # CHB-3/INS-1/INS-6: pointer selection opens the exact restored child at every responsive
        # width, then closes it at the narrow width and returns the root without starting work.
        with Terminal(project, environment, "delegate", "resume-pointer",
                      ("resume", journal.stem)) as terminal:
            root_screen = terminal.wait(
                f"assigned to {TARGET}", f"received from {TARGET}", SAW,
                UPDATED_TASK, UPDATE_DONE, RESTORED
            )
            assert_ordered(
                root_screen,
                ASK,
                "delegate · succeeded",
                f"assigned to {TARGET}",
                WAITING,
                f"received from {TARGET}",
                SAW,
                UPDATE_ASK,
                "update_task · succeeded",
                UPDATED_TASK,
                UPDATE_DONE,
                HANDOFF_ROW,
                HANDOFF_DONE,
                RESTORED,
            )
            requests_before, errors_before = provider.snapshot()
            open_child(terminal, *USER_CHILD_SIDE, "Controller: User")
            task_update_at_three_widths(terminal, "resumed-task-update")
            both_at_three_widths(terminal, "resumed-pointer")
            # INS-6: a semantic viewport anchor belongs to the child, not the transient window.
            # Park inside the long restored message, close while the Inspector owns focus, then
            # reopen by pointer at the same width and require the same first visible history line.
            open_child(terminal, *USER_CHILD_SIDE, "Controller: User")
            oldest = scroll_child_until(
                terminal, True, (f"assigned by {ROOT_AGENT}", "CHILD_WORKING")
            )
            assert_ordered(oldest, f"assigned by {ROOT_AGENT}", "CHILD_WORKING")
            newest = scroll_child_until(
                terminal,
                False,
                ("send_mail · succeeded", f"sent to {ROOT_AGENT}", DONE),
            )
            assert_ordered(newest, "send_mail · succeeded", f"sent to {ROOT_AGENT}", DONE)
            scroll_child_until(terminal, False, USER_CHILD_SIDE)
            terminal.resize(60, *USER_CHILD_SIDE, "Controller: User")
            anchor_marker = f"{HISTORY_PREFIX}20"
            scroll_child_until(terminal, True, (anchor_marker,))
            focus_primary(terminal)
            parked_screen = terminal.wait(anchor_marker, CHILD)
            write_frame(terminal, "resumed-pointer", "anchor-parked", parked_screen)
            parked = visible_history_anchor(parked_screen)
            assert parked.startswith(HISTORY_PREFIX), parked
            terminal.send(ESC, *ROOT_SIDE, absent=(DONE,))
            reopened = terminal.send(b"\x02" + DOWN + ENTER, CHILD, anchor_marker)
            write_frame(terminal, "resumed-pointer", "anchor-reopened", reopened)
            assert anchor_marker in reopened, \
                ("the resumed child's parked history left the viewport", anchor_marker, parked)
            close_child(terminal, *ROOT_SIDE, absent=(DONE,))
            terminal.resize(120, *ROOT_SIDE, absent=(DONE,))
            requests_after, errors_after = provider.snapshot()
            assert requests_after == requests_before, "passive child browsing contacted the provider"
            assert not errors_before and not errors_after, (errors_before, errors_after)
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()
        assert journal_snapshot(home) == durable_before_resume, \
            "pointer browsing appended a durable fact"

        # A fresh resume begins on the roster. Down opens the same persisted child by keyboard;
        # Enter explicitly moves into the read-only window, and every width retains its work.
        with Terminal(project, environment, "delegate", "resume-keyboard",
                      ("resume", journal.stem)) as terminal:
            terminal.wait(
                f"assigned to {TARGET}", f"received from {TARGET}", SAW,
                UPDATED_TASK, UPDATE_DONE, RESTORED
            )
            requests_before, errors_before = provider.snapshot()
            terminal.send(DOWN, *USER_CHILD_SIDE, "Controller: User")
            task_update_at_three_widths(terminal, "resumed-keyboard-task")
            terminal.send(ENTER, "Controller: User", "Message Plexmaton", *USER_CHILD_SIDE)
            terminal.widths(
                "resumed-keyboard",
                *USER_CHILD_SIDE,
                "Controller: User",
                "Message Plexmaton",
            )
            requests_after, errors_after = provider.snapshot()
            assert requests_after == requests_before, "keyboard browsing contacted the provider"
            assert not errors_before and not errors_after, (errors_before, errors_after)
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()
        assert journal_snapshot(home) == durable_before_resume, \
            "keyboard browsing appended a durable fact"


def run_kill_resume_smoke(provider, paused):
    """JRN-4/JRN-5/JRN-7/CHB-3: kill the real CLI and compare exact reopened semantics."""
    with tempfile.TemporaryDirectory(prefix="plexmaton-delegate-kill-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        project.mkdir()
        invocation_trace = Path(folder) / "collaboration-invocations.log"
        environment = dict(
            configure(home, provider),
            PLEXMATON_TEST_COLLABORATION_INVOCATIONS=str(invocation_trace),
        )

        # First kill after task and mail are acknowledged while both conversations are idle.
        with Terminal(project, environment, "delegate", "kill-idle") as terminal:
            terminal.wait("Message Plexmaton")
            terminal.prompt(KILL_ASK, KILL_WAITING, CHILD)
            idle_root_screen = terminal.wait(*KILL_IDLE_ROOT_MARKERS)
            idle_root = screen_manifest(idle_root_screen, KILL_IDLE_ROOT_MARKERS)
            requests_before_idle, errors = provider.snapshot()
            assert len(requests_before_idle) == 5 and not errors, (requests_before_idle, errors)
            idle_invocations = invocation_snapshot(invocation_trace)
            assert idle_invocations == ["delegate", "send_mail"], idle_invocations
            journal = one(home / "sessions")
            idle_bytes = journal_snapshot(home)
            idle_manifest = semantic_manifest(home)
            write_manifest(
                "idle-before",
                {
                    "durable": idle_manifest,
                    "root_screen": idle_root,
                    "provider": provider_manifest(requests_before_idle, errors),
                    "tool_invocations": idle_invocations,
                },
            )
            no_dropped_events(terminal)
            no_internal_names(terminal)
            kill_terminal(terminal)

        # The first reopen is passive until explicit task-update input, which then pauses the child.
        with Terminal(
            project,
            environment,
            "delegate",
            "kill-paused-live",
            ("resume", journal.stem),
        ) as terminal:
            idle_root_screen = terminal.wait(*KILL_IDLE_ROOT_MARKERS, RESTORED)
            assert screen_manifest(idle_root_screen, KILL_IDLE_ROOT_MARKERS) == idle_root
            assert provider.snapshot() == (requests_before_idle, errors)
            open_child(terminal, *KILL_IDLE_CHILD_MARKERS, "Controller: Main")
            terminal.wait(*KILL_IDLE_CHILD_MARKERS, "Controller: Main")
            assert provider.snapshot() == (requests_before_idle, errors)
            assert invocation_snapshot(invocation_trace) == idle_invocations
            close_child(terminal, *KILL_IDLE_ROOT_MARKERS)
            assert journal_snapshot(home) == idle_bytes, "idle process recovery wrote a fact"
            assert semantic_manifest(home) == idle_manifest
            focus_primary_and_type(terminal, KILL_UPDATE_ASK)
            terminal.send(ENTER, KILL_UPDATE_DONE, KILL_UPDATED_TASK)
            root_screen = terminal.wait(*KILL_ROOT_MARKERS)
            live_root = screen_manifest(root_screen, KILL_ROOT_MARKERS)
            open_child(terminal, *KILL_CHILD_MARKERS, KILL_CHILD_PAUSED)
            child_screen = terminal.wait(*KILL_CHILD_MARKERS, KILL_CHILD_PAUSED)
            live_child = screen_manifest(child_screen, KILL_CHILD_MARKERS)
            requests_before_kill, errors = provider.snapshot()
            assert len(requests_before_kill) == 8 and not errors, \
                (requests_before_kill, errors)
            invocations_before_kill = invocation_snapshot(invocation_trace)
            assert invocations_before_kill == ["delegate", "send_mail", "update_task"], \
                invocations_before_kill
            journal = one(home / "sessions")
            durable_before_resume = journal_snapshot(home)
            manifest_before_resume = semantic_manifest(home)
            write_manifest(
                "kill-before",
                {
                    "durable": manifest_before_resume,
                    "root_screen": live_root,
                    "child_screen": live_child,
                    "provider": provider_manifest(requests_before_kill, errors),
                    "tool_invocations": invocations_before_kill,
                },
            )
            no_dropped_events(terminal)
            no_internal_names(terminal)
            kill_terminal(terminal)

        paused.release.set()
        assert paused.finished.wait(timeout=10), "paused killed request did not finish"
        child_path = one(home / "delegated-sessions")
        child_bytes = child_path.read_bytes()
        assert KILL_CHILD_PAUSED.encode() not in child_bytes
        assert KILL_CHILD_LATE.encode() not in child_bytes

        # Passive pointer reopen preserves every durable byte and semantic row without a request.
        with Terminal(
            project,
            environment,
            "delegate",
            "kill-resume-pointer",
            ("resume", journal.stem),
        ) as terminal:
            root_screen = terminal.wait(*KILL_ROOT_MARKERS, RESTORED)
            assert screen_manifest(root_screen, KILL_ROOT_MARKERS) == live_root
            requests_before, errors_before = provider.snapshot()
            assert requests_before == requests_before_kill and not errors_before
            open_child(terminal, *KILL_CHILD_MARKERS, "Controller: Main")
            child_screen = terminal.wait(
                *KILL_CHILD_MARKERS,
                "Controller: Main",
                absent=(KILL_CHILD_PAUSED, KILL_CHILD_LATE),
            )
            assert screen_manifest(child_screen, KILL_CHILD_MARKERS) == live_child
            close_child(terminal, KILL_UPDATE_DONE, KILL_UPDATED_TASK)
            assert invocation_snapshot(invocation_trace) == invocations_before_kill
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()
        assert journal_snapshot(home) == durable_before_resume, \
            "passive process recovery appended a durable fact"
        assert semantic_manifest(home) == manifest_before_resume

        # A second passive launch opens the same child by keyboard and adds no recovery debt.
        with Terminal(
            project,
            environment,
            "delegate",
            "kill-resume-keyboard",
            ("resume", journal.stem),
        ) as terminal:
            terminal.wait(KILL_UPDATE_DONE, KILL_UPDATED_TASK, RESTORED)
            requests_before, errors_before = provider.snapshot()
            terminal.send(DOWN, *KILL_CHILD_MARKERS, "Controller: Main")
            child_screen = terminal.send(
                ENTER,
                *KILL_CHILD_MARKERS,
                "Controller: Main",
                absent=(KILL_CHILD_PAUSED, KILL_CHILD_LATE),
            )
            assert screen_manifest(child_screen, KILL_CHILD_MARKERS) == live_child
            assert provider.snapshot() == (requests_before, errors_before)
            assert invocation_snapshot(invocation_trace) == invocations_before_kill
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()
        assert journal_snapshot(home) == durable_before_resume, \
            "repeat passive recovery added debt"

        # Explicit root input is the first new effect and receives the same task/mail context.
        with Terminal(
            project,
            environment,
            "delegate",
            "kill-root-continuation",
            ("resume", journal.stem),
        ) as terminal:
            terminal.wait(KILL_UPDATE_DONE, KILL_UPDATED_TASK, RESTORED)
            requests_before, errors_before = provider.snapshot()
            focus_primary_and_type(terminal, KILL_ROOT_CONTINUE)
            terminal.send(ENTER, KILL_ROOT_ANSWER)
            requests_after, errors_after = provider.snapshot()
            assert len(requests_after) == len(requests_before) + 1
            context = json.dumps(requests_after[-1], ensure_ascii=False)
            assert KILL_REPORT in context and KILL_UPDATED_TASK in context, context
            assert not errors_before and not errors_after
            assert invocation_snapshot(invocation_trace) == invocations_before_kill
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()

        # The pending controller snapshot is process-local. Kill before the Handoff append, then
        # require canonical Main control and one typed root recovery suffix on reopen.
        pending_ready = Path(folder) / "pending-handoff.ready"
        pending_environment = dict(
            environment,
            PLEXMATON_TEST_PENDING_HANDOFF_READY=str(pending_ready),
        )
        with Terminal(
            project,
            pending_environment,
            "delegate",
            "kill-pending-handoff",
            ("resume", journal.stem),
        ) as terminal:
            terminal.wait(KILL_ROOT_ANSWER, RESTORED)
            requests_before_pending, errors = provider.snapshot()
            focus_primary_and_type(terminal, KILL_HANDOFF_ASK)
            os.write(terminal.master, ENTER)
            marker = wait_for_path(pending_ready, terminal)
            assert len(marker) == 2 and marker[0] and marker[1].isdigit(), marker
            requests_at_pending, errors = provider.snapshot()
            assert len(requests_at_pending) == len(requests_before_pending) + 1 and not errors
            invocations_at_pending = invocation_snapshot(invocation_trace)
            assert invocations_at_pending == [
                "delegate", "send_mail", "update_task", "handoff"
            ], invocations_at_pending
            collaboration_records = records(one(home / "collaborations"))
            assert not any(
                record.get("event", {}).get("kind") == "handoff_completed"
                for record in collaboration_records
            )
            manifest_before_pending = semantic_manifest(home)
            write_manifest(
                "pending-before",
                {
                    "durable": manifest_before_pending,
                    "provider": provider_manifest(requests_at_pending, errors),
                    "pending_worker": normalize_fixture(marker[0], {}, "conversation"),
                    "pending_revision": int(marker[1]),
                    "tool_invocations": invocations_at_pending,
                },
            )
            no_dropped_events(terminal)
            no_internal_names(terminal)
            kill_terminal(terminal)

        with Terminal(
            project,
            environment,
            "delegate",
            "pending-handoff-resume",
            ("resume", journal.stem),
        ) as terminal:
            terminal.wait(KILL_HANDOFF_ASK, RECOVERY_WARNING, RESTORED)
            requests_after_pending, errors = provider.snapshot()
            assert requests_after_pending == requests_at_pending and not errors
            assert invocation_snapshot(invocation_trace) == invocations_at_pending
            open_child(terminal, KILL_UPDATED_TASK, "Controller: Main")
            terminal.wait(
                KILL_UPDATED_TASK,
                "Controller: Main",
                absent=(HANDOFF_ROW, KILL_CHILD_LATE),
            )
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()
        manifest_after_pending = semantic_manifest(home)
        assert_recovery_prefix(manifest_before_pending, manifest_after_pending)
        write_manifest(
            "pending-after",
            {
                "durable": manifest_after_pending,
                "provider": provider_manifest(requests_at_pending, errors),
                "tool_invocations": invocations_at_pending,
            },
        )
        recovered_once = journal_snapshot(home)

        with Terminal(
            project,
            environment,
            "delegate",
            "pending-handoff-repeat",
            ("resume", journal.stem),
        ) as terminal:
            terminal.wait(KILL_HANDOFF_ASK, RECOVERY_WARNING, RESTORED)
            requests_repeat, errors = provider.snapshot()
            assert requests_repeat == requests_at_pending and not errors
            assert invocation_snapshot(invocation_trace) == invocations_at_pending
            terminal.send(DOWN, KILL_UPDATED_TASK, "Controller: Main", absent=(HANDOFF_ROW,))
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()
        assert journal_snapshot(home) == recovered_once, \
            "repeat pending-Handoff resume added recovery debt"


def approval_attention_at_three_widths(terminal, artifact):
    """A child request stays on its roster row until explicit user navigation."""
    for width, label in [(121, None), (120, "wide"), (95, "medium")]:
        screen = terminal.resize(
            width,
            CHILD,
            "approval",
            APPROVAL_WAITING,
            "Message Plexmaton",
            absent=("Allow once",),
        )
        if label:
            write_frame(terminal, artifact, label, screen)
    root = terminal.resize(
        60,
        APPROVAL_WAITING,
        "Message Plexmaton",
        "Agents ^B",
        absent=("Allow once",),
    )
    write_frame(terminal, artifact, "narrow", root)
    agents = click_narrow_agents(
        terminal,
        CHILD,
        "approval",
    )
    write_frame(terminal, f"{artifact}-agents", "narrow", agents)
    terminal.send(b"\x02", APPROVAL_WAITING, "Message Plexmaton", "Agents ^B")
    terminal.resize(120, CHILD, "approval", APPROVAL_WAITING, "Message Plexmaton")


def open_child_approval(terminal):
    click(terminal.master, ROSTER_ROW, terminal.capture)
    os.write(terminal.master, sgr_press(*ROSTER_ROW)[:-1] + b"m")
    terminal.wait(CHILD)
    return terminal.send(ENTER, "read_file", "Allow once", "Deny")


def run_approval_recovery_smoke(provider):
    """APV-6/ATT-1: old approval cannot continue; new child work uses current policy."""
    with tempfile.TemporaryDirectory(prefix="plexmaton-delegate-approval-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        project.mkdir()
        denied_read_marker = "DENIED_READ_CONTENT_MUST_NOT_APPEAR"
        (project / "first").write_text(denied_read_marker + "\n")
        invocation_trace = Path(folder) / "collaboration-invocations.log"
        environment = dict(
            configure(home, provider, "ask"),
            PLEXMATON_TEST_COLLABORATION_INVOCATIONS=str(invocation_trace),
        )

        with Terminal(project, environment, "delegate", "approval-before-death") as terminal:
            terminal.wait("Message Plexmaton")
            terminal.prompt(APPROVAL_ASK, APPROVAL_WAITING, CHILD)
            approval_attention_at_three_widths(terminal, "approval-live")
            requests_before, errors = provider.snapshot()
            assert len(requests_before) == 3 and not errors, (requests_before, errors)
            assert invocation_snapshot(invocation_trace) == ["delegate"]
            journal = one(home / "sessions")
            durable_before = journal_snapshot(home)
            open_child_approval(terminal)
            terminal.widths("approval-live-card", "read_file", "Allow once", "Deny")
            no_dropped_events(terminal)
            no_internal_names(terminal)
            kill_terminal(terminal)

        # Process recovery never continues the old call. New work reads a fresh startup policy.
        (home / "config.toml").write_text(configuration_source(provider, "deny"))
        with Terminal(
            project,
            environment,
            "delegate",
            "approval-after-death",
            ("resume", journal.stem),
        ) as terminal:
            terminal.wait(APPROVAL_WAITING, CHILD, "approval", RESTORED)
            assert provider.snapshot() == (requests_before, errors)
            assert invocation_snapshot(invocation_trace) == ["delegate"]
            assert journal_snapshot(home) == durable_before
            approval_attention_at_three_widths(terminal, "approval-resumed")
            open_child_approval(terminal)
            terminal.send(ENTER, "This request is no longer pending.")
            assert provider.snapshot() == (requests_before, errors)
            assert invocation_snapshot(invocation_trace) == ["delegate"]
            assert journal_snapshot(home) == durable_before
            for _ in range(3):
                os.write(terminal.master, ESC)
                observe_for(terminal.master, 0.05, terminal.capture)
            terminal.wait(APPROVAL_WAITING, absent=("Allow once", "Deny"))

            focus_primary_and_type(terminal, APPROVAL_UPDATE_ASK)
            terminal.send(ENTER, APPROVAL_UPDATE_DONE, APPROVAL_UPDATED_TASK)
            terminal.wait(APPROVAL_REPORT, APPROVAL_ROOT_SAW, absent=("( !1 )",))
            requests_after, errors_after = provider.snapshot()
            assert len(requests_after) == 9 and not errors_after, (requests_after, errors_after)
            fresh_result = json.dumps(requests_after[6], ensure_ascii=False)
            assert "forbidden" in fresh_result and denied_read_marker not in fresh_result, fresh_result
            assert invocation_snapshot(invocation_trace) == [
                "delegate", "update_task", "send_mail"
            ]
            open_child(terminal, APPROVAL_POLICY_DENIED, APPROVAL_CHILD_DONE)
            terminal.wait(APPROVAL_POLICY_DENIED, APPROVAL_CHILD_DONE, absent=("Allow once",))
            no_dropped_events(terminal)
            no_internal_names(terminal)
            terminal.quit()


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
            terminal.send(ENTER, "Controller: Main", STOP_CHILD_WORKING)
            stop_hint_at_three_widths(terminal)
            requests, errors = provider.snapshot()
            assert len(requests) == 3 and not errors, (requests, errors)
            assert STOP_CHILD_LATE.encode() not in bytes(terminal.capture)

            # The old route sends this addressed interrupt through Main and exits with WrongAgent.
            # The fixed route settles the child while leaving the PTY and root runtime alive.
            terminal.send(
                b"\x03",
                f"{CHILD} · Idle",
                "Controller: Main",
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
            no_internal_names(terminal)
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
    assert binary_contains(b"PLEXMATON_TEST_PENDING_HANDOFF_READY"), \
        "the debug binary does not contain the pending-Handoff process barrier; rebuild it"
    with CollaborationProvider(script()) as provider:
        run_smoke(provider)
    killed = paused_text_response(KILL_CHILD_PAUSED, KILL_CHILD_LATE, "killchild-paused")
    with CollaborationProvider(kill_resume_script(killed)) as provider:
        run_kill_resume_smoke(provider, killed)
    with CollaborationProvider(approval_recovery_script()) as provider:
        run_approval_recovery_smoke(provider)
    paused = paused_late_response()
    with AddressedProvider(stop_script(paused)) as provider:
        run_stop_smoke(provider, paused)
    print("delegate smoke passed: one delegation and task update; the child's own work and letter; Main Handoff "
          "and focused User child input; both "
          "conversations at 120 and 95, the child alone at 60, and the root readable there once "
          "the child is closed; Narrow Agents opens from its collapsed click handle; no dropped "
          "event; a durable ledger and child journal; passive "
          "pointer and keyboard resume at all three widths with durable task/mail/Handoff placement, "
          "a final restoration confirmation, no request or durable write; and "
          "focused-child Stop through a paused provider with root continuation; actual CLI "
          "kill/resume with equal passive projections, one explicit root continuation, and a "
          "pending-Handoff recovery that stays under Main control; restored child approvals stay "
          "cancelled, while fresh child work uses current policy")


if __name__ == "__main__":
    main()
