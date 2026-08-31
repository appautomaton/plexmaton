# Handoffs

A handoff is a letter from the agent who just finished a stretch of work to the agent who picks it
up. It exists because a fresh session inherits the repository but not the reasoning: which claims
are load-bearing, which are aspirational, which traps already cost a day.

**A handoff routes and warns. It never holds a rule.** Every rule lives in exactly one place —
`AGENTS.md`, `roadmap/`, `specs/`, or the code — and a handoff links to it. The moment a handoff is
the only statement of something, that thing is in the wrong file.

Handoffs are disposable. They go stale by design; a stale one is deleted, not corrected. Keep at
most the current one and any predecessor still worth reading. Name them `YYYY-MM-DD-<topic>.md`.
