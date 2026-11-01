//! Razor expressions inside script blocks, preserving UTF-16 positions.
use super::super::preprocess::{captures, test, utf16_len};
use anyhow::{Result, ensure};

const CONTROL: &[&str] = &[
    "if", "for", "foreach", "while", "switch", "lock", "using", "try", "do",
];

fn find(source: &[u16], pattern: &str, from: usize) -> Option<usize> {
    let pattern: Vec<_> = pattern.encode_utf16().collect();
    source
        .get(from..)?
        .windows(pattern.len())
        .position(|window| window == pattern)
        .map(|at| from + at)
}
fn character(source: &[u16], at: usize) -> u16 {
    source.get(at).copied().unwrap_or(0)
}
fn is_word(ch: u16) -> bool {
    (ch as u32)
        .try_into()
        .ok()
        .is_some_and(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
}
fn word(source: &[u16], at: usize, limit: usize) -> String {
    let first = character(source, at);
    if !(u16::from(b'A')..=u16::from(b'Z')).contains(&first)
        && !(u16::from(b'a')..=u16::from(b'z')).contains(&first)
        && first != u16::from(b'_')
    {
        return String::new();
    }
    let mut end = at + 1;
    while end < source.len().min(at + limit) && is_word(source[end]) {
        end += 1;
    }
    String::from_utf16_lossy(&source[at..end])
}
fn window(source: &[u16], at: usize, limit: usize) -> String {
    String::from_utf16_lossy(&source[at..source.len().min(at + limit)])
}
fn blank(out: &mut [u16], from: usize, to: usize) {
    for ch in &mut out[from..to] {
        if ![10, 13].contains(ch) {
            *ch = 32;
        }
    }
}
fn skip_string(source: &[u16], at: usize) -> usize {
    let quote = source[at];
    let verbatim = at > 0 && source[at - 1] == 64;
    let mut i = at + 1;
    while i < source.len() {
        let ch = source[i];
        if !verbatim && ch == 92 {
            i += 1;
        } else if ch == quote {
            if verbatim && character(source, i + 1) == quote {
                i += 1;
            } else {
                return i + 1;
            }
        } else if ch == 10 && !verbatim {
            return i;
        }
        i += 1;
    }
    source.len()
}
fn closer(ch: u16) -> Option<u16> {
    match ch {
        40 => Some(41),
        91 => Some(93),
        123 => Some(125),
        _ => None,
    }
}
fn past_bracket(source: &[u16], open: usize) -> Option<usize> {
    let mut stack = vec![closer(source[open])?];
    let mut i = open + 1;
    while i < source.len() {
        let ch = source[i];
        if ch == 34 || ch == 39 {
            i = skip_string(source, i);
            continue;
        }
        if ch == 47 && character(source, i + 1) == 47 {
            i = find(source, "\n", i).map_or(source.len(), |at| at + 1);
            continue;
        }
        if ch == 47 && character(source, i + 1) == 42 {
            i = find(source, "*/", i + 2)? + 2;
            continue;
        }
        if let Some(close) = closer(ch) {
            stack.push(close);
        } else if stack.last() == Some(&ch) {
            stack.pop();
            if stack.is_empty() {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}
fn past_expression(source: &[u16], at: usize) -> Result<usize> {
    let mut i = at;
    if let Some(m) = captures(r"^await\s+", &window(source, i, 16))?.first() {
        i += utf16_len(&m[0]);
    }
    let name = word(source, i, 128);
    if name.is_empty() {
        return Ok(at);
    }
    i += name.len();
    loop {
        let ch = character(source, i);
        if ch == 40 || ch == 91 {
            let Some(end) = past_bracket(source, i) else {
                return Ok(i);
            };
            i = end;
        } else if ch == 60 && test(r"^<[A-Z][\w.,\s<>?]*>\(", &window(source, i, 80))? {
            i = find(source, "(", i).unwrap();
        } else {
            let separator = if ch == 46 {
                1
            } else if ch == 63 && character(source, i + 1) == 46 {
                2
            } else {
                0
            };
            if separator == 0 || word(source, i + separator, 1).is_empty() {
                return Ok(i);
            }
            i += separator;
            let mut end = i;
            while end < source.len().min(i + 128) && is_word(source[end]) {
                end += 1;
            }
            i = end;
        }
    }
}
fn closing_text(source: &[u16], open: usize, to: usize) -> Option<usize> {
    let mut depth = 0;
    let mut at = open;
    while let Some(next) = find(source, "<", at).filter(|next| *next < to) {
        if source.get(next..next + 6) == Some(&[60, 116, 101, 120, 116, 62]) {
            depth += 1;
        } else if source.get(next..next + 7) == Some(&[60, 47, 116, 101, 120, 116, 62]) {
            depth -= 1;
            if depth == 0 {
                return Some(next);
            }
        }
        at = next + 1;
    }
    None
}
fn keep_emitted(
    source: &[u16],
    out: &mut [u16],
    from: usize,
    to: usize,
    depth: usize,
) -> Result<()> {
    let mut i = from;
    while i < to {
        let text = find(source, "<text>", i).filter(|at| *at < to);
        let line = find(source, "@:", i).filter(|at| *at < to);
        let Some(next) = text.into_iter().chain(line).min() else {
            blank(out, i, to);
            return Ok(());
        };
        blank(out, i, next);
        if Some(next) == line {
            let stop = find(source, "\n", next).unwrap_or(to).min(to);
            blank(out, next, next + 2);
            blank_razor(source, out, next + 2, stop, depth + 1)?;
            i = stop;
        } else {
            let Some(close) = closing_text(source, next, to) else {
                blank(out, next, to);
                return Ok(());
            };
            blank(out, next, next + 6);
            blank_razor(source, out, next + 6, close, depth + 1)?;
            blank(out, close, close + 7);
            i = close + 7;
        }
    }
    Ok(())
}
fn blank_control(
    source: &[u16],
    out: &mut [u16],
    mut at: usize,
    limit: usize,
    depth: usize,
) -> Result<usize> {
    let mut i = at;
    loop {
        while i < limit && source[i] != 123 {
            if source[i] == 40 {
                let Some(end) = past_bracket(source, i) else {
                    i = limit;
                    break;
                };
                i = end;
            } else {
                i += 1;
            }
        }
        if i >= limit {
            blank(out, at, limit);
            return Ok(limit);
        }
        let Some(end) = past_bracket(source, i).filter(|end| *end <= limit) else {
            blank(out, at, limit);
            return Ok(limit);
        };
        blank(out, at, i + 1);
        keep_emitted(source, out, i + 1, end - 1, depth)?;
        blank(out, end - 1, end);
        if !test(
            r"^\s*(?:else\s+if\b|else\b|catch\b|finally\b)",
            &window(source, end, 32),
        )? {
            return Ok(end);
        }
        at = end;
        i = end;
    }
}
fn blank_razor(
    source: &[u16],
    out: &mut [u16],
    from: usize,
    to: usize,
    depth: usize,
) -> Result<()> {
    ensure!(depth < 256, "Razor script nesting exceeds 256 levels");
    let mut i = from;
    while i < to {
        let Some(at) = find(source, "@", i).filter(|at| *at < to) else {
            return Ok(());
        };
        if at > 0 && is_word(source[at - 1]) {
            i = at + 1;
            continue;
        }
        match character(source, at + 1) {
            64 => {
                blank(out, at, at + 1);
                i = at + 2;
            }
            42 => {
                let stop = find(source, "*@", at + 2).map_or(to, |end| (end + 2).min(to));
                blank(out, at, stop);
                i = stop;
            }
            next @ (123 | 40) => {
                let Some(end) = past_bracket(source, at + 1).filter(|end| *end <= to) else {
                    return Ok(());
                };
                blank(out, at, end);
                if next == 40 {
                    out[at] = 48;
                }
                i = end;
            }
            58 => {
                blank(out, at, at + 2);
                i = at + 2;
            }
            _ => {
                let name = word(source, at + 1, 32);
                if name.is_empty() {
                    i = at + 1;
                } else if CONTROL.contains(&name.as_str()) {
                    i = blank_control(source, out, at, to, depth)?;
                } else {
                    let end = past_expression(source, at + 1)?.min(to);
                    blank(out, at, end);
                    out[at] = 48;
                    i = end.max(at + 1);
                }
            }
        }
    }
    Ok(())
}
pub(super) fn blank_in_script(source: &str) -> Result<String> {
    if !source.contains('@') {
        return Ok(source.to_owned());
    }
    let source: Vec<_> = source.encode_utf16().collect();
    let mut out = source.clone();
    blank_razor(&source, &mut out, 0, source.len(), 0)?;
    Ok(String::from_utf16(&out)?)
}
pub(super) fn blank_comments(source: &str) -> Result<String> {
    if !source.contains("@*") {
        return Ok(source.to_owned());
    }
    let source: Vec<_> = source.encode_utf16().collect();
    let mut out = source.clone();
    let mut i = 0;
    while let Some(at) = find(&source, "@*", i) {
        let Some(end) = find(&source, "*@", at + 2) else {
            break;
        };
        blank(&mut out, at, end + 2);
        i = end + 2;
    }
    Ok(String::from_utf16(&out)?)
}
pub(super) fn is_javascript(attrs: &str) -> Result<bool> {
    let types = captures(r#"(?i)\btype\s*=\s*["']([^"']*)["']"#, attrs)?;
    let Some(m) = types.first() else {
        return Ok(true);
    };
    let kind = m[1].trim().to_lowercase();
    Ok(kind.is_empty()
        || test(
            r"^(?:module|(?:text|application)/(?:javascript|ecmascript|typescript|babel|jsx)|text/(?:x-)?(?:babel|jsx))$",
            &kind,
        )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_pinned_razor_script_scanner() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/razor/cases.json"
        ))
        .unwrap();
        for case in corpus["scripts"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            assert_eq!(
                blank_in_script(source).unwrap(),
                case["prepared"].as_str().unwrap(),
                "script {source:?}"
            );
            assert_eq!(
                blank_comments(source).unwrap(),
                case["comments"].as_str().unwrap(),
                "comments {source:?}"
            );
        }
        for case in corpus["attributes"].as_array().unwrap() {
            assert_eq!(
                is_javascript(case["source"].as_str().unwrap()).unwrap(),
                case["javascript"].as_bool().unwrap()
            );
        }
    }
}
