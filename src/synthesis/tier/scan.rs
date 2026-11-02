//! Bounded source readers shared by HTTP, queue, event, and test-request channels.
use super::{Context, Result, regex};
use crate::frameworks::frontend::href::{read_string, string_end};

pub(super) fn trim(source: &str) -> &str {
    source.trim_matches(crate::search::query_whitespace)
}
pub(super) fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|ch| !crate::search::query_whitespace(*ch))
        .collect()
}
pub(super) fn prefix(source: &str, units: usize) -> &str {
    let mut length = 0;
    for (at, ch) in source.char_indices() {
        if length + ch.len_utf16() > units {
            return &source[..at];
        }
        length += ch.len_utf16();
    }
    source
}
pub(super) fn close_paren(ctx: &Context<'_>, source: &str, open: usize) -> Result<Option<usize>> {
    let bytes = source.as_bytes();
    let mut at = open;
    let mut depth = 0i32;
    while at < bytes.len() {
        ctx.step()?;
        match bytes[at] {
            b'\'' | b'"' | b'`' => {
                let end = string_end(source, at)?;
                ctx.steps(end.saturating_sub(at))?;
                at = end;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(at));
                }
            }
            _ => {}
        }
        at += 1;
    }
    Ok(None)
}
pub(super) fn arguments<'a>(
    ctx: &Context<'_>,
    source: &'a str,
    open: usize,
) -> Result<Option<Vec<&'a str>>> {
    let Some(close) = close_paren(ctx, source, open)? else {
        return Ok(None);
    };
    let inner = &source[open + 1..close];
    let bytes = inner.as_bytes();
    let mut at = 0;
    let mut start = 0;
    let mut depth = 0i32;
    let mut out = Vec::new();
    while at < bytes.len() {
        ctx.step()?;
        match bytes[at] {
            b'\'' | b'"' | b'`' => {
                let end = string_end(inner, at)?;
                ctx.steps(end.saturating_sub(at))?;
                at = end;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                out.push(trim(&inner[start..at]));
                start = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    out.push(trim(&inner[start..]));
    Ok(Some(
        out.into_iter()
            .enumerate()
            .filter_map(|(i, arg)| (!arg.is_empty() || i == 0).then_some(arg))
            .collect(),
    ))
}
pub(super) fn leading(ctx: &Context<'_>, source: &str) -> Result<Option<String>> {
    let mut text = trim(source);
    for _ in 0..256 {
        text = text.strip_prefix('(').map_or(text, |text| {
            text.trim_start_matches(crate::search::query_whitespace)
        });
        if let Some(found) = regex(r"^new\s+URL\s*\(")?.find(text)? {
            let Some(args) = arguments(ctx, text, found.end() - 1)? else {
                return Ok(None);
            };
            let Some(arg) = args.first().filter(|arg| !arg.is_empty()) else {
                return Ok(None);
            };
            text = trim(arg);
        } else {
            return read_string(text, 0);
        }
    }
    anyhow::bail!("HTTP URL nesting exceeds limit")
}
pub(super) fn method(text: &str) -> Result<Option<String>> {
    Ok(regex(r#"\bmethod\s*:\s*(['"`])([A-Za-z]+)\1"#)?
        .captures(text)?
        .map(|found| found[2].to_uppercase()))
}
pub(super) fn literals(text: &str) -> Result<Vec<String>> {
    regex(r#"(['"`])([^'"`]+)\1"#)?
        .captures_iter(text)
        .map(|found| Ok(found?[2].to_owned()))
        .collect()
}
pub(super) struct Decorator<'a> {
    pub args: &'a str,
    pub start: usize,
    pub end: usize,
}
pub(super) fn decorators<'a>(
    ctx: &Context<'_>,
    source: &'a str,
    name: &str,
) -> Result<Vec<Decorator<'a>>> {
    let pattern = regex(&format!(r"@{name}\s*\("))?;
    let mut at = 0;
    let mut out = Vec::new();
    while let Some(found) = pattern.find_from_pos(source, at)? {
        let Some(close) = close_paren(ctx, source, found.end() - 1)? else {
            at = found.end();
            continue;
        };
        out.push(Decorator {
            args: &source[found.end()..close],
            start: found.start(),
            end: close + 1,
        });
        at = close + 1;
    }
    Ok(out)
}
pub(super) fn method_after(
    ctx: &Context<'_>,
    source: &str,
    mut at: usize,
) -> Result<Option<(String, usize)>> {
    loop {
        at = source.len()
            - source[at..]
                .trim_start_matches(crate::search::query_whitespace)
                .len();
        let Some(found) = regex(r"^@[\w.]+")?.find(&source[at..])? else {
            break;
        };
        at += found.end();
        at = source.len()
            - source[at..]
                .trim_start_matches(crate::search::query_whitespace)
                .len();
        if source.as_bytes().get(at) == Some(&b'(') {
            let Some(end) = close_paren(ctx, source, at)? else {
                return Ok(None);
            };
            at = end + 1;
        }
    }
    loop {
        at = source.len()
            - source[at..]
                .trim_start_matches(crate::search::query_whitespace)
                .len();
        let Some(found) = regex(r"^(?:public|private|protected|async|static|readonly|override)\b")?
            .find(&source[at..])?
        else {
            break;
        };
        at += found.end();
    }
    Ok(regex(r"^([A-Za-z_$][\w$]*)\s*[<(]")?
        .captures(&source[at..])?
        .map(|found| (found[1].to_owned(), at)))
}
