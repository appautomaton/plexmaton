# Research — Native tool surface

| Field | Value |
| --- | --- |
| Status | Comparison complete; local OpenAI-compatible provider and `gpt-5.6-luna` selected, model trial is slice 8 |
| Unlocks | The dedicated file-mutation scope and model-facing codec for Phase 01 stage 2 tools |
| Corpus | Local Grok Build, Codex, DeepSeek Harness, Kimi, Pi and Hermes; official Claude, Gemini, OpenAI, Aider, Cline and Hashline contracts |

## Result constraints

The selected surface must satisfy these boundaries; the comparison chooses a codec, not whether to
weaken them.

- Model arguments are untrusted syntax. A codec resolves them against one observed file version
  into a bounded canonical mutation; filesystem authority remains APV-1's admission boundary.
- Read returns exact UTF-8 text in a line window with hard line, line-byte and total-byte limits,
  stable continuation metadata, and an opaque observation owned by session state. Binary data is a
  typed refusal until a binary tool exists.
- Existing-file edit and overwrite require the observed version; create requires absence. The
  executor rechecks the canonical target after approval and immediately before commit.
- One file's edits are all validated against the same original bytes, cannot overlap, preserve BOM,
  line endings, mode and every untouched byte, and become visible with one same-directory commit.
  A backend that cannot promise that states its weaker guarantee rather than calling it atomic.
- Exact mismatch, ambiguity, staleness, path escape, cancellation and partial application remain
  distinct typed outcomes. Fuzzy context may be returned as a suggestion but is never silently
  applied.
- Search shares the read capability, uses an argv-based ripgrep adapter or its maintained crates,
  and bounds scanned work, matches, individual records and total output. A command is a separate,
  broad capability with an owned process lifecycle, not the ordinary read or edit path.

## Candidates

| Codec | Standing | What the trial must answer |
| --- | --- | --- |
| Exact replacements | Baseline: JSON edits, unique and non-overlapping, all against one observed version | Whether the chosen model can reproduce enough exact context without retry loops |
| Strict patch | Candidate for a patch-trained model; add, update and delete compile to the same mutation representation | Whether its token and success advantage justifies a parser and honest multi-file partial outcomes |
| Hash- or range-anchored edit | Candidate: references text the model just read and rejects a stale observation | Whether lower output and retry cost outweigh a custom grammar and anchors in every read |
| Whole-file write | Create-only baseline, not the default edit path | Whether any model needs it as a bounded fallback; it never follows a failed edit automatically |

Silent fuzzy replacement is rejected: Codex, Pi, Gemini and Hermes each improve match rate by
normalizing or guessing, but can select or rewrite bytes the model did not name. Shell heredocs are
also rejected as the normal edit path because they bypass the typed precondition and diff preview.

## Trial

After the Phase 01 provider and model are selected, run each viable codec through the same small
driver and record first-attempt apply rate, wrong-region writes, retries, output tokens and parser
failures. The corpus includes:

- unique, repeated and nearby targets; two disjoint edits; insertion, deletion and create;
- LF, CRLF, mixed endings, BOM, tabs, Unicode punctuation and literal source escapes;
- a long line, a large file, bounded partial reads and a target outside the requested window;
- external mutation after read, two simultaneous edits, symlink escape and create collision;
- cancellation before commit, write failure and an oversized tool result.

A wrong target, unobserved overwrite, stale write or changed byte outside the admitted spans
disqualifies a codec regardless of its average score. If codecs tie on correctness, fewer retries
then fewer model-visible bytes decide. The winning codec is promoted into a native-tools spec with
the canonical mutation contract; this research file is then deleted.

## Comparison retained

| Source | Keep | Do not inherit |
| --- | --- | --- |
| Grok Build | Versioned tool profiles over one registry; direct search/replace, patch and hashline implementations; bounded line-anchored reads; responsive per-agent permission presentation | TUI-owned ACP response senders and permission queues as the live pending owner; treating in-tree ports as independent Grok designs |
| Codex | Patch grammar as the patch-trained candidate; bounded command head/tail; typed partial delta internally | Shell-only read/search, fuzzy first-match, overwrite-on-add, verify/write race |
| DeepSeek Harness | Observed-version integrity, create-if-absent, staged same-directory publish, typed rg adapter | Read/search outside capability scope, exact-total full scans, claims stronger than its replace race |
| Kimi | Schema validation before resolve, canonical access/display facts, layered output bounds | Lexical-only path security, prompt-only read-before-edit, truncate-in-place writes |
| Pi | Multi-edit validation against one original, overlap refusal, per-target mutation ownership, byte-shape preservation | Silent Unicode/whitespace fuzzy fallback and direct non-atomic write |
| Hermes | Same-directory replacement, real-target locking and post-write verification | Nine-strategy fuzzy matching and stale warnings that still write |
