#!/usr/bin/env python3
"""MDL-1/MDL-2/MDL-3/MDL-4: real menu, two loopback providers, history and credential isolation."""
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
ENTER, ESC = journey.ENTER, journey.ESC


def run_smoke(first, second):
    with tempfile.TemporaryDirectory(prefix="plexmaton-model-smoke-", dir="/tmp") as folder:
        home, project = Path(folder) / "home", Path(folder) / "project"
        home.mkdir()
        project.mkdir()
        (project / "AGENTS.md").write_text("Keep the workspace guidance marker.\n")
        status = 'test -z "${FIRST_LOGIN+x}${OTHER_LOGIN+x}" && jq -r \'"STATUS_CLEAN " + .model.id + " " + .effort.level\''
        config = f'''active_model = {{ provider = "first", model = "same" }}
[status_line]
command = {json.dumps(status)}
timeout_ms = 5000
max_rows = 1
'''
        for name, server, credential, effort in [("first", first, "FIRST_LOGIN", "low"), ("second", second, "OTHER_LOGIN", "high"), ("missing", second, "MISSING_LOGIN", "low")]:
            config += f'''[providers.{name}]
base_url = "{server.base_url}"
api_key_env = "{credential}"
api = "openai_chat_completions"
[providers.{name}.models.same]
id = "{name}-wire"
display_name = "{name.title()} Model"
reasoning_effort = "{effort}"
allowed_reasoning_efforts = ["low", "high"]
context_window_tokens = 32768
max_output_tokens = 4096
output_reserve_tokens = 4096
'''
        (home / "config.toml").write_text(config)
        environment = dict(fixture_environment(), PLEXMATON_HOME=str(home), FIRST_LOGIN="fixture-only", OTHER_LOGIN="fixture-only")
        with journey.Terminal(project, environment, "model") as terminal:
            terminal.wait("Message Plexmaton", "STATUS_CLEAN")
            terminal.prompt("/model", "Models", "first/same", "second/same")
            terminal.send(ESC, "/model", absent=("Enter confirm",))
            terminal.send(b"\x15", "Message Plexmaton", absent=("/model",))
            terminal.send(b"/model missing", "Models", "missing/same")
            terminal.send(ENTER, "credential is missing or invalid", "/model missing", "first-wire", "low")
            terminal.send(ESC, "/model missing", absent=("Enter confirm",))
            terminal.send(b"\x15", "Message Plexmaton", absent=("/model",))
            assert first.snapshot() == ([], []) and second.snapshot() == ([], [])
            assert not list((home / "sessions").glob("*.jsonl"))
            terminal.prompt("Remember this first message", "FIRST_DONE")
            terminal.send(b"/model second", "Models", "second/same")
            for width in [120, 88, 60]:
                terminal.resize(width + 1, "Models")
                terminal.resize(width, "Models", "second/same")
            terminal.send(ENTER, "second-wire", "high", absent=("Enter confirm",))
            terminal.wait("STATUS_CLEAN")
            terminal.prompt("Check the command environment", "Approval required", "Allow once")
            terminal.send(b"1", "SECOND_DONE", absent=("Approval required",))
            assert (project / "credential-result").read_text() == "unset/unset"
            terminal.prompt("/new", "first-wire", "low", absent=("SECOND_DONE",))
            terminal.quit()
        assert (home / "config.toml").read_text() == config
        before, errors = first.snapshot()
        after, other_errors = second.snapshot()
        assert not errors and not other_errors
        assert before[0]["model"] == "first-wire"
        assert all(body["model"] == "second-wire" and body["reasoning_effort"] == "high" for body in after)
        text = json.dumps(after[0]["messages"])
        for marker in ["Remember this first message", "FIRST_DONE", "Keep the workspace guidance marker."]:
            assert marker in text, marker


def main():
    subprocess.run(["cargo", "build", "--locked", "-p", "plexmaton-cli", "--bin", "plexmaton", "--quiet"], cwd=ROOT, check=True)
    command = 'printf "%s/%s" "${FIRST_LOGIN-unset}" "${OTHER_LOGIN-unset}" > credential-result'
    with ScriptedProvider([response({"role": "assistant", "content": "FIRST_DONE"}, "stop", "first")]) as first:
        with ScriptedProvider(command_turn(command, "SECOND_DONE")) as second:
            run_smoke(first, second)
    print("model smoke passed: two providers, preserved history, isolated credentials, default reset")


if __name__ == "__main__":
    main()
