//! ES module and CommonJS binding scans used by the reference resolver.
use anyhow::{Result, bail};
use fancy_regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Mapping {
    pub local_name: String,
    pub exported_name: String,
    pub source: String,
    pub is_default: bool,
    pub is_namespace: bool,
}
const IDENT: &str = r"[$_\p{ID_Start}][$\u{200c}\u{200d}\p{ID_Continue}]*";
const STRING: &str = r#""(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'"#;
fn regex(pattern: &str) -> Regex {
    Regex::new(&crate::parser::javascript_regex::pattern(pattern)).expect("constant import pattern")
}
fn trim(value: &str) -> &str {
    value.trim_matches(crate::search::query_whitespace)
}
fn mapping(local: &str, exported: &str, source: &str, namespace: bool) -> Mapping {
    Mapping {
        local_name: local.into(),
        exported_name: exported.into(),
        source: source.into(),
        is_default: exported == "default",
        is_namespace: namespace,
    }
}

pub(crate) fn extract(source: &str) -> Result<Vec<Mapping>> {
    static ES: LazyLock<Regex> = LazyLock::new(|| {
        regex(&format!(
            r#"(?<![$.\p{{ID_Continue}}])import(?:\s+|(?=[{{*]))(?:(?:type(?:of)?|defer)(?:\s*(?=[{{*])|\s+(?!from(?![$\p{{ID_Continue}}]))(?=[$_\p{{ID_Start}}])))?(?:({IDENT})(?![$\u{{200c}}\u{{200d}}\p{{ID_Continue}}{{])\s*(?:,\s*)?)?(?:\{{([^{{}}]+)\}}\s*)?(?:(\*)\s*as\s+({IDENT})\s*)?from\s*['"]([^'"]+)['"]"#
        ))
    });
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| regex(r"(?m)^[ \t]*\*(?!/)"));
    static REQUIRE: LazyLock<::regex::Regex> = LazyLock::new(|| {
        let pattern = crate::parser::javascript_regex::linear_pattern(
            r#"(?:\b(?:const|let|var)\s+|,\s*)(?:([A-Za-z_$][\w$]*)|\{([^}]+)\})\s*=\s*require\(\s*['"]([^'"]+)['"]\s*\)(?:\s*\.\s*([A-Za-z_$][\w$]*))?"#,
        );
        // The linear engine supports the same ASCII word boundary without
        // spending a backtracking budget while scanning large source files.
        ::regex::Regex::new(&pattern).expect("constant require pattern")
    });
    static ALIAS: LazyLock<Regex> =
        LazyLock::new(|| regex(r"([A-Za-z_$][\w$]*)\s*:\s*([A-Za-z_$][\w$]*)"));
    let mut out = Vec::new();
    for captures in ES.captures_iter(source) {
        let captures = captures?;
        let module = &captures[5];
        if let Some(default) = captures.get(1) {
            out.push(mapping(default.as_str(), "default", module, false));
        }
        if let Some(list) = captures.get(2) {
            let list = if captures.get(0).is_some_and(|whole| {
                whole.start() > 0 && source.as_bytes()[whole.start() - 1] == b'@'
            }) {
                let mut value = String::new();
                let mut at = 0;
                for prefix in PREFIX.find_iter(list.as_str()) {
                    let prefix = prefix?;
                    value.push_str(&list.as_str()[at..prefix.start()]);
                    at = prefix.end();
                }
                value.push_str(&list.as_str()[at..]);
                value
            } else {
                list.as_str().into()
            };
            let list = comments(&list);
            for item in split(&list) {
                if let Some((imported, local)) = specifier(item)? {
                    out.push(mapping(&local, &imported, module, false));
                }
            }
        }
        if let Some(namespace) = captures.get(4) {
            out.push(mapping(namespace.as_str(), "*", module, true));
        }
    }
    for captures in REQUIRE.captures_iter(source) {
        let module = &captures[3];
        if let Some(default) = captures.get(1) {
            let mut binding = mapping(
                default.as_str(),
                captures.get(4).map_or("default", |m| m.as_str()),
                module,
                false,
            );
            binding.is_default = captures.get(4).is_none();
            out.push(binding);
        }
        if let Some(list) = captures.get(2) {
            for item in list.as_str().split(',').map(trim) {
                let mut binding = if let Some(alias) = ALIAS.captures(item)? {
                    mapping(&alias[2], &alias[1], module, false)
                } else if !item.is_empty() {
                    mapping(item, item, module, false)
                } else {
                    continue;
                };
                binding.is_default = false;
                out.push(binding);
            }
        }
    }
    Ok(out)
}
pub(crate) fn split(list: &str) -> Vec<&str> {
    let bytes = list.as_bytes();
    let mut start = 0;
    let mut at = 0;
    let mut quote = None;
    let mut out = Vec::new();
    while at < bytes.len() {
        let ch = bytes[at];
        if let Some(end) = quote {
            if ch == b'\\' {
                at += 1;
            } else if ch == end {
                quote = None;
            }
        } else if matches!(ch, b'\'' | b'"') {
            quote = Some(ch);
        } else if ch == b',' {
            out.push(&list[start..at]);
            start = at + 1;
        }
        at += 1;
    }
    out.push(&list[start..]);
    out
}
pub(crate) fn comments(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    let mut quote = None;
    while let Some(ch) = chars.next() {
        if let Some(end) = quote {
            out.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if ch == end {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"' | '`') {
            quote = Some(ch);
            out.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}
pub(crate) fn specifier(raw: &str) -> Result<Option<(String, String)>> {
    static ALIAS: LazyLock<Regex> = LazyLock::new(|| {
        regex(&format!(
            r"^(?:(?:type(?:of)?\s+))?((?:{IDENT}|{STRING}))\s+as\s+((?:{IDENT}|{STRING}))$"
        ))
    });
    static PLAIN: LazyLock<Regex> =
        LazyLock::new(|| regex(&format!(r"^(?:(?:type(?:of)?\s+))?((?:{IDENT}|{STRING}))$")));
    let item = trim(raw);
    if let Some(alias) = ALIAS.captures(item)? {
        return Ok(Some((unquote(&alias[1])?, unquote(&alias[2])?)));
    }
    if let Some(plain) = PLAIN.captures(item)? {
        let name = unquote(&plain[1])?;
        return Ok(Some((name.clone(), name)));
    }
    Ok(None)
}
fn unquote(name: &str) -> Result<String> {
    if !name.starts_with(['\'', '"']) {
        return Ok(name.into());
    }
    static ESCAPE: LazyLock<Regex> =
        LazyLock::new(|| regex(r"\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|[\s\S])"));
    let source = &name[1..name.len() - 1];
    let mut units = Vec::new();
    let mut offset = 0;
    for captures in ESCAPE.captures_iter(source) {
        let captures = captures?;
        let whole = captures.get(0).unwrap();
        units.extend(source[offset..whole.start()].encode_utf16());
        let escape = &captures[1];
        if escape.starts_with(['u', 'x']) {
            let hex = if escape.as_bytes().get(1) == Some(&b'{') {
                &escape[2..escape.len() - 1]
            } else {
                &escape[1..]
            };
            if let Ok(value) = u32::from_str_radix(hex, 16) {
                if value <= 0xffff {
                    units.push(value as u16);
                } else if let Some(ch) = char::from_u32(value) {
                    units.extend(ch.encode_utf16(&mut [0; 2]).iter().copied());
                } else {
                    units.extend(escape.encode_utf16());
                }
            } else {
                units.extend(escape.encode_utf16());
            }
        } else {
            units.extend(
                match escape {
                    "n" => "\n",
                    "t" => "\t",
                    "r" => "\r",
                    "b" => "\u{8}",
                    "f" => "\u{c}",
                    "v" => "\u{b}",
                    "0" => "\0",
                    other => other,
                }
                .encode_utf16(),
            );
        }
        offset = whole.end();
    }
    units.extend(source[offset..].encode_utf16());
    match String::from_utf16(&units) {
        Ok(value) => Ok(value),
        Err(_) => bail!("unpaired UTF-16 escape in import specifier"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commonjs_bindings_survive_large_source_scans() {
        let mut source =
            format!("export function {}() {{ return 1; }}\n", "name".repeat(40)).repeat(3200);
        source.push_str("const finalBinding = require('late-module');");
        assert_eq!(
            extract(&source).unwrap(),
            vec![mapping("finalBinding", "default", "late-module", false)]
        );
    }
    #[test]
    fn bindings_match_independent_reference() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/workspace/imports.json"
        ))
        .unwrap();
        for case in corpus["imports"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            assert_eq!(
                serde_json::to_value(extract(source).unwrap()).unwrap(),
                case["expected"],
                "{source}"
            );
        }
    }
}
