# Plan — Phase 04 stage 28, a model switch degrades

| Field | Value |
| --- | --- |
| Phase | [Phase 04](../phases/phase-04-product-polish.md) stage 28 |
| Contract | [MDL-1/MDL-2/MDL-4](../specs/model-selection.md), [PRV-3/PRV-4](../specs/provider-adapter.md), [COM-3](../specs/composer.md), [UI/UX](../ui-ux.md) |
| Status | Active; all five slices implemented and locally verified; rendered frame and the user's terminal test pending |

## Outcome

A conversation stops being welded to the model that produced it. Replay compatibility is four
fields and its model family is the exact wire id, compared whole, so today *any* model change makes
*every* sidecar foreign, all four encoders refuse, and the switch pre-flight refuses with "This
conversation contains history the selected model cannot replay." PRV-4 already says the semantic
record is authority and replay is an optimisation; refusing a switch because an optimisation did not
carry over inverts that.

The rule this stage installs: **a wire encoder sends what the destination dialect can carry, omits
what it cannot, and preserves the semantic content in whatever form the dialect does accept.**

Applied to a foreign sidecar: discard it and encode from semantics.

| Block | Degraded |
| --- | --- |
| Text | visible assistant text |
| Reasoning, and the block completed | visible assistant text |
| Reasoning, interrupted or empty | dropped |
| Tool call | the call, no dialect-private identity, wire-shaped id |
| Replay-only | dropped — it was only ever its sidecar |

Completeness, not signature presence, is what separates the two reasoning rows, and the distinction
is per block rather than per output: an interrupted half-thought is never put in the model's mouth
as speech, while a finished thought whose replay form the destination cannot use is carried as text.
A block that completed is one that has a replay attachment. That keeps this rule and PRV-3's
existing interrupted-summary rule readable as one rule instead of two.

Rejected: dropping foreign reasoning outright, which is cheaper and deletes the Gemini unsigned
-thought hazard, but throws away a finished chain of reasoning the next model could have used. The
user chose demotion with that cost stated.

Rejected: arming the switch first. Nothing is destroyed — MDL-1 keeps the sidecars, so switching
back replays natively — and the contract reserves the last row for the irreversible.

## Slices

1. **Wire-shaped tool call ids.** Ids reach the Messages, Chat and Responses wires verbatim on both
   the call and the result side; Gemini never sends `call_id` and pairs positionally. Cross-dialect
   that is a live provider rejection, so without this the stage trades an honest refusal for a
   request that fails after the switch. One pure `wire_call_id(api, &ToolCallId)`, evaluated
   independently at both sites — any state or any predicate that could differ between them
   desynchronises the pair and orphans a result. Unacceptable ids become `call_` plus 32 hex of a
   domain-separated SHA-256, following `environment.rs`'s existing fingerprint pattern; 37 characters
   fits every known dialect bound. Originals are unique within an output, so an injective hash stays
   unique.

2. **One seam, stated once.** `degraded(output, model) -> Option<Vec<Carried>>` in the codec, `None`
   when compatible, dropped blocks simply absent. A pre-pass rather than a helper inside each
   dialect's loop, so that an output degrading to nothing is one observable fact at one place, and
   the degraded arm never has the semantic block in scope to reach for. Called from `encode_atom`,
   where the tool-result side of the wire lives, so one decision serves both sides. Each dialect
   gets a standalone `degraded_assistant` and a three-line early return, leaving the exact-replay
   path textually unchanged.

3. **The four dialects.** Responses is smallest: every block already has a no-replay path except
   replay-only. Messages needs degraded reasoning as a text block and replay-only dropped rather than
   reaching the missing-signature refusal. Chat routes degraded reasoning into content rather than
   the reasoning field, so the phase machine is never entered. Gemini needs a tool-call arm that does
   not exist today. Two dialects can now emit an invalid request where they never could before:
   Gemini ships `parts: []` and Chat ships `content: null` when an output degrades to nothing, which
   is the ordinary reasoning-only step, not a corner case. Both return an optional message and their
   callers collect. Degraded reasoning is emitted as plain text with no thought flag — the
   same-model arm's flagged, unsigned shape is exactly what Gemini rejects when the thought came
   from elsewhere. Adjacent carried text is not merged: every dialect accepts adjacent text, and
   concatenation would run reasoning into the answer with no separator.

4. **What the user is told.** The switch succeeds and closes the menu as it does now. When the
   projection held foreign replay, one notice through COM-3's route says so; free when nothing
   degraded. `IncompatibleHistory` keeps only the cases its name never fitted — the conversation does
   not *fit* the destination — and its sentence is rewritten to say that. `EncodeError::IncompatibleReplay`
   loses its last producer and is removed with its status-line mapping.

5. **The documents this makes false.** MDL-1's "incompatible provider replay is refused without
   stripping its sidecars" is reversed, and the same sentence's claim that model and effort share the
   encode pre-flight is false today — effort admission never runs it. PRV-3's "wire encoders omit it
   without fabricating replay" does not contemplate demotion. MDL-4 leans on STL-3 for a reopened
   conversation whose context is unavailable, which is the state this stage ends. The UI/UX contract
   reserves Notices "for future multi-agent workflows, not routine feedback", already false since a
   refused dispatch began opening one. Evidence tables follow; the contract sentence needs the user's
   agreement and a frame they have seen.

## Order and why

Ids first: the pairing rule constrains what a degraded call may look like, and it is testable before
any encoder moves. The seam before the dialects, so no dialect invents its own answer to what
survives. Gemini last, the only one needing both a new arm and an empty-message guard. The notice
reports what the encoders did; the documents describe behaviour that by then exists.

## Evidence this stage owes

Two tests assert the refusal and are the contract, so they invert rather than disappear. The
compatibility-axes fixture is a single empty reasoning block and would degrade to an empty input,
asserting nothing; it needs text, reasoning, a call and a replay-only block to stay evidence. The
resume witness asserts an unavailable status line and a refused switch across four assertions — it
states the defect as a contract and inverts whole. The interrupted-continuation test must keep
passing untouched: it is what proves demotion did not swallow the fragment case.

New evidence: a switch between wire ids under one provider and codec; a switch across codecs with
tool calls, where call and result carry the same wire-shaped id; a Gemini degraded call with neither
id nor signature; an output of only replay-only blocks degrading to no message in every dialect; and
the notice appearing exactly when the projection held foreign replay.

Replay occupancy no longer counts a degraded atom's sidecar bytes: they never ship, and charging for
them would bring compaction forward for bytes nobody sends.

## Deliberately not in this plan

Relaxing replay compatibility itself: the four fields and whole-struct comparison stay, because
degradation makes the comparison's strictness harmless. Compaction-summary provenance, which
compares the same value for a different purpose and is confirmed unaffected before landing rather
than changed. The effort selector's refusal styling. Live-provider acceptance of any degraded shape,
which no local evidence can establish.
