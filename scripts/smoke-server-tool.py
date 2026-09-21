#!/usr/bin/env python3
"""ENT-2/PRV-5: a provider-run search lands where the provider placed it, live and on reopen.

One loopback Responses stream with the shape a live gateway produced on 2026-09-21: every
`web_search_call` is added at its true position between the messages, every one of them is
finished only after the last message, and the whole stream arrives in one burst. The rows must
still sit between the narration lines they belong to, run while unfinished, finish without a
query when the route reported none, and read the same after the conversation is reopened.
"""
import json
from pathlib import Path
import subprocess
import tempfile

from provider_fixture import ScriptedProvider
from smoke_support import ROOT, Terminal, fixture_environment


def event(payload):
    return f"event: {payload['type']}\ndata: {json.dumps(payload)}\n\n".encode()


def message(index, item, text):
    return b"".join([
        event({"type": "response.output_item.added", "output_index": index,
               "item": {"id": item, "type": "message", "status": "in_progress", "role": "assistant", "content": []}}),
        event({"type": "response.output_text.delta", "output_index": index, "content_index": 0, "item_id": item, "delta": text}),
        event({"type": "response.output_text.done", "output_index": index, "content_index": 0, "item_id": item, "text": text}),
        event({"type": "response.output_item.done", "output_index": index,
               "item": {"id": item, "type": "message", "status": "completed", "role": "assistant",
                        "content": [{"type": "output_text", "text": text, "annotations": []}]}}),
    ])


def search_added(index, item):
    return event({"type": "response.output_item.added", "output_index": index,
                  "item": {"id": item, "type": "web_search_call", "status": "in_progress",
                           "action": {"type": "search", "query": ""}}})


def search_done(index, item, query):
    action = {"type": "search", "query": query}
    return event({"type": "response.output_item.done", "output_index": index,
                  "item": {"id": item, "type": "web_search_call", "status": "completed", "action": action}})


def burst():
    usage = {"input_tokens": 40, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 30,
             "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 70}
    return b"".join([
        event({"type": "response.created", "response": {"id": "resp_smoke", "status": "in_progress", "output": []}}),
        message(0, "msg_0", "SMOKE_FIRST checking the release page."),
        search_added(1, "ws_1"),
        message(2, "msg_2", "SMOKE_SECOND results point to 1.98.1."),
        search_added(3, "ws_3"),
        message(4, "msg_4", "SMOKE_ANSWER 1.98.1"),
        search_done(1, "ws_1", ""),
        search_done(3, "ws_3", "Rust 1.98.1 release"),
        event({"type": "response.completed", "response": {"id": "resp_smoke", "status": "completed", "usage": usage}}),
    ])


def rows(screen):
    return [row.rstrip("│ ").lstrip("│").strip() for row in screen.splitlines()]


def order(screen, *needles):
    positions = []
    for needle in needles:
        index = next((i for i, row in enumerate(rows(screen)) if row.startswith(needle)), None)
        assert index is not None, f"{needle!r} is not on screen:\n{screen}"
        positions.append(index)
    assert positions == sorted(positions), f"rows out of order {positions}:\n{screen}"


def run_smoke(provider):
    with tempfile.TemporaryDirectory(prefix="plexmaton-server-tool-smoke-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        home.mkdir()
        project.mkdir()
        (home / "config.toml").write_text(f'''active_model = {{ provider = "loop", model = "searcher" }}
[providers.loop]
base_url = "{provider.base_url}"
api_key_env = "LOOP_LOGIN"
api = "openai_responses"
[providers.loop.models.searcher]
id = "searcher-wire"
display_name = "Searcher"
reasoning_effort = "low"
allowed_reasoning_efforts = ["low", "high"]
server_tools = ["web_search"]
context_window_tokens = 32768
max_output_tokens = 4096
output_reserve_tokens = 4096
''')
        environment = dict(fixture_environment(), PLEXMATON_HOME=str(home), LOOP_LOGIN="fixture-only")
        with Terminal(project, environment, "server-tool", "placement") as terminal:
            terminal.wait("Message Plexmaton")
            screen = terminal.prompt("What is the latest stable Rust release?", "SMOKE_ANSWER", "[+] web_search · Rust 1.98.1 release",
                                     absent=("Preparing text", "Running web_search", "· Thinking"))
            # Placed where the provider put them: between the narration lines, not after the answer.
            order(screen, "SMOKE_FIRST", "[+] web_search", "SMOKE_SECOND", "[+] web_search · Rust 1.98.1 release", "SMOKE_ANSWER")
            assert "󱌣 2" in screen, screen
            terminal.widths("placement", "SMOKE_ANSWER", "[+] web_search")
            terminal.quit()
        sessions = list((home / "sessions").glob("*.jsonl"))
        assert len(sessions) == 1, sessions
        with Terminal(project, environment, "server-tool", "reopen", arguments=("resume", sessions[0].stem)) as terminal:
            screen = terminal.wait("SMOKE_ANSWER", "[+] web_search · Rust 1.98.1 release", absent=("Preparing text",))
            order(screen, "SMOKE_FIRST", "[+] web_search", "SMOKE_SECOND", "[+] web_search · Rust 1.98.1 release", "SMOKE_ANSWER")
            terminal.quit()
    requests, errors = provider.snapshot()
    assert not errors, errors
    assert {"type": "web_search"} in requests[0]["tools"], "the declaration crossed the wire"


def main():
    subprocess.run(["cargo", "build", "--locked", "-p", "plexmaton-cli", "--bin", "plexmaton", "--quiet"], cwd=ROOT, check=True)
    ScriptedProvider.endpoint = "/v1/responses"
    with ScriptedProvider([burst()]) as provider:
        run_smoke(provider)
    print("server-tool smoke passed: rows placed where the provider put them, live and on reopen")


if __name__ == "__main__":
    main()
