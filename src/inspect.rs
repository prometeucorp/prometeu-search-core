//! Bounded file and symbol reads from the same committed source snapshot.
use crate::index::{clip, generation, validate_limits};
use crate::{Engine, parser::ParsedFile};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use std::path::{Component, Path};

#[derive(Debug, Serialize)]
pub struct IndexedFile {
    pub path: String,
    pub language: String,
    pub symbols: usize,
    pub syntax_errors: bool,
}

#[derive(Debug, Serialize)]
pub struct FilesResult {
    pub generation: u64,
    pub files: Vec<IndexedFile>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct IndexedNode {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
    pub source_truncated: bool,
    pub syntax_errors: bool,
    /// Complete upstream extraction metadata when available and within budget.
    pub metadata: Option<prometeu_extract::Node>,
}

#[derive(Debug, Serialize)]
pub struct NodeResult {
    pub generation: u64,
    /// None means the node's identifying fields alone exceed the response budget.
    pub node: Option<IndexedNode>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct FileContent {
    pub path: String,
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub source: String,
    pub source_truncated: bool,
    pub syntax_errors: bool,
}

#[derive(Debug, Serialize)]
pub struct FileResult {
    pub generation: u64,
    /// None means the file's identifying fields alone exceed the response budget.
    pub file: Option<FileContent>,
    pub truncated: bool,
}

impl Engine {
    /// List indexed files, optionally filtering paths by a case-insensitive substring.
    /// Reads a committed snapshot; call `refresh` when fresh source is required.
    pub fn files(&self, query: &str, limit: usize, max_bytes: usize) -> Result<FilesResult> {
        validate_limits(limit, max_bytes)?;
        let fetch_limit = limit.saturating_add(1).min(i64::MAX as usize) as i64;
        ensure!(
            query.encode_utf16().count() <= 10_000,
            "query must be at most 10000 UTF-16 units"
        );
        let snapshot = self.connection.unchecked_transaction()?;
        let mut statement = snapshot.prepare(
            "SELECT f.path, f.language, f.syntax_errors,
                    (SELECT count(*) FROM symbols s WHERE s.path=f.path AND s.kind!='file')
             FROM files f WHERE instr(lower(f.path), lower(?1)) > 0 ORDER BY f.path LIMIT ?2",
        )?;
        let rows = statement.query_map(params![query.trim(), fetch_limit], |row| {
            Ok(IndexedFile {
                path: row.get(0)?,
                language: row.get(1)?,
                syntax_errors: row.get(2)?,
                symbols: row.get::<_, i64>(3)? as usize,
            })
        })?;
        let mut result = FilesResult {
            generation: generation(&snapshot)?,
            files: Vec::new(),
            truncated: false,
        };
        for row in rows {
            let file = row?;
            if result.files.len() == limit {
                result.truncated = true;
                break;
            }
            result.files.push(file);
            if max_bytes != usize::MAX && serde_json::to_vec(&result)?.len() > max_bytes {
                result.files.pop();
                result.truncated = true;
                break;
            }
        }
        Ok(result)
    }

