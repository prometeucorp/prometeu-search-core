use super::{query::length, ranking::add};
use regex::Regex;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;
static RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}]+").unwrap());
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{N}+$").unwrap());
static LOWER_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{Ll}\p{N}]$").unwrap());
static UPPER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Lu}$").unwrap());
static LOWER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Ll}$").unwrap());
static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{M}+").unwrap());
fn matches(regex: &Regex, ch: char) -> bool {
    let mut buffer = [0; 4];
    regex.is_match(ch.encode_utf8(&mut buffer))
}
/// Identifier word segments, capped to exclude minified names and hashes.
pub fn split_identifier_segments(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for run in RUN.find_iter(name) {
        let run = run.as_str();
        let chars: Vec<_> = run.char_indices().collect();
        let mut starts = vec![0];
        for i in 1..chars.len() {
            if matches(&UPPER, chars[i].1)
                && (matches(&LOWER_NUMBER, chars[i - 1].1)
                    || (matches(&UPPER, chars[i - 1].1)
                        && chars.get(i + 1).is_some_and(|(_, ch)| matches(&LOWER, *ch))))
            {
                starts.push(chars[i].0);
            }
        }
        starts.push(run.len());
        for bounds in starts.windows(2) {
            if out.len() >= 12 {
                return out;
            }
            let segment = run[bounds[0]..bounds[1]].to_lowercase();
            if (2..=32).contains(&length(&segment)) && !NUMBER.is_match(&segment) {
                add(&mut out, segment);
            }
        }
    }
    out
}
pub fn normalize_prose_word(word: &str) -> String {
    MARK.replace_all(&word.nfd().collect::<String>(), "")
        .to_lowercase()
}
pub fn extract_prose_candidates(prompt: &str) -> Vec<String> {
    let mut out = Vec::new();
    for run in RUN.find_iter(prompt) {
        if out.len() >= 16 {
            break;
        }
        if length(run.as_str()) > 24 {
            continue;
        }
        let word = normalize_prose_word(run.as_str());
        if (4..=24).contains(&length(&word))
            && !NUMBER.is_match(&word)
            && !STOPWORDS.contains(&word.as_str())
        {
            add(&mut out, word);
        }
    }
    out
}
pub fn extract_segment_search_words(query: &str) -> Vec<String> {
    static HUMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{Ll}\p{N}]\p{Lu}").unwrap());
    let mut out = extract_prose_candidates(query);
    let mut segments = Vec::new();
    for run in RUN.find_iter(query) {
        if HUMP.is_match(run.as_str()) {
            segments.extend(split_identifier_segments(run.as_str()));
        }
    }
    for word in extract_prose_candidates(&segments.join(" ")) {
        add(&mut out, word);
    }
    out
}
pub fn segment_lookup_variants(word: &str) -> Vec<String> {
    let mut out = vec![word.to_string()];
    let len = length(word);
    if ["xes", "shes", "sses", "zzes"]
        .iter()
        .any(|suffix| word.ends_with(suffix))
    {
        if len >= 6 {
            out.push(word[..word.len() - 2].into());
        }
    } else if ["ches", "ses", "zes", "oes"]
        .iter()
        .any(|suffix| word.ends_with(suffix))
    {
        if len >= 6 {
            out.push(word[..word.len() - 2].into());
        }
        if len >= 5 {
            out.push(word[..word.len() - 1].into());
        }
    } else if word.ends_with('s') && !word.ends_with("ss") && len >= 5 {
        out.push(word[..word.len() - 1].into());
    }
    out
}

const STOPWORDS: &[&str] = &[
    "about",
    "above",
    "actually",
    "after",
    "again",
    "against",
    "almost",
    "along",
    "also",
    "always",
    "another",
    "anything",
    "around",
    "away",
    "back",
    "because",
    "been",
    "before",
    "behind",
    "being",
    "below",
    "best",
    "better",
    "between",
    "both",
    "cannot",
    "come",
    "could",
    "does",
    "doing",
    "done",
    "down",
    "each",
    "either",
    "else",
    "even",
    "ever",
    "every",
    "everything",
    "fine",
    "first",
    "from",
    "getting",
    "give",
    "goes",
    "going",
    "gone",
    "good",
    "great",
    "have",
    "having",
    "help",
    "here",
    "inside",
    "instead",
    "into",
    "just",
    "keep",
    "know",
    "last",
    "least",
    "less",
    "like",
    "likely",
    "little",
    "look",
    "looking",
    "made",
    "make",
    "making",
    "many",
    "maybe",
    "mind",
    "more",
    "most",
    "much",
    "must",
    "need",
    "needs",
    "never",
    "next",
    "nice",
    "none",
    "nothing",
    "okay",
    "only",
    "onto",
    "other",
    "otherwise",
    "over",
    "please",
    "pretty",
    "probably",
    "quite",
    "rather",
    "really",
    "right",
    "same",
    "seem",
    "seems",
    "should",
    "show",
    "since",
    "some",
    "someone",
    "something",
    "somewhere",
    "soon",
    "still",
    "such",
    "sure",
    "take",
    "than",
    "thank",
    "thanks",
    "that",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "thing",
    "things",
    "think",
    "this",
    "those",
    "though",
    "tried",
    "tries",
    "trying",
    "under",
    "until",
    "upon",
    "very",
    "want",
    "wants",
    "well",
    "went",
    "were",
    "what",
    "when",
    "which",
    "while",
    "will",
    "wish",
    "with",
    "within",
    "without",
    "would",
    "wrong",
    "your",
    "yours",
    "again",
    "change",
    "changes",
    "check",
    "class",
    "classes",
    "code",
    "detail",
    "details",
    "directory",
    "error",
    "errors",
    "example",
    "examples",
    "file",
    "files",
    "folder",
    "function",
    "functions",
    "issue",
    "issues",
    "line",
    "lines",
    "method",
    "methods",
    "name",
    "names",
    "problem",
    "problems",
    "project",
    "question",
    "questions",
    "rename",
    "test",
    "tests",
    "type",
    "types",
    "update",
    "value",
    "values",
    "warning",
    "warnings",
    "work",
    "working",
    "write",
    "writing",
];
