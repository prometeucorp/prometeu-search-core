//! Field-qualified lexical search and reference query normalization.
mod identifiers;
mod paths;
mod query;
pub use identifiers::{
    extract_prose_candidates, extract_segment_search_words, normalize_prose_word,
    segment_lookup_variants, split_identifier_segments,
};
pub use paths::{
    QueryLineAnchor, QueryPathExtraction, QueryPathOptions, QuerySetAsideMatch, SymbolFileLookup,
    extract_query_paths, query_might_contain_paths,
};
mod ranking;
pub use query::{KINDS, ParsedQuery, bounded_edit_distance, parse_query};
pub use ranking::{
    blank_non_ascii_words, extract_search_terms, has_cjk, is_distinctive_identifier,
    is_query_name_token, kind_bonus, meets_query_word_floor, name_match_bonus,
    normalize_name_token, score_path_relevance, split_at_cjk_switches, stem_variants,
};

use anyhow::Result;
pub(crate) use query::whitespace as query_whitespace;
use query::{length, split_words};
use rusqlite::{Connection, OptionalExtension, params_from_iter, types::Value};
use std::{collections::HashSet, path::Path};

const SELECT: &str = "SELECT n.symbol_id,n.name,n.kind,n.path,f.deprioritized,";
const FROM: &str = " FROM canonical_search n JOIN files f ON f.path=n.path ";
/// Search paging follows exact-name supplementation and final ranking.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    pub offset: usize,
    /// Optional serialized response budget. No byte ceiling is imposed by default.
    pub max_bytes: Option<usize>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 100,
            offset: 0,
            max_bytes: None,
        }
    }
}
#[derive(Debug)]
pub(crate) struct RankedHit {
    pub id: i64,
    pub score: f64,
    name: String,
    kind: String,
    path: String,
    deprioritized: bool,
}

