//! Balanced argument lists used by the reference overload selector.
use super::{Context, regex};
use anyhow::Result;

fn quoted_end(text: &str, open: usize) -> usize {
    let bytes = text.as_bytes();
    let quote = bytes[open];
    let mut end = open + 1;
    while end < bytes.len() && bytes[end] != quote {
        end += if bytes[end] == b'\\' { 2 } else { 1 };
    }
    (end + 1).min(bytes.len())
}
fn template_end(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if open == 0
        || !bytes[open - 1].is_ascii_alphanumeric() && bytes[open - 1] != b'_'
        || matches!(bytes.get(open + 1), Some(b'<' | b'='))
    {
        return None;
    }
    let mut angles = 0;
    let mut others = 0;
    let mut at = open;
    while at < bytes.len() {
        match bytes[at] {
            b'"' | b'\'' => {
                at = quoted_end(text, at);
                continue;
            }
            b'(' | b'[' | b'{' => others += 1,
            b')' | b']' | b'}' => {
                others -= 1;
                if others < 0 {
                    return None;
                }
            }
            _ if others > 0 => {}
            b'<' => angles += 1,
            b'>' if bytes.get(at.wrapping_sub(1)) != Some(&b'-') => {
                angles -= 1;
                if angles == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}
pub(super) fn split(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut closes = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' | b'\'' => {
                at = quoted_end(text, at);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = (depth - 1).max(0),
            b'<' => {
                if let Some(end) = template_end(text, at) {
                    closes.push(end)
                }
            }
            b'>' if closes.last() == Some(&at) => {
                closes.pop();
            }
            _ => {}
        }
        if bytes[at] == b',' && depth == 0 && closes.is_empty() {
            out.push(text[start..at].trim_matches(crate::search::query_whitespace));
            start = at + 1;
        }
        at += 1;
    }
    let last = text[start..].trim_matches(crate::search::query_whitespace);
    if !last.is_empty() {
        out.push(last)
    }
    out
}
pub(super) fn list(
    ctx: &Context<'_>,
    file: &str,
    line: u32,
    column: u32,
    name: &str,
) -> Result<Option<String>> {
    let text = ctx
        .lines(file)
        .iter()
        .skip(line.saturating_sub(1) as usize)
        .take(12)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    ctx.step(text.len())?;
    let column = byte(&text, column as usize);
    let rest = &text[column..];
    let pattern = regex(&format!(
        r"\b{}\s*(?:<[^<>()]*>)?\s*\(",
        regex::escape(name)
    ))?;
    let Some(m) = pattern.find(rest)? else {
        return Ok(None);
    };
    let open = column + m.end() - 1;
    let mut depth = 0;
    for (at, ch) in text[open..].char_indices() {
        if ch == '(' {
            depth += 1
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                return Ok(Some(text[open + 1..open + at].into()));
            }
        }
    }
    Ok(None)
}
pub(super) fn arity(
    ctx: &Context<'_>,
    node: &prometeu_extract::Node,
) -> Result<Option<(usize, usize)>> {
    let Some(list) = list(ctx, &node.file_path, node.start_line, 0, &node.name)? else {
        return Ok(None);
    };
    let params = split(&list)
        .into_iter()
        .filter(|p| !p.is_empty() && *p != "void")
        .collect::<Vec<_>>();
    let generic = regex(r"<[^<>]*>")?;
    let variadic = regex(r"\.\.\.|\bparams\s|\bvararg\s")?;
    let mut min = 0;
    let mut pack = false;
    for param in &params {
        let is_pack = variadic.is_match(&generic.replace_all(param, ""))?;
        pack |= is_pack;
        if !param.contains('=') && !is_pack {
            min += 1
        }
    }
    Ok(Some((min, if pack { usize::MAX } else { params.len() })))
}

pub(super) fn byte(text: &str, units: usize) -> usize {
    let mut counted = 0;
    for (at, ch) in text.char_indices() {
        if counted >= units {
            return at;
        }
        counted += ch.len_utf16();
    }
    text.len()
}
