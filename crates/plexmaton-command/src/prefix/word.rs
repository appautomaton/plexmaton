//! POSIX quote removal for a tree-validated literal word. Expansion is never evaluated.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Quote {
    None,
    Single,
    Double,
}

pub(super) fn decode(source: &str) -> Option<String> {
    let mut quote = Quote::None;
    let mut result = String::new();
    let mut chars = source.chars();
    while let Some(character) = chars.next() {
        match (quote, character) {
            (Quote::None, '\'') => quote = Quote::Single,
            (Quote::None, '"') => quote = Quote::Double,
            (Quote::Single, '\'') | (Quote::Double, '"') => quote = Quote::None,
            (Quote::None, '\\') => {
                let escaped = chars.next()?;
                if escaped != '\n' {
                    result.push(escaped);
                }
            }
            (Quote::Double, '\\') => {
                let escaped = chars.next()?;
                match escaped {
                    '\n' => {}
                    '$' | '`' | '"' | '\\' => result.push(escaped),
                    _ => {
                        result.push('\\');
                        result.push(escaped);
                    }
                }
            }
            (Quote::None | Quote::Double, '$' | '`') => return None,
            (Quote::None, '~' | '*' | '?' | '[' | ']' | '{' | '}' | '(' | ')' | '!' | '\0') => {
                return None;
            }
            (Quote::None, character) if character.is_whitespace() => return None,
            (_, '\0') => return None,
            (_, character) => result.push(character),
        }
    }
    (quote == Quote::None).then_some(result)
}
