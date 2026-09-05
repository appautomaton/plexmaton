# Provider fixtures

The OpenAI Responses and Chat streams were reduced and sanitized from the local development proxy
exercised on 2026-09-02. Duplicate-ID streams are synthetic invalid mutations of those shapes.

The Messages and Gemini streams are synthetic protocol fixtures derived on 2026-09-05 from the
[Messages streaming reference](https://platform.claude.com/docs/en/build-with-claude/streaming),
[GenerateContent reference](https://ai.google.dev/api/generate-content), and
[thought-signature guidance](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures),
cross-checked against the pinned pi and Kimi sources in the
[spike](../../../../.agents/spikes/provider-adapter-parity/README.md). They are not live recordings.

The Messages fixtures' final `output_tokens_details.thinking_tokens` field follows the
[thinking pricing reference](https://platform.claude.com/docs/en/build-with-claude/thinking-steering-and-cost#pricing)
and [Messages response example](https://platform.claude.com/docs/en/api/messages/create).
It is optional; HTTP tests also remove it and verify partial usage with final cost.

Identifiers, keys and signatures are inert fixture values. Each file is one complete HTTP response
body. Tests vary transport chunk boundaries, mutate malformed cases, and exercise the real local
journal and HTTP boundaries without contacting a model service.
