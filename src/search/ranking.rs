use super::query::{length, split_words, whitespace};
use regex::Regex;
use std::{collections::HashSet, sync::LazyLock};

pub fn normalize_name_token(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}
static CJK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{scx:Han}\p{scx:Hiragana}\p{scx:Katakana}\p{scx:Hangul}]").unwrap()
});
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}\p{M}]+").unwrap());
static LETTER_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}]$").unwrap());
static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{M}$").unwrap());
pub fn has_cjk(text: &str) -> bool {
    CJK.is_match(text)
}
pub fn split_at_cjk_switches(text: &str) -> Vec<&str> {
    if !has_cjk(text) {
        return vec![text];
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut previous = None;
    for (offset, ch) in text.char_indices() {
        let mut buffer = [0; 4];
        let value = ch.encode_utf8(&mut buffer);
        if LETTER_NUMBER.is_match(value) {
            let cjk = has_cjk(value);
            if previous.is_some_and(|old| old != cjk) {
                parts.push(&text[start..offset]);
                start = offset;
            }
            previous = Some(cjk);
        } else if !MARK.is_match(value) {
            previous = None;
        }
    }
    if start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}
pub fn meets_query_word_floor(word: &str) -> bool {
    if has_cjk(word) {
        word.chars().count() >= 2
    } else {
        length(word) >= 3
    }
}
pub fn is_query_name_token(token: &str) -> bool {
    static NAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[\p{L}\p{Nl}_$][\p{L}\p{N}\p{M}_$]*(?:(?:::|\.)[\p{L}\p{N}\p{M}_$]+)*$")
            .unwrap()
    });
    meets_query_word_floor(token) && NAME.is_match(token)
}
pub fn blank_non_ascii_words(text: &str) -> String {
    WORD.replace_all(text, |captures: &regex::Captures<'_>| {
        split_at_cjk_switches(&captures[0])
            .into_iter()
            .map(|part| {
                if !part.is_ascii() && !has_cjk(part) {
                    " ".repeat(length(part))
                } else {
                    part.to_string()
                }
            })
            .collect::<String>()
    })
    .into_owned()
}
pub(super) fn camel_split(query: &str, acronyms: bool) -> String {
    static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([a-z])([A-Z])").unwrap());
    static ACRONYM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"([A-Z]+)([A-Z][a-z])").unwrap());
    let result = CAMEL.replace_all(query, "$1 $2");
    if acronyms {
        ACRONYM.replace_all(&result, "$1 $2").into_owned()
    } else {
        result.into_owned()
    }
}
pub(super) fn add(out: &mut Vec<String>, value: String) {
    if !out.contains(&value) {
        out.push(value);
    }
}
pub fn stem_variants(term: &str) -> Vec<String> {
    let t = term.to_lowercase();
    let len = length(&t);
    let mut out = Vec::new();
    for suffix in ["ing", "er"] {
        if t.ends_with(suffix) && len > if suffix == "ing" { 5 } else { 4 } {
            let base = &t[..t.len() - suffix.len()];
            add(&mut out, base.into());
            add(&mut out, format!("{base}e"));
            let chars: Vec<_> = base.char_indices().collect();
            if chars.len() >= 2
                && chars[chars.len() - 1].1.len_utf16() == 1
                && chars[chars.len() - 1].1 == chars[chars.len() - 2].1
            {
                add(&mut out, base[..chars[chars.len() - 1].0].into());
            }
        }
        // The -er pass follows all other suffixes in the reference.
        if suffix == "ing" {
            if (t.ends_with("tion") || t.ends_with("sion")) && len > 5 {
                add(&mut out, t[..t.len() - 3].into());
            }
            if t.ends_with("ment") && len > 6 {
                add(&mut out, t[..t.len() - 4].into());
            }
            if t.ends_with("ies") && len > 4 {
                add(&mut out, format!("{}y", &t[..t.len() - 3]));
            } else if t.ends_with("es") && len > 4 {
                add(&mut out, t[..t.len() - 2].into());
            } else if t.ends_with('s') && !t.ends_with("ss") && len > 4 {
                add(&mut out, t[..t.len() - 1].into());
            }
            if t.ends_with("ed") && !t.ends_with("eed") && len > 4 {
                add(&mut out, t[..t.len() - 1].into());
                add(&mut out, t[..t.len() - 2].into());
                if t.ends_with("ied") && len > 5 {
                    add(&mut out, format!("{}y", &t[..t.len() - 3]));
                }
            }
        }
    }
    out.retain(|v| length(v) >= 3 && v != &t);
    out
}
pub fn extract_search_terms(query: &str, stems: bool) -> Vec<String> {
    static COMPOUND: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?-u:\b)([a-zA-Z][a-zA-Z0-9]*(?:[A-Z][a-z]+)+|[A-Z][a-z]+(?:[A-Z][a-z]*)+)(?-u:\b)",
        )
        .unwrap()
    });
    static SNAKE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?-u:\b)([a-zA-Z][a-zA-Z0-9]*(?:_[a-zA-Z0-9]+)+)(?-u:\b)").unwrap()
    });
    let ascii = blank_non_ascii_words(query);
    let mut out = Vec::new();
    for regex in [&*COMPOUND, &*SNAKE] {
        for part in regex.find_iter(&ascii) {
            if length(part.as_str()) >= 3 {
                add(&mut out, part.as_str().to_lowercase());
            }
        }
    }
    let normal = camel_split(query, true).replace(['_', '.'], " ");
    for found in WORD.find_iter(&normal) {
        for word in split_at_cjk_switches(found.as_str()) {
            let lower = word.to_lowercase();
            if meets_query_word_floor(&lower) && !STOP_WORDS.contains(&lower.as_str()) {
                add(&mut out, lower);
            }
        }
    }
    if stems {
        let mut variants = Vec::new();
        for token in &out {
            for stem in stem_variants(token) {
                if !STOP_WORDS.contains(&stem.as_str()) && !out.contains(&stem) {
                    add(&mut variants, stem);
                }
            }
        }
        out.extend(variants);
    }
    out
}

