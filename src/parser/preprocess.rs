//! Upstream extraction-wrapper passes, ported from upstream commit
//! 6aa358ccbae30fbf27318f79ee183212f726f9ba (MIT; see THIRD_PARTY_NOTICES.md).
//! Positions follow upstream's UTF-16 columns, even though Rust strings use UTF-8.
use anyhow::{Context, Result};
use fancy_regex::{Captures, Regex, RegexBuilder};
use prometeu_extract::{ExtractionResult, UnresolvedReference};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

mod ccpp;
mod patterns;

pub(crate) fn prepare(path: &Path, source: &str, language: &str) -> Result<String> {
    match language {
        "c" => ccpp::prepare_c(source),
        "cpp" => ccpp::prepare_cpp(path, source),
        "objc" => ccpp::prepare_objc(source),
        "csharp" => replace(
            source,
            r"(?m)^([ \t]*)#[ \t]*(if|elif|else|endif)\b[^\n]*",
            |m| {
                format!(
                    "{}{}",
                    &m[1],
                    " ".repeat(utf16_len(&m[0]) - utf16_len(&m[1]))
                )
            },
        ),
        "typescript" | "tsx" => {
            let flow = blank_flow(path, source)?;
            blank_export_type_star(&flow)
        }
        "kotlin" => prepare_kotlin(source),
        "swift" => prepare_swift(source),
        _ => Ok(source.to_owned()),
    }
}

fn prepare_kotlin(source: &str) -> Result<String> {
    let mut out = replace(
        source,
        r"(?m)^([ \t]*(?:(?:public|private|internal|protected|expect|actual)[ \t]+)*)fun([ \t]+interface\b)",
        |m| format!("{}   {}", &m[1], &m[2]),
    )?;
    out = replace(
        &out,
        r"\b(?:[A-Za-z_]\w*(?:<[^<>()\n]*>)?\.)+(?=[A-Za-z_]\w*(?:<[^<>()\n]*>)?\.\()",
        |m| " ".repeat(utf16_len(&m[0])),
    )?;
    out = replace(
        &out,
        r"(\bclass\s+[A-Za-z_]\w*(?:\s*<[^<>{}]*(?:<[^<>{}]*>[^<>{}]*)*>)?[ \t]*\r?\n(?:[ \t]*//[^\n]*\n)*[ \t]*)((?:(?:public|private|protected|internal|actual|expect)\s+|@[\w.]+(?:\([^)]*\))?\s+)*constructor)(?=\s*\()",
        |m| format!("{}{}", &m[1], " ".repeat(utf16_len(&m[2]))),
    )?;
    if out.contains(" if") {
        out = replace(
            &out,
            r#"(?m)^([ \t]*!?(?:is|in)[ \t]+(?:(?!->)[^\n])*?[\w>?)\]'"])([ \t]+)if(?=[ \t(])"#,
            |m| format!("{},{}", &m[1], " ".repeat(m[2].len() + 1)),
        )?;
    }
    out = out.replace("..<", ".. ");
    out = replace(&out, r#"(?<![\w$"])\$\$+(?=")"#, |m| " ".repeat(m[0].len()))?;
    replace(&out, r"([\w>])\?\.\(\)(?=\s*->)", |m| {
        format!("{}.( )", &m[1])
    })
}

fn prepare_swift(source: &str) -> Result<String> {
    const HEAD: &str =
        r"^[ \t]+(?:@[\w.]+(?:\([^)]*\))?\s*)*(?:[a-z]+(?:\([a-z]+\))?\s+)*(?:typealias|let|var)\s";
    const CONTINUATION: &str = r"^([ \t]*)&([ \t]+)(?=\S)";
    if !test(r"\n[ \t]*&[ \t]", source)? {
        return Ok(source.to_owned());
    }
    let original: Vec<_> = source.split('\n').collect();
    let mut lines: Vec<_> = original.iter().map(|line| (*line).to_owned()).collect();
    for i in 1..lines.len() {
        let matches = captures(CONTINUATION, &lines[i])?;
        let Some(continuation) = matches.first() else {
            continue;
        };
        let mut head = i - 1;
        while head > 0 && test(CONTINUATION, original[head])? {
            head -= 1;
        }
        if !test(HEAD, original[head])? {
            continue;
        }
        let body = lines[i - 1].strip_suffix('\r').unwrap_or(&lines[i - 1]);
        if !test(r"[\w>)\]?!]$", body)? || body.contains("//") {
            continue;
        }
        let previous = format!(
            "{body}&{}",
            if lines[i - 1].ends_with('\r') {
                "\r"
            } else {
                ""
            }
        );
        let current = format!(
            "{}{}{}",
            &continuation[1],
            &continuation[2],
            &lines[i][continuation.get(0).unwrap().end()..]
        );
        lines[i - 1] = previous;
        lines[i] = current;
    }
    Ok(lines.join("\n"))
}

pub(crate) fn finish(
    path: &Path,
    source: &str,
    language: &str,
    result: &mut ExtractionResult,
) -> Result<()> {
    if matches!(language, "typescript" | "tsx") && blank_export_type_star(source)? != source {
        for node in &mut result.nodes {
            if node.kind == "import"
                && node
                    .signature
                    .as_deref()
                    .is_some_and(|s| s.starts_with("export"))
            {
                let start = source_offset(source, node.start_line, node.start_column)
                    .context("invalid re-export start position")?;
                let end = source_offset(source, node.end_line, node.end_column)
                    .context("invalid re-export end position")?;
                node.signature = Some(
                    source
                        .get(start..end)
                        .context("invalid re-export range")?
                        .trim()
                        .to_owned(),
                );
            }
        }
    }
    result
        .unresolved_references
        .extend(commonjs_references(path, source, language)?);
    Ok(())
}

fn source_offset(source: &str, line: u32, column: u32) -> Option<usize> {
    if line == 0 {
        return None;
    }
    let mut start = 0;
    for _ in 1..line {
        start += source.get(start..)?.find('\n')? + 1;
    }
    let mut units = 0;
    for (offset, ch) in source.get(start..)?.char_indices() {
        if units == column {
            return Some(start + offset);
        }
        if ch == '\n' {
            return None;
        }
        units += ch.len_utf16() as u32;
        if units > column {
            return None;
        }
    }
    (units == column).then_some(source.len())
}

fn blank_export_type_star(source: &str) -> Result<String> {
    if !source.contains("export") || !test(r"export\s+type\s*\*", source)? {
        return Ok(source.to_owned());
    }
    // Consume the lookahead and inspect its final boundary separately, allowing
    // the regular-expression engine to scan large declaration files linearly.
    let mut out = String::new();
    let mut at = 0;
    for m in captures(
        r"(?m)(^|[;{}]|\*/)([ \t]*export\s+)(type)(\s*\*\s*(?:from|as))",
        source,
    )? {
        let end = m.get(0).unwrap().end();
        if source
            .as_bytes()
            .get(end)
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == b'_')
        {
            continue;
        }
        let keyword = m.get(3).unwrap();
        out.push_str(&source[at..keyword.start()]);
        out.push_str("    ");
        at = keyword.end();
    }
    out.push_str(&source[at..]);
    Ok(out)
}

