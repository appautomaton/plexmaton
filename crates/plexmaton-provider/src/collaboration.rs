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
    for item in resolved.items() {
        match &item.event {
            // An ordering point with no content of its own; its siblings carry the sources.
            CollaborationEvent::TurnAdmitted { .. } => {}
            CollaborationEvent::DelegationCreated {
                delegation,
                delegator,
                task,
                ..
            } => element(
                &mut out,
                "task",
                &[
                    ("from", &endpoint(delegator)),
                    ("delegation", delegation.as_str()),
                ],
                task.as_str(),
            ),
            CollaborationEvent::TaskUpdated {
                delegation,
                author,
                task,
                ..
            } => element(
                &mut out,
                "task-update",
                &[
                    ("from", &endpoint(author)),
                    ("delegation", delegation.as_str()),
                ],
                task.as_str(),
            ),
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
