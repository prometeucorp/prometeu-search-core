use super::*;

pub(super) fn blank_strings(text: &str) -> String {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut out = units.clone();
    let mut at = 0;
    while at < units.len() {
        let quote = units[at];
        if quote != u16::from(b'\'') && quote != u16::from(b'"') {
            at += 1;
            continue;
        }
        let width = if units.get(at..at + 3) == Some(&[quote; 3]) {
            3
        } else {
            1
        };
        let mut end = at + width;
        while end < units.len() && units.get(end..end + width) != Some(&units[at..at + width]) {
            end += if units[end] == u16::from(b'\\') { 2 } else { 1 };
        }
        out[at + width..end.min(units.len())].fill(u16::from(b' '));
        at = (end + width).min(units.len());
    }
    String::from_utf16_lossy(&out)
}

pub(super) fn closing(text: &str, open: usize) -> usize {
    let mut depth = 0i32;
    for (at, byte) in text.bytes().enumerate().skip(open) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return at;
                }
            }
            _ => {}
        }
    }
    text.len()
}

pub(super) fn parts(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    for (at, byte) in text.bytes().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push(&text[from..at]);
                from = at + 1;
            }
            _ => {}
        }
    }
    out.push(&text[from..]);
    out
}

pub(super) fn params(text: &str) -> Result<Vec<String>> {
    let pattern = regex(r"^\s*\*{0,2}\s*([A-Za-z_]\w*)")?;
    let mut out = Vec::new();
    for part in parts(text) {
        if let Some(capture) = pattern.captures(part)? {
            out.push(capture[1].into());
        }
    }
    Ok(out)
}

pub(super) fn global(statement: &str, name: &str) -> Result<bool> {
    Ok(regex(r"^global\s+(.+)$")?
        .captures(statement)?
        .is_some_and(|capture| capture[1].split(',').any(|part| trim(part) == name)))
}

/// The outer option distinguishes an import from an ordinary statement.
pub(super) fn import_binding(
    statement: &str,
    name: &str,
) -> Result<Option<Option<scope::Binding>>> {
    if let Some(capture) = regex(r"^import\s+([\s\S]+)$")?.captures(statement)? {
        for part in capture[1].replace(['(', ')'], " ").split(',') {
            let Some(spec) = regex(r"^([\w.]+)(?:\s+as\s+(\w+))?$")?.captures(trim(part))? else {
                continue;
            };
            if spec.get(2).is_some_and(|alias| alias.as_str() == name) {
                return Ok(Some(Some(scope::Binding::ImportedModule {
                    module: spec[1].into(),
                })));
            }
            if spec.get(2).is_none() && spec[1].split('.').next() == Some(name) {
                return Ok(Some(Some(scope::Binding::ImportedModule {
                    module: name.into(),
                })));
            }
        }
        return Ok(Some(None));
    }
    let Some(capture) = regex(r"^from\s+(\.*[\w.]*)\s+import\s+([\s\S]+)$")?.captures(statement)?
    else {
        return Ok(None);
    };
    for part in capture[2].replace(['(', ')'], " ").split(',') {
        if let Some(spec) = regex(r"^(\w+)(?:\s+as\s+(\w+))?$")?.captures(trim(part))?
            && spec.get(2).unwrap_or(spec.get(1).unwrap()).as_str() == name
        {
            return Ok(Some(Some(scope::Binding::ImportedName {
                source: capture[1].into(),
                exported_name: spec[1].into(),
            })));
        }
    }
    Ok(Some(None))
}

pub(super) fn assignment_targets(statement: &str) -> Result<Vec<String>> {
    let stripped = regex(r"^\s*(?:(?:el)?if|while|else|try|finally|except|with|for)\b[^:]*:(?!=)")?
        .try_replacen(statement, 1, "")?;
    let text = stripped.as_ref();
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 => {
                if bytes.get(at + 1) == Some(&b'=') {
                    at += 2;
                    continue;
                }
                let previous = at.checked_sub(1).map(|at| bytes[at]);
                if matches!(previous, Some(b'!' | b':'))
                    || matches!(previous, Some(b'<' | b'>'))
                        && at.checked_sub(2).map(|at| bytes[at]) != previous
                {
                    at += 1;
                    continue;
                }
                let segment = &text[from..at];
                if regex(r"\blambda\b")?.is_match(segment)? {
                    break;
                }
                let segment =
                    regex(r"(?:\*\*|//|>>|<<|[-+*/%&|^@])$")?.try_replacen(segment, 1, "")?;
                out.push(trim(segment.split(':').next().unwrap_or_default()).into());
                from = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    Ok(out)
}

pub(super) fn target_binds(target: &str, name: &str) -> Result<bool> {
    let pattern = regex(&format!(r"(?<![.\w]){}\b", regex::escape(name)))?;
    for found in pattern.find_iter(target) {
        let found = found?;
        let after = target[found.end()..].trim_start_matches(crate::search::query_whitespace);
        if after.starts_with(['.', '[', '(']) {
            continue;
        }
        let mut stack = Vec::new();
        for (at, byte) in target.bytes().enumerate().take(found.start()) {
            match byte {
                b'[' => stack.push(regex(r"[\w)\]]\s*$")?.is_match(&target[..at])?),
                b']' | b')' => {
                    stack.pop();
                }
                b'(' => stack.push(false),
                _ => {}
            }
        }
        if !stack.contains(&true) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn binds(statement: &str, name: &str) -> Result<bool> {
    for target in assignment_targets(statement)? {
        if target_binds(&target, name)? {
            return Ok(true);
        }
    }
    let escaped = regex::escape(name);
    if regex(&format!(r"^{escaped}\s*:(?!=)"))?.is_match(statement)? {
        return Ok(true);
    }
    if let Some(capture) = regex(r"^(?:async\s+)?for\s+([\s\S]+?)\s+in\b")?.captures(statement)?
        && target_binds(&capture[1], name)?
    {
        return Ok(true);
    }
    for capture in regex(r"\bas\s+(\([^)]*\)|[\w.]+)")?.captures_iter(statement) {
        if target_binds(&capture?[1], name)? {
            return Ok(true);
        }
    }
    if regex(&format!(r"(?<![.\w]){escaped}\s*:="))?.is_match(statement)? {
        return Ok(true);
    }
    if let Some(capture) = regex(r"^del\s+([\s\S]+)$")?.captures(statement)? {
        for part in parts(&capture[1]) {
            if target_binds(part, name)? {
                return Ok(true);
            }
        }
    }
    case_captures(statement, name)
}

fn case_captures(statement: &str, name: &str) -> Result<bool> {
    let Some(capture) = regex(r"^case\b([\s\S]*)$")?.captures(statement)? else {
        return Ok(false);
    };
    if !assignment_targets(statement)?.is_empty() {
        return Ok(false);
    }
    let text = &capture[1];
    let mut depth = 0usize;
    let mut end = None;
    for (at, byte) in text.bytes().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b':' if depth == 0 => {
                end = Some(at);
                break;
            }
            _ => {}
        }
    }
    let Some(end) = end else { return Ok(false) };
    let pattern = regex(r"\bif\b[\s\S]*$")?.try_replacen(&text[..end], 1, "")?;
    for found in regex(&format!(r"(?<![.\w]){}\b", regex::escape(name)))?.find_iter(&pattern) {
        let after = pattern[found?.end()..].trim_start_matches(crate::search::query_whitespace);
        if after.starts_with(['.', '(']) || after.starts_with('=') && !after.starts_with("==") {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}
