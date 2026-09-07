//! SKL-4/SKL-5/SKL-6: skills cross the real runtime, HTTP, tool, and JSONL boundaries.

use std::{ffi::OsString, path::Path, time::Duration};

use plexmaton_agent::{
    AdmissionRefusal, ContextAtomValue, Input, JournalEntryPayload, SkillSource, ToolOutcome,
    UndeliveredReason, UnixMillis,
};
use plexmaton_core::{AgentId, ConversationId, HeadName};
use plexmaton_file_tools::FileCancellation;
use plexmaton_provider::{ApiKey, ModelRegistry, ResolvedModel, resolve_api_key};
use plexmaton_runtime::{DispatchReport, LiveRuntime, NativeToolCatalog, RuntimeUpdate};
use plexmaton_session_store::{AutomaticJournal, JournalFile};

#[path = "skills/fixture.rs"]
mod fixture;
use fixture::{Scratch, ScriptedServer};

const FINAL: &str = include_str!("../../plexmaton-provider/tests/fixtures/chat_final_answer.sse");
const TIMEOUT: Duration = Duration::from_secs(5);

fn skill(name: &str, description: &str, fields: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n{fields}---\n{body}")
}

fn tool_call(call_id: &str, name: &str, resource: Option<&str>) -> String {
    let arguments = serde_json::json!({"name":name, "resource":resource}).to_string();
    format!(
        "data: {{\"id\":\"skill_call\",\"object\":\"chat.completion.chunk\",\"choices\":[{{\"index\":0,\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"{call_id}\",\"type\":\"function\",\"function\":{{\"name\":\"skill\",\"arguments\":{arguments:?}}}}}]}},\"finish_reason\":null}}]}}\n\n\
         data: {{\"id\":\"skill_call\",\"object\":\"chat.completion.chunk\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\n\
         data: [DONE]\n\n"
    )
}

fn transport(base_url: &str) -> (ResolvedModel, ApiKey) {
    let registry = ModelRegistry::parse(&format!(
        r#"
active_model = {{ provider = "fixture", model = "model" }}
[providers.fixture]
base_url = "{base_url}"
api_key_env = "FIXTURE_KEY"
api = "openai_chat_completions"
[providers.fixture.models.model]
id = "fixture-model"
reasoning_effort = "none"
context_window_tokens = 100000
max_output_tokens = 10000
output_reserve_tokens = 5000
"#
    ))
    .unwrap_or_else(|error| panic!("model fixture: {error}"));
    let model = registry.active_model().clone();
    let key = resolve_api_key(&model, Some(OsString::from("fixture-secret")))
        .unwrap_or_else(|error| panic!("fixture key: {error}"));
    (model, key)
}

fn tools(scratch: &Scratch, model: &ResolvedModel) -> NativeToolCatalog {
    NativeToolCatalog::open(
        scratch.project(),
        model.api_key_env(),
        "/bin/false",
        "/bin/false",
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("native tools: {error}"))
    .with_skill_roots(
        &scratch.user_home(),
        &scratch.project(),
        &FileCancellation::new(),
    )
    .unwrap_or_else(|error| panic!("skill roots: {error}"))
}

fn agent_id() -> AgentId {
    AgentId::new("agent-skills").unwrap_or_else(|error| panic!("agent id: {error}"))
}

async fn settle(runtime: &mut LiveRuntime) -> Vec<DispatchReport> {
    let mut reports = Vec::new();
    while runtime.has_active_work() {
        match tokio::time::timeout(TIMEOUT, runtime.next_update())
            .await
            .unwrap_or_else(|_| panic!("runtime skill fixture timed out"))
            .unwrap_or_else(|error| panic!("runtime skill fixture: {error}"))
        {
            RuntimeUpdate::Event(_) => {}
            RuntimeUpdate::Report(report) => reports.push(report),
            RuntimeUpdate::Finished => break,
        }
    }
    while runtime.try_next_event().is_some() {}
    reports
}

fn skill_description(request: &serde_json::Value) -> &str {
    request["tools"]
        .as_array()
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool["function"]["name"] == "skill")
        })
        .and_then(|tool| tool["function"]["description"].as_str())
        .unwrap_or_else(|| panic!("request advertises the skill tool"))
}

fn message_contents(request: &serde_json::Value) -> Vec<&str> {
    request["messages"]
        .as_array()
        .unwrap_or_else(|| panic!("chat messages"))
        .iter()
        .filter_map(|message| message["content"].as_str())
        .collect()
}

