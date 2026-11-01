//! Vue template call scanner ported from the pinned upstream implementation.
//! SPDX-License-Identifier: MIT. See crates/prometeu-extract/UPSTREAM.md.
use crate::parser::preprocess::regex;
use anyhow::{Result, ensure};
use std::collections::HashSet;

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];
const GLOBALS: &[&str] = &[
    "Infinity",
    "undefined",
    "NaN",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "Math",
    "Number",
    "Date",
    "Array",
    "Object",
    "Boolean",
    "String",
    "RegExp",
    "Map",
    "Set",
    "JSON",
    "Intl",
    "BigInt",
    "console",
    "Error",
    "Symbol",
];
const KEYWORDS: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "do",
    "switch",
    "case",
    "default",
    "break",
    "continue",
    "return",
    "throw",
    "try",
    "catch",
    "finally",
    "function",
    "class",
    "new",
    "delete",
    "typeof",
    "void",
    "instanceof",
    "in",
    "of",
    "var",
    "let",
    "const",
    "this",
    "super",
    "import",
    "export",
    "await",
    "async",
    "yield",
    "with",
    "debugger",
    "true",
    "false",
    "null",
    "undefined",
];
const VALUES: &[&str] = &["this", "super", "true", "false", "null", "undefined"];

fn whitespace(source: &str, index: usize) -> usize {
    source.get(index..).and_then(|rest| rest.chars().next()).filter(|ch| matches!(ch, '\u{0009}'..='\u{000D}' | '\u{0020}' | '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')).map_or(0, char::len_utf8)
}
fn skip_ws(source: &str, mut at: usize, end: usize) -> usize {
    while at < end {
        let count = whitespace(source, at);
        if count == 0 {
            break;
        }
        at += count;
    }
    at
}
fn byte(source: &str, at: usize) -> u8 {
    source.as_bytes().get(at).copied().unwrap_or(0)
}
fn ident_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || matches!(ch, b'_' | b'$')
}
fn ident_char(ch: u8) -> bool {
    ident_start(ch) || ch.is_ascii_digit()
}
fn word_end(source: &str, mut at: usize, end: usize) -> usize {
    while at < end && ident_char(byte(source, at)) {
        at += 1;
    }
    at
}
fn identifiers(source: &str) -> Result<HashSet<String>> {
    regex(r"[A-Za-z_$][\w$]*")?
        .find_iter(source)
        .map(|found| {
            found
                .map(|found| found.as_str().to_owned())
                .map_err(Into::into)
        })
        .collect()
}

