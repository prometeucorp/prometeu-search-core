use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedQuery {
    pub text: String,
    pub kinds: Vec<String>,
    pub languages: Vec<String>,
    pub path_filters: Vec<String>,
    pub name_filters: Vec<String>,
}

pub(crate) fn whitespace(ch: char) -> bool {
    matches!(ch, '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
pub(super) fn length(value: &str) -> usize {
    value.encode_utf16().count()
}
pub(super) fn split_words(value: &str) -> impl Iterator<Item = &str> {
    value.split(whitespace).filter(|part| !part.is_empty())
}

/// Parse field filters, preserving unknown fields and malformed quotes as text.
pub fn parse_query(raw: &str) -> ParsedQuery {
    let mut out = ParsedQuery::default();
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < raw.len() {
        let ch = raw[pos..].chars().next().unwrap();
        if whitespace(ch) {
            pos += ch.len_utf8();
            continue;
        }
        let start = pos;
        while pos < raw.len() {
            let ch = raw[pos..].chars().next().unwrap();
            if whitespace(ch) {
                break;
            }
            if ch == '"' {
                pos = raw[pos + 1..]
                    .find('"')
                    .map_or(raw.len(), |end| pos + end + 2);
            } else {
                pos += ch.len_utf8();
            }
        }
        tokens.push(&raw[start..pos]);
    }
    let mut text = Vec::new();
    for token in tokens {
        let Some((key, value)) = token.split_once(':') else {
            text.push(token);
            continue;
        };
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        if key.is_empty() || value.is_empty() {
            text.push(token);
            continue;
        }
        match key.to_lowercase().as_str() {
            "kind" if KINDS.contains(&value) => out.kinds.push(value.into()),
            "lang" | "language"
                if crate::parser::LANGUAGES.contains(&value.to_lowercase().as_str())
                    || value.eq_ignore_ascii_case("unknown") =>
            {
                out.languages.push(value.to_lowercase())
            }
            "path" => out.path_filters.push(value.into()),
            "name" => out.name_filters.push(value.into()),
            _ => text.push(token),
        }
    }
    out.text = text.join(" ").trim_matches(whitespace).into();
    out
}
pub const KINDS: &[&str] = &[
    "file",
    "module",
    "class",
    "struct",
    "interface",
    "trait",
    "protocol",
    "function",
    "method",
    "property",
    "field",
    "variable",
    "constant",
    "enum",
    "enum_member",
    "type_alias",
    "namespace",
    "parameter",
    "import",
    "export",
    "route",
    "component",
    "union",
];

/// Bounded Levenshtein distance on UTF-16 units, matching the reference runtime.
pub fn bounded_edit_distance(a: &str, b: &str, max: usize) -> usize {
    if a == b {
        return 0;
    }
    let a: Vec<_> = a.encode_utf16().collect();
    let b: Vec<_> = b.encode_utf16().collect();
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<_> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, ch) in a.iter().enumerate() {
        current[0] = i + 1;
        let mut minimum = current[0];
        for (j, target) in b.iter().enumerate() {
            current[j + 1] = (current[j] + 1)
                .min(prev[j + 1] + 1)
                .min(prev[j] + usize::from(ch != target));
            minimum = minimum.min(current[j + 1]);
        }
        if minimum > max {
            return max + 1;
        }
        std::mem::swap(&mut prev, &mut current);
    }
    prev[b.len()]
}
