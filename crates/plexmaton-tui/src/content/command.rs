//! Terminal-safe command views and complete invocation text share their semantic fields.

pub(crate) fn command_transcript_source(source: &str, root: &str, timeout_ms: u64) -> String {
    let source = serde_json::to_string(source).expect("a retained command string serializes");
    let root = serde_json::to_string(root).expect("a retained workspace string serializes");
    format!("Command {source}\ncwd: {root}\ntimeout_ms: {timeout_ms}")
}

pub(crate) fn command_display_source(source: &str) -> String {
    let mut visible = String::with_capacity(source.len());
    for ch in source.chars() {
        match ch {
            '\n' => visible.push('\n'),
            '\t' => visible.push_str("    "),
            ch if ch.is_control() => visible.extend(ch.escape_default()),
            ch => visible.push(ch),
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    /// ENT-4/SURF-4: inspectable control bytes stay inert without losing source line boundaries.
    #[test]
    fn command_controls_are_visible_text_not_terminal_sequences() {
        assert_eq!(
            super::command_display_source("printf\n\t\u{1b}[31m\r"),
            "printf\n    \\u{1b}[31m\\r"
        );
    }
}
