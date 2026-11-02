use super::query::{length, split_words, whitespace};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueryPathExtraction {
    pub stripped_query: String,
    pub pinned_files: Vec<String>,
    pub unresolved_path_spans: Vec<String>,
    pub line_anchors: Vec<QueryLineAnchor>,
    pub set_aside_matches: Vec<QuerySetAsideMatch>,
}
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct QueryLineAnchor {
    pub file: String,
    pub start: u32,
    pub end: u32,
}
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct QuerySetAsideMatch {
    pub span: String,
    pub files: Vec<String>,
}
pub type SymbolFileLookup<'a> = dyn Fn(&str) -> Vec<String> + 'a;
#[derive(Default)]
pub struct QueryPathOptions<'a> {
    pub max_pins: Option<usize>,
    pub max_matches_per_span: Option<usize>,
    pub exists_on_disk: Option<&'a dyn Fn(&str) -> bool>,
    pub symbol_files: Option<&'a SymbolFileLookup<'a>>,
}
static EXTENSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.[A-Za-z][A-Za-z0-9]{0,7}$").unwrap());
static KEBAB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9]+(?:-[A-Za-z0-9]+)+$").unwrap());
static SYMBOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z_$][A-Za-z0-9_$]*(?:(?:::|\.)[A-Za-z0-9_$]+)*$").unwrap()
});
static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z][A-Z]").unwrap());
static LINE_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?::([0-9]+)(?:-([0-9]+))?|#L([0-9]+)(?:-L?([0-9]+))?)$").unwrap()
});
static LINE_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(L?)([0-9]+)(?:(?:-|–|\.\.)L?([0-9]+))?$").unwrap());
fn dotted(value: &str) -> bool {
    !value
        .chars()
        .any(|ch| whitespace(ch) || matches!(ch, '/' | '\\'))
        && EXTENSION.find(value).is_some_and(|m| m.start() > 0)
}
fn basename(value: &str) -> &str {
    value.rsplit(['/', '\\']).next().unwrap_or(value)
}
/// Cheap shape test; only indexed path resolution determines whether a span is a file.
pub fn query_might_contain_paths(query: &str) -> bool {
    static EXT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\.[A-Za-z][A-Za-z0-9]{0,7}").unwrap());
    static KEBAB_RUN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[A-Za-z0-9]+(?:-[A-Za-z0-9]+)+").unwrap());
    query.contains(['/', '\\'])
        || EXT.find_iter(query).any(|m| {
            query[m.end()..]
                .chars()
                .next()
                .is_none_or(|ch| whitespace(ch) || ",;:)]'\"`".contains(ch))
        })
        || KEBAB_RUN.find_iter(query).any(|m| {
            let boundary = |ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '-');
            query[..m.start()].chars().next_back().is_none_or(boundary)
                && query[m.end()..].chars().next().is_none_or(boundary)
        })
}
fn span(a: &str, b: Option<&str>) -> Option<(u32, u32)> {
    // Leading zeros may exceed the integer parser's width while still naming a small line.
    let parse = |s: &str| s.trim_start_matches('0').parse::<u32>().ok();
    let start = parse(a)?;
    let end = if let Some(b) = b { parse(b)? } else { start };
    if start == 0 || end == 0 || start > 1_000_000 || end > 1_000_000 {
        None
    } else {
        Some((start.min(end), start.max(end)))
    }
}
fn strip_line(token: &str) -> &str {
    token
        .trim_start_matches(['(', '\'', '"', '`', '['])
        .trim_end_matches([')', '\'', '"', '`', ']', '.', ',', ';', ':', '!', '?'])
}
fn strip_wrapping(token: &str) -> (&str, Option<(u32, u32)>) {
    let mut value = token;
    while let Some(ch) = value.chars().next() {
        if "'\"`<".contains(ch)
            || (ch == '(' && !value.contains(')'))
            || (ch == '[' && !value.contains(']'))
            || (ch == '{' && !value.contains('}'))
        {
            value = &value[ch.len_utf8()..];
        } else {
            break;
        }
    }
    while let Some(ch) = value.chars().next_back() {
        if "'\"`>.,;!?".contains(ch)
            || (ch == ')' && !value.contains('('))
            || (ch == ']' && !value.contains('['))
            || (ch == '}' && !value.contains('{'))
        {
            value = &value[..value.len() - ch.len_utf8()];
        } else {
            break;
        }
    }
    if let Some(m) = LINE_SUFFIX.captures(value) {
        let line = if let Some(a) = m.get(1) {
            span(a.as_str(), m.get(2).map(|m| m.as_str()))
        } else {
            span(m.get(3).unwrap().as_str(), m.get(4).map(|m| m.as_str()))
        };
        (&value[..m.get(0).unwrap().start()], line)
    } else {
        (value, None)
    }
}
fn normalize_span(span: &str) -> String {
    let replaced = span.replace('\\', "/");
    let mut span = replaced.as_str();
    while let Some(rest) = span.strip_prefix("./") {
        span = rest;
    }
    let mut out = String::new();
    for ch in span.chars() {
        if ch != '/' || !out.ends_with('/') {
            out.push(ch);
        }
    }
    out.trim_end_matches('/').into()
}
fn symbols(tokens: &[&str], indexed: &[String]) -> Vec<String> {
    let basenames: HashSet<_> = indexed.iter().map(|p| basename(p).to_lowercase()).collect();
    let mut out = Vec::new();
    for raw in tokens {
        for part in raw.split([',', '(', ')', '[', ']', '{', '}']) {
            let token = part
                .trim_start_matches(['\'', '"', '`', '<'])
                .trim_end_matches(['\'', '"', '`', '>', '.', ',', ';', ':', '!', '?']);
            if length(token) < 3
                || !SYMBOL.is_match(token)
                || !(token.contains(['.', '_', '$'])
                    || token.contains("::")
                    || CAMEL.is_match(token)
                    || token.starts_with(|c: char| c.is_ascii_uppercase()))
                || basenames.contains(&token.to_lowercase())
            {
                continue;
            }
            if !out.iter().any(|old| old == token) {
                out.push(token.into());
            }
            if out.len() == 16 {
                return out;
            }
        }
    }
    out
}
struct Narrow<'a> {
    options: &'a QueryPathOptions<'a>,
    tokens: Vec<String>,
    cache: HashMap<String, Vec<String>>,
    aside: Vec<QuerySetAsideMatch>,
    max: usize,
}
impl Narrow<'_> {
    fn matches(&mut self, span: &str, matches: Vec<String>) -> Option<Vec<String>> {
        if matches.len() > 1
            && let Some(lookup) = self.options.symbol_files
        {
            let mut definers = HashSet::new();
            for token in &self.tokens {
                for file in self
                    .cache
                    .entry(token.clone())
                    .or_insert_with(|| lookup(token))
                {
                    if matches.contains(file) {
                        definers.insert(file.clone());
                    }
                }
            }
            if !definers.is_empty() && definers.len() < matches.len() && definers.len() <= self.max
            {
                self.aside.push(QuerySetAsideMatch {
                    span: span.into(),
                    files: matches
                        .iter()
                        .filter(|f| !definers.contains(*f))
                        .cloned()
                        .collect(),
                });
                return Some(
                    matches
                        .into_iter()
                        .filter(|f| definers.contains(f))
                        .collect(),
                );
            }
        }
        (matches.len() <= self.max).then_some(matches)
    }
}
/// Resolve path mentions and line anchors against a committed indexed file list.
pub fn extract_query_paths(
    query: &str,
    indexed: &[String],
    options: QueryPathOptions<'_>,
) -> QueryPathExtraction {
    let mut out = QueryPathExtraction {
        stripped_query: query.into(),
        ..Default::default()
    };
    if query.trim_matches(whitespace).is_empty() || indexed.is_empty() {
        return out;
    }
    let max_pins = options.max_pins.unwrap_or(8).max(1);
    let max_matches = options.max_matches_per_span.unwrap_or(3).max(1);
    let mut lower: Vec<(String, String)> = Vec::new();
    let mut slots = HashMap::new();
    for path in indexed {
        let key = path.to_lowercase();
        if let Some(&slot) = slots.get(&key) {
            lower[slot] = (key, path.clone());
        } else {
            slots.insert(key.clone(), lower.len());
            lower.push((key, path.clone()));
        }
    }
    let tokens: Vec<_> = split_words(query).collect();
    let mut consumed = HashSet::new();
    let mut pinned = HashSet::new();
    let mut single: Vec<(usize, String)> = Vec::new();
    let mut examined = 0;
    let mut narrow = Narrow {
        options: &options,
        tokens: if options.symbol_files.is_some() {
            symbols(&tokens, indexed)
        } else {
            Vec::new()
        },
        cache: HashMap::new(),
        aside: Vec::new(),
        max: max_matches,
    };
    for (i, token) in tokens.iter().enumerate() {
        if out.pinned_files.len() >= max_pins || examined >= 8 {
            break;
        }
        let (stripped, lines) = strip_wrapping(token);
        if length(stripped) < 4 || (!stripped.contains(['/', '\\']) && !dotted(stripped)) {
            continue;
        }
        let normalized = normalize_span(stripped);
        if normalized.is_empty() {
            continue;
        }
        examined += 1;
        let folded = normalized.to_lowercase();
        let mut matches = Vec::new();
        if let Some(&slot) = slots.get(&folded) {
            matches.push(lower[slot].1.clone());
        } else {
            let segments: Vec<_> = folded.split('/').filter(|p| !p.is_empty()).collect();
            for drop in 0..segments.len().min(8) {
                let suffix = segments[drop..].join("/");
                let ending = format!("/{suffix}");
                matches = lower
                    .iter()
                    .filter(|(key, _)| key == &suffix || key.ends_with(&ending))
                    .map(|(_, path)| path.clone())
                    .collect();
                if !matches.is_empty() {
                    break;
                }
            }
        }
        let matched = !matches.is_empty();
        let matches = if matched {
            narrow.matches(&normalized, matches)
        } else {
            None
        };
        if let Some(matches) = matches {
            consumed.insert(i);
            for file in &matches {
                if out.pinned_files.len() < max_pins && pinned.insert(file.clone()) {
                    out.pinned_files.push(file.clone());
                }
            }
            if matches.len() == 1 && pinned.contains(&matches[0]) {
                single.push((i, matches[0].clone()));
                if let Some((start, end)) = lines {
                    out.line_anchors.push(QueryLineAnchor {
                        file: matches[0].clone(),
                        start,
                        end,
                    });
                }
            }
        } else if matched
            || normalized
                .rsplit_once('/')
                .is_some_and(|(prefix, last)| !prefix.is_empty() && dotted(last))
            || (normalized.contains('/')
                && options
                    .exists_on_disk
                    .is_some_and(|exists| exists(&normalized)))
        {
            consumed.insert(i);
            if out.unresolved_path_spans.len() < 4 {
                out.unresolved_path_spans.push(normalized);
            }
        }
    }
    let mut stems: HashMap<String, Vec<String>> = HashMap::new();
    for path in indexed {
        let base = basename(path);
        if base.contains('-') {
            stems
                .entry(EXTENSION.replace(base, "").to_lowercase())
                .or_default()
                .push(path.clone());
        }
    }
    for (i, token) in tokens.iter().enumerate() {
        if out.pinned_files.len() >= max_pins {
            break;
        }
        if consumed.contains(&i) {
            continue;
        }
        let (stripped, lines) = strip_wrapping(token);
        if length(stripped) < 4 || !KEBAB.is_match(stripped) {
            continue;
        }
        let Some(matches) = stems
            .get(&stripped.to_lowercase())
            .and_then(|m| narrow.matches(stripped, m.clone()))
        else {
            continue;
        };
        consumed.insert(i);
        for file in &matches {
            if out.pinned_files.len() < max_pins && pinned.insert(file.clone()) {
                out.pinned_files.push(file.clone());
            }
        }
        if matches.len() == 1 && pinned.contains(&matches[0]) {
            single.push((i, matches[0].clone()));
            if let Some((start, end)) = lines {
                out.line_anchors.push(QueryLineAnchor {
                    file: matches[0].clone(),
                    start,
                    end,
                });
            }
        }
    }
    if !single.is_empty() {
        for (i, token) in tokens.iter().enumerate() {
            if consumed.contains(&i) {
                continue;
            }
            let after_word = i > 0
                && !consumed.contains(&(i - 1))
                && matches!(
                    strip_line(tokens[i - 1]).to_ascii_lowercase().as_str(),
                    "line" | "lines"
                );
            let after_path = i > 0 && single.iter().any(|(p, _)| *p == i - 1);
            let Some(m) = LINE_NUMBER.captures(strip_line(token)) else {
                continue;
            };
            if m[1].is_empty() && !after_word && !after_path {
                continue;
            }
            let mut lines = span(&m[2], m.get(3).map(|m| m.as_str()));
            let mut used = vec![i];
            if lines.is_some()
                && m.get(3).is_none()
                && i + 2 < tokens.len()
                && matches!(
                    strip_line(tokens[i + 1]).to_ascii_lowercase().as_str(),
                    "to" | "through" | "thru"
                )
                && let Some(tail) = LINE_NUMBER.captures(strip_line(tokens[i + 2]))
                && tail.get(3).is_none()
            {
                lines = span(&m[2], Some(&tail[2]));
                used.extend([i + 1, i + 2]);
            }
            if let Some((start, end)) = lines {
                let file = &single.iter().min_by_key(|(p, _)| p.abs_diff(i)).unwrap().1;
                out.line_anchors.push(QueryLineAnchor {
                    file: file.clone(),
                    start,
                    end,
                });
                consumed.extend(used);
                if after_word {
                    consumed.insert(i - 1);
                }
            }
        }
    }
    if consumed.is_empty() {
        return out;
    }
    out.stripped_query = tokens
        .iter()
        .enumerate()
        .filter(|(i, _)| !consumed.contains(i))
        .map(|(_, v)| *v)
        .collect::<Vec<_>>()
        .join(" ");
    let mut seen = HashSet::new();
    out.line_anchors
        .retain(|a| seen.insert((a.file.clone(), a.start, a.end)));
    for mut aside in narrow.aside {
        aside.files.retain(|f| !pinned.contains(f));
        if !aside.files.is_empty() {
            out.set_aside_matches.push(aside);
        }
    }
    out
}
