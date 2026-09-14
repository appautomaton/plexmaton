//! Renders one collaboration atom identically for every provider dialect (PRV-1).
//!
//! No wire grammar in use has a role for "another session said this", so the sender is named in the
//! payload instead. That moves attribution from a field the runtime enforces into text the model
//! reads, which is why every element here carries its sender explicitly and why the body is escaped:
//! an unattributed rendering would be indistinguishable from the user's own words, and an unescaped
//! one would let a summary close its own element.

use plexmaton_agent::collaboration::{
    CollaborationContext, CollaborationEvent, MailEndpoint, ResolvedTurnAdmission,
};

use crate::codec::EncodeError;

const LABEL: &str = "Plexmaton delegated collaboration. Each element below was written by the\n\
                     session named in its `from` attribute, not by the user:\n";

/// Closes an envelope that assigned work *to its recipient*, because only that owes an answer.
///
/// A worker that answers only by writing into its own transcript has delivered nothing: the result
/// travels as mail, and the turn is the only place a model learns that — the tool's description
/// says what `send_mail` does, not that it is the way a result gets home.
///
/// Rejected: also telling the worker that whatever else it writes is seen by nobody. That was true
/// while a delegated session's transcript reached no surface, and it read as an instruction to stop
/// writing: every worker went straight from task to tool calls to mail, producing no prose at all,
/// so the user watching it work saw the ask, the tools and the answer with no reasoning between
/// them. Its conversation is now on screen and the sentence had become both false and the reason
/// there was nothing to show. Also rejected: closing every envelope this way, which told a
/// recipient of ordinary mail it owed a reply; and asking only whether a task is present, which is
/// true of the delegator's own envelope — its inclusion window opens at the start of the log, so it
/// re-reads the task it sent and dutifully reports back to itself.
const TASK_CONTRACT: &str = "\nThe task above is yours. Work in this session as you normally would;\n\
                             the user can read it. Send the result to the session that assigned it\n\
                             with `send_mail`, which is the only way your answer reaches it.\n";

/// Renders the resolved sources of one collaboration atom in canonical order.
///
/// A reference that never resolved carries no content, so it is refused rather than rendered as an
/// empty turn: the recipient would otherwise be woken for a message it cannot read.
pub(crate) fn collaboration_context(context: &CollaborationContext) -> Result<String, EncodeError> {
    let CollaborationContext::Resolved(resolved) = context else {
        return Err(EncodeError::UnresolvedCollaboration);
    };
    Ok(render(resolved))
}

fn render(resolved: &ResolvedTurnAdmission) -> String {
    let mut out = String::from(LABEL);
    let recipient = &resolved.admission().boundary.recipient;
    let mut assigned = false;
    for item in resolved.items() {
        match &item.event {
            // An ordering point with no content of its own; its siblings carry the sources.
            CollaborationEvent::TurnAdmitted { .. } => {}
            CollaborationEvent::DelegationCreated {
                delegation,
                delegator,
                worker,
                task,
            } => {
                assigned |= worker == recipient;
                element(
                    &mut out,
                    "task",
                    &[
                        ("from", &endpoint(delegator)),
                        ("delegation", delegation.as_str()),
                    ],
                    task.as_str(),
                );
            }
            CollaborationEvent::TaskUpdated {
                delegation,
                author,
                task,
                ..
            } => {
                assigned |= author != recipient;
                element(
                    &mut out,
                    "task-update",
                    &[
                        ("from", &endpoint(author)),
                        ("delegation", delegation.as_str()),
                    ],
                    task.as_str(),
                );
            }
            CollaborationEvent::MailAccepted { mail } => {
                // The summary is escaped text and each pointer is already an element, so the body
                // is assembled pre-escaped rather than escaped once more as a whole.
                let mut body = escape(mail.summary.as_str());
                for artifact in &mail.artifacts {
                    body.push('\n');
                    empty_element(
                        &mut body,
                        "artifact",
                        &[
                            ("session", artifact.conversation.as_str()),
                            ("id", artifact.artifact.as_str()),
                        ],
                    );
                    body.pop();
                }
                raw_element(
                    &mut out,
                    "mail",
                    &[("from", &endpoint(&mail.from)), ("id", mail.id.as_str())],
                    &body,
                );
            }
            CollaborationEvent::HandoffCompleted {
                delegation, author, ..
            } => empty_element(
                &mut out,
                "handoff",
                &[
                    ("from", &endpoint(author)),
                    ("delegation", delegation.as_str()),
                ],
            ),
        }
    }
    if assigned {
        out.push_str(TASK_CONTRACT);
    }
    out
}

/// `conversation/agent`, because an agent name is unique only inside its own conversation.
fn endpoint(endpoint: &MailEndpoint) -> String {
    format!(
        "{}/{}",
        endpoint.conversation.as_str(),
        endpoint.agent.as_str()
    )
}

fn element(out: &mut String, tag: &str, attributes: &[(&str, &str)], body: &str) {
    raw_element(out, tag, attributes, &escape(body));
}

fn raw_element(out: &mut String, tag: &str, attributes: &[(&str, &str)], body: &str) {
    open(out, tag, attributes);
    out.push_str(">\n");
    out.push_str(body);
    out.push_str(&format!("\n</{tag}>\n"));
}

fn empty_element(out: &mut String, tag: &str, attributes: &[(&str, &str)]) {
    open(out, tag, attributes);
    out.push_str("/>\n");
}

fn open(out: &mut String, tag: &str, attributes: &[(&str, &str)]) {
    out.push('<');
    out.push_str(tag);
    for (name, value) in attributes {
        out.push_str(&format!(" {name}=\"{}\"", escape(value)));
    }
}

/// The five XML entities. A summary is arbitrary user or model text and may contain `</mail>`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(character),
        }
    }
    out
}
