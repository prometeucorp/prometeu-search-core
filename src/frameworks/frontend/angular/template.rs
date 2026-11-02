//! Angular expression and microsyntax readers, preserving byte offsets.
use super::{object, regex};
use anyhow::{Result, ensure};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub(crate) struct Part {
    pub at: usize,
    pub text: String,
}
#[derive(Debug, Serialize)]
pub(crate) struct Microsyntax {
    pub expressions: Vec<Part>,
    pub locals: Vec<String>,
}
#[derive(Debug, Serialize)]
pub(crate) struct MemberUse {
    pub name: String,
    pub at: usize,
    pub call: bool,
    pub via: String,
}
struct Expression {
    via: String,
    at: usize,
    text: String,
    scope: usize,
    sees_own: bool,
}
struct Local {
    name: String,
    from: usize,
    scope: Option<usize>,
}
fn blank_strings(source: &str, depth: usize) -> Result<String> {
    ensure!(
        depth < 256,
        "Angular template string nesting exceeds 256 levels"
    );
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let close = object::skip_string(source, i)?.map_or(bytes.len(), |end| end + 1);
                out.resize(out.len() + close - i, b' ');
                i = close;
            }
            b'`' => {
                out.push(b' ');
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'`' {
                        out.push(b' ');
                        i += 1;
                        break;
                    }
                    let close = if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
                        object::matching(source, i + 1)?
                    } else {
                        None
                    };
                    if let Some(close) = close {
                        out.extend_from_slice(b"  ");
                        out.extend_from_slice(
                            blank_strings(&source[i + 2..close], depth + 1)?.as_bytes(),
                        );
                        out.push(b' ');
                        i = close + 1;
                        continue;
                    }
                    let step = if bytes[i] == b'\\' {
                        2.min(bytes.len() - i)
                    } else {
                        1
                    };
                    out.resize(out.len() + step, b' ');
                    i += step;
                }
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    Ok(String::from_utf8(out)?)
}
fn expression_uses(source: &str) -> Result<Vec<(String, usize, bool)>> {
    let text = blank_strings(source, 0)?;
    let mut out = Vec::new();
    for capture in regex(r"(?<![\w$.])(?:this\s*\.\s*)?([A-Za-z_$][\w$]*)")?.captures_iter(&text) {
        let capture = capture?;
        let whole = capture.get(0).unwrap();
        let before = text[..whole.start()].trim_end_matches(crate::search::query_whitespace);
        let after = text[whole.end()..].trim_start_matches(crate::search::query_whitespace);
        let prev = before.chars().next_back();
        let next = after.chars().next();
        if prev == Some('.')
            || (prev == Some('|') && !before.ends_with("||"))
            || (matches!(prev, Some('{') | Some(',')) && next == Some(':'))
        {
            continue;
        }
        let name = capture.get(1).unwrap();
        out.push((name.as_str().into(), name.start(), next == Some('(')));
    }
    Ok(out)
}
fn whitespace(text: &str, at: usize) -> bool {
    text.get(at..)
        .and_then(|tail| tail.chars().next())
        .is_some_and(crate::search::query_whitespace)
}
fn skip(text: &str, at: &mut usize, separators: bool) {
    while *at < text.len() {
        let ch = text[*at..].chars().next().unwrap();
        if !crate::search::query_whitespace(ch) && !(separators && matches!(ch, ';' | ',')) {
            break;
        }
        *at += ch.len_utf8();
    }
}
fn word(text: &str, at: &mut usize) -> Result<Option<String>> {
    let found = regex(r"^[A-Za-z_$][\w$]*")?.find(&text[*at..])?;
    if let Some(found) = found {
        *at += found.end();
        Ok(Some(found.as_str().into()))
    } else {
        Ok(None)
    }
}
fn expression_end(text: &str, from: usize) -> Result<usize> {
    let mut depth = 0i32;
    let mut i = from;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            let Some(end) = object::skip_string(text, i)? else {
                return Ok(text.len());
            };
            i = end + 1;
            continue;
        }
        if matches!(ch, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(ch, b')' | b']' | b'}') {
            depth -= 1;
        } else if depth == 0
            && (matches!(ch, b';' | b',')
                || (text[..i]
                    .chars()
                    .next_back()
                    .is_some_and(crate::search::query_whitespace)
                    && regex(r"^(?:as|let)(?![\w$])")?.is_match(object::prefix(&text[i..], 4))?))
        {
            return Ok(i);
        }
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    Ok(text.len())
}
fn top_level_parts(text: &str) -> Result<Vec<Part>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut i = 0;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            let Some(end) = object::skip_string(text, i)? else {
                break;
            };
            i = end + 1;
            continue;
        }
        if matches!(ch, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(ch, b')' | b']' | b'}') {
            depth -= 1;
        } else if ch == b';' && depth == 0 {
            parts.push(Part {
                at: start,
                text: text[start..i].into(),
            });
            start = i + 1;
        }
        i += 1;
    }
    parts.push(Part {
        at: start,
        text: text[start..].into(),
    });
    Ok(parts)
}
pub(crate) fn microsyntax(text: &str) -> Result<Microsyntax> {
    let mut expressions = Vec::new();
    let mut locals = Vec::new();
    let mut i = 0;
    let mut first = true;
    while i < text.len() {
        skip(text, &mut i, true);
        if i >= text.len() {
            break;
        }
        let start = i;
        let key = word(text, &mut i)?;
        if key.as_deref() == Some("let") && whitespace(text, i) {
            skip(text, &mut i, false);
            if let Some(local) = word(text, &mut i)? {
                locals.push(local);
            }
            skip(text, &mut i, false);
            if text.as_bytes().get(i) == Some(&b'=') {
                i += 1;
                skip(text, &mut i, false);
                word(text, &mut i)?;
            }
            first = false;
            continue;
        }
        if first || key.is_none() {
            i = start;
        } else {
            skip(text, &mut i, false);
            if text.as_bytes().get(i) == Some(&b':') {
                i += 1;
            }
            skip(text, &mut i, false);
        }
        let end = expression_end(text, i)?;
        if end > i {
            expressions.push(Part {
                at: i,
                text: text[i..end].into(),
            });
        }
        i = end;
        skip(text, &mut i, false);
        if regex(r"^as(?![\w$])")?.is_match(object::prefix(&text[i..], 3))? {
            i += 2;
            skip(text, &mut i, false);
            if let Some(alias) = word(text, &mut i)? {
                locals.push(alias);
            }
        }
        if i == start {
            i += text[i..].chars().next().unwrap().len_utf8();
        }
        first = false;
    }
    Ok(Microsyntax {
        expressions,
        locals,
    })
}
fn statement_end(text: &str, from: usize) -> Result<Option<usize>> {
    let mut depth = 0i32;
    let mut i = from;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            let Some(end) = object::skip_string(text, i)? else {
                return Ok(None);
            };
            i = end + 1;
            continue;
        }
        if matches!(ch, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(ch, b')' | b']' | b'}') {
            depth -= 1;
            if depth < 0 {
                return Ok(None);
            }
        } else if ch == b';' && depth == 0 {
            return Ok(Some(i));
        }
        i += 1;
    }
    Ok(None)
}
pub(crate) fn member_uses(template: &str) -> Result<Vec<MemberUse>> {
    let text = regex(r"<!--[\s\S]*?-->")?
        .replace_all(template, |capture: &fancy_regex::Captures<'_>| {
            capture[0]
                .as_bytes()
                .iter()
                .map(|byte| if *byte == b'\n' { '\n' } else { ' ' })
                .collect::<String>()
        })
        .into_owned();
    let mut expressions = Vec::new();
    let mut locals = Vec::new();
    let mut scope = 0;
    for capture in
        regex(r#"\[(\([A-Za-z][\w.-]*\)|[A-Za-z@][\w.@-]*)\]\s*=\s*(?:"([^"]*)"|'([^']*)')"#)?
            .captures_iter(&text)
    {
        let capture = capture?;
        let value = capture.get(2).or_else(|| capture.get(3)).unwrap();
        scope += 1;
        expressions.push(Expression {
            via: format!("[{}]", &capture[1]),
            at: value.start(),
            text: value.as_str().into(),
            scope,
            sees_own: false,
        });
    }
    for capture in regex(r#"(?<![\w$*-])\*([A-Za-z][\w.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)?
        .captures_iter(&text)
    {
        let capture = capture?;
        let value = capture.get(2).or_else(|| capture.get(3)).unwrap();
        scope += 1;
        let parsed = microsyntax(value.as_str())?;
        for part in parsed.expressions {
            expressions.push(Expression {
                via: format!("*{}", &capture[1]),
                at: value.start() + part.at,
                text: part.text,
                scope,
                sees_own: false,
            });
        }
        let from = text[..capture.get(0).unwrap().start() + 1]
            .rfind('<')
            .unwrap_or(0);
        for name in parsed.locals {
            locals.push(Local {
                name,
                from,
                scope: Some(scope),
            });
        }
    }
    for capture in regex(r"\{\{([\s\S]*?)\}\}")?.captures_iter(&text) {
        let capture = capture?;
        let value = capture.get(1).unwrap();
        scope += 1;
        expressions.push(Expression {
            via: "{{ }}".into(),
            at: value.start(),
            text: value.as_str().into(),
            scope,
            sees_own: false,
        });
    }
    for capture in
        regex(r"(?<![\w$@])@(if|else\s+if|for|switch|case|defer)\s*\(")?.captures_iter(&text)
    {
        let capture = capture?;
        let whole = capture.get(0).unwrap();
        let open = whole.end() - 1;
        let Some(close) = object::matching(&text, open)? else {
            continue;
        };
        let kind = regex(r"\s+")?.replace_all(&capture[1], " ").into_owned();
        let via = format!("@{kind}");
        scope += 1;
        let parts = top_level_parts(&text[open + 1..close])?
            .into_iter()
            .map(|part| Part {
                at: open + 1 + part.at,
                text: part.text,
            })
            .collect::<Vec<_>>();
        let mut push = |part: &Part, offset: usize, sees_own: bool| {
            expressions.push(Expression {
                via: via.clone(),
                at: part.at + offset,
                text: part.text[offset..].into(),
                scope,
                sees_own,
            })
        };
        match kind.as_str() {
            "switch" | "case" => {
                for part in &parts {
                    push(part, 0, false);
                }
            }
            "defer" => {
                for part in &parts {
                    if let Some(when) =
                        regex(r"^\s*(?:(?:prefetch|hydrate)\s+)?when(?![\w$])")?.find(&part.text)?
                    {
                        push(part, when.end(), false);
                    }
                }
            }
            "for" => {
                if let Some(head) = parts.first()
                    && let Some(loop_) =
                        regex(r"^(\s*)([A-Za-z_$][\w$]*)\s+of(?![\w$])")?.captures(&head.text)?
                {
                    locals.push(Local {
                        name: loop_[2].into(),
                        from: whole.start(),
                        scope: Some(scope),
                    });
                    push(head, loop_.get(0).unwrap().end(), false);
                }
                for part in parts.iter().skip(1) {
                    if let Some(track) = regex(r"^\s*track(?![\w$])")?.find(&part.text)? {
                        push(part, track.end(), true);
                        continue;
                    }
                    if regex(r"^\s*let(?![\w$])")?.is_match(&part.text)? {
                        for alias in
                            regex(r"([A-Za-z_$][\w$]*)\s*=\s*\$?[\w$]+")?.captures_iter(&part.text)
                        {
                            let alias = alias?;
                            locals.push(Local {
                                name: alias[1].into(),
                                from: whole.start(),
                                scope: Some(scope),
                            });
                        }
                    }
                }
            }
            _ => {
                if let Some(head) = parts.first() {
                    push(head, 0, false);
                }
                for part in parts.iter().skip(1) {
                    if let Some(alias) =
                        regex(r"^\s*as\s+([A-Za-z_$][\w$]*)")?.captures(&part.text)?
                    {
                        locals.push(Local {
                            name: alias[1].into(),
                            from: whole.start(),
                            scope: Some(scope),
                        });
                    }
                }
            }
        }
    }
    for capture in regex(r"(?<![\w$@])@let\s+([A-Za-z_$][\w$]*)\s*=")?.captures_iter(&text) {
        let capture = capture?;
        let whole = capture.get(0).unwrap();
        let at = whole.end();
        let Some(end) = statement_end(&text, at)? else {
            continue;
        };
        scope += 1;
        expressions.push(Expression {
            via: "@let".into(),
            at,
            text: text[at..end].into(),
            scope,
            sees_own: false,
        });
        locals.push(Local {
            name: capture[1].into(),
            from: whole.start(),
            scope: Some(scope),
        });
    }
    for capture in regex(r"(?<=\s)(?:#|ref-)([A-Za-z_$][\w$]*)(?=[\s=/>])")?.captures_iter(&text) {
        let capture = capture?;
        locals.push(Local {
            name: capture[1].into(),
            from: 0,
            scope: None,
        });
    }
    for capture in regex(r"(?<=\s)let-([A-Za-z_$][\w$]*)(?=[\s=/>])")?.captures_iter(&text) {
        let capture = capture?;
        let from = text[..capture.get(0).unwrap().start() + 1]
            .rfind('<')
            .unwrap_or(0);
        locals.push(Local {
            name: capture[1].into(),
            from,
            scope: None,
        });
    }
    let mut uses = Vec::new();
    for expression in expressions {
        for (name, offset, call) in expression_uses(&expression.text)? {
            let at = expression.at + offset;
            if !locals.iter().any(|local| {
                local.name == name
                    && local.from <= at
                    && (local.scope != Some(expression.scope) || expression.sees_own)
            }) {
                uses.push(MemberUse {
                    name,
                    at,
                    call,
                    via: expression.via.clone(),
                });
            }
        }
    }
    uses.sort_by_key(|u| u.at);
    Ok(uses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn template_members_and_microsyntax_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/upstream/angular/cases.json"
        ))
        .unwrap();
        for case in corpus["templates"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            let mut actual = member_uses(text).unwrap();
            for use_ in &mut actual {
                use_.at = text[..use_.at].encode_utf16().count();
            }
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "{text}"
            );
        }
        for case in corpus["microsyntax"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            let mut actual = microsyntax(text).unwrap();
            for part in &mut actual.expressions {
                part.at = text[..part.at].encode_utf16().count();
            }
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "{text}"
            );
        }
    }
}
