//! JavaScript regular-expression character classes used by upstream extraction.
/// Preserve ECMAScript character classes with the linear engine's ASCII boundary.
pub(crate) fn linear_pattern(source: &str) -> String {
    pattern(source).replace(&pattern(r"\b"), r"(?-u:\b)")
}

pub(crate) fn pattern(pattern: &str) -> String {
    const SPACE: &str = r"\x09-\x0D\x20\u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";
    let mut out = String::new();
    let mut chars = pattern.chars();
    let mut class = false;
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('w') => out.push_str(if class { "A-Za-z0-9_" } else { "[A-Za-z0-9_]" }),
                Some('W') => out.push_str("[^A-Za-z0-9_]"),
                Some('d') => out.push_str(if class { "0-9" } else { "[0-9]" }),
                Some('D') => out.push_str("[^0-9]"),
                Some('s') => {
                    if !class {
                        out.push('[');
                    }
                    out.push_str(SPACE);
                    if !class {
                        out.push(']');
                    }
                }
                Some('S') => {
                    out.push_str("[^");
                    out.push_str(SPACE);
                    out.push(']');
                }
                Some('b') if !class => out.push_str(
                    "(?:(?<=[A-Za-z0-9_])(?![A-Za-z0-9_])|(?<![A-Za-z0-9_])(?=[A-Za-z0-9_]))",
                ),
                Some('b') => out.push_str(r"\x08"),
                Some('/') => out.push('/'),
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                }
                None => out.push('\\'),
            }
        } else if ch == '[' {
            if class {
                out.push('\\');
            } else {
                class = true;
            }
            out.push(ch);
        } else if ch == '.' && !class {
            out.push_str(r"[^\r\n\u{2028}\u{2029}]");
        } else {
            if ch == ']' {
                class = false;
            }
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::pattern;
    use fancy_regex::Regex;

    #[test]
    fn ecmascript_character_classes_keep_their_source_semantics() {
        let regex = |source| Regex::new(&pattern(source)).unwrap();
        assert!(regex(r"^\s$").is_match("\u{feff}").unwrap());
        assert!(!regex(r"^\s$").is_match("\u{85}").unwrap());
        assert!(regex(r"^[\S]$").is_match("\u{85}").unwrap());
        assert!(!regex(r"^[\S]$").is_match("\u{feff}").unwrap());
        assert!(regex(r"^[\s\S]*$").is_match("é😀\u{85}\u{feff}\n").unwrap());
        assert!(!regex(r"^\d$").is_match("١").unwrap());
        assert!(regex(r"^[\D]$").is_match("١").unwrap());
        for line_end in ["\n", "\r", "\u{2028}", "\u{2029}"] {
            assert!(!regex(r"^.$").is_match(line_end).unwrap());
        }
    }
}
