# Spec — Conversation model selection

| Field | Value |
| --- | --- |
| Status | Implemented; local runtime, menu and executable witnesses passed |
| Owns | Exact configured model selection, idle replacement, and conversation-local model overrides |
| Depends on | CMC-1/CMC-2, SKP-3/SKP-4, PRV-6, CMD-2, EFF-1/EFF-5, STL-3 |
| Proven by | Runtime and TUI tests below, plus the offline model PTY journey |

## Invariants

**MDL-1 — Replacement has one idle boundary, and nothing in the past closes it.** Model and effort
changes share runtime admission: wrong-agent, busy/queued work, approval, compaction, persistence
failure and shutdown refuse a replacement. A fully constructed driver replaces the current one
atomically, without starting a request or altering canonical history; failure leaves the old driver
intact. A model change additionally requires that the selected journal projection encode under the
destination before acceptance, so a history that cannot be prepared at all is still refused; effort
runs no such check, the projection it would test being the one already in use. That pre-flight
encodes what the next request will encode, delegated turns resolved from their canonical references
(CIN-2) exactly as the request path resolves them. Each way it can fail says which: the conversation
is too long for the destination, its delegated context could not be read, or its history cannot be
encoded. Rejected: one sentence for all three, which named the model as the problem in two cases
where no choice of model was. Provider replay the
destination cannot use is not such a case: PRV-3 carries the reply's content across instead, the
sidecars stay in the record, and selecting the original model again replays them exactly. The
conversation is told once, afterwards, when a switch cost it that. Rejected: refusing the switch,
which read replay compatibility as a property of the conversation rather than of one encoder, and so
welded every conversation that had ever reasoned to the model that produced it — its own siblings
included, since compatibility carries the exact wire id.

**MDL-2 — A model is an exact configured pair.** The CLI supplies bounded menu summaries from its
immutable provider/model registry; a selected row carries both identities. Only acceptance updates
the confirmed model, effort capabilities, context budget and status. Filtering, hover and Escape
never apply a choice; unknown queries never become a model request.

**MDL-3 — Switching cannot expose credentials or reuse the wrong request environment.** Every
switchable provider credential name is excluded from captured command environments before scopes
are compiled. Replacement refuses an unprotected credential, retains the workspace instruction
snapshot and native tool owners, and computes the destination model's request environment.

**MDL-4 — Overrides belong to the open conversation.** A selected model uses its configured effort
default and persists while that conversation remains open. New, resume and restart use the
configured default. Model selection writes neither user/project configuration nor historical
request metadata; EFF-5 owns the corresponding effort lifetime. Opening saved history does not
assert that the default model can encode it, and a conversation another model wrote reports a real
context budget rather than an unavailable one; STL-3 still keeps historical status available for the
encoding failures that remain.

## Grammar

`/model` opens the existing composer menu. `/model <query>` filters configured model names,
display names, wire IDs and provider names (case-insensitive substring matching). Arrows or actual
pointer movement choose; Enter confirms and
Escape dismisses while retaining the draft; Tab never confirms a model row. A matching press/release
also confirms the row by identity (INV-11). A failed selection keeps the menu and prior model;
choosing another row clears the old refusal. Refusal text uses the theme's Failure style, including
wrapped lines; catalog-limit and empty-list explanations retain the Muted style. An accepted
selection that cost the conversation its replay closes the menu like any other and adds one Muted
notice naming what the new model now reads as text. Rejected: asking first, which taxes every
switch to warn about the reversible minority of them.
No configuration discovery or network request is made by opening the menu. The catalog retains at
most 256 complete entries / 64 KiB of metadata and identifies a limited list explicitly. Empty and
no-match lists remain open; no query becomes a provider prompt. Rows display provider/configured
name, display name and wire ID, with the exact accepted pair marked current.

## Evidence

[Named proofs](../evidence/model-selection.md), one row an invariant.