pub(crate) fn has_flow_pragma(source: &str) -> Result<bool> {
    let end = source
        .char_indices()
        .scan(0, |units, (at, ch)| {
            *units += ch.len_utf16();
            Some((at, *units))
        })
        .find(|(_, units)| *units > 4096)
        .map_or(source.len(), |(at, _)| at);
    let head = replace(&source[..end], r"^#![^\n]*\n", |_| String::new())?;
    let leading = captures(r"^(?:\s*(?://[^\n]*|/\*[\s\S]*?\*/))*", &head)?;
    let comments = leading.first().map_or("", |m| &m[0]);
    Ok(test(r"@flow\b", comments)? && !test(r"@noflow\b", comments)?)
}

fn blank_flow(path: &Path, source: &str) -> Result<String> {
    if !test(r"\.(?:jsx?|mjs|cjs)$", &path.to_string_lossy())? || !has_flow_pragma(source)? {
        return Ok(source.to_owned());
    }
    let mut out = source.replace("{|", "{ ").replace("|}", " }");
    out = replace(&out, r"([:<,]\s*)\?(?=[\w$({\[])", |m| {
        format!("{} ", &m[1])
    })?;
    out = replace(&out, r"\bimport\s+typeof\b", |m| {
        format!("import{}", " ".repeat(utf16_len(&m[0]) - 6))
    })?;
    out = replace(&out, r"\bopaque(?=\s+type\b)", |_| "      ".into())?;
    replace(&out, r"\.\.\.(?=\s*[},])", |_| "   ".into())
}

