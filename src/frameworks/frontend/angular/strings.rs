use super::{href, object, regex, segments, trim};
use anyhow::Result;

pub(crate) fn localize_default(text: &str) -> Result<Option<String>> {
    let Some(capture) = regex(r"^\$localize\s*`((?:[^`\\$]|\\.)*)`$")?.captures(trim(text))? else {
        return Ok(None);
    };
    let body = &capture[1];
    let meta = regex(r"^:[^:]*:")?.find(body)?;
    Ok(Some(body[meta.map_or(0, |m| m.end())..].into()))
}
pub(crate) fn static_string(text: &str) -> Result<Option<String>> {
    let mut parts = Vec::new();
    let mut rest = trim(text);
    while !rest.is_empty() {
        let mut part = None;
        let mut used = 0;
        if rest.starts_with(['\'', '"']) {
            part = href::read_string(rest, 0)?;
            used = if part.is_none() {
                0
            } else {
                object::skip_string(rest, 0)?.map_or(0, |end| end + 1)
            };
        } else if rest.starts_with("$localize") {
            if let Some(tick) = rest.find('`')
                && let Some(end) = object::skip_string(rest, tick)?.filter(|&end| end > 0)
            {
                part = localize_default(&rest[..end + 1])?;
                used = end + 1;
            }
        } else if rest.starts_with('`')
            && let Some(end) = object::skip_string(rest, 0)?.filter(|&end| end > 0)
        {
            let body = &rest[1..end];
            if !body.contains("${") {
                part = Some(body.into());
                used = end + 1;
            }
        }
        let Some(part) = part.filter(|_| used > 0) else {
            return Ok(None);
        };
        parts.push(part);
        rest = trim(&rest[used..]);
        if rest.is_empty() {
            break;
        }
        let Some(tail) = rest.strip_prefix('+') else {
            return Ok(None);
        };
        rest = trim(tail);
    }
    Ok((!parts.is_empty()).then(|| parts.join("")))
}
fn constant(text: &str) -> Result<bool> {
    regex(r"^[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)+$")?
        .is_match(text)
        .map_err(Into::into)
}
fn compact(text: &str) -> String {
    text.chars()
        .filter(|&ch| !crate::search::query_whitespace(ch))
        .collect()
}
pub(super) fn path_segments(text: &str) -> Result<Option<Vec<String>>> {
    let text = trim(text);
    if let Some(value) = static_string(text)? {
        return Ok((!value.contains("**")).then(|| segments(&value)));
    }
    if constant(text)? {
        return Ok(Some(vec![format!("{{{}}}", compact(text))]));
    }
    if text.starts_with('`') && object::skip_string(text, 0)? == Some(text.len() - 1) {
        let body = &text[1..text.len() - 1];
        let mut out = String::new();
        let mut at = 0;
        let mut okay = true;
        for capture in regex(r"\$\{([^}]*)\}")?.captures_iter(body) {
            let capture = capture?;
            let whole = capture.get(0).unwrap();
            out.push_str(&body[at..whole.start()]);
            if !constant(trim(&capture[1]))? {
                okay = false;
            }
            out.push_str(&format!("{{{}}}", compact(&capture[1])));
            at = whole.end();
        }
        out.push_str(&body[at..]);
        return Ok((okay && !out.contains("**")).then(|| segments(&out)));
    }
    if text.contains('+') {
        let mut joined = String::new();
        for part in split_plus(text)? {
            if let Some(value) = static_string(part)? {
                joined.push_str(&value);
            } else if constant(trim(part))? {
                joined.push_str(&format!("{{{}}}", compact(trim(part))));
            } else {
                return Ok(None);
            }
        }
        return Ok((!joined.contains("**")).then(|| segments(&joined)));
    }
    Ok(None)
}
fn split_plus(text: &str) -> Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            let Some(end) = object::skip_string(text, i)? else {
                return Ok(vec![text]);
            };
            i = end;
        } else if matches!(ch, b'(' | b'[' | b'{') {
            let Some(end) = object::matching(text, i)? else {
                return Ok(vec![text]);
            };
            i = end;
        } else if ch == b'+' {
            parts.push(&text[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    parts.push(&text[start..]);
    Ok(parts)
}
pub(crate) fn names_somewhere(href: Option<href::Href>) -> Option<href::Href> {
    href.filter(|href| {
        let segs = segments(&href.path);
        segs.is_empty() || segs.iter().any(|seg| !seg.contains(href::HOLE))
    })
}
pub(crate) fn commands_href(text: &str, from_root: bool) -> Result<Option<href::Href>> {
    let arr = trim(text);
    if !arr.starts_with('[') {
        return Ok(None);
    }
    let Some(close) = object::matching(arr, 0)? else {
        return Ok(None);
    };
    let mut parts = Vec::new();
    let mut i = 1;
    let mut start = 1;
    let mut flush = |element: &str| -> Result<()> {
        let element = trim(element);
        if element.is_empty() {
            return Ok(());
        }
        if let Some(literal) = static_string(element)? {
            parts.extend(segments(&literal));
        } else if !element.starts_with('{') {
            parts.push(href::HOLE.to_string());
        }
        Ok(())
    };
    while i < close {
        let ch = arr.as_bytes()[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            let Some(end) = object::skip_string(arr, i)? else {
                return Ok(None);
            };
            i = end + 1;
            continue;
        }
        if matches!(ch, b'[' | b'{' | b'(') {
            let Some(end) = object::matching(arr, i)? else {
                return Ok(None);
            };
            i = end + 1;
            continue;
        }
        if ch == b',' {
            flush(&arr[start..i])?;
            start = i + 1;
        }
        i += 1;
    }
    flush(&arr[start..close])?;
    let first = static_string(arr[1..close].split(',').next().unwrap_or(""))?;
    let Some(first) = first else { return Ok(None) };
    if !first.starts_with('/') && !(from_root && !first.starts_with('.')) {
        return Ok(None);
    }
    let path = format!(
        "/{}",
        parts
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    );
    let display = path.replace(href::HOLE, "${…}");
    Ok(names_somewhere(Some(href::Href {
        path,
        display,
        alternates: Vec::new(),
    })))
}
