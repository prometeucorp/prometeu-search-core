use super::{reference, regex, scan, trim};
use anyhow::Result;
use prometeu_extract::{Node, UnresolvedReference, supplemental::Context};
use std::collections::HashSet;
use std::sync::LazyLock;

fn tail(expression: &str) -> Result<Option<String>> {
    let cleaned = regex(r"\s+")?.replace_all(expression, "");
    let cleaned = cleaned.strip_suffix("()").unwrap_or(&cleaned);
    Ok(regex(r"(?:\.|^)([A-Za-z_][A-Za-z0-9_]*)$")?
        .captures(cleaned)?
        .map(|capture| capture[1].into()))
}
fn calls(body: &str) -> Result<Vec<String>> {
    static RESERVED: LazyLock<HashSet<&str>> = LazyLock::new(|| {
        "json jsonp send sendStatus sendFile status end redirect render set get header type format attachment download cookie clearCookie append location vary links accepts is next then catch finally resolve reject all race map filter forEach reduce find push pop slice splice includes keys values entries assign parse stringify log error warn info String Number Boolean Array Object Date Math JSON Promise require fail".split(' ').collect()
    });
    static KEYWORDS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
        "if for while switch function async return typeof await new do else with yield void delete in super import".split(' ').collect()
    });
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for capture in
        regex(r"((?:[A-Za-z_$][\w$]*\s*\??\.\s*)*)([A-Za-z_$][\w$]*)\s*\(")?.captures_iter(body)
    {
        let capture = capture?;
        let method = &capture[2];
        let before = &body[..capture.get(0).unwrap().start()];
        let receiver = regex(r"\s|\?")?.replace_all(&capture[1], "");
        let receiver = receiver.strip_suffix('.').unwrap_or(&receiver);
        if receiver.is_empty() && regex(r"\.\s*$")?.is_match(before)? {
            continue;
        }
        if receiver.is_empty() && KEYWORDS.contains(method) {
            continue;
        }
        let prefix = scan::suffix(before, 12);
        if receiver.is_empty() && regex(r"\bfunction\s*\*?\s*$")?.is_match(prefix)? {
            continue;
        }
        let name = if receiver.is_empty() {
            method.into()
        } else {
            format!("{receiver}.{method}")
        };
        if RESERVED.contains(method) || !seen.insert(name.clone()) {
            continue;
        }
        out.push(name);
    }
    Ok(out)
}
fn replies(
    ctx: &Context<'_>,
    safe: &str,
    start: usize,
    end: usize,
    node: &Node,
) -> Result<Vec<UnresolvedReference>> {
    let mut out = Vec::new();
    for capture in regex(r"\b(res|response|reply|rep|ctx)\s*\.\s*(?:[A-Za-z_$][\w$]*\s*\([^()]*\)\s*\.\s*)*([A-Za-z_$][\w$]*)\s*\(")?.captures_iter(&safe[start..end]){
        let capture=capture?;let at=start+capture.get(0).unwrap().start();let mut raw=reference(node,&format!("{}.{}",&capture[1],&capture[2]),"calls");
        (raw.line,raw.column)=ctx.position(at);out.push(raw);
    }
    Ok(out)
}
pub(super) fn references(
    ctx: &Context<'_>,
    safe: &str,
    args_start: usize,
    args: &str,
    node: &Node,
) -> Result<Vec<UnresolvedReference>> {
    let mut parts = scan::nonempty_arguments(args)?;
    while parts.len() > 1
        && parts
            .last()
            .is_some_and(|part| trim(part.text).starts_with('{'))
    {
        parts.pop();
    }
    if let Some(last) = parts.last().copied() {
        let text = last
            .text
            .trim_start_matches(crate::search::query_whitespace);
        let array_at = last.text.len() - text.len();
        if text.starts_with('[') {
            let inner = scan::match_delim(last.text, array_at, b'[', b']')
                .filter(|&close| close > array_at)
                .map(|close| scan::nonempty_arguments(&last.text[array_at + 1..close]))
                .transpose()?
                .and_then(|parts| parts.last().copied());
            parts.pop();
            if let Some(inner) = inner {
                parts.push(scan::Argument {
                    text: inner.text,
                    start: last.start + array_at + 1 + inner.start,
                });
            }
        }
    }
    let Some(handler) = parts.last() else {
        return Ok(Vec::new());
    };
    let text = handler.text;
    let mut arrow = text.find("=>");
    let opening =
        regex(r"^\s*(?:[A-Za-z_$][\w$.]*\s*\(\s*)*(?:async\s+)?function\b[^(]*\(")?.find(text)?;
    let mut function = opening
        .as_ref()
        .and_then(|matched| matched.as_str().rfind("function"));
    if let Some(at) = function {
        let end = scan::function_body_end(text, at);
        let wrapped = text[..at].contains('(');
        let later = end
            .filter(|_| wrapped)
            .and_then(|end| text[end..].find("=>").map(|offset| end + offset));
        if later.is_some() {
            arrow = later;
            function = None;
        } else if let Some(end) = end.filter(|_| wrapped)
            && !regex(r"^[\s)]*$")?.is_match(&text[end + 1..])?
        {
            arrow = None;
            function = None;
        } else {
            arrow = None;
        }
    }
    if arrow.is_none() && function.is_none() {
        let name = tail(trim(text))?.or(tail(trim(
            text.rsplit_once(',').map_or(text, |(_, tail)| tail),
        ))?);
        return Ok(name
            .map(|name| vec![reference(node, &name, "references")])
            .unwrap_or_default());
    }
    let body_from = if let Some(arrow) = arrow {
        arrow + 2
    } else {
        let at = function.unwrap();
        let open = at + text[at..].find('(').unwrap();
        scan::match_delim(text, open, b'(', b')')
            .filter(|&close| close > open)
            .map_or(text.len(), |close| close + 1)
    };
    let after = &text[body_from..];
    let mut body = after;
    let mut body_start = args_start + handler.start + body_from;
    if let Some(brace) = after.find('{')
        && trim(&after[..brace]).is_empty()
        && let Some(end) = scan::match_delim(after, brace, b'{', b'}').filter(|&end| end > brace)
    {
        body = &after[brace + 1..end];
        body_start += brace + 1;
    }
    let source = &safe[body_start..body_start + body.len()];
    let mut out = calls(source)?
        .into_iter()
        .map(|name| reference(node, &name, "calls"))
        .collect::<Vec<_>>();
    out.extend(replies(
        ctx,
        safe,
        body_start,
        body_start + body.len(),
        node,
    )?);
    Ok(out)
}