pub fn name_match_bonus(name: &str, query: &str) -> f64 {
    let name = name.to_lowercase();
    let split = camel_split(query, false);
    let terms: Vec<_> = split
        .split(|ch| whitespace(ch) || matches!(ch, '_' | '.' | '-'))
        .map(str::to_lowercase)
        .filter(|t| length(t) >= 2)
        .collect();
    let tokens: Vec<_> = split_words(query)
        .map(str::to_lowercase)
        .filter(|t| length(t) >= 2)
        .collect();
    let query: String = query
        .chars()
        .filter(|ch| !whitespace(*ch))
        .collect::<String>()
        .to_lowercase();
    if name == query {
        80.0
    } else if tokens.len() > 1 && tokens.contains(&name) {
        60.0
    } else if name.starts_with(&query) {
        (10.0 + 30.0 * length(&query) as f64 / length(&name) as f64).round()
    } else if terms.len() > 1 && terms.iter().all(|term| name.contains(term)) {
        15.0
    } else if name.contains(&query) {
        10.0
    } else {
        0.0
    }
}
pub fn kind_bonus(kind: &str) -> f64 {
    match kind {
        "function" | "method" => 10.0,
        "class" | "component" => 8.0,
        "interface" | "trait" | "protocol" | "route" => 9.0,
        "type_alias" | "struct" | "union" => 6.0,
        "enum" => 5.0,
        "module" | "namespace" => 4.0,
        "property" | "field" | "constant" | "enum_member" => 3.0,
        "variable" => 2.0,
        "import" | "export" => 1.0,
        _ => 0.0,
    }
}
pub fn is_distinctive_identifier(token: &str) -> bool {
    has_cjk(token)
        || token.chars().any(|ch| ch == '_' || ch.is_ascii_digit())
        || token.chars().skip(1).any(|ch| ch.is_ascii_uppercase())
}
pub(super) struct PathQuery {
    empty: bool,
    words: Vec<(Vec<String>, usize)>,
    tests: bool,
}
impl PathQuery {
    pub(super) fn new(query: &str, project: &HashSet<String>) -> Self {
        let all: Vec<_> = split_words(query).collect();
        let kept: Vec<_> = all
            .iter()
            .copied()
            .filter(|w| !project.contains(&normalize_name_token(w)))
            .collect();
        let scored = if kept.is_empty() { &all } else { &kept };
        let mut words: Vec<(Vec<String>, usize)> = Vec::new();
        for word in scored {
            let tokens = extract_search_terms(word, false);
            if tokens.is_empty() {
                continue;
            }
            if let Some((_, count)) = words.iter_mut().find(|(old, _)| old == &tokens) {
                *count += 1;
            } else {
                words.push((tokens, 1));
            }
        }
        let lower = query.to_lowercase();
        Self {
            empty: all.is_empty(),
            words,
            tests: lower.contains("test") || lower.contains("spec"),
        }
    }
    pub(super) fn score(&self, path: &str, deprioritized: bool) -> f64 {
        if self.empty {
            return 0.0;
        }
        let lower = path.to_lowercase();
        let name = lower.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
        let dir = lower
            .trim_end_matches('/')
            .rsplit_once('/')
            .map_or(".", |(dir, _)| if dir.is_empty() { "/" } else { dir });
        let mut score = 0;
        for (tokens, count) in &self.words {
            if tokens.iter().any(|t| name.contains(t)) {
                score += 10 * count;
            }
            if tokens.iter().any(|t| dir.contains(t)) {
                score += 5 * count;
            } else if tokens.iter().any(|t| lower.contains(t)) {
                score += 3 * count;
            }
        }
        score as f64
            - if (!self.tests && crate::classify::is_test_file(path)) || deprioritized {
                15.0
            } else {
                0.0
            }
    }
}
pub fn score_path_relevance(
    path: &str,
    query: &str,
    project: &HashSet<String>,
    deprioritized: bool,
) -> f64 {
    PathQuery::new(query, project).score(path, deprioritized)
}

const STOP_WORDS: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "or",
    "but",
    "in",
    "on",
    "at",
    "to",
    "for",
    "of",
    "with",
    "by",
    "from",
    "is",
    "it",
    "that",
    "this",
    "are",
    "was",
    "be",
    "has",
    "had",
    "have",
    "do",
    "does",
    "did",
    "will",
    "would",
    "could",
    "should",
    "may",
    "might",
    "can",
    "shall",
    "not",
    "no",
    "all",
    "each",
    "every",
    "how",
    "what",
    "where",
    "when",
    "who",
    "which",
    "why",
    "i",
    "me",
    "my",
    "we",
    "our",
    "you",
    "your",
    "he",
    "she",
    "they",
    "show",
    "give",
    "tell",
    "been",
    "done",
    "made",
    "used",
    "using",
    "work",
    "works",
    "found",
    "also",
    "into",
    "then",
    "than",
    "just",
    "more",
    "some",
    "such",
    "over",
    "only",
    "out",
    "its",
    "so",
    "up",
    "as",
    "if",
    "look",
    "need",
    "needs",
    "want",
    "happen",
    "happens",
    "affect",
    "affected",
    "break",
    "breaks",
    "failing",
    "implemented",
    "implement",
    "code",
    "file",
    "files",
    "function",
    "method",
    "class",
    "type",
    "fix",
    "bug",
    "called",
];
