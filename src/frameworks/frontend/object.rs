//! The reference's depth-aware object-field scanner, with byte offsets into source.
use super::regex;
use anyhow::{Result, ensure};
use std::collections::HashMap;

pub(crate) struct Field<'a> {
    pub text: &'a str,
    pub at: usize,
}

pub(crate) fn matching(source: &str, open: usize) -> Result<Option<usize>> {
    bracket(source.as_bytes(), open, 0)
}

pub(crate) fn skip_string(source: &str, at: usize) -> Result<Option<usize>> {
    string(source.as_bytes(), at, 0)
}

pub(crate) fn objects(source: &str, from: usize, to: usize) -> Result<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    let mut i = from;
    while i < to && i < source.len() {
        if matches!(source.as_bytes()[i], b'\'' | b'"' | b'`') {
            let Some(end) = skip_string(source, i)? else {
                break;
            };
            i = end;
        } else if source.as_bytes()[i] == b'{' {
            let Some(end) = matching(source, i)? else {
                break;
            };
            out.push((i, end));
            i = end;
        }
        i += 1;
    }
    Ok(out)
}

fn bracket(bytes: &[u8], open: usize, nesting: usize) -> Result<Option<usize>> {
    ensure!(nesting < 256, "object scanner nesting limit exceeded");
    let mut i = open;
    let mut depth = 0i64;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                let Some(end) = string(bytes, i, nesting + 1)? else {
                    return Ok(None);
                };
                i = end;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
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

fn string(bytes: &[u8], at: usize, nesting: usize) -> Result<Option<usize>> {
    ensure!(nesting < 256, "object scanner nesting limit exceeded");
    let Some(&quote) = bytes.get(at) else {
        return Ok(None);
    };
    let mut i = at + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return Ok(Some(i));
        }
        if quote == b'`' && bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
            let Some(end) = bracket(bytes, i + 1, nesting + 1)? else {
                return Ok(None);
            };
            i = end;
        }
        i += 1;
    }
    Ok(None)
}

pub(crate) fn fields(source: &str, start: usize, end: usize) -> Result<HashMap<String, Field<'_>>> {
    let key = regex(r"^([A-Za-z_$][\w$]*)\s*:")?;
    let mut out = HashMap::new();
    let mut i = start + 1;
    while i < end && i < source.len() {
        match source.as_bytes()[i] {
            b'\'' | b'"' | b'`' => {
                let Some(close) = skip_string(source, i)? else {
                    break;
                };
                i = close + 1;
                continue;
            }
            b'{' | b'[' | b'(' => {
                let Some(close) = matching(source, i)? else {
                    break;
                };
                i = close + 1;
                continue;
            }
            _ => {}
        }
        let head = prefix(&source[i..], 64);
        let Some(found) = key.captures(head)? else {
            i += source[i..].chars().next().unwrap().len_utf8();
            continue;
        };
        let value_at = i + found.get(0).unwrap().end();
        let mut j = value_at;
        let mut malformed = false;
        while j < end && j < source.len() {
            let close = match source.as_bytes()[j] {
                b'\'' | b'"' | b'`' => Some(skip_string(source, j)?),
                b'{' | b'[' | b'(' => Some(matching(source, j)?),
                b',' => break,
                _ => None,
            };
            match close {
                Some(Some(close)) => j = close + 1,
                Some(None) => {
                    malformed = true;
                    break;
                }
                None => j += source[j..].chars().next().unwrap().len_utf8(),
            }
        }
        if malformed {
            break;
        }
        out.entry(found[1].to_owned()).or_insert(Field {
            text: &source[value_at..j],
            at: i,
        });
        i = j + 1;
    }
    Ok(out)
}

pub(crate) fn prefix(source: &str, max_units: usize) -> &str {
    let mut units = 0;
    for (at, ch) in source.char_indices() {
        units += ch.len_utf16();
        if units > max_units {
            return &source[..at];
        }
    }
    source
}
