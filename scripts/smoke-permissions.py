#!/usr/bin/env python3
"""PER-6/PER-8/PER-10: trust, prefix reuse and revoke through the real executable."""

import base64
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pty
import subprocess
import tempfile
import termios

from permission_fixture import ScriptedProvider, command_turn
from smoke_support import fixture_environment

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("terminal_smoke", ROOT / "scripts/smoke-tui.py")
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)
UP, DOWN, ENTER, ESC = b"\x1b[A", b"\x1b[B", b"\r", b"\x1b"


class Terminal:
    def __init__(self, project, environment, name, arguments=()):
        self.project, self.environment, self.name = project, environment, name
        self.arguments = tuple(arguments)
        self.size = (30, 120)
        self.capture = bytearray()
        self.frame_start = 0

    def __enter__(self):
        self.master, slave = pty.openpty()
        try:
            smoke.set_size(slave, self.size)
            self.process = subprocess.Popen(
                [str(ROOT / "target/debug/plexmaton"), *self.arguments], cwd=self.project, env=self.environment,
                stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
                preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0))
        except BaseException:
            os.close(self.master)
            raise
        finally:
            os.close(slave)
        return self

    def __exit__(self, *_error):
        try:
            output = ROOT / "target/smoke"
            output.mkdir(parents=True, exist_ok=True)
            (output / f"permissions-{self.name}.raw").write_bytes(self.capture)
            (output / f"permissions-{self.name}.txt").write_text(
                smoke.rendered_screen(bytes(self.capture[self.frame_start:]), self.size))
        finally:
            try:
                if self.process.poll() is None:
                    self.process.kill()
            finally:
                os.close(self.master)
            self.process.wait(timeout=10)

    def wait(self, *markers, absent=()):
        return smoke.await_screen(self.master, self.capture, self.size, markers, absent, self.frame_start)

    def send(self, keys, *markers, absent=()):
        os.write(self.master, keys)
        return self.wait(*markers, absent=absent)

    def resize(self, width, *markers):
        self.frame_start = len(self.capture)
        self.size = (30, width)
        smoke.set_size(self.master, self.size)
        return self.wait(*markers)

    def widths(self, name, *markers):
        for width, label in [(121, None), (120, "wide"), (95, "medium"), (60, "narrow")]:
            screen = self.resize(width, *markers)
            if label:
                # PRE-1: resize may first publish placeholders. Review the settled frame,
                # and fail if the owned preparation never supplies it.
                screen = self.wait(*markers, absent=("Preparing text",))
                output = ROOT / "target/smoke"
                output.mkdir(parents=True, exist_ok=True)
                (output / f"permissions-{name}-{label}.txt").write_text(
                    "\n".join(row.rstrip() for row in screen.splitlines()) + "\n")
        self.resize(120, *markers)

    def permissions(self):
        self.send(b"\x10perm", "Workspace", "> Permissions", "Esc close")
        return self.send(ENTER, "Permissions", "Project grants")

    def close_permissions(self):
        # One layer per Escape (DRW-3): the page returns to the list, the list to the origin.
        self.send(ESC, "> Permissions", "Esc close")
        self.send(ESC, "Message Plexmaton", absent=("Type to filter",))

    def prompt(self, message, *markers, absent=()):
        at = (5, self.size[0] - 3)
        smoke.click(self.master, at, self.capture, cursor=True)
        os.write(self.master, smoke.sgr_press(*at)[:-1] + b"m")
        return self.send(message.encode() + ENTER, *markers, absent=absent)

    def quit(self):
        self.send(b"\x04", "press Ctrl-D again to quit")
        os.write(self.master, b"\x04")
        smoke.read_until(self.master, self.capture, lambda: smoke.ALTERNATE_SCREEN_EXIT in self.capture,
                         description="permission terminal release")
        # Restoration and the durable-conversation handoff may still be writing. Keep draining
        # the PTY until EOF before joining; waiting first can hold a terminal drain on macOS.
        smoke.read_to_eof(self.master, self.capture)
        assert self.process.wait(timeout=3) == 0


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
        with Terminal(project, environment, "first") as terminal:
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
            smoke.read_until(terminal.master, terminal.capture,
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

        with Terminal(project, environment, "restart") as terminal:
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
