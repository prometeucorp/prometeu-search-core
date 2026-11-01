//! Vue Options API member discovery, preserving the upstream scanner's spans.
use super::super::preprocess::{captures, test};
use anyhow::{Result, ensure};

const COMPONENT_FUNCTIONS: &[&str] = &[
    "data",
    "setup",
    "render",
    "beforeCreate",
    "created",
    "beforeMount",
    "mounted",
    "beforeUpdate",
    "updated",
    "activated",
    "deactivated",
    "beforeDestroy",
    "destroyed",
    "beforeUnmount",
    "unmounted",
    "errorCaptured",
    "renderTracked",
    "renderTriggered",
    "serverPrefetch",
    "asyncData",
    "fetch",
    "head",
    "validate",
    "middleware",
];

pub(super) struct Member {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

struct RawMember {
    member: Member,
    value_at: usize,
    shorthand: bool,
}

fn skip_quoted(source: &str, at: usize, nesting: usize) -> Result<Option<usize>> {
    ensure!(nesting < 256, "Vue options nesting exceeds 256 levels");
    let bytes = source.as_bytes();
    let quote = bytes[at];
    let mut i = at + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if quote == b'`' && bytes[i..].starts_with(b"${") {
            let Some(close) = match_close(source, i + 1, nesting + 1)? else {
                return Ok(None);
            };
            i = close + 1;
            continue;
        }
        if bytes[i] == quote {
            return Ok(Some(i + 1));
        }
        i += 1;
    }
    Ok(None)
}

fn match_close(source: &str, open: usize, nesting: usize) -> Result<Option<usize>> {
    ensure!(nesting < 256, "Vue options nesting exceeds 256 levels");
    let bytes = source.as_bytes();
    let mut depth = 0i64;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' | b'`' => {
                let Some(end) = skip_quoted(source, i, nesting + 1)? else {
                    return Ok(None);
                };
                i = end;
                continue;
            }
            b'/' if bytes[i..].starts_with(b"//") => {
                i = source[i..]
                    .find('\n')
                    .map_or(source.len(), |end| i + end + 1);
                continue;
            }
            b'/' if bytes[i..].starts_with(b"/*") => {
                i = source[i + 2..]
                    .find("*/")
                    .map_or(source.len(), |end| i + end + 4);
                continue;
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    Ok(None)
}

fn object_members(source: &str, open: usize) -> Result<Vec<RawMember>> {
    let Some(close) = match_close(source, open, 0)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut i = open + 1;
    while i < close {
        if let Some(ws) =
            captures(r"^(?:\s|,|//[^\n]*|/\*[\s\S]*?\*/)+", &source[i..close])?.first()
        {
            i += ws.get(0).unwrap().end();
            continue;
        }
        let heads = captures(
            r#"^(?:(?:async|get|set)\s+(?=[\w$'"[]))?\*?\s*(?:([A-Za-z_$][\w$]*)|(['"])((?:(?!\2).)*)\2|(\[)|(\.\.\.))"#,
            &source[i..close],
        )?;
        let Some(head) = heads.first() else {
            i += source[i..].chars().next().unwrap().len_utf8();
            continue;
        };
        let name = head
            .get(1)
            .or_else(|| head.get(3))
            .map(|m| m.as_str().to_owned());
        let mut j = i + head.get(0).unwrap().end();
        // Retain the pinned scanner's capture index, including its spread handling.
        if head.get(5).is_some() {
            let Some(end) = match_close(source, j - 1, 0)? else {
                return Ok(out);
            };
            j = end + 1;
        }
        let mut k = j;
        while k < close {
            let bytes = source.as_bytes();
            match bytes[k] {
                b'"' | b'\'' | b'`' => {
                    let Some(end) = skip_quoted(source, k, 0)? else {
                        return Ok(out);
                    };
                    k = end;
                    continue;
                }
                b'/' if bytes[k..].starts_with(b"//") => {
                    k = source[k..].find('\n').map_or(close, |end| k + end + 1);
                    continue;
                }
                b'/' if bytes[k..].starts_with(b"/*") => {
                    k = source[k + 2..].find("*/").map_or(close, |end| k + end + 4);
                    continue;
                }
                b'{' | b'[' | b'(' => {
                    let Some(end) = match_close(source, k, 0)? else {
                        return Ok(out);
                    };
                    k = end + 1;
                    continue;
                }
                b',' => break,
                _ => k += 1,
            }
        }
        let rest = &source[j..k];
        let colon = captures(r"^\s*:", rest)?
            .first()
            .map(|m| m.get(0).unwrap().end());
        if let Some(name) = name {
            out.push(RawMember {
                member: Member {
                    name,
                    start: i,
                    end: k,
                },
                value_at: j + colon.unwrap_or(0),
                shorthand: colon.is_none() && test(r"^\s*\(", rest)?,
            });
        }
        i = k + 1;
    }
    Ok(out)
}

fn is_function(source: &str, member: &RawMember) -> Result<bool> {
    let value = source[member.value_at..member.member.end].trim_start();
    Ok(member.shorthand
        || test(r"^(?:async\s+)?function\b", value)?
        || test(r"^(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*=>", value)?)
}

pub(super) fn members(source: &str, language: &str) -> Result<Vec<Member>> {
    let mut masked = source.as_bytes().to_vec();
    for (range, kind) in super::preprocess::javascript_non_code(source, language)? {
        if kind == "regex" {
            masked[range].fill(b' ');
        }
    }
    let masked = String::from_utf8(masked)?;
    let source = masked.as_str();
    let starts = captures(
        r#"\bexport\s+default\s+(?:(?:defineComponent|defineNuxtComponent|Vue\s*\.\s*extend)\s*\(\s*|Vue\s*\.\s*component\s*\(\s*(['"])[^'"]*\1\s*,\s*)?\{"#,
        source,
    )?;
    let Some(start) = starts.first() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for member in object_members(source, start.get(0).unwrap().end() - 1)? {
        if COMPONENT_FUNCTIONS.contains(&member.member.name.as_str())
            && is_function(source, &member)?
        {
            out.push(member.member);
            continue;
        }
        if !["methods", "computed", "watch"].contains(&member.member.name.as_str())
            || member.shorthand
        {
            continue;
        }
        let Some(offset) = source[member.value_at..].find('{') else {
            continue;
        };
        let open = member.value_at + offset;
        if open >= member.member.end || !source[member.value_at..open].trim().is_empty() {
            continue;
        }
        for entry in object_members(source, open)? {
            let value = source[entry.value_at..entry.member.end].trim_start();
            if is_function(source, &entry)?
                || member.member.name != "methods" && value.starts_with('{')
            {
                out.push(entry.member);
            }
        }
    }
    Ok(out)
}
