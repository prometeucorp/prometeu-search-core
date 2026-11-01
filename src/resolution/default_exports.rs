//! Stated default exports: named ESM declarations/bindings and CommonJS assignments.
use anyhow::Result;
use fancy_regex::Regex;
use prometeu_extract::Node;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

pub(super) enum DefaultExport {
    DeclarationFile,
    Component,
    Node(String),
    CommonJs(String),
    Unstated,
}
fn regex(pattern: &str) -> Regex {
    Regex::new(&crate::parser::javascript_regex::pattern(pattern))
        .expect("constant default export pattern")
}
impl DefaultExport {
    pub(super) fn read(path: &str, source: &str, nodes: &[&Node]) -> Result<Self> {
        static JS: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r"\.(?:[cm]?[jt]sx?)$").unwrap());
        static DECLARATION_FILE: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r"(?i)\.d\.[cm]?ts$").unwrap());
        static HEAD: LazyLock<Regex> = LazyLock::new(|| regex(r"(?m)^[ \t]*export\s+default\s+"));
        static DECLARATION: LazyLock<Regex> = LazyLock::new(|| {
            regex(
                r"^(?:async\s+)?function\b\s*(?:\*\s*)?([A-Za-z_$][\w$]*)?|^(?:abstract\s+)?class\b(?:\s+(?!extends\b|implements\b)([A-Za-z_$][\w$]*))?",
            )
        });
        static BINDING: LazyLock<Regex> =
            LazyLock::new(|| regex(r"^([A-Za-z_$][\w$]*)\s*;?[ \t]*(?:\r?\n|$)"));
        static COMMON: LazyLock<Regex> = LazyLock::new(|| {
            regex(
                r#"(?m)^[ \t]*(?:exports\s*=\s*)?module\.exports\s*=\s*(?:exports\s*=\s*)?(?:require\(\s*['"]([^'"]+)['"]\s*\)\s*;?[ \t]*$|(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)\s*\(|class\s+([A-Za-z_$][\w$]*)|([A-Za-z_$][\w$]*)\s*;?[ \t]*$)"#,
            )
        });
        if !JS.is_match(path) {
            return Ok(Self::Component);
        }
        if DECLARATION_FILE.is_match(path) {
            return Ok(Self::DeclarationFile);
        }
        let mut named: HashMap<&str, Vec<&Node>> = HashMap::new();
        for &node in nodes {
            named.entry(&node.name).or_default().push(node);
        }
        let code = crate::synthesis::comments::strip_for_regex(source, "typescript");
        let spans = template_text(source);
        let lines: Vec<_> = std::iter::once(0)
            .chain(code.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        for head in HEAD.find_iter(&code) {
            let head = head?;
            if spans.as_ref().is_some_and(|spans| {
                let at = spans.partition_point(|span| span.end <= head.start());
                spans.get(at).is_some_and(|span| span.start <= head.start())
            }) {
                continue;
            }
            let mut units = 0;
            let tail = &code[head.end()..];
            let end = tail
                .char_indices()
                .find_map(|(at, ch)| {
                    units += ch.len_utf16();
                    (units > 2048).then_some(at)
                })
                .unwrap_or(tail.len());
            let rest = &tail[..end];
            let (name, name_at, declared) = if let Some(declaration) = DECLARATION.captures(rest)? {
                let Some(name) = declaration.get(1).or_else(|| declaration.get(2)) else {
                    continue;
                };
                (name.as_str(), head.end() + name.start(), true)
            } else if let Some(binding) = BINDING.captures(rest)? {
                (binding.get(1).unwrap().as_str(), head.end(), false)
            } else {
                continue;
            };
            let Some(candidates) = named.get(name) else {
                continue;
            };
            let candidate = if declared {
                let from = lines.partition_point(|&line| line <= head.start()) as u32;
                let to = lines.partition_point(|&line| line <= name_at) as u32;
                candidates.iter().copied().find(|node| {
                    matches!(node.kind.as_str(), "function" | "class" | "component")
                        && node.start_line >= from
                        && node.start_line <= to
                })
            } else {
                candidates
                    .iter()
                    .copied()
                    .filter(|node| {
                        matches!(
                            node.kind.as_str(),
                            "function" | "class" | "component" | "constant" | "variable"
                        )
                    })
                    .min_by_key(|node| {
                        (
                            node.qualified_name != node.name,
                            node.start_line,
                            node.start_column,
                        )
                    })
            };
            if let Some(node) = candidate {
                return Ok(Self::Node(node.id.clone()));
            }
        }
        if let Some(common) = COMMON.captures(source)?
            && let Some(name) = common
                .get(2)
                .or_else(|| common.get(3))
                .or_else(|| common.get(4))
        {
            return Ok(Self::CommonJs(name.as_str().into()));
        }
        Ok(Self::Unstated)
    }
    pub(super) fn matches(&self, node: &Node) -> bool {
        match self {
            Self::DeclarationFile => true,
            Self::Component => node.kind == "component",
            Self::Node(id) => node.id == *id,
            Self::CommonJs(name) => node.name == *name,
            Self::Unstated => false,
        }
    }
}