fn commonjs_references(
    path: &Path,
    source: &str,
    language: &str,
) -> Result<Vec<UnresolvedReference>> {
    if !matches!(language, "javascript" | "jsx" | "typescript" | "tsx")
        || !source.contains("require")
    {
        return Ok(Vec::new());
    }
    let excluded = javascript_non_code(source, language)?;
    let mut safe = source.as_bytes().to_vec();
    for (range, kind) in &excluded {
        if *kind == "comment" {
            for byte in &mut safe[range.clone()] {
                if !matches!(*byte, b'\n' | b'\r') {
                    *byte = b' ';
                }
            }
        }
    }
    let safe = String::from_utf8(safe)?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let file_path = path.to_string_lossy();
    for m in captures(
        r#"(?<![\w$.])require\s*\(\s*(?:'([^'\n]+)'|"([^"\n]+)"|`([^`$\n]+)`)\s*\)"#,
        &safe,
    )? {
        let at = m.get(0).unwrap().start();
        let before = excluded.partition_point(|(range, _)| range.start <= at);
        if before > 0 && at < excluded[before - 1].0.end {
            continue;
        }
        let specifier = (1..=3)
            .find_map(|i| m.get(i))
            .map_or("", |m| m.as_str())
            .trim();
        if !test(r"^(?:\.{1,2}(?:/|$)|/|[@~]/|#)", specifier)? || seen.contains(specifier) {
            continue;
        }
        let line_start = safe[..at].rfind('\n').map_or(0, |i| i + 1);
        if test(
            r"\bimport\s+(?:type\s+)?[\w$]+\s*=\s*$",
            &safe[line_start..at],
        )? {
            continue;
        }
        seen.insert(specifier.to_owned());
        out.push(UnresolvedReference {
            from_node_id: format!("file:{file_path}"),
            reference_name: specifier.to_owned(),
            reference_kind: "imports".into(),
            line: safe[..at].bytes().filter(|c| *c == b'\n').count() as u32 + 1,
            column: utf16_len(&source[line_start..at]) as u32,
            file_path: Some(file_path.to_string()),
            language: Some(language.to_owned()),
            candidates: None,
        });
    }
    Ok(out)
}

/// Token spans exclude literal text while leaving template substitutions executable.
pub(super) fn javascript_non_code(
    source: &str,
    language: &str,
) -> Result<Vec<(Range<usize>, &'static str)>> {
    let grammar = match language {
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX,
        _ => tree_sitter_javascript::LANGUAGE,
    };
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&grammar.into())?;
    let tree = parser
        .parse(source, None)
        .context("cannot parse JavaScript token spans")?;
    let mut pending = vec![tree.root_node()];
    let mut ranges = Vec::new();
    while let Some(node) = pending.pop() {
        if matches!(
            node.kind(),
            "comment" | "string" | "regex" | "string_fragment" | "escape_sequence"
        ) {
            ranges.push((node.byte_range(), node.kind()));
        } else {
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
    }
    ranges.sort_unstable_by_key(|(range, _)| range.start);
    Ok(ranges)
}

pub(super) fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn blank(text: &str, keep_cr: bool) -> String {
    text.chars()
        .flat_map(|ch| {
            let keep = ch == '\n' || keep_cr && ch == '\r';
            std::iter::repeat_n(
                if keep { ch } else { ' ' },
                if keep { 1 } else { ch.len_utf16() },
            )
        })
        .collect()
}

fn blank_spans(source: &str, spans: &[Range<usize>], keep_cr: bool) -> String {
    let mut spans = spans.to_vec();
    spans.sort_unstable_by_key(|span| span.start);
    let mut out = String::new();
    let mut at = 0;
    for span in spans {
        let from = span.start.max(at);
        if from >= span.end {
            continue;
        }
        out.push_str(&source[at..from]);
        out.push_str(&blank(&source[from..span.end], keep_cr));
        at = span.end;
    }
    out.push_str(&source[at..]);
    out
}

pub(super) fn regex(pattern: &'static str) -> Result<Arc<Regex>> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<Regex>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("preparse regex cache poisoned"))?;
    if let Some(compiled) = cache.get(pattern) {
        return Ok(Arc::clone(compiled));
    }
    // JavaScript's \w and word boundaries are ASCII. Do not reinterpret non-ASCII names.
    let pattern_ascii = super::javascript_regex::pattern(pattern);
    let compiled = Arc::new(
        RegexBuilder::new(&pattern_ascii)
            .backtrack_limit(1_000_000)
            .build()
            .with_context(|| format!("invalid upstream preparse pattern: {pattern}"))?,
    );
    cache.insert(pattern, Arc::clone(&compiled));
    Ok(compiled)
}

