//! Committed includes, templates, and skipped Go declarations outside indexed graph files.
use anyhow::{Context, Result, ensure};
use cap_std::fs::Dir;
use rusqlite::{Connection, OptionalExtension};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Read;
use std::path::{Component, Path};
use std::sync::LazyLock;

const MAX_TOTAL: usize = 8 * 1024 * 1024;
const MAX_FILE: usize = 1024 * 1024;
pub(crate) const GO_UNREAD_UNKNOWN: &str = "// Method declarations unavailable.\n";
pub(crate) fn go_declarations(file: &mut std::fs::File) -> Result<String> {
    const LIMIT: u64 = 32 * 1024 * 1024;
    if file.metadata()?.len() > LIMIT {
        return Ok(GO_UNREAD_UNKNOWN.into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Ok(GO_UNREAD_UNKNOWN.into());
    }
    let Ok(source) = std::str::from_utf8(&bytes) else {
        return Ok(GO_UNREAD_UNKNOWN.into());
    };
    static METHOD: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?m)^func\s*\(\s*(?:[A-Za-z_]\w*\s+)?\*?\s*[A-Za-z_]\w*\s*(?:\[[^\]\n]*\])?\s*\)\s*[A-Za-z_]\w*\s*[\[(]").unwrap()
    });
    let mut declarations = String::new();
    for declaration in METHOD.find_iter(source) {
        if declarations.len() + declaration.as_str().len() + 1 > MAX_FILE {
            return Ok(GO_UNREAD_UNKNOWN.into());
        }
        declarations.push_str(declaration.as_str());
        declarations.push('\n');
    }
    Ok(declarations)
}
static INCLUDE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r##"#[ \t]*include[ \t]+"([^"\n]+)""##).unwrap());
pub(crate) fn c_file(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "c" | "h"
                | "cc"
                | "cpp"
                | "cxx"
                | "hpp"
                | "hh"
                | "hxx"
                | "cppm"
                | "ipp"
                | "inl"
                | "tcc"
        )
    })
}
pub(crate) fn includable(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "def"
                | "inc"
                | "h"
                | "hh"
                | "hpp"
                | "hxx"
                | "c"
                | "cc"
                | "cpp"
                | "cxx"
                | "ipp"
                | "tcc"
                | "tbl"
        )
    })
}
pub(crate) fn include_paths(source: &str) -> impl Iterator<Item = &str> {
    INCLUDE
        .captures_iter(source)
        .map(|m| m.get(1).unwrap().as_str())
        .filter(|path| includable(path))
}
pub(crate) fn normalize(base: &str, path: &str) -> Option<String> {
    if path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return None;
    }
    let mut parts: Vec<_> = base
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
pub(crate) fn load(connection: &Connection) -> Result<BTreeMap<String, String>> {
    let json: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='source_includes'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(json) = json else {
        return Ok(BTreeMap::new());
    };
    ensure!(
        json.len() <= 64 * 1024 * 1024,
        "cached include inventory exceeds limit"
    );
    let sources: BTreeMap<String, String> =
        serde_json::from_str(&json).context("invalid cached include inventory")?;
    ensure!(
        sources.len() <= 4096 && sources.values().map(String::len).sum::<usize>() <= MAX_TOTAL,
        "cached include source exceeds limit"
    );
    for (path, source) in &sources {
        ensure!(
            source.len() <= MAX_FILE
                && path.len() <= 4096
                && Path::new(path)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "invalid cached include source"
        );
    }
    Ok(sources)
}
pub(crate) fn reconcile(
    connection: &Connection,
    root: &Dir,
    go_unread: BTreeMap<String, String>,
) -> Result<bool> {
    let mut indexed = BTreeMap::new();
    let mut templates = Vec::new();
    let mut query=connection.prepare("SELECT f.path,s.code,f.analysis FROM files f JOIN symbols s ON s.path=f.path AND s.kind='file' ORDER BY f.path,s.ordinal")?;
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let angular = path.ends_with(".ts") || path.ends_with(".mts") || path.ends_with(".cts");
        if (!c_file(&path) && !angular) || indexed.contains_key(&path) {
            continue;
        }
        let source: String = row.get(1)?;
        if angular && source.contains("Component") {
            let analysis: String = row.get(2)?;
            let parsed: crate::parser::ParsedFile =
                serde_json::from_str(&analysis).context("invalid component extraction snapshot")?;
            if let Some(extraction) = parsed.extraction {
                templates.extend(crate::frameworks::frontend::angular::template_targets(
                    &path,
                    &source,
                    &extraction.nodes,
                )?);
            }
        }
        let includes: Vec<String> = if c_file(&path) {
            include_paths(&source).map(str::to_owned).collect()
        } else {
            Vec::new()
        };
        indexed.insert(path, includes);
    }
    let mut queue: VecDeque<_> = indexed
        .keys()
        .filter(|path| c_file(path))
        .map(|path| (path.clone(), 0usize))
        .collect();
    let mut visited = HashMap::new();
    let mut auxiliary = go_unread;
    let mut bytes = auxiliary.values().map(String::len).sum::<usize>();
    ensure!(
        auxiliary.len() <= 4096 && bytes <= MAX_TOTAL,
        "Go declaration snapshot exceeds limit"
    );
    let mut probes = 0usize;
    while let Some((path, depth)) = queue.pop_front() {
        if depth >= 2
            || visited
                .get(&path)
                .is_some_and(|&previous| previous <= depth)
        {
            continue;
        }
        visited.insert(path.clone(), depth);
        let includes = if let Some(includes) = indexed.get(&path) {
            includes.clone()
        } else if let Some(source) = auxiliary.get(&path) {
            include_paths(source).map(str::to_owned).collect()
        } else {
            continue;
        };
        for include in includes {
            probes += 1;
            ensure!(probes <= 100_000, "include scan exceeds work limit");
            let base = path.rsplit_once('/').map_or("", |v| v.0);
            let candidates = [normalize(base, &include), normalize("", &include)];
            for candidate in candidates.into_iter().flatten() {
                if indexed.contains_key(&candidate) || auxiliary.contains_key(&candidate) {
                    queue.push_back((candidate, depth + 1));
                    break;
                }
                if !read_auxiliary(root, &candidate, &mut auxiliary, &mut bytes)? {
                    continue;
                }
                queue.push_back((candidate, depth + 1));
                break;
            }
        }
    }
    for target in templates {
        probes += 1;
        ensure!(probes <= 100_000, "auxiliary scan exceeds work limit");
        let Some(path) = normalize("", &target) else {
            continue;
        };
        if !indexed.contains_key(&path) && !auxiliary.contains_key(&path) {
            read_auxiliary(root, &path, &mut auxiliary, &mut bytes)?;
        }
    }
    let json = serde_json::to_string(&auxiliary)?;
    let previous: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='source_includes'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let changed = previous
        .as_deref()
        .map_or(!auxiliary.is_empty(), |old| old != json);
    connection.execute(
        "INSERT OR REPLACE INTO metadata(key,value) VALUES('source_includes',?1)",
        [json],
    )?;
    Ok(changed)
}