fn activated_instructions(request: &serde_json::Value) -> Vec<String> {
    const LABEL: &str = "Plexmaton activated skill context:\n";
    message_contents(request)
        .into_iter()
        .filter_map(|content| content.strip_prefix(LABEL))
        .map(|envelope| {
            let envelope: serde_json::Value = serde_json::from_str(envelope)
                .unwrap_or_else(|error| panic!("skill activation envelope: {error}"));
            envelope["instructions"]
                .as_str()
                .unwrap_or_else(|| panic!("skill activation instructions"))
                .to_owned()
        })
        .collect()
}

fn loaded_tool_texts(request: &serde_json::Value) -> Vec<String> {
    message_contents(request)
        .into_iter()
        .filter_map(|content| serde_json::from_str::<serde_json::Value>(content).ok())
        .filter_map(|content| content["text"].as_str().map(str::to_owned))
        .collect()
}

fn remove(path: &Path) {
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("remove fixture file: {error}"));
}

/// SKL-5/SKL-6: explicit preparation persists separate exact context before HTTP dispatch, and
/// later replay never rereads a source that has changed or disappeared.
#[tokio::test]
async fn explicit_skill_context_survives_source_deletion_and_jsonl_resume() {
    const ORIGINAL: &str = "$review inspect parser";
    const BODY: &str = "EXACT_EXPLICIT_BODY\r\nkeep this\n";
    const DESCRIPTION: &str = "Review parser changes";
    let scratch = Scratch::new("explicit-resume");
    let skill_path = scratch.project().join(".plexmaton/skills/review/SKILL.md");
    scratch.write(
        "project/.plexmaton/skills/review/SKILL.md",
        skill("review", DESCRIPTION, "", BODY),
    );
    let canonical_skill_path = std::fs::canonicalize(&skill_path)
        .unwrap_or_else(|error| panic!("canonical skill fixture: {error}"));
    let server = ScriptedServer::start([FINAL.to_owned(), FINAL.to_owned()]);
    let (model, key) = transport(&server.base_url);
    let automatic = AutomaticJournal::new(scratch.path(), UnixMillis::new(1));
    let journal_path = automatic.path().to_path_buf();
    let mut runtime = LiveRuntime::provider_with_automatic_journal(
        agent_id(),
        "Plexmaton",
        model.clone(),
        key,
        tools(&scratch, &model),
        automatic,
    )
    .await
    .unwrap_or_else(|error| panic!("automatic runtime: {error}"));
    assert!(runtime.skill_diagnostics().is_empty());
    assert_eq!(
        runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: ORIGINAL.to_owned(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("explicit submit: {error}")),
        DispatchReport::default(),
        "preparation returns before model dispatch"
    );
    assert!(
        !runtime.has_active_model(),
        "the provider starts only after next_update settles preparation"
    );
    assert!(settle(&mut runtime).await.is_empty());
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("first shutdown: {error}"));

    remove(&skill_path);
    let reopened = JournalFile::open(&journal_path)
        .unwrap_or_else(|error| panic!("reopen explicit journal: {error}"));
    let path = reopened
        .journal()
        .path(&HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}")))
        .unwrap_or_else(|error| panic!("journal path: {error:?}"));
    assert!(path.iter().any(|entry| matches!(
        &entry.payload,
        JournalEntryPayload::TurnStarted { text, .. } if text == ORIGINAL
    )));
    assert!(path.iter().any(|entry| matches!(
        &entry.payload,
        JournalEntryPayload::SkillActivated { activation, .. }
            if activation.name() == "review"
                && activation.source() == SkillSource::ProjectNative
                && activation.location() == canonical_skill_path.to_string_lossy()
                && activation.instructions() == BODY
    )));

    let resumed_key = resolve_api_key(&model, Some(OsString::from("fixture-secret")))
        .unwrap_or_else(|error| panic!("resumed fixture key: {error}"));
    let (mut resumed, recovery) = LiveRuntime::provider_with_resumed_journal(
        agent_id(),
        model.clone(),
        resumed_key,
        tools(&scratch, &model),
        reopened,
    )
    .await
    .unwrap_or_else(|error| panic!("resume explicit runtime: {error}"));
    assert!(recovery.is_clean());
    resumed
        .submit(
            agent_id(),
            Input::Submitted {
                text: "continue".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("continuation submit: {error}"));
    assert!(settle(&mut resumed).await.is_empty());
    resumed
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("resumed shutdown: {error}"));

    let requests = server.finish().await;
    assert_eq!(requests.len(), 2);
    for request in &requests {
        let contents = message_contents(request);
        assert!(contents.contains(&ORIGINAL));
        assert!(
            activated_instructions(request)
                .iter()
                .any(|body| body == BODY)
        );
    }
    let description = skill_description(&requests[0]);
    assert!(description.contains(DESCRIPTION));
    assert!(!description.contains(BODY));
    assert!(
        !requests[1]["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool["function"]["name"] == "skill")),
        "deleted catalog metadata is current while recorded context remains"
    );
}

