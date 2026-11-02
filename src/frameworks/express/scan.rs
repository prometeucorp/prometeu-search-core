use super::{regex, trim};
use anyhow::Result;

pub(super) fn match_delim(source: &str, open: usize, left: u8, right: u8) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        let ch = bytes[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            i += 1;
            while i < bytes.len() && bytes[i] != ch {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if ch == left {
            depth += 1;
        } else if ch == right {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}
pub(super) fn prefix(source: &str, max: usize) -> &str {
    let mut count = 0;
    let end = source
        .char_indices()
        .find_map(|(at, ch)| {
            count += ch.len_utf16();
            (count > max).then_some(at)
        })
        .unwrap_or(source.len());
    &source[..end]
}
pub(super) fn suffix(source: &str, max: usize) -> &str {
    let mut count = 0;
    let start = source
        .char_indices()
        .rev()
        .find_map(|(at, ch)| {
            count += ch.len_utf16();
            (count > max).then_some(at + ch.len_utf8())
        })
        .unwrap_or(0);
    &source[start..]
}
fn closes_on_line(source: &str, at: usize) -> bool {
    let bytes = source.as_bytes();
    let quote = bytes[at];
    let mut i = at + 1;
    while i < bytes.len() && bytes[i] != b'\n' {
        if bytes[i] == b'\\' {
            i += 1;
        } else if bytes[i] == quote {
            return true;
        }
        i += 1;
    }
    false
}
fn regex_end(source: &str, at: usize) -> Result<Option<usize>> {
    let bytes = source.as_bytes();
    let mut p = at;
    while p > 0 && matches!(bytes[p - 1], b' ' | b'\t') {
        p -= 1;
    }
    if p > 0 && matches!(bytes[p - 1], b')' | b']' | b'}') {
        return Ok(None);
    }
    if p > 1
        && bytes[p - 1] == b'!'
        && (bytes[p - 2].is_ascii_alphanumeric()
            || matches!(bytes[p - 2], b'_' | b'$' | b')' | b']'))
    {
        return Ok(None);
    }
    if p > 1 && matches!(bytes[p - 1], b'+' | b'-') && bytes[p - 2] == bytes[p - 1] {
        return Ok(None);
    }
    if !regex(r"(?:^|[=(:,)!&|?;{}\[\]+*%~^<>-]|\b(?:return|throw|case|yield|await|else|do|typeof|void|delete|new|in|of|instanceof))\s*$")?.is_match(suffix(&source[..at],32))?{return Ok(None);}
    let mut end = at + 1;
    let mut class = false;
    while end < bytes.len() && bytes[end] != b'\n' {
        if bytes[end] == b'\\' {
            end += 2;
            continue;
        }
        if bytes[end] == b'[' {
            class = true;
        } else if bytes[end] == b']' {
            class = false;
        } else if bytes[end] == b'/' && !class {
            break;
        }
        end += 1;
    }
    if bytes.get(end) != Some(&b'/') {
        return Ok(None);
    }
    Ok(regex(r"^[dgimsuyv]*[ \t]*(?:\.\s*(?:test|exec|source|flags|global|lastIndex)\b|[,;)\]}:?]|&&|\|\||\r?\n|$)")?.is_match(prefix(&source[end+1..],23))?.then_some(end))
}
pub(super) fn call(source: &str, open: usize) -> Result<Option<(usize, String)>> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        let ch = bytes[i];
        out.push(ch);
        if matches!(ch, b'"' | b'\'') && !closes_on_line(source, i) {
            i += 1;
            continue;
        }
        if matches!(ch, b'"' | b'\'' | b'`') {
            i += 1;
            while i < bytes.len() && bytes[i] != ch {
                if ch != b'`' && bytes[i] == b'\n' {
                    break;
                }
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out.push(b' ');
                    i += 1;
                }
                out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                i += 1;
            }
            if let Some(&end) = bytes.get(i) {
                out.push(end);
            }
            i += 1;
            continue;
        }
        if ch == b'/'
            && !matches!(bytes.get(i + 1), Some(b'/') | Some(b'*'))
            && let Some(end) = regex_end(source, i)?
        {
            out.resize(out.len() + end - i - 1, b' ');
            out.push(b'/');
            i = end + 1;
            continue;
        }
        if ch == b'(' {
            depth += 1;
        } else if ch == b')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Ok(Some((i, String::from_utf8(out)?)));
            }
        }
        i += 1;
    }
    Ok(None)
}
#[derive(Clone, Copy)]
pub(super) struct Argument<'a> {
    pub text: &'a str,
    pub start: usize,
}
pub(super) fn arguments(source: &str) -> Result<Vec<Argument<'_>>> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            i += 1;
            while i < bytes.len() && bytes[i] != ch {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if ch == b'<'
            && i > 0
            && (bytes[i - 1].is_ascii_alphanumeric() || matches!(bytes[i - 1], b'_' | b'$'))
            && let Some(generic) =
                regex(r"^<[^()<>]*(?:<[^()<>]*>[^()<>]*)*>\s*\(")?.find(&source[i..])?
        {
            i += generic.end() - 1;
            continue;
        }
        if matches!(ch, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(ch, b')' | b']' | b'}') {
            depth -= 1;
        } else if ch == b',' && depth == 0 {
            out.push(Argument {
                text: &source[start..i],
                start,
            });
            start = i + 1;
        }
        i += 1;
    }
    out.push(Argument {
        text: &source[start..],
        start,
    });
    Ok(out)
}
pub(super) fn nonempty_arguments(source: &str) -> Result<Vec<Argument<'_>>> {
    Ok(arguments(source)?
        .into_iter()
        .filter(|part| !trim(part.text).is_empty())
        .collect())
}
pub(super) fn function_body_end(source: &str, at: usize) -> Option<usize> {
    let open = at + source[at..].find('(')?;
    let close = match_delim(source, open, b'(', b')')?;
    let brace = close + source[close..].find('{')?;
    match_delim(source, brace, b'{', b'}')
}