fn read_auxiliary(
    root: &Dir,
    candidate: &str,
    auxiliary: &mut BTreeMap<String, String>,
    bytes: &mut usize,
) -> Result<bool> {
    let mut file = match crate::source::open(root, Path::new(candidate)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read auxiliary source {candidate}"));
        }
    };
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "auxiliary source is not a regular file: {candidate}"
    );
    ensure!(
        metadata.len() <= MAX_FILE as u64,
        "auxiliary source exceeds file limit: {candidate}"
    );
    let mut source = String::new();
    (&mut file)
        .take((MAX_FILE + 1) as u64)
        .read_to_string(&mut source)
        .with_context(|| format!("cannot decode auxiliary source {candidate}"))?;
    ensure!(
        source.len() <= MAX_FILE,
        "auxiliary source exceeds file limit: {candidate}"
    );
    *bytes += source.len();
    ensure!(
        *bytes <= MAX_TOTAL && auxiliary.len() < 4096,
        "auxiliary source snapshot exceeds limit"
    );
    auxiliary.insert(candidate.to_owned(), source);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_indexed_snapshots_only_bound_the_auxiliary_inputs() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = Dir::open_ambient_dir(temporary.path(), cap_std::ambient_authority())?;
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "CREATE TABLE files(path TEXT PRIMARY KEY, analysis TEXT NOT NULL);
             CREATE TABLE symbols(path TEXT, kind TEXT, code TEXT, ordinal INTEGER);
             CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT);",
        )?;
        let analysis = serde_json::json!({"padding": "x".repeat(MAX_FILE)}).to_string();
        for index in 0..65 {
            let path = format!("ordinary{index}.ts");
            connection.execute("INSERT INTO files VALUES(?1,?2)", [&path, &analysis])?;
            connection.execute(
                "INSERT INTO symbols VALUES(?1,'file','export const marker = 1;',0)",
                [&path],
            )?;
        }
        let source = "import { Component } from '@angular/core';\n@Component({templateUrl:'./panel.html'})\nexport class Panel {}\n";
        let parsed = crate::parser::parse(Path::new("panel.ts"), source)?;
        connection.execute(
            "INSERT INTO files VALUES('panel.ts',?1)",
            [serde_json::to_string(&parsed)?],
        )?;
        connection.execute(
            "INSERT INTO symbols VALUES('panel.ts','file',?1,0)",
            [source],
        )?;
        root.write("panel.html", "<button (click)=\"save()\">Save</button>")?;
        assert!(reconcile(&connection, &root, BTreeMap::new())?);
        let auxiliary = load(&connection)?;
        assert_eq!(auxiliary.len(), 1);
        assert!(auxiliary["panel.html"].contains("save()"));
        assert!(!reconcile(&connection, &root, BTreeMap::new())?);
        Ok(())
    }
}