#[derive(Clone, Copy)]
enum Open {
    Text(usize),
    Braces(usize),
}
fn template_text(source: &str) -> Option<Vec<Range<usize>>> {
    let bytes = source.as_bytes();
    let mut spans = Vec::new();
    let mut open = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        let top = open.last().copied();
        if let Some(Open::Text(start)) = top {
            if ch == b'\\' {
                i += 1;
            } else if ch == b'`' {
                spans.push(start..i);
                open.pop();
            } else if ch == b'$' && bytes.get(i + 1) == Some(&b'{') {
                spans.push(start..i);
                open.push(Open::Braces(0));
                i += 1;
            }
            i += 1;
            continue;
        }
        if ch == b'`' {
            open.push(Open::Text(i + 1));
        } else if matches!(ch, b'"' | b'\'') {
            i += 1;
            while i < bytes.len() && bytes[i] != ch && bytes[i] != b'\n' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
        } else if ch == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if ch == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = bytes[i + 2..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(bytes.len(), |close| i + close + 3);
        } else if ch == b'/' && slash_opens_regex(bytes, i) {
            let mut in_class = false;
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'\n' {
                if bytes[j] == b'\\' {
                    j += 1;
                } else if bytes[j] == b'[' {
                    in_class = true;
                } else if bytes[j] == b']' {
                    in_class = false;
                } else if bytes[j] == b'/' && !in_class {
                    i = j;
                    break;
                }
                j += 1;
            }
        } else if let Some(Open::Braces(braces)) = top {
            if ch == b'{' {
                *open.last_mut().unwrap() = Open::Braces(braces + 1);
            } else if ch == b'}' {
                if braces == 0 {
                    open.pop();
                    *open.last_mut().unwrap() = Open::Text(i + 1);
                } else {
                    *open.last_mut().unwrap() = Open::Braces(braces - 1);
                }
            }
        }
        i += 1;
    }
    open.is_empty().then_some(spans)
}
fn slash_opens_regex(bytes: &[u8], at: usize) -> bool {
    let mut end = at;
    while end > 0 && matches!(bytes[end - 1], b' ' | b'\t' | b'\n' | b'\r') {
        end -= 1;
    }
    if end == 0 {
        return true;
    }
    let before = bytes[end - 1];
    if matches!(before, b')' | b']' | b'"' | b'\'' | b'`') {
        return false;
    }
    let word = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$');
    if !word(before) {
        return true;
    }
    let start = end.saturating_sub(11);
    [
        "return",
        "throw",
        "case",
        "yield",
        "await",
        "else",
        "do",
        "typeof",
        "void",
        "delete",
        "new",
        "in",
        "of",
        "instanceof",
    ]
    .iter()
    .any(|keyword| {
        let length = keyword.len();
        end - start >= length
            && &bytes[end - length..end] == keyword.as_bytes()
            && (end - length == start || !word(bytes[end - length - 1]))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stated_defaults_and_nested_template_text_match_reference() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/workspace/defaults.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let nodes: Vec<Node> = serde_json::from_value(case["nodes"].clone()).unwrap();
            let proof = DefaultExport::read(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
                &nodes.iter().collect::<Vec<_>>(),
            )
            .unwrap();
            for (node, expected) in nodes.iter().zip(case["expected"].as_array().unwrap()) {
                assert_eq!(
                    proof.matches(node),
                    expected["stated"].as_bool().unwrap(),
                    "{} {}: {}",
                    case["path"],
                    node.name,
                    case["source"]
                );
            }
        }
        for case in corpus["spans"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let spans = template_text(source);
            let Some(expected) = case["expected"].as_array() else {
                assert!(spans.is_none(), "{source}");
                continue;
            };
            let spans = spans.unwrap();
            let mut offset = 0;
            for (byte, ch) in source.char_indices() {
                assert_eq!(
                    spans.iter().any(|span| span.contains(&byte)),
                    expected[offset].as_bool().unwrap(),
                    "{source} offset={offset}"
                );
                offset += ch.len_utf16();
            }
        }
    }
}
