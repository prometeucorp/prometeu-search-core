//! Persist declarations discovered through another file's route configuration.
use crate::parser::{self, ParsedFile};
use anyhow::{Context, Result, ensure};
use prometeu_extract::{Node, UnresolvedReference};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const KEY: &str = "framework_additions";
#[derive(Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Additions {
    pub nodes: Vec<Node>,
    pub references: Vec<UnresolvedReference>,
}
pub(super) type Stored = BTreeMap<String, Additions>;

pub(super) fn read(transaction: &Transaction<'_>) -> Result<Stored> {
    let stored: Option<String> = transaction
        .query_row("SELECT value FROM metadata WHERE key=?1", [KEY], |row| {
            row.get(0)
        })
        .optional()?;
    stored.map_or_else(
        || Ok(BTreeMap::new()),
        |stored| Ok(serde_json::from_str(&stored)?),
    )
}

pub(super) fn collect(nodes: Vec<Node>, references: Vec<UnresolvedReference>) -> Result<Stored> {
    let mut stored = Stored::new();
    for mut node in nodes {
        node.updated_at = 0;
        ensure!(node.kind == "route", "framework addition is not a route");
        stored
            .entry(node.file_path.clone())
            .or_default()
            .nodes
            .push(node);
    }
    for reference in references {
        let path = reference
            .file_path
            .as_ref()
            .context("framework reference has no source file")?;
        stored
            .entry(path.clone())
            .or_default()
            .references
            .push(reference);
    }
    Ok(stored)
}

pub(super) fn apply(
    transaction: &Transaction<'_>,
    frameworks: &[String],
    sources: &BTreeMap<String, String>,
    parsed: &mut BTreeMap<String, ParsedFile>,
    previous: &Stored,
    current: &Stored,
) -> Result<bool> {
    let mut changed = false;
    for path in previous
        .keys()
        .chain(current.keys())
        .collect::<BTreeSet<_>>()
    {
        let present = current.get(path).is_none_or(|wanted| {
            parsed
                .get(path)
                .and_then(|file| file.extraction.as_ref())
                .is_some_and(|extraction| {
                    wanted.nodes.iter().all(|node| {
                        extraction
                            .nodes
                            .iter()
                            .any(|existing| existing.id == node.id)
                    }) && wanted
                        .references
                        .iter()
                        .all(|reference| extraction.unresolved_references.contains(reference))
                })
        });
        if previous.get(path) == current.get(path) && present {
            continue;
        }
        let Some(source) = sources.get(path) else {
            continue;
        };
        let file = parsed
            .get(path)
            .context("framework addition references an unindexed file")?;
        let empty = Additions::default();
        let additions = current.get(path).unwrap_or(&empty);
        let file = parser::parse_with_additions(
            Path::new(path),
            source,
            &file.language,
            frameworks,
            &additions.nodes,
            &additions.references,
        )?;
        let (hash, deprioritized, signature): (String, bool, Option<String>) = transaction.query_row(
            "SELECT f.hash,f.deprioritized,s.signature FROM files f LEFT JOIN file_state s ON s.path=f.path WHERE f.path=?1", [path],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?;
        crate::index::store_file(transaction, path, source, &hash, &file)?;
        transaction.execute(
            "UPDATE files SET deprioritized=?2 WHERE path=?1",
            params![path, deprioritized],
        )?;
        if let Some(signature) = signature {
            transaction.execute(
                "INSERT OR REPLACE INTO file_state(path,signature) VALUES(?1,?2)",
                params![path, signature],
            )?;
        }
        parsed.insert(path.clone(), file);
        changed = true;
    }
    if previous != current {
        let json = serde_json::to_string(current)?;
        transaction.execute(
            "INSERT OR REPLACE INTO metadata(key,value) VALUES(?1,?2)",
            params![KEY, json],
        )?;
        changed = true;
    }
    Ok(changed)
}
