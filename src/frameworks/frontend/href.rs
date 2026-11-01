//! Literal destination text shared by frontend navigation passes.
use super::regex;
use anyhow::{Result, ensure};

pub(crate) const HOLE: char = '\0';
pub(crate) fn lines(source: &str) -> Vec<&str> {
    let mut lines: Vec<_> = source.split('\n').collect();
    let last = lines.len().saturating_sub(1);
    for line in &mut lines[..last] {
        *line = line.strip_suffix('\r').unwrap_or(line);
    }
    lines
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub(crate) struct Href {
    pub path: String,
    pub display: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub alternates: Vec<Href>,
}
impl Href {
    pub fn arms(&self) -> impl Iterator<Item = &Href> {
        std::iter::once(self).chain(self.alternates.iter())
    }
}
pub(crate) fn to_href(literal: &str) -> Option<Href> {
    let path = literal.split(['?', '#']).next().unwrap_or("");
    if path.is_empty() {
        return None;
    }
    Some(Href {
        path: path.to_owned(),
        display: literal.replace(HOLE, "${…}"),
        alternates: Vec::new(),
    })
}
pub(crate) fn read_string(source: &str, at: usize) -> Result<Option<String>> {
    let bytes = source.as_bytes();
    let Some(&quote) = bytes.get(at) else {
        return Ok(None);
    };
    if !matches!(quote, b'\'' | b'"' | b'`') {
        return Ok(None);
    }
    let end = if quote == b'`' {
        template_end(bytes, at, 0)?
    } else {
        bytes.len()
    };
    let mut output = Vec::new();
    let mut i = at + 1;
    while i < end {
        let byte = bytes[i];
        if byte == b'\\' {
            if let Some(&escaped) = bytes.get(i + 1) {
                output.push(escaped);
            }
            i += 2;
            continue;
        }
        if byte == quote && quote != b'`' {
            return Ok(Some(String::from_utf8(output)?));
        }
        if quote == b'`' && byte == b'$' && bytes.get(i + 1) == Some(&b'{') {
            let mut depth = 0;
            i += 1;
            while i < end {
                if bytes[i] == b'{' {
                    depth += 1;
                } else if bytes[i] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            output.push(0);
        } else {
            output.push(byte);
        }
        i += 1;
    }
    if quote == b'`' {
        Ok(Some(String::from_utf8(output)?))
    } else {
        Ok(None)
    }
}
pub(crate) fn string_end(source: &str, at: usize) -> Result<usize> {
    let bytes = source.as_bytes();
    let Some(&quote) = bytes.get(at) else {
        return Ok(bytes.len());
    };
    if quote == b'`' {
        return template_end(bytes, at, 0);
    }
    let mut i = at + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == quote {
            return Ok(i);
        } else {
            i += 1;
        }
    }
    Ok(bytes.len())
}

pub(crate) fn parse(expression: &str) -> Result<Option<Href>> {
    parse_inner(expression, 0)
}
fn parse_inner(expression: &str, nesting: usize) -> Result<Option<Href>> {
    ensure!(
        nesting < 256,
        "navigation expression nesting limit exceeded"
    );
    let cast = regex(r"\s+(?:as|satisfies)\s+[\w$.<>\[\]|&\s]+$")?;
    let mut arguments = cast.replace(expression.trim(), "").into_owned();
    let mut layers = 0;
    while arguments.starts_with('(') && match_paren(&arguments, 0)? == Some(arguments.len() - 1) {
        layers += 1;
        ensure!(layers < 256, "navigation grouping limit exceeded");
        arguments = cast
            .replace(arguments[1..arguments.len() - 1].trim(), "")
            .into_owned();
    }
    if arguments.is_empty() {
        return Ok(None);
    }
    if let Some(question) = at_depth_zero(&arguments, b'?', 0)?
        && question > 0
        && let Some(colon) = ternary_colon(&arguments, question)?
    {
        let yes = parse_inner(&arguments[question + 1..colon], nesting + 1)?;
        let no = parse_inner(&arguments[colon + 1..], nesting + 1)?;
        let mut arms = Vec::new();
        for branch in [yes, no].into_iter().flatten() {
            let Href {
                path,
                display,
                alternates,
            } = branch;
            arms.push(Href {
                path,
                display,
                alternates: Vec::new(),
            });
            arms.extend(alternates);
        }
        if arms.is_empty() {
            return Ok(None);
        }
        let mut first = arms.remove(0);
        first.alternates = arms;
        return Ok(Some(first));
    }
    let at = if arguments.starts_with('{') {
        let Some(key) = regex(r"\bpathname\s*:\s*")?.find(&arguments)? else {
            return Ok(None);
        };
        key.end()
    } else {
        0
    };
    Ok(read_string(&arguments, at)?.and_then(|literal| to_href(&literal)))
}
fn match_paren(source: &str, open: usize) -> Result<Option<usize>> {
    let bytes = source.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        let byte = bytes[i];
        if matches!(byte, b'\'' | b'"' | b'`') {
            i = string_end(source, i)?.saturating_add(1);
            continue;
        }
        if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth -= 1;
            if depth == 0 {
                return Ok(Some(i));
            }
        }
        i += 1;
    }
    Ok(None)
}
fn at_depth_zero(source: &str, wanted: u8, from: usize) -> Result<Option<usize>> {
    let bytes = source.as_bytes();
    let mut depth = 0i32;
    let mut i = from;
    while i < bytes.len() {
        let byte = bytes[i];
        if matches!(byte, b'\'' | b'"' | b'`') {
            i = string_end(source, i)?.saturating_add(1);
            continue;
        }
        if matches!(byte, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(byte, b')' | b']' | b'}') {
            depth -= 1;
        } else if byte == b'?' && matches!(bytes.get(i + 1), Some(b'.' | b'?')) {
            i += 1;
        } else if depth == 0 && byte == wanted {
            return Ok(Some(i));
        }
        i += 1;
    }
    Ok(None)
}
fn ternary_colon(source: &str, question: usize) -> Result<Option<usize>> {
    let mut depth = 0;
    let mut i = question + 1;
    for _ in 0..64 {
        let question = at_depth_zero(source, b'?', i)?;
        let Some(colon) = at_depth_zero(source, b':', i)? else {
            return Ok(None);
        };
        if let Some(question) = question
            && question < colon
        {
            depth += 1;
            i = question + 1;
            continue;
        }
        if depth == 0 {
            return Ok(Some(colon));
        }
        depth -= 1;
        i = colon + 1;
    }
    Ok(None)
}
pub(crate) fn argument(
    lines: &[&str],
    line: u32,
    column: u32,
    method: &str,
    index: usize,
) -> Result<Option<String>> {
    let Some(first) = line
        .checked_sub(1)
        .map(|i| i as usize)
        .filter(|&i| i < lines.len())
    else {
        return Ok(None);
    };
    let text = lines[first..lines.len().min(first + 12)].join("\n");
    let offset = utf16_byte(&text, column as usize);
    let Some(at) = text[offset..].find(method).map(|at| at + offset) else {
        return Ok(None);
    };
    let mut open = at + method.len();
    for ch in text[open..].chars() {
        if js_space(ch) {
            open += ch.len_utf8();
        } else {
            break;
        }
    }
    if text.as_bytes().get(open) != Some(&b'(') {
        return Ok(None);
    }
    let close = match_paren(&text, open)?.unwrap_or(text.len());
    let mut args = &text[open + 1..close];
    for _ in 0..index {
        let Some(comma) = at_depth_zero(args, b',', 0)? else {
            return Ok(None);
        };
        args = &args[comma + 1..];
    }
    let end = at_depth_zero(args, b',', 0)?.unwrap_or(args.len());
    Ok(Some(args[..end].to_owned()))
}
pub(crate) fn via_local(
    lines: &[&str],
    line: u32,
    column: u32,
    method: &str,
    enclosing_start: u32,
) -> Result<Option<Href>> {
    let Some(argument) = argument(lines, line, column, method, 0)? else {
        return Ok(None);
    };
    let cast = regex(r"\s+(?:as|satisfies)\s+[\w$.<>\[\]|&\s]+$")?;
    let name = cast.replace(argument.trim(), "");
    if !regex(r"^[A-Za-z_$][\w$]*$")?.is_match(&name)? {
        return Ok(None);
    }
    let name = ::regex::escape(&name);
    let declaration = regex(&format!(
        r"\b(?:const|let|var)\s+{name}\s*(?::[^=]*?)?=(?!=)"
    ))?;
    let reassignment = regex(&format!(r"(?:^|[^.\w$]){name}\s*=(?!=)"))?;
    let continuation = regex(r#"^[?:.)\]}`'"+&|]"#)?;
    let first = enclosing_start.saturating_sub(1) as usize;
    let end = line.saturating_sub(1) as usize;
    if end > lines.len() {
        return Ok(None);
    }
    for i in (first..end).rev() {
        let text = lines[i];
        let Some(found) = declaration.find(text)? else {
            if reassignment.is_match(text)? {
                return Ok(None);
            }
            continue;
        };
        let mut initializer = text[found.end()..].to_owned();
        for next in lines.iter().take(end.min(i + 12)).skip(i + 1) {
            if !continuation.is_match(next.trim_start_matches(js_space))? && balanced(&initializer)?
            {
                break;
            }
            initializer.push('\n');
            initializer.push_str(next);
        }
        return parse(&initializer);
    }
    Ok(None)
}
fn balanced(source: &str) -> Result<bool> {
    let bytes = source.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'\'' | b'"' | b'`') {
            let end = string_end(source, i)?;
            if end >= bytes.len() {
                return Ok(false);
            }
            i = end + 1;
            continue;
        }
        if matches!(bytes[i], b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(bytes[i], b')' | b']' | b'}') {
            depth -= 1;
        }
        i += 1;
    }
    Ok(depth <= 0)
}
fn utf16_byte(source: &str, offset: usize) -> usize {
    let mut units = 0;
    for (at, ch) in source.char_indices() {
        if units >= offset {
            return at;
        }
        units += ch.len_utf16();
    }
    source.len()
}
fn js_space(ch: char) -> bool {
    matches!(ch,'\u{0009}'..='\u{000d}'|'\u{0020}'|'\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}')
}
fn template_end(bytes: &[u8], open: usize, nesting: usize) -> Result<usize> {
    ensure!(nesting < 256, "navigation template nesting limit exceeded");
    let mut i = open + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            return Ok(i);
        }
        if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
            let mut depth = 0;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'{' {
                    depth += 1;
                } else if bytes[i] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if bytes[i] == b'`' {
                    i = template_end(bytes, i, nesting + 1)?;
                }
                i += 1;
            }
        }
        i += 1;
    }
    Ok(bytes.len())
}