pub(super) fn test(pattern: &'static str, source: &str) -> Result<bool> {
    Ok(regex(pattern)?.is_match(source)?)
}
pub(super) fn captures<'a>(pattern: &'static str, source: &'a str) -> Result<Vec<Captures<'a>>> {
    regex(pattern)?
        .captures_iter(source)
        .map(|result| result.map_err(Into::into))
        .collect()
}
fn first_at<'a>(
    pattern: &'static str,
    source: &'a str,
    start: usize,
) -> Result<Option<Captures<'a>>> {
    Ok(regex(pattern)?
        .captures_from_pos(source, start)?
        .filter(|m| m.get(0).unwrap().start() == start))
}
pub(super) fn replace(
    source: &str,
    pattern: &'static str,
    mut replacement: impl FnMut(&Captures<'_>) -> String,
) -> Result<String> {
    let mut out = String::new();
    let mut at = 0;
    for m in captures(pattern, source)? {
        let span = m.get(0).unwrap();
        out.push_str(&source[at..span.start()]);
        out.push_str(&replacement(&m));
        at = span.end();
    }
    out.push_str(&source[at..]);
    Ok(out)
}

pub(super) fn skip_space(source: &str, mut at: usize) -> usize {
    while let Some(ch) = source.get(at..).and_then(|s| s.chars().next()) {
        if !ch.is_whitespace() && ch != '\u{feff}' {
            break;
        }
        at += ch.len_utf8();
    }
    at
}
pub(super) fn byte(source: &str, at: usize) -> u8 {
    source.as_bytes().get(at).copied().unwrap_or(0)
}
fn prefix_start(source: &str, at: usize, distance: usize) -> usize {
    let mut start = at.saturating_sub(distance);
    while !source.is_char_boundary(start) {
        start += 1;
    }
    start
}
fn word(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

fn balanced_parens(source: &str, open: usize, stop_punctuation: bool) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < source.len() {
        match byte(source, i) {
            b'\'' | b'"' => {
                let quote = byte(source, i);
                i += 1;
                while i < source.len() && byte(source, i) != quote {
                    if byte(source, i) == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            b';' | b'{' | b'}' if stop_punctuation => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Fixture {
        upstream: String,
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        name: String,
        language: String,
        path: String,
        source: String,
        prepared: String,
        commonjs: Vec<UnresolvedReference>,
    }

    #[test]
    fn matches_pinned_upstream_preparse_and_commonjs() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/preprocessing/cases.json"
        ))
        .unwrap();
        assert_eq!(fixture.upstream, "6aa358ccbae30fbf27318f79ee183212f726f9ba");
        let mut failures = Vec::new();
        for case in fixture.cases {
            match prepare(Path::new(&case.path), &case.source, &case.language) {
                Ok(actual) if actual == case.prepared => {}
                Ok(actual) => {
                    let first = actual
                        .bytes()
                        .zip(case.prepared.bytes())
                        .position(|(a, b)| a != b)
                        .unwrap_or(actual.len().min(case.prepared.len()));
                    failures.push(format!(
                        "{} ({}): preparse differs at byte {first}\nactual: {:?}\nexpected: {:?}",
                        case.name, case.language, actual, case.prepared
                    ));
                }
                Err(error) => {
                    failures.push(format!("{} ({}): {error:#}", case.name, case.language))
                }
            }
            let actual =
                commonjs_references(Path::new(&case.path), &case.source, &case.language).unwrap();
            assert_eq!(
                actual, case.commonjs,
                "{} ({}) CommonJS",
                case.name, case.language
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn restores_type_reexport_signatures_at_utf16_columns() {
        let source = "/* 😀 */ export type * from './a';\nexport type * as ns from './b';\n";
        let prepared = prepare(Path::new("sample.ts"), source, "typescript").unwrap();
        let mut extracted =
            prometeu_extract::extract("sample.ts", &prepared, "typescript").unwrap();
        finish(Path::new("sample.ts"), source, "typescript", &mut extracted).unwrap();
        let signatures: Vec<_> = extracted
            .nodes
            .iter()
            .filter(|n| n.kind == "import")
            .map(|n| n.signature.as_deref().unwrap())
            .collect();
        assert!(signatures.contains(&"export type * from './a';"));
        assert!(signatures.contains(&"export type * as ns from './b';"));
    }

    #[test]
    fn unchanged_large_typescript_skips_backtracking_patterns() {
        let mut source = (0..20_000)
            .map(|n| format!("function f{n}() {{}}\n"))
            .collect::<String>();
        assert_eq!(
            prepare(Path::new("large.ts"), &source, "typescript").unwrap(),
            source
        );
        source.push_str("export type * from './types';\n");
        let prepared = prepare(Path::new("large.ts"), &source, "typescript").unwrap();
        assert!(prepared.ends_with("export      * from './types';\n"));
        source.push_str("const dependency = require('./local');\n");
        let imports = commonjs_references(Path::new("large.ts"), &source, "typescript").unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].reference_name, "./local");
        assert_eq!(imports[0].line, 20_002);
    }
}
