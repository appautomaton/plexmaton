"""Bounded loopback Chat Completions fixtures for the executable journeys.

One owned server, two ways to pick the reply. A single-conversation journey scripts its turns in
order. A journey with a delegated child cannot: the root and the child are independent runners
against one endpoint, and their requests interleave in an order no script can predict, so that
journey addresses each reply to the request that earns it.
"""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading

MAX_REQUEST_BYTES = 1024 * 1024
MAX_RESPONSES = 16


class PausedResponse:
    """An SSE prefix followed by an explicitly released, bounded remainder."""

    def __init__(self, data):
        boundary = data.index(b"\n\n") + 2
        self.prefix, self.remainder = data[:boundary], data[boundary:]
        self.release = threading.Event()

    def __len__(self):
        return len(self.prefix) + len(self.remainder)

    def write(self, stream):
        stream.write(self.prefix)
        stream.flush()
        assert self.release.wait(timeout=30), "paused fixture was not released"
        stream.write(self.remainder)


def response(delta, finish, identity):
    def event(choices, **extra):
        return {"id": identity, "object": "chat.completion.chunk", "choices": choices, **extra}
    events = [event([{"index": 0, "delta": delta, "finish_reason": None}]),
              event([{"index": 0, "delta": {}, "finish_reason": finish}]),
              event([], usage={"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17})]
    return ("".join("data: " + json.dumps(value) + "\n\n" for value in events)
            + "data: [DONE]\n\n").encode()


def calls(identity, *invocations, text=None):
    """One assistant turn requesting tools, each call identified so its result is recognisable."""
    requested = [{"index": index, "id": "call_" + name.upper() + "_" + identity, "type": "function",
                  "function": {"name": name, "arguments": json.dumps(arguments)}}
                 for index, (name, arguments) in enumerate(invocations)]
    delta = {"role": "assistant", "tool_calls": requested}
    if text is not None:
        delta["content"] = text
    return response(delta, "tool_calls", identity + "_call")


def says(text, identity=None):
    """One assistant turn of plain prose, which ends the turn."""
    return response({"role": "assistant", "content": text}, "stop", (identity or text) + "_final")


def command_turn(command, marker):
    return [calls(marker, ("exec_command", {"cmd": command, "timeout_ms": None})), says(marker)]


def asked(body):
    """What this request last asked, as the text a cue is matched against."""
    messages = body.get("messages") or []
    return json.dumps(messages[-1], ensure_ascii=False) if messages else ""


class LoopbackProvider:
    """One owned server thread; fixed replies, bounded requests, no outbound connection."""

    def __init__(self, responses):
        assert 0 < len(responses) <= MAX_RESPONSES
        assert all(len(self.payload(entry)) <= MAX_REQUEST_BYTES for entry in responses)
        self.responses = tuple(responses)
        self.requests = []
        self.errors = []
        self.lock = threading.Lock()

    @staticmethod
    def payload(entry):
        return entry

    def choose(self, body):
        """Pick this request's reply. Called with the lock held, before the request is recorded."""
        raise NotImplementedError

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
                        data = owner.choose(body)
                        owner.requests.append(body)
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers()
                    if isinstance(data, PausedResponse):
                        data.write(self.wfile)
                    else:
                        self.wfile.write(data)
                except (AssertionError, ValueError, OSError) as error:
                    with owner.lock:
                        if len(owner.errors) < MAX_RESPONSES:
                            owner.errors.append(str(error))
                    self.send_error(400, "fixture refused request")

        # Delegated conversations own independent provider requests. A single handler would make
        # whichever stream pauses first serialize the other agent and turn scheduling into fixture
        # behavior rather than product behavior.
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
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
            # A failed terminal assertion must not strand a handler at the stream barrier.
            for entry in self.responses:
                data = self.payload(entry)
                if isinstance(data, PausedResponse):
                    data.release.set()
            self.server.shutdown()
            self.worker.join(timeout=3)
            assert not self.worker.is_alive(), "fixture worker did not stop"
        finally:
            self.server.server_close()
        if error_type is None:
            requests, errors = self.snapshot()
            assert not errors, errors
            assert len(requests) == len(self.responses), "fixture responses were not consumed"


class ScriptedProvider(LoopbackProvider):
    """Replies in the order requests arrive: one conversation, one script."""

    def choose(self, _body):
        assert len(self.requests) < len(self.responses), "script exhausted"
        return self.responses[len(self.requests)]


class AddressedProvider(LoopbackProvider):
    """Replies to what a request last asked, so independent runners may interleave freely.

    Each entry is `(cue, reply)`, and a cue is matched against the request's final message only.
    Every cue is text the journey itself minted — its own prose, or a tool-call identity this
    module issued — so no reply depends on wording the runtime chose.

    Rejected: matching a cue anywhere in the request. Each turn carries the whole conversation, so
    a root request repeating the task it delegated would claim the child's reply.
    """

    def __init__(self, responses):
        cues = [cue for cue, _ in responses]
        assert len(set(cues)) == len(cues), "two replies answer the same cue"
        self.answered = set()
        super().__init__(responses)

    @staticmethod
    def payload(entry):
        return entry[1]

    def choose(self, body):
        question = asked(body)
        for index, (cue, data) in enumerate(self.responses):
            if index not in self.answered and cue in question:
                self.answered.add(index)
                return data
        raise AssertionError(f"no scripted reply is addressed to {question[:200]}")