/// Returns call names and byte offsets in the original SFC source.
pub(super) fn calls(source: &str) -> Result<Vec<(String, usize)>> {
    let mut masked = source.as_bytes().to_vec();
    for matched in regex(r"<(script|style)(\s[^>]*?)?(?:/>|>[\s\S]*?</\1\s*>)")?.find_iter(source) {
        let matched = matched?;
        if !matched.as_str().ends_with("/>") {
            for byte in &mut masked[matched.range()] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
        }
    }
    let mut masked = String::from_utf8(masked)?;
    let ranges: Vec<_> = regex(r"<!--[\s\S]*?-->")?
        .find_iter(&masked)
        .map(|found| {
            found
                .map(|found| found.range())
                .map_err(anyhow::Error::from)
        })
        .collect::<Result<_>>()?;
    // Blanking bytes preserves original byte offsets, including Unicode inside comments.
    let mut bytes = masked.into_bytes();
    for range in ranges {
        for byte in &mut bytes[range] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    masked = String::from_utf8(bytes)?;
    let Some((start, end)) = root_template(&masked)? else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    scan_markup(&masked, start, end, &mut found)?;
    found.sort_by_key(|(_, offset)| *offset);
    Ok(found)
}

fn root_template(source: &str) -> Result<Option<(usize, usize)>> {
    let mut start = None;
    let mut depth = 0;
    for captures in
        regex(r#"<(/?)template(?=[\s/>])((?:[^>"']|"[^"]*"|'[^']*')*)>"#)?.captures_iter(source)
    {
        let captures = captures?;
        let whole = captures.get(0).unwrap();
        let closing = &captures[1] == "/";
        let attributes = &captures[2];
        let self_closing = attributes.trim_end().ends_with('/');
        if start.is_none() {
            if closing || self_closing {
                continue;
            }
            if let Some(language) = regex(r#"\blang\s*=\s*["']?([\w-]+)"#)?.captures(attributes)?
                && !language[1].eq_ignore_ascii_case("html")
            {
                return Ok(None);
            }
            start = Some(whole.end());
            depth = 1;
        } else if closing {
            depth -= 1;
            if depth == 0 {
                return Ok(start.map(|start| (start, whole.start())));
            }
        } else if !self_closing {
            depth += 1;
        }
    }
    Ok(start.map(|start| (start, source.len())))
}

struct Attribute<'a> {
    name: &'a str,
    value: Option<&'a str>,
    start: usize,
}
struct Tag<'a> {
    name: &'a str,
    attributes: Vec<Attribute<'a>>,
    close: usize,
    self_closing: bool,
}
struct Frame {
    tag: String,
    locals: HashSet<String>,
    pre: bool,
}

fn directive(name: &str) -> &str {
    if name == "v-for" {
        "for"
    } else if name.starts_with('#')
        || name == "v-slot"
        || name.starts_with("v-slot:")
        || matches!(name, "slot-scope" | "scope")
    {
        "slot"
    } else if name.starts_with('@') || name == "v-on" || name.starts_with("v-on:") {
        "on"
    } else if name.starts_with([':', '.']) || name.starts_with("v-") {
        "expr"
    } else {
        ""
    }
}

fn open_tag(source: &str, at: usize, end: usize) -> Tag<'_> {
    let mut cursor = at + 1;
    let start = cursor;
    while cursor < end
        && whitespace(source, cursor) == 0
        && !matches!(byte(source, cursor), b'/' | b'>')
    {
        cursor += 1;
    }
    let name = &source[start..cursor];
    let mut attributes = Vec::new();
    while cursor < end {
        cursor = skip_ws(source, cursor, end);
        if cursor >= end {
            break;
        }
        if byte(source, cursor) == b'>' {
            return Tag {
                name,
                attributes,
                close: cursor + 1,
                self_closing: false,
            };
        }
        if byte(source, cursor) == b'/' && byte(source, cursor + 1) == b'>' {
            return Tag {
                name,
                attributes,
                close: cursor + 2,
                self_closing: true,
            };
        }
        let start = cursor;
        while cursor < end
            && whitespace(source, cursor) == 0
            && !matches!(byte(source, cursor), b'"' | b'\'' | b'>' | b'/' | b'=')
        {
            cursor += 1;
        }
        if cursor == start {
            cursor += 1;
            continue;
        }
        let name = &source[start..cursor];
        let mut next = skip_ws(source, cursor, end);
        if byte(source, next) != b'=' {
            attributes.push(Attribute {
                name,
                value: None,
                start: 0,
            });
            continue;
        }
        next = skip_ws(source, next + 1, end);
        let quote = byte(source, next);
        if matches!(quote, b'"' | b'\'') {
            let close = source
                .get(next + 1..)
                .and_then(|rest| rest.find(quote as char))
                .map_or(end, |offset| (next + 1 + offset).min(end));
            attributes.push(Attribute {
                name,
                value: Some(&source[next + 1..close]),
                start: next + 1,
            });
            cursor = close + 1;
        } else {
            let start = next;
            while next < end && whitespace(source, next) == 0 && byte(source, next) != b'>' {
                next += 1;
            }
            attributes.push(Attribute {
                name,
                value: Some(&source[start..next]),
                start,
            });
            cursor = next;
        }
    }
    Tag {
        name,
        attributes,
        close: end,
        self_closing: false,
    }
}

