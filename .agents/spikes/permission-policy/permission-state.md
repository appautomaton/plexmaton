# Permission state investigation

| Field | Value |
| --- | --- |
| Read when | Comparing the finite policy model with the implemented ownership boundary |
| Status | Ownership decisions promoted to PER-1–PER-9 |
| Basis | [Finite experiment and source comparison](./README.md) |

The finite `PolicyLog::snapshot` and `evaluate` separate reduction from evaluation with bounded
fixture identities. Its `ConversationId` and `CodingSessionId` made replacement lifetime testable
before integration. It has no runtime workers, journal barrier or project-store refresh.

[Permission policy](../../specs/permission-policy.md) owns the implemented Session/Conversation
vocabulary, native create/edit preset, revision controls, dispatch and audit ordering. The runtime
shares one explicit `CodingSessionPermissions` owner across Conversation replacement; its evidence
table includes lazy startup, `/new`, restart, cancellation and failed-audit cases. The TUI's setting
is a projection of the named grant under PER-7.

[Project storage](../../specs/project-permissions.md) owns persistent transactions. The
[POSIX experiment](./store-experiment.md) retains the independent failure-protocol evidence;
production latency has not been benchmarked. Future delegation still needs Phase 03's explicit
owner handoff. The [HTML view](./approval-view.html) remains a presentation fixture.
