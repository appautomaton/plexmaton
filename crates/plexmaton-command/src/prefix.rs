//! Complete literal lowering at the executor boundary. No shell is invoked to derive scope.
use std::time::{Duration, Instant};

use plexmaton_agent::{CommandSyntax, LiteralCommand, LiteralShell, PrefixUnavailable};
use tree_sitter::{Node, ParseOptions, Parser};

mod word;

const MAX_NODES: usize = 2048;
const MAX_DEPTH: usize = 32;
const MAX_COMMANDS: usize = 32;
const MAX_ARGUMENTS: usize = 128;
const MAX_PROGRESS_CHECKS: usize = 512;
const MAX_PARSE_TIME: Duration = Duration::from_millis(20);

pub(super) fn analyze(source: &str, cancelled: &dyn Fn() -> bool) -> CommandSyntax {
    lower(source, cancelled).map_or_else(CommandSyntax::ExactOnly, CommandSyntax::Literal)
}

fn lower(source: &str, cancelled: &dyn Fn() -> bool) -> Result<LiteralShell, PrefixUnavailable> {
    if cancelled() || source.len() > crate::MAX_COMMAND_BYTES {
        return Err(PrefixUnavailable::Limit);
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .map_err(|_| PrefixUnavailable::ParserUnavailable)?;
    let started = Instant::now();
    let mut checks = 0;
    let mut progress = |_: &tree_sitter::ParseState| {
        checks += 1;
        cancelled() || checks > MAX_PROGRESS_CHECKS || started.elapsed() >= MAX_PARSE_TIME
    };
    let bytes = source.as_bytes();
    let tree = parser
        .parse_with_options(
            &mut |offset, _| bytes.get(offset..).unwrap_or_default(),
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
        .ok_or(PrefixUnavailable::Limit)?;
    let root = tree.root_node();
    if root.has_error()
        || root.kind() != "program"
        || !literal_gap(&source[..root.start_byte()])
        || !literal_gap(&source[root.end_byte()..])
    {
        return Err(PrefixUnavailable::UnsupportedSyntax);
    }
    let command_nodes = validate_tree(root, source)?;
    let commands = command_nodes
        .into_iter()
        .map(|node| lower_command(node, source))
        .collect::<Result<Vec<_>, _>>()?;
    LiteralShell::new(commands, source.len()).ok_or(PrefixUnavailable::Limit)
}

fn validate_tree<'tree>(
    root: Node<'tree>,
    source: &str,
) -> Result<Vec<Node<'tree>>, PrefixUnavailable> {
    let mut stack = vec![(root, 0)];
    let mut commands = Vec::new();
    let mut visited = 0;
    while let Some((node, depth)) = stack.pop() {
        visited += 1;
        if visited + stack.len() + node.child_count() > MAX_NODES || depth > MAX_DEPTH {
            return Err(PrefixUnavailable::Limit);
        }
        let valid = if node.is_named() {
            matches!(
                node.kind(),
                "program"
                    | "list"
                    | "command"
                    | "command_name"
                    | "word"
                    | "string"
                    | "string_content"
                    | "raw_string"
                    | "number"
                    | "concatenation"
            )
        } else {
            matches!(node.kind(), "&&" | "||" | ";" | "\"" | "'")
        };
        if !valid || node.is_missing() || node.is_error() {
            return Err(PrefixUnavailable::UnsupportedSyntax);
        }
        if node.kind() == "command" {
            if commands.len() >= MAX_COMMANDS {
                return Err(PrefixUnavailable::Limit);
            }
            commands.push(node);
        }
        // Check structural gaps against /bin/sh; quoted data is decoded from its complete span.
        // A continuation between word nodes can join argv text despite the grammar splitting it.
        let mut cursor = node.walk();
        let structural = matches!(node.kind(), "program" | "list" | "command");
        let mut end = node.start_byte();
        for child in node.children(&mut cursor) {
            if structural && !literal_gap(&source[end..child.start_byte()]) {
                return Err(PrefixUnavailable::UnsupportedSyntax);
            }
            end = child.end_byte();
            stack.push((child, depth + 1));
        }
        if structural && node.child_count() > 0 && !literal_gap(&source[end..node.end_byte()]) {
            return Err(PrefixUnavailable::UnsupportedSyntax);
        }
    }
    commands.sort_by_key(Node::start_byte);
    Ok(commands)
}

fn literal_gap(source: &str) -> bool {
    source
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
}

fn lower_command(node: Node<'_>, source: &str) -> Result<LiteralCommand, PrefixUnavailable> {
    if node.named_child_count() > MAX_ARGUMENTS {
        return Err(PrefixUnavailable::Limit);
    }
    let mut cursor = node.walk();
    let mut arguments = Vec::new();
    let mut previous_end = None;
    for child in node.named_children(&mut cursor) {
        if !matches!(
            child.kind(),
            "command_name" | "word" | "number" | "string" | "raw_string" | "concatenation"
        ) {
            return Err(PrefixUnavailable::UnsupportedSyntax);
        }
        if previous_end.is_some_and(|end| end == child.start_byte()) {
            return Err(PrefixUnavailable::UnsupportedSyntax);
        }
        previous_end = Some(child.end_byte());
        arguments.push(
            word::decode(&source[child.byte_range()])
                .ok_or(PrefixUnavailable::UnsupportedSyntax)?,
        );
    }
    if matches!(
        arguments.first().map(String::as_str),
        Some("alias" | "unalias")
    ) {
        return Err(PrefixUnavailable::UnsupportedSyntax);
    }
    LiteralCommand::new(arguments, node.byte_range()).ok_or(PrefixUnavailable::Limit)
}

#[cfg(test)]
mod tests;
