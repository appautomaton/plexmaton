import http.client
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("provider_fixture", Path(__file__).resolve().parents[1] / "provider_fixture.py")
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class ProviderFixtureTests(unittest.TestCase):
    def request(self, provider, body, **headers):
        client = http.client.HTTPConnection("127.0.0.1", provider.server.server_port, timeout=3)
        try:
            client.request("POST", "/v1/chat/completions", body,
                           {"Authorization": "Bearer fixture-only", **headers})
            reply = client.getresponse()
            reply.read()
            return reply.status
        finally:
            client.close()

    def test_request_byte_bound_refuses_before_reading_body_and_closes_worker(self):
        with self.assertRaisesRegex(AssertionError, "request byte bound"):
            with fixture.ScriptedProvider([b"unused"]) as provider:
                self.assertEqual(self.request(provider, b"", **{"Content-Length": str(fixture.MAX_REQUEST_BYTES + 1)}), 400)
                self.assertEqual(provider.snapshot()[0], [])
        self.assertFalse(provider.worker.is_alive())
        self.assertEqual(provider.server.socket.fileno(), -1)

    def test_extra_requests_fail_instead_of_replaying_a_response(self):
        with self.assertRaisesRegex(AssertionError, "script exhausted"):
            with fixture.ScriptedProvider([b"fixture"]) as provider:
                self.assertEqual(self.request(provider, b'{"stream":true}'), 200)
                self.assertEqual(self.request(provider, b'{"stream":true}'), 400)
                self.assertEqual(len(provider.snapshot()[0]), 1)
        self.assertFalse(provider.worker.is_alive())

    def test_assertion_failure_still_stops_the_server(self):
        with self.assertRaisesRegex(ValueError, "journey failed"):
            with fixture.ScriptedProvider([b"unused"]) as provider:
                raise ValueError("journey failed")
        self.assertFalse(provider.worker.is_alive())
        self.assertEqual(provider.server.socket.fileno(), -1)

    def test_failed_journey_releases_a_paused_stream_and_joins_its_handler(self):
        paused = fixture.PausedResponse(fixture.response(
            {"role": "assistant", "content": "stream prefix"}, "stop", "paused"))
        client = None
        try:
            with self.assertRaisesRegex(ValueError, "journey failed after prefix"):
                with fixture.ScriptedProvider([paused]) as provider:
                    client = http.client.HTTPConnection("127.0.0.1", provider.server.server_port, timeout=3)
                    client.request("POST", "/v1/chat/completions", b'{"stream":true}',
                                   {"Authorization": "Bearer fixture-only"})
                    reply = client.getresponse()
                    self.assertEqual(reply.read(len(paused.prefix)), paused.prefix)
                    self.assertFalse(paused.release.is_set())
                    raise ValueError("journey failed after prefix")
            self.assertTrue(paused.release.is_set())
            self.assertFalse(provider.worker.is_alive())
            self.assertEqual(provider.server.socket.fileno(), -1)
        finally:
            if client is not None:
                client.close()

    def reply(self, provider, body):
        """The bytes a request was answered with, which is the only thing routing can get wrong."""
        client = http.client.HTTPConnection("127.0.0.1", provider.server.server_port, timeout=3)
        try:
            client.request("POST", "/v1/chat/completions", body, {"Authorization": "Bearer fixture-only"})
            reply = client.getresponse()
            self.assertEqual(reply.status, 200)
            return reply.read()
        finally:
            client.close()

    def test_a_reply_answers_the_request_that_asked_for_it_whoever_arrives_first(self):
        """Two runners share one endpoint, so order cannot choose; the last message must.

        The replies are scripted in the opposite order to the requests, and the bodies are
        compared: a status code alone passes whatever the routing does, including first-in-first-out.
        """
        with fixture.AddressedProvider([("ROOT_ASK", b"to-root"), ("CHILD_TASK", b"to-child")]) as provider:
            child = '{"stream":true,"messages":[{"role":"user","content":"CHILD_TASK"}]}'
            root = '{"stream":true,"messages":[{"role":"user","content":"ROOT_ASK"}]}'
            self.assertEqual(self.reply(provider, child.encode()), b"to-child")
            self.assertEqual(self.reply(provider, root.encode()), b"to-root")

    def test_a_cue_only_earlier_in_the_conversation_does_not_claim_the_reply(self):
        """A root turn repeats the task it delegated; matching anywhere would steal the child's."""
        with self.assertRaisesRegex(AssertionError, "no scripted reply is addressed to"):
            with fixture.AddressedProvider([("CHILD_TASK", b"to-child")]) as provider:
                body = ('{"stream":true,"messages":['
                        '{"role":"user","content":"do CHILD_TASK"},'
                        '{"role":"tool","content":"unrelated"}]}')
                self.assertEqual(self.request(provider, body.encode()), 400)
        self.assertFalse(provider.worker.is_alive())

    def test_two_replies_may_not_answer_the_same_cue(self):
        with self.assertRaisesRegex(AssertionError, "two replies answer the same cue"):
            fixture.AddressedProvider([("SAME", b"first"), ("SAME", b"second")])


if __name__ == "__main__":
    unittest.main()
