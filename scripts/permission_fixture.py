"""Bounded loopback Chat Completions fixture for the permission executable journey."""

from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import threading

MAX_REQUEST_BYTES = 1024 * 1024
MAX_RESPONSES = 12


def response(delta, finish, identity):
    def event(choices, **extra):
        return {"id": identity, "object": "chat.completion.chunk", "choices": choices, **extra}
    events = [event([{"index": 0, "delta": delta, "finish_reason": None}]),
              event([{"index": 0, "delta": {}, "finish_reason": finish}]),
              event([], usage={"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17})]
    return ("".join("data: " + json.dumps(value) + "\n\n" for value in events)
            + "data: [DONE]\n\n").encode()


def command_turn(command, marker):
    call = {"index": 0, "id": "call_" + marker, "type": "function",
            "function": {"name": "exec_command", "arguments": json.dumps({"cmd": command, "timeout_ms": None})}}
    return [response({"role": "assistant", "tool_calls": [call]}, "tool_calls", marker + "_call"),
            response({"role": "assistant", "content": marker}, "stop", marker + "_final")]


class ScriptedProvider:
    """One owned server thread; fixed responses, bounded requests, no outbound connection."""

    def __init__(self, responses):
        assert 0 < len(responses) <= MAX_RESPONSES
        assert all(len(value) <= MAX_REQUEST_BYTES for value in responses)
        self.responses = tuple(responses)
        self.requests = []
        self.errors = []
        self.lock = threading.Lock()

    def __enter__(self):
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def setup(self):
                self.request.settimeout(2)
                super().setup()

            def log_message(self, *_args):
                pass

            def do_POST(self):
                try:
                    assert self.path == "/v1/chat/completions", "unexpected endpoint"
                    assert self.headers.get("Authorization") == "Bearer fixture-only", "unexpected credential"
                    assert not self.headers.get("Transfer-Encoding"), "chunked request refused"
                    count = int(self.headers.get("Content-Length", "0"))
                    assert 0 < count <= MAX_REQUEST_BYTES, "request byte bound"
                    body = json.loads(self.rfile.read(count))
                    assert isinstance(body, dict) and body.get("stream") is True, "expected streaming request"
                    with owner.lock:
                        assert len(owner.requests) < len(owner.responses), "script exhausted"
                        data = owner.responses[len(owner.requests)]
                        owner.requests.append(body)
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                except (AssertionError, ValueError, OSError) as error:
                    with owner.lock:
                        if len(owner.errors) < MAX_RESPONSES:
                            owner.errors.append(str(error))
                    self.send_error(400, "fixture refused request")

        self.server = HTTPServer(("127.0.0.1", 0), Handler)
        self.base_url = f"http://127.0.0.1:{self.server.server_port}/v1"
        self.worker = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.05})
        try:
            self.worker.start()
        except BaseException:
            self.server.server_close()
            raise
        return self

    def snapshot(self):
        with self.lock:
            return list(self.requests), list(self.errors)

    def __exit__(self, error_type, _error, _traceback):
        try:
            self.server.shutdown()
            self.worker.join(timeout=3)
            assert not self.worker.is_alive(), "fixture worker did not stop"
        finally:
            self.server.server_close()
        if error_type is None:
            requests, errors = self.snapshot()
            assert not errors, errors
            assert len(requests) == len(self.responses), "fixture responses were not consumed"
