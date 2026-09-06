# Codec prefix probe

| Field | Value |
| --- | --- |
| Status | Offline codec feasibility evidence; no production mechanism |
| Read when | Designing a compaction request or claiming cache-prefix behavior |
| Question | Can a stable summary instruction be appended after existing semantic context without rewriting the current wire input? |
| Contract | JRN-5, PRV-1, PRV-3, PRV-4, BUD-2 |

## Method

`codec-prefix.rs` builds synthetic histories through the public `SessionJournal` projection, then
passes the resulting `ModelRequest` to the public `plexmaton_provider::encode_request`. It does not
copy an encoder, call a provider, write a journal, or use a transport fixture. Each history is
encoded once, then encoded again after appending one `ContextAtom::User` containing the fixed
summary instruction. It also constructs the provider's actual `request_environment` for the fixed
resolved model, `read_file` tool definition and output cap. The prefix assertion compares codec conversation
array elements, rather than serialized JSON text.

| Dialect | Compared input array | Stable environment evidence |
| --- | --- | --- |
| Responses | `input` | whole request body except `input`, including instructions, `read_file`, output cap and `prompt_cache_key` |
| Chat Completions | `messages` | whole request body except `messages`, including `read_file`, output cap and `prompt_cache_key`; system instructions are a leading `messages` element |
| Messages | `messages` | whole request body except `messages`, including system, `read_file`, output cap, thinking and cache-control |
| GenerateContent | `contents` | whole request body except `contents`, including system instruction, `read_file` declarations and generation configuration |

The stable fixture has the same session identity, resolved model, instructions, one real
`read_file` `FunctionTool` schema, and output cap for both encodes. A complete tool batch calls
that same `read_file` tool. The probe separately removes the tool and changes its required schema,
and verifies both the actual `request_environment` fingerprint and emitted non-conversation body
change.

## Command and result

Run from this worktree:

```sh
./.agents/spikes/compaction/run-codec-spike.sh
```

The runner executed `cargo build --offline -p plexmaton-provider --target-dir
target/compaction-spike`, compiled the standalone test against those exact first-party rlibs, and
ran it. The runner enables Bash `nullglob` and accepts exactly one hashed rlib for each direct
crate from that target directory; zero or multiple candidates fail rather than selecting a literal
glob or stale rlib. It then also compiles and runs the separate `budget-boundary-model.rs` owned by
the companion budget probe. On 2026-09-05 the result was:

```text
running 5 tests
test ordinary_final_assistant_extension_keeps_the_wire_input_prefix ... ok
test complete_tool_batch_extension_keeps_the_wire_input_prefix ... ok
test changed_environment_and_flattened_transcript_are_not_prefix_preserving ... ok
test compatible_replay_extension_keeps_the_wire_input_prefix ... ok
test user_and_tool_result_tails_do_not_coalesce_with_the_appended_instruction ... ok

test result: ok. 5 passed; 0 failed

running 4 tests
test result: ok. 4 passed; 0 failed
```

## Observations

1. For all four current codecs, appending the fixed user summary after a journal-projected
   ordinary final assistant answer retains the whole previous `input`/`messages`/`contents` array
   as an exact element prefix. The test requires the encoded array length to grow by exactly one
   and its new final element to contain the exact summary text at that dialect's native text path.
   Thus an encoder that silently drops the summary cannot pass.
2. The same holds for a complete projected tool batch. It covers one assistant call and its
   terminal result as JRN-5's indivisible source atom; no encoder moves the prior call/result
   elements when the summary is appended. Gemini requires its retained function-call replay
   metadata for this native request to be representable, and the probe supplies an exact compatible
   minimal sidecar. Every case holds a stable real `read_file` tool definition in the request
   environment.
3. The same holds for retained provider replay. The probe uses a dialect-valid compatible replay
   item for Responses encrypted reasoning, Chat's recognized reasoning-field marker, Messages
   signed thinking, and Gemini signed thought. Before comparing prefixes, the test asserts the
   associated encrypted/reasoning/signature field is present in the baseline wire input. This is an
   encoder result: it proves the existing sidecar remains in the old element prefix, not that a
   provider will accept every synthetic opaque value.
4. Appending a user instruction after an existing user tail, and after a tool-result tail, stays a
   distinct array element in all four present encoders. In particular, Messages and Gemini do not
   currently coalesce adjacent user-role elements; the old element prefix therefore remains intact.
   A future encoder normalizer that folds such elements would invalidate this evidence and must be
   tested explicitly.
5. A changed system/instructions slot is a concrete counterexample. It changes Responses,
   Messages, and Gemini environment fields; Chat represents it as the first `messages` element,
   which breaks the old array prefix. Replacing history with one flattened user transcript also
   breaks the old array prefix for every dialect, even though OpenAI's session-derived cache key
   can remain unchanged.
6. Removing `read_file`, or changing its required schema, changes the actual provider
   `request_environment` fingerprint and the emitted request object after only its conversation
   array is removed. Matching tool definitions are therefore necessary for this result; cache-key
   stability alone does not make the requests equivalent.

## Meaning and limits

The result supports a future summarization-request shape that extends one selected semantic request
with a stable final instruction while keeping the current resolved environment unchanged. It does
not establish a checkpoint payload, checkpoint persistence, covered-range shadowing, automatic
compaction orchestration, a semantic cache epoch, dispatch policy, token sufficiency, or a durable
measurement boundary.

`prompt_cache_key` is only a harness cache-affinity hint for the OpenAI codecs and is stable here
because the fixture session ID is stable. Messages uses its native `cache_control` hint and Gemini
uses implicit caching. No currently encoded request contains a semantic cache-epoch field. Exact
input-array prefix, a whole serialized HTTP JSON byte prefix, and a realized provider cache hit are
different claims: this probe establishes only the first. It serializes no HTTP body for comparison
and makes no live request, so provider cache retention, cache-hit accounting, billing and vendor
prefix rules remain unverified.