/// SKL-4/SKL-5/SKL-6: a model activation uses normal admission and records the exact real read;
/// only catalog metadata appears in the definition sent before that read.
#[tokio::test]
async fn model_skill_call_records_real_read_while_catalog_omits_body() {
    const BODY: &str = "MODEL_SKILL_BODY_SENTINEL\n";
    const DESCRIPTION: &str = "Review with the project checklist";
    let scratch = Scratch::new("model-call");
    scratch.write(
        "project/.agents/skills/review/SKILL.md",
        skill("review", DESCRIPTION, "", BODY),
    );
    let server = ScriptedServer::start([
        tool_call("call_skill_model", "review", None),
        FINAL.to_owned(),
    ]);
    let (model, key) = transport(&server.base_url);
    let journal_path = scratch.path().join("model-call.jsonl");
    let journal = JournalFile::create(
        &journal_path,
        ConversationId::new("model-call-session")
            .unwrap_or_else(|error| panic!("session id: {error}")),
        UnixMillis::new(2),
    )
    .unwrap_or_else(|error| panic!("fresh journal: {error}"));
    let mut runtime = LiveRuntime::provider_with_fresh_journal(
        agent_id(),
        "Plexmaton",
        model.clone(),
        key,
        tools(&scratch, &model),
        journal,
    )
    .await
    .unwrap_or_else(|error| panic!("fresh runtime: {error}"));
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "use the review skill".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("model skill submit: {error}"));
    assert!(settle(&mut runtime).await.is_empty());
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("model skill shutdown: {error}"));
    let requests = server.finish().await;
    assert_eq!(requests.len(), 2);
    let description = skill_description(&requests[0]);
    assert!(description.contains(DESCRIPTION));
    assert!(!description.contains(BODY));
    assert_eq!(loaded_tool_texts(&requests[1]), [BODY]);

    let reopened = JournalFile::open(journal_path)
        .unwrap_or_else(|error| panic!("reopen model skill journal: {error}"));
    let projection = reopened
        .journal()
        .project(&HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}")))
        .unwrap_or_else(|error| panic!("model skill projection: {error:?}"));
    let output = projection
        .request()
        .atoms
        .iter()
        .find_map(|atom| match atom.value() {
            ContextAtomValue::ToolBatch(batch) => {
                batch
                    .results()
                    .iter()
                    .find_map(|result| match result.outcome() {
                        ToolOutcome::Succeeded { output } => Some(output),
                        _ => None,
                    })
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("recorded skill tool result"));
    let loaded: serde_json::Value = serde_json::from_str(output)
        .unwrap_or_else(|error| panic!("loaded skill result JSON: {error}"));
    assert_eq!(loaded["text"], BODY);
    assert_eq!(loaded["resource"], serde_json::Value::Null);
}

/// SKL-4: explicit activation of a user-only skill authorizes its relative resource, while a later
/// model-only attempt to load that skill's body remains a typed admission refusal.
#[tokio::test]
async fn explicit_user_only_skill_allows_resource_but_not_unprompted_body() {
    const BODY: &str = "USER_ONLY_BODY_SENTINEL\n";
    const GUIDE: &str = "EXACT_USER_ONLY_GUIDE\r\n";
    const DESCRIPTION: &str = "Private user workflow";
    let scratch = Scratch::new("user-only");
    scratch.write(
        "project/.agents/skills/private/SKILL.md",
        skill(
            "private",
            DESCRIPTION,
            "disable-model-invocation: true\n",
            BODY,
        ),
    );
    scratch.write("project/.agents/skills/private/guide.md", GUIDE);
    let server = ScriptedServer::start([
        tool_call("call_skill_resource", "private", Some("guide.md")),
        FINAL.to_owned(),
        tool_call("call_skill_body", "private", None),
        FINAL.to_owned(),
    ]);
    let (model, key) = transport(&server.base_url);
    let journal_path = scratch.path().join("user-only.jsonl");
    let journal = JournalFile::create(
        &journal_path,
        ConversationId::new("user-only-session")
            .unwrap_or_else(|error| panic!("user-only session id: {error}")),
        UnixMillis::new(4),
    )
    .unwrap_or_else(|error| panic!("user-only journal: {error}"));
    let mut runtime = LiveRuntime::provider_with_fresh_journal(
        agent_id(),
        "Plexmaton",
        model.clone(),
        key,
        tools(&scratch, &model),
        journal,
    )
    .await
    .unwrap_or_else(|error| panic!("user-only runtime: {error}"));
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "$private follow it".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("explicit user-only submit: {error}"));
    assert!(settle(&mut runtime).await.is_empty());
    runtime
        .submit(
            agent_id(),
            Input::Submitted {
                text: "try loading its body yourself".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("unprompted submit: {error}"));
    assert!(settle(&mut runtime).await.is_empty());
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("user-only shutdown: {error}"));

    let requests = server.finish().await;
    assert_eq!(requests.len(), 4);
    assert!(!skill_description(&requests[0]).contains(DESCRIPTION));
    assert_eq!(activated_instructions(&requests[0]), [BODY]);
    assert_eq!(loaded_tool_texts(&requests[1]), [GUIDE]);
    let refused: serde_json::Value = message_contents(&requests[3])
        .into_iter()
        .find(|text| text.contains("admission_refused"))
        .map(|text| {
            serde_json::from_str(text).unwrap_or_else(|error| panic!("typed refusal JSON: {error}"))
        })
        .unwrap_or_else(|| panic!("typed refusal reaches the next request"));
    assert_eq!(refused["status"], "admission_refused");
    assert_eq!(refused["reason"], "invalid_arguments");
    assert!(
        !loaded_tool_texts(&requests[3])
            .iter()
            .any(|text| text == BODY),
        "the refused tool result did not perform another body read"
    );

    let reopened = JournalFile::open(journal_path)
        .unwrap_or_else(|error| panic!("reopen user-only journal: {error}"));
    let projection = reopened
        .journal()
        .project(&HeadName::new("main").unwrap_or_else(|error| panic!("head: {error}")))
        .unwrap_or_else(|error| panic!("user-only projection: {error:?}"));
    let outcomes: Vec<_> = projection
        .request()
        .atoms
        .iter()
        .flat_map(|atom| match atom.value() {
            ContextAtomValue::ToolBatch(batch) => batch.results(),
            ContextAtomValue::Collaboration(_)
            | ContextAtomValue::User { .. }
            | ContextAtomValue::Skill(_)
            | ContextAtomValue::Assistant(_)
            | ContextAtomValue::CompactionSummary { .. } => &[],
        })
        .map(|result| result.outcome())
        .collect();
    assert!(outcomes.iter().any(|outcome| matches!(
        outcome,
        ToolOutcome::Succeeded { output }
            if serde_json::from_str::<serde_json::Value>(output).is_ok_and(|value| value["text"] == GUIDE)
    )));
    assert!(outcomes.iter().any(|outcome| matches!(
        outcome,
        ToolOutcome::AdmissionRefused {
            reason: AdmissionRefusal::InvalidArguments
        }
    )));
}

