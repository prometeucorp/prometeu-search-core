// Symbol selection port; see the repository third-party notices.
use crate::search::{blank_non_ascii_words, meets_query_word_floor, split_at_cjk_switches};
use anyhow::Result;
use std::collections::HashSet;
const COMMON: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "from",
    "this",
    "that",
    "have",
    "been",
    "will",
    "would",
    "could",
    "should",
    "does",
    "done",
    "make",
    "made",
    "use",
    "used",
    "using",
    "work",
    "works",
    "find",
    "found",
    "show",
    "call",
    "called",
    "calling",
    "get",
    "set",
    "add",
    "all",
    "any",
    "how",
    "what",
    "when",
    "where",
    "which",
    "who",
    "why",
    "not",
    "but",
    "are",
    "was",
    "were",
    "has",
    "had",
    "its",
    "can",
    "did",
    "may",
    "also",
    "into",
    "than",
    "then",
    "them",
    "each",
    "other",
    "some",
    "such",
    "only",
    "same",
    "about",
    "after",
    "before",
    "between",
    "through",
    "during",
    "without",
    "again",
    "further",
    "once",
    "here",
    "there",
    "both",
    "just",
    "more",
    "most",
    "very",
    "being",
    "having",
    "doing",
    "system",
    "need",
    "needs",
    "want",
    "wants",
    "like",
    "look",
    "change",
    "changes",
    "changed",
    "changing",
    "layer",
    "handle",
    "handles",
    "handling",
    "incoming",
    "outgoing",
    "data",
    "flow",
    "flows",
    "level",
    "levels",
    "request",
    "requests",
    "response",
    "responses",
    "implement",
    "implements",
    "implementation",
    "interface",
    "interfaces",
    "class",
    "classes",
    "method",
    "methods",
    "trigger",
    "triggers",
    "affected",
    "affect",
    "affects",
    "else",
    "code",
    "failing",
    "failed",
    "silently",
    "decide",
    "decides",
    "return",
    "returns",
    "returned",
    "take",
    "takes",
    "taken",
    "check",
    "checks",
    "checked",
    "create",
    "creates",
    "created",
    "read",
    "reads",
    "write",
    "writes",
    "written",
    "start",
    "starts",
    "stop",
    "stops",
    "run",
    "runs",
    "running",
];
pub fn extract_symbols_from_query(query: &str) -> Result<Vec<String>> {
    let ascii = blank_non_ascii_words(query);
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |s: &str| {
        if seen.insert(s.to_string()) {
            found.push(s.to_string());
        }
    };
    for pattern in [
        r"\b([A-Z][a-z]+(?:[A-Z][a-z]*)*|[a-z]+(?:[A-Z][a-z]*)+)\b",
        r"(?i)\b([a-z][a-z0-9]*(?:_[a-z0-9]+)+)\b",
        r"\b([A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+)\b",
        r"\b([A-Z]{2,})\b",
        r"\b([a-zA-Z][a-zA-Z0-9]*(?:\.[a-zA-Z][a-zA-Z0-9]*)+)\b",
        r"\b([a-z][a-z0-9]{2,})\b",
    ] {
        let regex = fancy_regex::Regex::new(&crate::parser::javascript_regex::pattern(pattern))?;
        for captures in regex.captures_iter(&ascii) {
            let captures = captures?;
            let matched = captures.get(1).unwrap().as_str();
            add(matched);
            if matched.contains('.') {
                for part in matched.split('.').filter(|s| s.len() >= 2) {
                    add(part);
                }
            }
        }
    }
    let regex = regex::Regex::new(r"[\p{L}\p{N}\p{M}_$]+")?;
    for run in regex
        .find_iter(query)
        .map(|m| m.as_str())
        .filter(|s| !s.is_ascii())
    {
        let words: Vec<_> = run
            .split(['_', '$'])
            .flat_map(split_at_cjk_switches)
            .collect();
        if words.len() > 1 && meets_query_word_floor(run) {
            add(run);
        }
        for word in words {
            if !word.is_ascii() && meets_query_word_floor(word) {
                add(word);
            }
        }
    }
    Ok(found
        .into_iter()
        .filter(|s| !COMMON.contains(&s.to_lowercase().as_str()))
        .collect())
}
