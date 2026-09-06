use super::*;
use proptest::prelude::*;

fn arguments(source: &str) -> Vec<Vec<String>> {
    let CommandSyntax::Literal(shell) = analyze(source, &|| false) else {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .expect("grammar");
        panic!(
            "literal rejected: {source:?}: {}",
            parser
                .parse(source, None)
                .expect("tree")
                .root_node()
                .to_sexp()
        );
    };
    for command in shell.commands() {
        assert!(source.is_char_boundary(command.span().start));
        assert!(source.is_char_boundary(command.span().end));
    }
    shell
        .commands()
        .iter()
        .map(|command| command.arguments().to_vec())
        .collect()
}

/// PER-10: quoting changes token spelling, never argument boundaries or neighboring operations.
#[test]
fn per_10_literal_shell_preserves_posix_quotes_tokens_spans_and_complete_sequences() {
    for (source, expected) in [
        ("ls '' \"\r\"", vec![vec!["ls", "", "\r"]]),
        ("ls -la src", vec![vec!["ls", "-la", "src"]]),
        (
            "git fetch \"team origin\"",
            vec![vec!["git", "fetch", "team origin"]],
        ),
        (
            "g'i'\"t\" fetch team\\ origin",
            vec![vec!["git", "fetch", "team origin"]],
        ),
        (
            "ls '' \"\" '$(inert)' \"a\\q\"",
            vec![vec!["ls", "", "", "$(inert)", "a\\q"]],
        ),
        ("ls 'it'\\''s'", vec![vec!["ls", "it's"]]),
        (
            "ls one && ls two || ls three; ls four\nls five",
            vec![
                vec!["ls", "one"],
                vec!["ls", "two"],
                vec!["ls", "three"],
                vec!["ls", "four"],
                vec!["ls", "five"],
            ],
        ),
    ] {
        let expected: Vec<Vec<String>> = expected
            .into_iter()
            .map(|row| row.into_iter().map(str::to_owned).collect())
            .collect();
        assert_eq!(arguments(source), expected, "{source}");
    }
}

/// PER-10: an unsupported neighbor invalidates the complete reusable interpretation.
#[test]
fn per_10_expansions_redirections_control_flow_and_ambiguous_syntax_have_no_literal_scope() {
    for source in [
        "ls foo\\\nbar",
        "ls $(touch marker)",
        "ls \"$(touch marker)\"",
        "ls `touch marker`",
        "ls $HOME",
        "ls ${HOME}",
        "ls *",
        "ls foo?",
        "ls [ab]",
        "ls ~",
        "ls {a,b}",
        "ls $'hi'",
        "ls > file",
        "ls 2>&1",
        "ls <<< here",
        "ls <<EOF\nhello\nEOF",
        "ls <(echo x)",
        "ls &",
        "ls | cat",
        "ls |& cat",
        "(ls)",
        "{ ls; }",
        "if true; then ls; fi",
        "for a in x; do ls; done",
        "f() { ls; }; f",
        "VAR=value ls",
        "ls; VAR=value",
        "ls; alias ls=false",
        "ls 'unfinished",
        "ls &&",
        "! ls",
        "ls # comment",
        "ls; # comment\nls",
        "ls\rfoo",
        "ls\r foo",
        "ls\u{a0}foo",
        "ls\0foo",
    ] {
        assert!(
            matches!(analyze(source, &|| false), CommandSyntax::ExactOnly(_)),
            "must reject {source:?}"
        );
    }
}

/// PER-10: parser and semantic capacities return an explicit fallback without a partial prefix.
#[test]
fn per_10_parser_limits_never_publish_partial_literal_commands() {
    for source in [
        "x".repeat(crate::MAX_COMMAND_BYTES + 1),
        std::iter::repeat_n("ls", 33).collect::<Vec<_>>().join("; "),
        format!(
            "ls {}",
            std::iter::repeat_n("arg", 128)
                .collect::<Vec<_>>()
                .join(" ")
        ),
        format!("{}ls{}", "$(".repeat(200), ")".repeat(200)),
    ] {
        assert!(matches!(
            analyze(&source, &|| false),
            CommandSyntax::ExactOnly(_)
        ));
    }
}

fn quote(source: &str) -> String {
    format!("'{}'", source.replace('\'', "'\\''"))
}

fn encoded_word(source: &str, index: usize) -> String {
    let double = |text: &str| {
        format!(
            "\"{}\"",
            text.chars()
                .flat_map(|ch| {
                    if matches!(ch, '$' | '`' | '\\' | '"') {
                        vec!['\\', ch]
                    } else {
                        vec![ch]
                    }
                })
                .collect::<String>()
        )
    };
    match index % 3 {
        0 => quote(source),
        1 => double(source),
        _ => {
            let mid = source
                .char_indices()
                .nth(source.chars().count() / 2)
                .map_or(source.len(), |(index, _)| index);
            format!("{}{}", quote(&source[..mid]), double(&source[mid..]))
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// PER-10: generated accepted words agree with the executor dialect, including empty arguments.
    #[test]
    fn per_10_generated_literal_words_agree_with_bin_sh(
        words in prop::collection::vec(prop::collection::vec(prop::sample::select(vec!['a', 'Z', '0', ' ', '\t', '\n', '\'', '"', '\\', '$', '`', '*', '{', '}', 'é', '🌱', '\r']), 0..24), 1..8)
    ) {
        let words: Vec<String> = words.into_iter().map(|chars| chars.into_iter().collect()).collect();
        let source = format!("printf '%s\\0' {}", words.iter().enumerate().map(|(index, word)| encoded_word(word, index)).collect::<Vec<_>>().join(" "));
        let parsed = arguments(&source);
        prop_assert_eq!(&parsed[0][2..], &words);
        // Only our fixed printf plus single-quoted generated data reaches the real shell.
        let output = std::process::Command::new("/bin/sh").args(["-c", &source]).env_clear().output().expect("shell oracle");
        prop_assert!(output.status.success());
        let expected: Vec<u8> = words.iter().flat_map(|word| word.bytes().chain(std::iter::once(0))).collect();
        prop_assert_eq!(output.stdout, expected);
    }

    /// PER-10: arbitrary source never exceeds the bounded immutable representation.
    #[test]
    fn per_10_arbitrary_shell_source_stays_bounded(source in ".{0,512}") {
        if let CommandSyntax::Literal(literal) = analyze(&source, &|| false) {
            prop_assert!(!literal.commands().is_empty());
            prop_assert!(literal.commands().len() <= MAX_COMMANDS);
            for command in literal.commands() {
                prop_assert!(command.arguments().len() <= MAX_ARGUMENTS);
                prop_assert!(command.span().end <= source.len());
            }
        }
    }
}
