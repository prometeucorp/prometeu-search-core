use super::*;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(super) enum Evidence {
    Runtime(Runtime),
    Type {
        #[serde(rename = "type")]
        name: String,
        #[serde(rename = "classLevel", skip_serializing_if = "Option::is_none")]
        class_level: Option<bool>,
    },
    Factory {
        factory: String,
        awaited: bool,
    },
    Arms {
        arms: Box<[Evidence; 2]>,
    },
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Runtime {
    Runtime,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Open {
    None,
    Continues,
    Broken,
}
#[derive(Debug, Serialize)]
pub(super) struct Expression {
    pub shape: Option<String>,
    pub open: Open,
}

fn runtime(name: &str) -> bool {
    matches!(
        name,
        "list"
            | "dict"
            | "set"
            | "frozenset"
            | "tuple"
            | "str"
            | "bytes"
            | "bytearray"
            | "int"
            | "float"
            | "complex"
            | "bool"
            | "object"
            | "type"
            | "range"
            | "slice"
            | "List"
            | "Dict"
            | "Set"
            | "FrozenSet"
            | "Tuple"
            | "Sequence"
            | "Mapping"
            | "MutableMapping"
            | "MutableSequence"
            | "Iterable"
            | "Iterator"
            | "Callable"
            | "deque"
            | "defaultdict"
            | "OrderedDict"
            | "Counter"
            | "collections.deque"
            | "collections.defaultdict"
            | "collections.OrderedDict"
            | "collections.Counter"
    )
}
fn unquote(text: &str) -> &str {
    let text = trim(text);
    let text = text.strip_prefix(['\'', '"']).unwrap_or(text);
    trim(text.strip_suffix(['\'', '"']).unwrap_or(text))
}
pub(super) fn annotation(raw: &str) -> Result<Option<Evidence>> {
    let mut text = unquote(raw).to_owned();
    for _ in 0..4 {
        if text.is_empty() {
            break;
        }
        let mut inner = None;
        for pattern in [
            r"^(?:typing\.)?Optional\s*\[\s*(.+)\s*\]$",
            r"^(?:typing\.)?Union\s*\[\s*([^,\]]+?)\s*,\s*None\s*\]$",
            r"^(.+?)\s*\|\s*None$",
            r"^None\s*\|\s*(.+)$",
        ] {
            if let Some(capture) = regex(pattern)?.captures(&text)? {
                inner = Some(unquote(&capture[1]).to_owned());
                break;
            }
        }
        let Some(inner) = inner else {
            break;
        };
        text = inner;
    }
    let head = trim(text.split('[').next().unwrap_or_default());
    let head = head.strip_prefix("typing.").unwrap_or(head);
    if runtime(head) {
        return Ok(Some(Evidence::Runtime(Runtime::Runtime)));
    }
    if text.contains('[') || !regex(r"^[A-Za-z_][\w.]*$")?.is_match(&text)? || head == "Any" {
        return Ok(None);
    }
    Ok(Some(Evidence::Type {
        name: text,
        class_level: None,
    }))
}
fn quote_end(raw: &str, start: usize) -> (usize, usize, bool) {
    let bytes = raw.as_bytes();
    let quote = bytes[start];
    let triple = bytes.get(start..start + 3) == Some(&[quote; 3]);
    let width = if triple { 3 } else { 1 };
    let mut end = start + width;
    while end < bytes.len() && bytes.get(end..end + width) != Some(&bytes[start..start + width]) {
        end += if bytes[end] == b'\\' { 2 } else { 1 };
    }
    (end, width, triple)
}
pub(super) fn expression(raw: &str) -> Result<Expression> {
    let mut out = String::new();
    let mut depth = 0i32;
    let mut at = 0;
    while at < raw.len() {
        let ch = raw[at..].chars().next().unwrap();
        match ch {
            '\'' | '"' => {
                let (end, width, triple) = quote_end(raw, at);
                if end >= raw.len() {
                    return Ok(Expression {
                        shape: None,
                        open: if triple {
                            Open::Continues
                        } else {
                            Open::Broken
                        },
                    });
                }
                if depth == 0 {
                    out = regex(r"[rbfuRBFU]{1,2}$")?
                        .try_replacen(&out, 1, "")?
                        .into_owned();
                    out.push('S');
                }
                at = end + width;
                continue;
            }
            '(' | '[' | '{' => {
                if depth == 0 {
                    out.push(ch);
                }
                depth += 1;
            }
            ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return Ok(Expression {
                        shape: None,
                        open: Open::Broken,
                    });
                }
                if depth == 0 {
                    out.push(ch);
                }
            }
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
        at += ch.len_utf8();
    }
    let out = regex(r"\s+")?.try_replacen(&out, 0, " ")?;
    let shape = trim(&out);
    Ok(if depth > 0 || shape.ends_with('\\') {
        Expression {
            shape: None,
            open: Open::Continues,
        }
    } else {
        Expression {
            shape: Some(shape.into()),
            open: Open::None,
        }
    })
}
pub(super) fn conditional(raw: &str) -> Option<[&str; 2]> {
    let mut depth = 0i32;
    let mut if_at = None;
    let mut at = 0;
    while at < raw.len() {
        let ch = raw[at..].chars().next().unwrap();
        match ch {
            '\'' | '"' => {
                let (end, width, _) = quote_end(raw, at);
                at = end + width;
                continue;
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if depth == 0
                && raw[..at]
                    .chars()
                    .next_back()
                    .is_some_and(crate::search::query_whitespace) =>
            {
                if if_at.is_none()
                    && raw[at..]
                        .strip_prefix("if")
                        .and_then(|tail| tail.chars().next())
                        .is_some_and(crate::search::query_whitespace)
                {
                    if_at = Some(at);
                } else if let Some(first) = if_at
                    && raw[at..]
                        .strip_prefix("else")
                        .and_then(|tail| tail.chars().next())
                        .is_some_and(crate::search::query_whitespace)
                {
                    return Some([trim(&raw[..first]), trim(&raw[at + 4..])]);
                }
            }
            _ => {}
        }
        at += ch.len_utf8();
    }
    None
}
pub(super) fn value(raw: &str, signature: Option<&str>) -> Result<Option<Evidence>> {
    value_at(raw, signature, 0)
}
fn value_at(raw: &str, signature: Option<&str>, depth: usize) -> Result<Option<Evidence>> {
    ensure!(
        depth < 128,
        "Python conditional evidence nesting limit exceeded"
    );
    let mut text = trim(raw);
    if let Some([left, right]) = conditional(text) {
        return Ok(
            match (
                value_at(left, signature, depth + 1)?,
                value_at(right, signature, depth + 1)?,
            ) {
                (Some(left), Some(right)) => Some(Evidence::Arms {
                    arms: Box::new([left, right]),
                }),
                _ => None,
            },
        );
    }
    let awaited = regex(r"^await\s")?.is_match(text)?;
    if awaited {
        text = text
            .strip_prefix("await")
            .unwrap()
            .trim_start_matches(crate::search::query_whitespace);
    }
    let Some(shape) = expression(text)?.shape else {
        return Ok(None);
    };
    if regex(r"^(?:S(?: S)*|-?\d[\w.]*|True|False|\[\]|\{\}|\(\))$")?.is_match(&shape)? {
        return Ok((!awaited).then_some(Evidence::Runtime(Runtime::Runtime)));
    }
    if let Some(capture) = regex(r"^([A-Za-z_][\w.]*) ?\(\)$")?.captures(&shape)? {
        let callee = &capture[1];
        if runtime(callee) {
            return Ok((!awaited).then_some(Evidence::Runtime(Runtime::Runtime)));
        }
        if callee
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .starts_with(|ch: char| ch.is_ascii_uppercase())
        {
            return Ok((!awaited).then(|| Evidence::Type {
                name: callee.into(),
                class_level: None,
            }));
        }
        return Ok(Some(Evidence::Factory {
            factory: callee.into(),
            awaited,
        }));
    }
    if !awaited
        && let Some(signature) = signature
        && regex(r"^[A-Za-z_]\w*$")?.is_match(text)?
        && let Some(capture) = regex(&format!(
            r"[(,]\s*\*{{0,2}}{}\s*:\s*([^,)=]+)",
            regex::escape(text)
        ))?
        .captures(signature)?
    {
        return annotation(&capture[1]);
    }
    Ok(None)
}