fn constraints(parsed: &ParsedQuery, values: &mut Vec<Value>) -> String {
    let mut sql = String::new();
    for (column, filters) in [("n.kind", &parsed.kinds), ("n.language", &parsed.languages)] {
        if !filters.is_empty() {
            sql.push_str(&format!(
                " AND {column} IN ({})",
                vec!["?"; filters.len()].join(",")
            ));
            values.extend(filters.iter().cloned().map(Value::Text));
        }
    }
    // ASCII narrowing preserves Unicode matches for the final JavaScript-style fold.
    for (column, filters) in [
        ("n.path", &parsed.path_filters),
        ("n.name", &parsed.name_filters),
    ] {
        if !filters.is_empty() {
            sql.push_str(" AND (");
            for (i, value) in filters.iter().enumerate() {
                if i > 0 {
                    sql.push_str(" OR ");
                }
                if value.chars().all(|c| (' '..='~').contains(&c)) {
                    sql.push_str(&format!("instr(lower({column}),?)>0"));
                    values.push(Value::Text(value.to_lowercase()));
                } else {
                    sql.push('1');
                }
            }
            sql.push_str(&format!(" OR {column} GLOB '*[^ -~]*')"));
        }
    }
    sql
}
fn passes(value: &str, filters: &[String]) -> bool {
    filters.is_empty() || {
        let lower = value.to_lowercase();
        filters
            .iter()
            .any(|filter| lower.contains(&filter.to_lowercase()))
    }
}
fn page(
    db: &Connection,
    mut sql: String,
    mut values: Vec<Value>,
    parsed: &ParsedQuery,
    order: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<RankedHit>> {
    sql.push_str(&constraints(parsed, &mut values));
    sql.push_str(order);
    // Stream field-filtered rows: the limit counts matches, not rejected candidates.
    let filtered = !parsed.path_filters.is_empty() || !parsed.name_filters.is_empty();
    if !filtered {
        sql.push_str(" LIMIT ? OFFSET ?");
        values.push(Value::Integer(limit as i64));
        values.push(Value::Integer(offset as i64));
    }
    let mut statement = db.prepare(&sql)?;
    let mut rows = statement.query(params_from_iter(values))?;
    let mut out = Vec::new();
    let mut skipped = 0;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        let path: String = row.get(3)?;
        if !passes(&name, &parsed.name_filters) || !passes(&path, &parsed.path_filters) {
            continue;
        }
        if filtered && skipped < offset {
            skipped += 1;
            continue;
        }
        out.push(RankedHit {
            id: row.get(0)?,
            name,
            kind: row.get(2)?,
            path,
            deprioritized: row.get(4)?,
            score: row.get(5)?,
        });
        if out.len() == limit {
            break;
        }
    }
    Ok(out)
}
fn fuzzy(
    db: &Connection,
    text: &str,
    parsed: &ParsedQuery,
    limit: usize,
) -> Result<Vec<RankedHit>> {
    let lowered = text.to_lowercase();
    let max = if length(&lowered) <= 4 { 1 } else { 2 };
    let mut statement = db.prepare("SELECT DISTINCT name FROM canonical_search")?;
    let mut rows = statement.query([])?;
    let mut names = Vec::new();
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let lower = name.to_lowercase();
        if length(&lower).abs_diff(length(&lowered)) > max {
            continue;
        }
        let dist = bounded_edit_distance(&lower, &lowered, max);
        if dist <= max {
            names.push((name, dist));
        }
    }
    names.sort_by_key(|(_, dist)| *dist);
    if parsed.name_filters.is_empty() && parsed.path_filters.is_empty() {
        names.truncate(limit.saturating_mul(2).max(50));
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (name, dist) in names {
        if !passes(&name, &parsed.name_filters) {
            continue;
        }
        let hits = page(
            db,
            format!("{SELECT}?{FROM}WHERE n.name=?"),
            vec![Value::Real(1.0 / (1 + dist) as f64), Value::Text(name)],
            parsed,
            "",
            5,
            0,
        )?;
        for hit in hits {
            if seen.insert(hit.id) {
                out.push(hit);
            }
            if out.len() == limit {
                return Ok(out);
            }
        }
    }
    Ok(out)
}
fn project_names(db: &Connection, root: &Path) -> Result<HashSet<String>> {
    let mut tokens = HashSet::new();
    let mut add = |name: &str| {
        let name = normalize_name_token(name);
        if name.len() >= 5 {
            tokens.insert(name);
        }
    };
    if let Some(name) = root.file_name().and_then(|s| s.to_str()) {
        add(name);
    }
    for (key, file) in [
        ("project_manifests", "package.json"),
        ("resolution_go_modules", "go.mod"),
    ] {
        let value: Option<String> = db
            .query_row("SELECT value FROM metadata WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(value) = value {
            let values: std::collections::BTreeMap<String, String> = serde_json::from_str(&value)?;
            if let Some(source) = values.get(file) {
                if file == "package.json" {
                    if let Ok(package) = serde_json::from_str::<serde_json::Value>(source)
                        && let Some(name) = package.get("name").and_then(|s| s.as_str())
                    {
                        add(if name.starts_with('@') {
                            name.split_once('/').map_or(name, |(_, n)| n)
                        } else {
                            name
                        });
                    }
                } else {
                    for line in source.lines() {
                        let mut words = split_words(line);
                        if words.next() == Some("module")
                            && let Some(module) = words.next()
                        {
                            add(module.rsplit('/').next().unwrap_or(module));
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(tokens)
}
pub(crate) fn ranked_kinds(
    db: &Connection,
    root: &Path,
    query: &str,
    limit: usize,
    kinds: &[String],
) -> Result<Vec<RankedHit>> {
    ranked_page(db, root, query, limit, 0, kinds)
}

pub(crate) fn ranked_page(
    db: &Connection,
    root: &Path,
    query: &str,
    limit: usize,
    offset: usize,
    kinds: &[String],
) -> Result<Vec<RankedHit>> {
    let mut parsed = parse_query(query);
    let mut merged = kinds.to_vec();
    for kind in parsed.kinds {
        if !merged.contains(&kind) {
            merged.push(kind);
        }
    }
    parsed.kinds = merged;
    let text = &parsed.text;
    let candidate_limit = limit.saturating_add(offset);
    let mut results = if text.is_empty() {
        page(
            db,
            format!("{SELECT}1.0{FROM}WHERE 1=1"),
            vec![],
            &parsed,
            " ORDER BY n.name",
            limit.saturating_mul(5).saturating_add(offset),
            0,
        )?
    } else {
        let clean = text
            .replace("::", " ")
            .replace(['\'', '"', '*', '(', ')', ':', '^'], " ");
        let expression = split_words(&clean)
            .filter(|word| {
                !["AND", "OR", "NOT", "NEAR"]
                    .iter()
                    .any(|op| word.eq_ignore_ascii_case(op))
            })
            .map(|word| format!("\"{word}\"*"))
            .collect::<Vec<_>>()
            .join(" OR ");
        if expression.is_empty() || expression.contains('\0') {
            Vec::new()
        } else {
            let sql = format!(
                "{SELECT}bm25(canonical_fts,0,20,5,1,2) AS score FROM canonical_fts CROSS JOIN canonical_search n ON n.symbol_id=canonical_fts.rowid JOIN files f ON f.path=n.path WHERE canonical_fts MATCH ?"
            );
            let mut hits = page(
                db,
                sql,
                vec![Value::Text(expression)],
                &parsed,
                " ORDER BY score",
                limit.saturating_mul(5).max(100).saturating_add(offset),
                0,
            )?;
            for hit in &mut hits {
                hit.score = hit.score.abs();
            }
            hits
        }
    };
    if results.is_empty() && length(text) >= 2 {
        results = page(
            db,
            format!(
                "{SELECT}CASE WHEN n.name=? THEN 1.0 WHEN n.name LIKE ? THEN 0.9 WHEN n.name LIKE ? THEN 0.8 WHEN n.qualified_name LIKE ? THEN 0.7 ELSE 0.5 END AS score{FROM}WHERE (n.name LIKE ? OR n.qualified_name LIKE ? OR n.name LIKE ?)"
            ),
            [
                text.clone(),
                format!("{text}%"),
                format!("%{text}%"),
                format!("%{text}%"),
                format!("%{text}%"),
                format!("%{text}%"),
                format!("{text}%"),
            ]
            .into_iter()
            .map(Value::Text)
            .collect(),
            &parsed,
            " ORDER BY score DESC,length(n.name)",
            candidate_limit,
            0,
        )?;
    }
    if results.is_empty() && length(text) >= 3 {
        results = fuzzy(db, text, &parsed, candidate_limit)?;
    }
    if !results.is_empty() && !query.is_empty() {
        let base = results
            .iter()
            .map(|hit| hit.score)
            .fold(f64::NEG_INFINITY, f64::max);
        let mut seen: HashSet<_> = results.iter().map(|hit| hit.id).collect();
        for term in split_words(query).filter(|t| length(t) >= 2) {
            let hits = page(
                db,
                format!("{SELECT}?{FROM}WHERE lower(n.name)=lower(?)"),
                vec![Value::Real(base), Value::Text(term.into())],
                &parsed,
                "",
                20,
                0,
            )?;
            for hit in hits {
                if seen.insert(hit.id) {
                    results.push(hit);
                }
            }
        }
    }
    if !results.is_empty() && (!text.is_empty() || !query.is_empty()) {
        let scoring = if text.is_empty() { query } else { text };
        let path = ranking::PathQuery::new(scoring, &project_names(db, root)?);
        for hit in &mut results {
            let bonus = name_match_bonus(&hit.name, scoring);
            hit.score += kind_bonus(&hit.kind)
                + path.score(&hit.path, hit.deprioritized)
                + if hit.deprioritized {
                    (bonus * 0.75).round()
                } else {
                    bonus
                };
        }
        results.sort_by(|a, b| b.score.total_cmp(&a.score));
    }
    Ok(results.into_iter().skip(offset).collect())
}

#[cfg(test)]
mod tests;