    /// Read one indexed symbol by its current search-result ID, including source and metadata.
    /// IDs are cache-local and can change when a file is reindexed.
    pub fn node(&self, id: i64, max_bytes: usize) -> Result<NodeResult> {
        validate_limits(1, max_bytes)?;
        ensure!(id > 0, "node id must be positive");
        let snapshot = self.connection.unchecked_transaction()?;
        let (mut node, ordinal, analysis): (IndexedNode, usize, String) = snapshot
            .query_row(
                "SELECT s.id, s.path, s.name, s.kind, s.start_line, s.end_line, s.code,
                        f.syntax_errors, s.ordinal, f.analysis
                 FROM symbols s JOIN files f ON f.path=s.path WHERE s.id=?1",
                [id],
                |row| {
                    Ok((
                        IndexedNode {
                            id: row.get(0)?,
                            path: row.get(1)?,
                            name: row.get(2)?,
                            kind: row.get(3)?,
                            start_line: row.get::<_, i64>(4)? as usize,
                            end_line: row.get::<_, i64>(5)? as usize,
                            source: row.get(6)?,
                            source_truncated: false,
                            syntax_errors: row.get(7)?,
                            metadata: None,
                        },
                        row.get::<_, i64>(8)? as usize,
                        row.get(9)?,
                    ))
                },
            )
            .optional()?
            .with_context(|| format!("indexed node {id} not found"))?;
        let parsed: ParsedFile = serde_json::from_str(&analysis)?;
        if let Some(extraction) = parsed.extraction {
            let matches = |candidate: &prometeu_extract::Node| {
                let end_line = if candidate.end_column == 0 {
                    candidate
                        .end_line
                        .saturating_sub(1)
                        .max(candidate.start_line)
                } else {
                    candidate.end_line
                };
                candidate.name == node.name
                    && candidate.kind == node.kind
                    && candidate.start_line as usize == node.start_line
                    && (end_line as usize == node.end_line
                        || candidate.end_line as usize == node.end_line)
            };
            node.metadata = extraction
                .nodes
                .get(ordinal)
                .filter(|node| matches(node))
                .cloned();
            if node.metadata.is_none() {
                let mut candidates = extraction.nodes.into_iter().filter(matches);
                let candidate = candidates.next();
                // A changed projection must not attach another overload's metadata.
                if candidates.next().is_none() {
                    node.metadata = candidate;
                }
            }
        }
        let source = std::mem::take(&mut node.source);
        node.source_truncated = !source.is_empty();
        let mut result = NodeResult {
            generation: generation(&snapshot)?,
            node: Some(node),
            truncated: false,
        };
        if serde_json::to_vec(&result)?.len() > max_bytes {
            result.node.as_mut().unwrap().metadata = None;
            result.truncated = true;
        }
        if serde_json::to_vec(&result)?.len() > max_bytes {
            result.node = None;
            result.truncated = true;
            return Ok(result);
        }
        let node = result.node.as_mut().unwrap();
        node.source = source;
        node.source_truncated = false;
        loop {
            let bytes = serde_json::to_vec(&result)?.len();
            if bytes <= max_bytes {
                break;
            }
            result.truncated = true;
            let node = result.node.as_mut().unwrap();
            node.source = clip(
                &node.source,
                node.source.len().saturating_sub(bytes - max_bytes),
            )
            .into();
            node.source_truncated = true;
        }
        Ok(result)
    }

    /// Read inclusive lines from the indexed full-file source, never from the live filesystem.
    /// The response reports its snapshot generation and any source truncation explicitly.
    pub fn file(
        &self,
        path: &str,
        start_line: usize,
        end_line: Option<usize>,
        max_bytes: usize,
    ) -> Result<FileResult> {
        validate_limits(1, max_bytes)?;
        let path = indexed_path(path)?;
        ensure!(start_line > 0, "start_line must be positive");
        ensure!(
            end_line.is_none_or(|end| end >= start_line),
            "end_line must not precede start_line"
        );
        let snapshot = self.connection.unchecked_transaction()?;
        let (language, syntax_errors, source): (String, bool, Option<String>) = snapshot
            .query_row(
                "SELECT f.language, f.syntax_errors,
                        (SELECT s.code FROM symbols s WHERE s.path=f.path AND s.kind='file'
                         ORDER BY s.ordinal LIMIT 1)
                 FROM files f WHERE f.path=?1",
                [&path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .with_context(|| format!("indexed file not found: {path}"))?;
        let source = source.context("indexed file source unavailable; reindex this file")?;
        let starts: Vec<_> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(offset, _)| offset + 1))
            .collect();
        let total_lines = starts.len();
        ensure!(
            start_line <= total_lines,
            "start_line exceeds indexed file length"
        );
        let last = end_line.unwrap_or(total_lines).min(total_lines);
        let start = starts[start_line - 1];
        let end = starts.get(last).copied().unwrap_or(source.len());
        let mut result = FileResult {
            generation: generation(&snapshot)?,
            file: Some(FileContent {
                path,
                language,
                start_line,
                end_line: last,
                total_lines,
                source: source[start..end].into(),
                source_truncated: false,
                syntax_errors,
            }),
            truncated: false,
        };
        loop {
            let bytes = serde_json::to_vec(&result)?.len();
            if bytes <= max_bytes {
                break;
            }
            result.truncated = true;
            let file = result.file.as_mut().unwrap();
            if file.source.is_empty() {
                result.file = None;
                break;
            }
            file.source = clip(
                &file.source,
                file.source.len().saturating_sub(bytes - max_bytes),
            )
            .into();
            file.source_truncated = true;
            let newlines = file.source.bytes().filter(|&byte| byte == b'\n').count();
            file.end_line = file.start_line + newlines - usize::from(file.source.ends_with('\n'));
        }
        Ok(result)
    }
}

pub(crate) fn indexed_path(path: &str) -> Result<String> {
    ensure!(
        !path.is_empty() && path.encode_utf16().count() <= 4096,
        "path must contain 1 to 4096 UTF-16 units"
    );
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_str().context("path must be valid UTF-8")?)
            }
            Component::CurDir => {}
            _ => anyhow::bail!("path must stay inside the fixed worktree"),
        }
    }
    ensure!(!parts.is_empty(), "path must name an indexed file");
    Ok(parts.join("/"))
}
