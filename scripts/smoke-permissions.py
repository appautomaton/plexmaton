#!/usr/bin/env python3
"""PER-6/PER-8/PER-10: trust, prefix reuse and revoke through the real executable."""

import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

from provider_fixture import ScriptedProvider, command_turn
from smoke_support import ENTER, ESC, ROOT, UP, Terminal, fixture_environment, read_until


class PermissionTerminal(Terminal):
    """The Drawer route this journey walks repeatedly; no other journey opens Permissions."""

    def __init__(self, project, environment, name, arguments=()):
        super().__init__(project, environment, "permissions", name, arguments)

    def permissions(self):
        self.send(b"\x10perm", "Workspace", "> Permissions", "Esc close")
        return self.send(ENTER, "Permissions", "Project grants")

    def close_permissions(self):
        # One layer per Escape (DRW-3): the page returns to the list, the list to the origin.
        self.send(ESC, "> Permissions", "Esc close")
        self.send(ESC, "Message Plexmaton", absent=("Type to filter",))


def changes(home):
    paths = list((home / "projects").glob("*/permissions.jsonl"))
    assert len(paths) == 1, paths
    records = [json.loads(line) for line in paths[0].read_text().splitlines()]
    return [record["change"] for record in records[1:]]


def run_smoke(provider):
    with tempfile.TemporaryDirectory(prefix="plexmaton-permission-smoke-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        home.mkdir()
        project.mkdir()
        config_dir = project / ".plexmaton"
        config_dir.mkdir()
        config = '''[[permissions.rules]]
action = "allow"
match = { kind = "exact_command", source = "printf trusted > trusted-result" }
'''
        (config_dir / "config.toml").write_text(config)
        (project / "first").write_text("first fixture\n")
        (project / "second").write_text("second fixture\n")
        (home / "config.toml").write_text(f'''active_model = {{ provider = "fixture", model = "permission" }}
[providers.fixture]
base_url = "{provider.base_url}"
api_key_env = "PLEXMATON_PERMISSION_FIXTURE_KEY"
api = "openai_chat_completions"
[providers.fixture.models.permission]
id = "fixture-only"
display_name = "PermissionFixture"
context_window_tokens = 32768
max_output_tokens = 4096
output_reserve_tokens = 4096
''')
        environment = dict(fixture_environment(), PLEXMATON_HOME=str(home),
                           PLEXMATON_PERMISSION_FIXTURE_KEY="fixture-only")
        with PermissionTerminal(project, environment, "first") as terminal:
            terminal.wait("Message Plexmaton")
            terminal.permissions()
            terminal.wait("> Review project configuration rules")
            terminal.send(ENTER, "Project configuration rules", "Continue to activation")
            terminal.widths("trust", "Project configuration rules", "Exact command", "Continue to activation")
            terminal.send(ENTER, "Activate the reviewed project Allow rules?", "> Back")
            terminal.send(UP, "> Activate these Allow rules")
            terminal.send(ENTER, "Permission updated", "Project configuration Allow rules are active")
            assert not list((home / "sessions").glob("*.jsonl")), "trust alone created a Conversation"
            assert provider.snapshot() == ([], []), "trust alone contacted the provider"
            assert changes(home) == [{"kind": "trust", "fingerprint": list(hashlib.sha256(config.encode()).digest())}]
            terminal.close_permissions()
            terminal.prompt("trusted fixture", "TRUSTED_DONE", absent=("Approval required",))
            assert (project / "trusted-result").read_text() == "trusted"
            terminal.prompt("remember prefix fixture", "Approval required", "Allow and remember", "ls first")
            terminal.send(b"\x0f", "Command", "ls first", "c copy", "Esc back")
            start = len(terminal.capture)
            os.write(terminal.master, b"c")
            expected = b"]52;c;" + base64.b64encode(b"ls first")
            read_until(terminal.master, terminal.capture,
                       lambda: expected in bytes(terminal.capture[start:]),
                       description="command inspection copy")
            terminal.send(ESC, "Approval required", "1. Allow once", "3. Deny")
            terminal.send(b"2", "Remember permission", "Scope: ls", "> 1. This Session")
            terminal.widths("prefix", "Remember permission", "Scope: ls", "same cwd/environment", "This Project")
            terminal.send(b"2", "PREFIX_SAVED", absent=("Remember permission",))
            grant = changes(home)[-1]
            assert grant["kind"] == "grant", grant
            assert grant["grant"]["matcher"]["kind"] == "command_prefix", grant
            assert grant["grant"]["matcher"]["prefix"]["arguments"] == ["ls"], grant
            terminal.quit()

        with PermissionTerminal(project, environment, "restart") as terminal:
            terminal.wait("Message Plexmaton")
            terminal.prompt("reuse prefix fixture", "PREFIX_REUSED", absent=("Approval required",))
            requests, errors = provider.snapshot()
            assert not errors, errors
            tool_messages = [message for message in requests[-1]["messages"] if message["role"] == "tool"]
            assert len(tool_messages) == 1 and "second" in tool_messages[0]["content"], tool_messages
            terminal.permissions()
            terminal.wait("> Revoke Project: Command prefix: ls")
            terminal.send(ENTER, "Revoke this permission?", "> Back")
            terminal.send(UP, "> Revoke permission")
            terminal.send(ENTER, "Permission updated", absent=("Revoke Project:",))
            assert changes(home)[-1] == {"kind": "revoke", "id": grant["grant"]["id"]}
            terminal.close_permissions()
            terminal.prompt("revoked prefix fixture", "Approval required", "ls first", "> 3. Deny")
            terminal.send(b"3", "PREFIX_DENIED", absent=("Approval required",))
            terminal.quit()
        requests, errors = provider.snapshot()
        assert not errors and len(requests) == 8, (len(requests), errors)
        denied = [message for message in requests[-1]["messages"] if message["role"] == "tool"][-1]
        assert "denied" in denied["content"].lower(), denied
        assert len(list((home / "sessions").glob("*.jsonl"))) == 2


def main():
    subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "plexmaton-cli", "--bin", "plexmaton", "--quiet"],
                   cwd=ROOT, check=True)
    responses = []
    for command, marker in [("printf trusted > trusted-result", "TRUSTED_DONE"),
                            ("ls first", "PREFIX_SAVED"), ("ls second", "PREFIX_REUSED"),
                            ("ls first", "PREFIX_DENIED")]:
        responses.extend(command_turn(command, marker))
    with ScriptedProvider(responses) as provider:
        run_smoke(provider)
    print("permission smoke: trust before first Conversation, real command, Project prefix across restart, "
          "revoke, deny, three widths and shutdown passed; eight local fixture requests")


if __name__ == "__main__":
    main()