/// SKL-4/SKL-5: unknown and user-disabled explicit invocations return the exact original input,
/// report a typed reason, start no provider, and never materialize the lazy journal.
#[tokio::test]
async fn unavailable_explicit_skills_restore_input_without_model_dispatch() {
    for (label, text_name, selected_name, configured, delete_source) in [
        ("unknown", "missing", "missing", None, false),
        (
            "user-disabled",
            "model-only",
            "model-only",
            Some(skill(
                "model-only",
                "Model-only workflow",
                "user-invocable: false\n",
                "MODEL_ONLY_BODY\n",
            )),
            false,
        ),
        (
            "mismatched-binding",
            "review",
            "other",
            Some(skill("review", "Review workflow", "", "REVIEW_BODY\n")),
            false,
        ),
        (
            "stale-selection",
            "review",
            "review",
            Some(skill("review", "Review workflow", "", "REVIEW_BODY\n")),
            true,
        ),
    ] {
        let scratch = Scratch::new(label);
        if let Some(configured) = configured {
            scratch.write(
                format!("project/.agents/skills/{text_name}/SKILL.md"),
                configured,
            );
        }
        let (model, key) = transport("http://127.0.0.1:9/v1");
        let automatic = AutomaticJournal::new(scratch.path(), UnixMillis::new(3));
        let journal_path = automatic.path().to_path_buf();
        let mut runtime = LiveRuntime::provider_with_automatic_journal(
            agent_id(),
            "Plexmaton",
            model.clone(),
            key,
            tools(&scratch, &model),
            automatic,
        )
        .await
        .unwrap_or_else(|error| panic!("{label} runtime: {error}"));
        if delete_source {
            remove(
                &scratch
                    .project()
                    .join(format!(".agents/skills/{text_name}/SKILL.md")),
            );
        }
        let original = format!("${text_name} keep original");
        let immediate = runtime
            .submit_skill(
                agent_id(),
                Input::Submitted {
                    text: original.clone(),
                },
                selected_name.to_owned(),
            )
            .await
            .unwrap_or_else(|error| panic!("{label} submit: {error}"));
        let mut reports = if immediate == DispatchReport::default() {
            Vec::new()
        } else {
            vec![immediate]
        };
        reports.extend(settle(&mut runtime).await);
        assert_eq!(reports.len(), 1, "{label}");
        assert!(
            reports[0]
                .skill_errors
                .iter()
                .all(|message| message.len() <= 1024)
        );
        assert!(matches!(
            reports[0].undelivered.as_slice(),
            [input] if input.text == original
                && input.skill.as_deref() == Some(selected_name)
                && input.reason == UndeliveredReason::SkillUnavailable
        ));
        assert!(!runtime.has_active_model(), "{label}");
        assert!(
            !journal_path.exists(),
            "{label} must not persist user input"
        );
        runtime
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("{label} shutdown: {error}"));
        assert!(!journal_path.exists(), "{label} shutdown stays lazy");
    }
}