fn scan_markup(
    source: &str,
    start: usize,
    end: usize,
    out: &mut Vec<(String, usize)>,
) -> Result<()> {
    let mut stack: Vec<Frame> = Vec::new();
    let mut cursor = start;
    let mut next_mustache = start;
    let closing = regex(r"^</([A-Za-z][^\s/>]*)\s*>")?;
    let aliases = regex(r"([\s\S]*?)\s+(?:in|of)\s+(\S[\s\S]*)")?;
    while cursor < end {
        if next_mustache <= cursor {
            next_mustache = source[cursor..]
                .find("{{")
                .map_or(end, |offset| (cursor + offset).min(end));
        }
        let next_tag = source[cursor..]
            .find('<')
            .map_or(end, |offset| (cursor + offset).min(end));
        if next_mustache < next_tag {
            let expression_end = source[next_mustache + 2..]
                .find("}}")
                .map_or(end, |offset| (next_mustache + 2 + offset).min(end));
            if !stack.iter().any(|frame| frame.pre) {
                scan_expression(
                    source,
                    next_mustache + 2,
                    expression_end,
                    &|name| stack.iter().any(|frame| frame.locals.contains(name)),
                    out,
                    0,
                )?;
            }
            cursor = expression_end + 2;
            continue;
        }
        if next_tag >= end {
            break;
        }
        if let Some(captures) = closing.captures(&source[next_tag..])? {
            if let Some(index) = stack
                .iter()
                .rposition(|frame| frame.tag.eq_ignore_ascii_case(&captures[1]))
            {
                stack.truncate(index);
            }
            cursor = next_tag + captures.get(0).unwrap().end();
            continue;
        }
        if !byte(source, next_tag + 1).is_ascii_alphabetic() {
            cursor = next_tag + 1;
            continue;
        }
        let tag = open_tag(source, next_tag, end);
        cursor = tag.close;
        let mut own = HashSet::new();
        let mut pre = false;
        let mut for_source = None;
        for attribute in &tag.attributes {
            if attribute.name == "v-pre" {
                pre = true;
            }
            let Some(value) = attribute.value else {
                continue;
            };
            match directive(attribute.name) {
                "for" => {
                    if let Some(captures) = aliases.captures(value)? {
                        own.extend(identifiers(&captures[1])?);
                        let expression = captures.get(2).unwrap();
                        for_source = Some((
                            attribute.start + expression.start(),
                            attribute.start + value.len(),
                        ));
                    }
                }
                "slot" => own.extend(identifiers(value)?),
                _ => {}
            }
        }
        if !pre && !stack.iter().any(|frame| frame.pre) {
            let local = |name: &str| {
                own.contains(name) || stack.iter().any(|frame| frame.locals.contains(name))
            };
            for attribute in &tag.attributes {
                let Some(value) = attribute.value else {
                    continue;
                };
                match directive(attribute.name) {
                    "expr" | "on" => scan_expression(
                        source,
                        attribute.start,
                        attribute.start + value.len(),
                        &local,
                        out,
                        0,
                    )?,
                    "for" => {
                        if let Some((start, end)) = for_source {
                            scan_expression(source, start, end, &local, out, 0)?;
                        }
                    }
                    _ => {}
                }
            }
        }
        if !tag.self_closing && !VOID.contains(&tag.name.to_ascii_lowercase().as_str()) {
            stack.push(Frame {
                tag: tag.name.into(),
                locals: own,
                pre,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Previous {
    Start,
    Operand,
    Dot,
    New,
    Declaration,
}
struct Paren {
    open: usize,
    call: Option<usize>,
    parameters: bool,
}

fn scan_expression(
    source: &str,
    start: usize,
    end: usize,
    outer: &dyn Fn(&str) -> bool,
    out: &mut Vec<(String, usize)>,
    depth: usize,
) -> Result<()> {
    // ponytail: bound recursive template substitutions; use an explicit task stack for deeper generated expressions.
    ensure!(
        depth < 256,
        "Vue template expression nesting exceeds 256 levels"
    );
    let mut params = HashSet::new();
    let mut found: Vec<(String, usize, bool)> = Vec::new();
    let mut parens: Vec<Paren> = Vec::new();
    let mut last_group = None;
    let mut last_ident = "";
    let mut function_pending = false;
    let mut previous = Previous::Start;
    let mut cursor = start;
    while cursor < end {
        let ch = byte(source, cursor);
        let count = whitespace(source, cursor);
        if count != 0 {
            cursor += count;
            continue;
        }
        if matches!(ch, b'"' | b'\'') {
            cursor = skip_string(source, cursor, end);
            previous = Previous::Operand;
            continue;
        }
        if ch == b'`' {
            let (next, substitutions) = skip_template(source, cursor, end, depth + 1)?;
            for (start, end) in substitutions {
                scan_expression(
                    source,
                    start,
                    end,
                    &|name| params.contains(name) || outer(name),
                    out,
                    depth + 1,
                )?;
            }
            cursor = next;
            previous = Previous::Operand;
            continue;
        }
        if ch == b'/' {
            if byte(source, cursor + 1) == b'/' {
                cursor = source[cursor..]
                    .find('\n')
                    .map_or(end, |offset| (cursor + offset).min(end));
                continue;
            }
            if byte(source, cursor + 1) == b'*' {
                cursor = source[cursor + 2..].find("*/").map_or(end, |offset| {
                    if cursor + 2 + offset > end {
                        end
                    } else {
                        cursor + 2 + offset + 2
                    }
                });
                continue;
            }
            if previous != Previous::Operand {
                cursor = skip_regex(source, cursor, end);
                previous = Previous::Operand;
                continue;
            }
            cursor += 1;
            previous = Previous::Start;
            continue;
        }
        if ch.is_ascii_digit() || ch == b'.' && byte(source, cursor + 1).is_ascii_digit() {
            cursor += 1;
            while cursor < end
                && (byte(source, cursor).is_ascii_alphanumeric()
                    || matches!(byte(source, cursor), b'_' | b'.'))
            {
                cursor += 1;
            }
            previous = Previous::Operand;
            continue;
        }
        if ident_start(ch) {
            let finish = word_end(source, cursor, end);
            let word = &source[cursor..finish];
            if previous == Previous::Dot {
                cursor = finish;
                previous = Previous::Operand;
                last_ident = "";
                continue;
            }
            if KEYWORDS.contains(&word) {
                cursor = finish;
                last_ident = "";
                previous = if word == "new" {
                    Previous::New
                } else if matches!(word, "function" | "class") {
                    if word == "function" {
                        function_pending = true;
                    }
                    Previous::Declaration
                } else if VALUES.contains(&word) {
                    Previous::Operand
                } else {
                    Previous::Start
                };
                continue;
            }
            if previous == Previous::Declaration {
                cursor = finish;
                previous = Previous::Operand;
                continue;
            }
            let mut segments = vec![word];
            let mut after = finish;
            loop {
                let mut next = skip_ws(source, after, end);
                if byte(source, next) == b'!' && byte(source, next + 1) == b'.' {
                    next += 1;
                }
                let member = if byte(source, next) == b'.' && byte(source, next + 1) != b'.' {
                    skip_ws(source, next + 1, end)
                } else if byte(source, next) == b'?'
                    && byte(source, next + 1) == b'.'
                    && !byte(source, next + 2).is_ascii_digit()
                {
                    skip_ws(source, next + 2, end)
                } else {
                    break;
                };
                if !ident_start(byte(source, member)) {
                    break;
                }
                let finish = word_end(source, member, end);
                segments.push(&source[member..finish]);
                after = finish;
            }
            let mut next = skip_ws(source, after, end);
            if byte(source, next) == b'?'
                && byte(source, next + 1) == b'.'
                && byte(source, skip_ws(source, next + 2, end)) == b'('
            {
                next = skip_ws(source, next + 2, end);
            }
            if byte(source, next) == b'(' {
                let call = if previous != Previous::New
                    && !word.starts_with('$')
                    && !GLOBALS.contains(&word)
                    && !params.contains(word)
                    && !outer(word)
                {
                    found.push((segments.join("."), cursor, false));
                    Some(found.len() - 1)
                } else {
                    None
                };
                parens.push(Paren {
                    open: next,
                    call,
                    parameters: false,
                });
                cursor = next + 1;
                previous = Previous::Start;
                last_ident = "";
                continue;
            }
            cursor = after;
            previous = Previous::Operand;
            last_ident = if segments.len() == 1 { word } else { "" };
            continue;
        }
        if ch == b'(' {
            parens.push(Paren {
                open: cursor,
                call: None,
                parameters: function_pending,
            });
            function_pending = false;
            cursor += 1;
            previous = Previous::Start;
            continue;
        }
        if ch == b')' {
            if let Some(paren) = parens.pop() {
                last_group = Some((paren.open, cursor));
                if paren.parameters {
                    params.extend(identifiers(&source[paren.open + 1..cursor])?);
                }
                if let Some(call) = paren.call
                    && byte(source, skip_ws(source, cursor + 1, end)) == b'{'
                {
                    found[call].2 = true;
                }
            }
            cursor += 1;
            previous = Previous::Operand;
            last_ident = "";
            continue;
        }
        if ch == b'=' && byte(source, cursor + 1) == b'>' {
            if !last_ident.is_empty() {
                params.insert(last_ident.into());
            } else if let Some((open, close)) = last_group {
                let prior = source[start..cursor]
                    .char_indices()
                    .rev()
                    .find(|(offset, _)| whitespace(source, start + offset) == 0)
                    .map(|(offset, _)| start + offset);
                if prior == Some(close) {
                    params.extend(identifiers(&source[open + 1..close])?);
                }
            }
            cursor += 2;
            previous = Previous::Start;
            last_ident = "";
            continue;
        }
        if ch == b'.' {
            if byte(source, cursor + 1) == b'.' && byte(source, cursor + 2) == b'.' {
                cursor += 3;
                previous = Previous::Start;
            } else {
                cursor += 1;
                previous = Previous::Dot;
            }
            last_ident = "";
            continue;
        }
        if ch == b'?'
            && byte(source, cursor + 1) == b'.'
            && !byte(source, cursor + 2).is_ascii_digit()
        {
            cursor += 2;
            previous = Previous::Dot;
            last_ident = "";
            continue;
        }
        cursor += 1;
        previous = if matches!(ch, b']' | b'}') {
            Previous::Operand
        } else {
            Previous::Start
        };
        last_ident = "";
    }
    out.extend(
        found
            .into_iter()
            .filter(|(_, _, dropped)| !dropped)
            .map(|(name, offset, _)| (name, offset)),
    );
    Ok(())
}

fn skip_string(source: &str, start: usize, end: usize) -> usize {
    let quote = byte(source, start);
    let mut cursor = start + 1;
    while cursor < end {
        if byte(source, cursor) == b'\\' {
            cursor += 2;
        } else if byte(source, cursor) == quote {
            return cursor + 1;
        } else {
            cursor += 1;
        }
    }
    end
}

fn skip_template(
    source: &str,
    start: usize,
    end: usize,
    depth: usize,
) -> Result<(usize, Vec<(usize, usize)>)> {
    ensure!(
        depth < 256,
        "Vue template literal nesting exceeds 256 levels"
    );
    let mut substitutions = Vec::new();
    let mut cursor = start + 1;
    while cursor < end {
        if byte(source, cursor) == b'\\' {
            cursor += 2;
        } else if byte(source, cursor) == b'`' {
            return Ok((cursor + 1, substitutions));
        } else if byte(source, cursor) == b'$' && byte(source, cursor + 1) == b'{' {
            let close = matching_brace(source, cursor + 2, end, depth + 1)?;
            substitutions.push((cursor + 2, close));
            cursor = close + 1;
        } else {
            cursor += 1;
        }
    }
    Ok((end, substitutions))
}

fn matching_brace(source: &str, start: usize, end: usize, recursion: usize) -> Result<usize> {
    ensure!(
        recursion < 256,
        "Vue template literal nesting exceeds 256 levels"
    );
    let mut depth = 0;
    let mut cursor = start;
    while cursor < end {
        match byte(source, cursor) {
            b'"' | b'\'' => cursor = skip_string(source, cursor, end),
            b'`' => cursor = skip_template(source, cursor, end, recursion + 1)?.0,
            b'{' => {
                depth += 1;
                cursor += 1;
            }
            b'}' => {
                if depth == 0 {
                    return Ok(cursor);
                }
                depth -= 1;
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }
    Ok(end)
}

fn skip_regex(source: &str, start: usize, end: usize) -> usize {
    let mut cursor = start + 1;
    let mut class = false;
    while cursor < end {
        let ch = byte(source, cursor);
        if ch == b'\\' {
            cursor += 2;
            continue;
        }
        if ch == b'\n' {
            return start + 1;
        }
        if ch == b'[' {
            class = true;
        } else if ch == b']' {
            class = false;
        } else if ch == b'/' && !class {
            cursor += 1;
            while cursor < end && byte(source, cursor).is_ascii_alphabetic() {
                cursor += 1;
            }
            return cursor;
        }
        cursor += 1;
    }
    start + 1
}

#[cfg(test)]
mod tests {
    use super::calls;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Case {
        id: String,
        source: String,
        expected: Vec<(String, usize)>,
    }
    #[derive(Deserialize)]
    struct Corpus {
        revision: String,
        cases: Vec<Case>,
    }

    #[test]
    fn matches_upstream_template_calls_and_original_byte_offsets() {
        let corpus: Corpus = serde_json::from_str(include_str!(
            "../../../tests/fixtures/vue-template/cases.json"
        ))
        .unwrap();
        assert_eq!(corpus.revision, "6aa358ccbae30fbf27318f79ee183212f726f9ba");
        for case in corpus.cases {
            assert_eq!(calls(&case.source).unwrap(), case.expected, "{}", case.id);
        }
    }
}
