//! Commit cross-file route names without replacing their symbol identities.
use crate::parser::ParsedFile;
mod additions;
use anyhow::{Context, Result, ensure};
use prometeu_extract::Node;
use rusqlite::{OptionalExtension, Transaction, params};
use std::collections::{BTreeMap, HashSet};

pub(super) fn run(transaction: &Transaction<'_>, frameworks: &[String]) -> Result<bool> {
    let aspnet = frameworks.iter().any(|name| name == "aspnet");
    let fastapi = frameworks.iter().any(|name| name == "fastapi");
    let express = frameworks.iter().any(|name| name == "express");
    let laravel = frameworks.iter().any(|name| name == "laravel");
    let nestjs = frameworks.iter().any(|name| name == "nestjs");
    let react = frameworks.iter().any(|name| name == "react");
    let angular = frameworks.iter().any(|name| name == "angular-router");
    let previous = additions::read(transaction)?;
    if !aspnet
        && !fastapi
        && !express
        && !nestjs
        && !laravel
        && !react
        && !angular
        && previous.is_empty()
    {
        return Ok(false);
    }
    let mut statement = transaction
        .prepare("SELECT path,analysis FROM files WHERE (?1 AND language='csharp') OR (?2 AND language='python') OR (?3 AND language IN ('javascript','typescript','jsx','tsx')) OR (?4 AND language='php') ORDER BY path")?;
    let mut rows = statement.query(params![
        aspnet,
        fastapi,
        express || nestjs || react || angular || !previous.is_empty(),
        laravel
    ])?;
    let mut parsed = BTreeMap::new();
    let mut files = BTreeMap::new();
    let mut nodes = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let json: String = row.get(1)?;
        let source: String = transaction.query_row(
            "SELECT code FROM symbols WHERE path=?1 AND kind='file' ORDER BY ordinal LIMIT 1",
            [&path],
            |row| row.get(0),
        )?;
        let file: ParsedFile = serde_json::from_str(&json)?;
        if let Some(extraction) = &file.extraction {
            nodes.extend(
                extraction
                    .nodes
                    .iter()
                    .filter(|node| {
                        !previous
                            .get(&path)
                            .is_some_and(|extra| extra.nodes.iter().any(|old| old.id == node.id))
                    })
                    .cloned(),
            );
        }
        files.insert(path.clone(), source);
        parsed.insert(path, file);
    }
    drop(rows);
    drop(statement);
    let mut updates = Vec::new();
    let modules = (express || react || angular)
        .then(|| crate::graph::ProjectModules::new(transaction, &files))
        .transpose()?;
    let sources = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let resolve_module = |specifier: &str, from: &str, language: &str| {
        modules
            .as_ref()
            .expect("module resolver initialized")
            .resolve(from, specifier, language)
    };
    let current = if react {
        let output = super::frontend::react::finalize(&nodes, &sources, &resolve_module)?;
        updates.extend(output.updates);
        additions::collect(output.nodes, output.references)?
    } else {
        additions::Stored::new()
    };
    if angular {
        let stored: Option<String> = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key='project_manifests'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let manifests: BTreeMap<String, String> = stored
            .map(|json| serde_json::from_str(&json))
            .transpose()?
            .unwrap_or_default();
        let manifests = manifests
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        updates.extend(
            super::frontend::angular::AngularContext::new(
                &nodes,
                &sources,
                &resolve_module,
                &manifests,
            )
            .post_extract()?,
        );
    }

    if nestjs {
        updates.extend(super::nestjs::post_extract(&nodes, &files)?);
    }
    if laravel {
        updates.extend(super::laravel::post_extract(&nodes, &files)?);
    }
    if aspnet {
        updates.extend(super::aspnet::post_extract(&nodes, &files)?);
    }
    if express {
        let modules = modules.as_ref().expect("module resolver initialized");
        updates.extend(super::express::post_extract(
            &nodes,
            &files,
            |from, specifier, language| modules.resolve(from, specifier, language),
        )?);
    }
    if fastapi {
        let stored: Option<String> = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key='project_manifests'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let manifests = stored
            .map(|json| serde_json::from_str(&json))
            .transpose()?
            .unwrap_or_default();
        let imports: BTreeMap<_, _> = parsed
            .iter()
            .map(|(path, file)| (path.clone(), file.imports.clone()))
            .collect();
        let modules = crate::graph::PythonModules::new(
            files.keys().cloned(),
            &manifests,
            &nodes,
            imports.clone(),
        )?;
        updates.extend(super::python::post_extract(
            &nodes, &files, &imports, &modules,
        )?);
    }
    let added = additions::apply(
        transaction,
        frameworks,
        &files,
        &mut parsed,
        &previous,
        &current,
    )?;
    Ok(commit(transaction, &mut parsed, updates)? || added)
}

fn commit(
    transaction: &Transaction<'_>,
    files: &mut BTreeMap<String, ParsedFile>,
    updates: Vec<Node>,
) -> Result<bool> {
    let mut changed = HashSet::new();
    for updated in updates {
        let file = files
            .get_mut(&updated.file_path)
            .context("route update references an unknown file")?;
        let extraction = file
            .extraction
            .as_mut()
            .context("route update has no canonical extraction")?;
        let mut found = false;
        for (ordinal, node) in extraction
            .nodes
            .iter_mut()
            .enumerate()
            .filter(|(_, node)| node.id == updated.id)
        {
            found = true;
            let mut expected = updated.clone();
            expected.name = node.name.clone();
            ensure!(
                node.kind == "route" && *node == expected,
                "route finalization may only update a route name"
            );
            if node.name == updated.name {
                continue;
            }
            let symbol = file
                .symbols
                .get_mut(ordinal)
                .context("route projection is missing")?;
            ensure!(
                symbol.kind == "route" && symbol.name == node.name,
                "route projection disagrees with its canonical record"
            );
            for export in &mut symbol.exports {
                if *export == node.name {
                    *export = updated.name.clone();
                }
            }
            node.name = updated.name.clone();
            symbol.name = updated.name.clone();
            ensure!(
                transaction.execute(
                    "UPDATE symbols SET name=?1,terms=?2 WHERE path=?3 AND ordinal=?4",
                    params![
                        node.name,
                        crate::index::words(&node.name).join(" "),
                        updated.file_path,
                        ordinal as i64
                    ]
                )? == 1,
                "stored route projection is missing"
            );
            ensure!(transaction.execute("UPDATE canonical_search SET name=?1 WHERE symbol_id=(SELECT id FROM symbols WHERE path=?2 AND ordinal=?3)",params![node.name,updated.file_path,ordinal as i64])?==1,"stored canonical route is missing");
            changed.insert(updated.file_path.clone());
        }
        ensure!(found, "route finalization references an unknown node");
    }
    for path in &changed {
        transaction.execute(
            "UPDATE files SET analysis=?1 WHERE path=?2",
            params![serde_json::to_string(&files[path])?, path],
        )?;
    }
    Ok(!changed.is_empty())
}