/// SKP-2/SKL-4: dollar-prefixed shell, currency, unknown, and prose text remains ordinary model
/// input; a numeric skill activates only when the composer supplies its explicit selection.
#[tokio::test]
async fn dollar_text_is_literal_unless_a_current_skill_selection_binds_it() {
    const NUMERIC_BODY: &str = "NUMERIC_SKILL_BODY\n";
    let scratch = Scratch::new("dollar-literals");
    scratch.write(
        "project/.agents/skills/review/SKILL.md",
        skill("review", "Review code", "", "REVIEW_BODY\n"),
    );
    scratch.write(
        "project/.agents/skills/100/SKILL.md",
        skill("\"100\"", "Numeric selection", "", NUMERIC_BODY),
    );
    scratch.write(
        "project/.agents/skills/model-only/SKILL.md",
        skill(
            "model-only",
            "Model-only skill",
            "user-invocable: false\n",
            "MODEL_ONLY_BODY\n",
        ),
    );
    let literals = [
        "$HOME path",
        "$amount total",
        "$100 literal",
        "$5 currency",
        "$(command)",
        "$model-only request",
        "mention $review in prose",
        "`$review` in code",
        "    $review indented code",
        "\n$review later line",
        "$ review separated token",
        "$",
    ];
    let server = ScriptedServer::start(
        std::iter::repeat_n(FINAL.to_owned(), literals.len() + 1).collect::<Vec<_>>(),
    );
    let (model, key) = transport(&server.base_url);
    let mut runtime = LiveRuntime::provider(
        agent_id(),
        "Plexmaton",
        model.clone(),
        key,
        tools(&scratch, &model),
    )
    .unwrap_or_else(|error| panic!("dollar literal runtime: {error}"));
    for literal in literals {
        let report = runtime
            .submit(
                agent_id(),
                Input::Submitted {
                    text: literal.to_owned(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("literal submit: {error}"));
        assert_eq!(report, DispatchReport::default(), "{literal}");
        assert!(settle(&mut runtime).await.is_empty(), "{literal}");
    }
    let selected_text = "$100 request";
    let report = runtime
        .submit_skill(
            agent_id(),
            Input::Submitted {
                text: selected_text.to_owned(),
            },
            "100".to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("numeric skill selection: {error}"));
    assert_eq!(report, DispatchReport::default());
    assert!(!runtime.has_active_model());
    assert!(settle(&mut runtime).await.is_empty());
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("dollar literal shutdown: {error}"));

    let requests = server.finish().await;
    assert_eq!(requests.len(), literals.len() + 1);
    for (request, literal) in requests.iter().zip(literals) {
        assert!(message_contents(request).contains(&literal));
        assert!(activated_instructions(request).is_empty(), "{literal}");
    }
    let selected = requests
        .last()
        .unwrap_or_else(|| panic!("numeric selection request"));
    assert!(message_contents(selected).contains(&selected_text));
    assert_eq!(activated_instructions(selected), [NUMERIC_BODY]);
}
