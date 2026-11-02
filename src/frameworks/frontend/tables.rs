//! Canonical route tables and ambiguity-preserving dynamic matching.
use super::{
    href::{HOLE, Href},
    paths, regex,
};
use anyhow::Result;
use prometeu_extract::Node;
use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
pub(crate) struct Table<'a> {
    pub exact: BTreeMap<String, &'a Node>,
    dynamic: Vec<(&'a Node, Vec<String>)>,
    pub roots: Vec<String>,
}
pub(crate) type Rooted<'a> = BTreeMap<String, Table<'a>>;

pub(crate) fn expo(nodes: &[Node]) -> Result<Table<'_>> {
    let mut table = Table::default();
    for node in nodes.iter().filter(|n| n.kind == "route") {
        if paths::expo(&node.file_path)?.as_deref() != Some(node.name.as_str()) {
            continue;
        }
        table.exact.insert(node.name.clone(), node);
        if node.name.contains('[') {
            table.dynamic.push((node, segments(&node.name)));
        }
    }
    Ok(table)
}
pub(crate) fn normalize(path: &str, file: &str) -> Result<Option<Vec<String>>> {
    let mut path = path.to_owned();
    if !path.starts_with('/') {
        if paths::expo(file)?.is_none() {
            return Ok(None);
        }
        let directory = file.rsplit_once('/').map_or("", |(directory, _)| directory);
        let index = if directory.is_empty() {
            "index.tsx".to_owned()
        } else {
            format!("{directory}/index.tsx")
        };
        let Some(parent) = paths::expo(&index)? else {
            return Ok(None);
        };
        let mut parent = segments(&parent);
        if let Some(relative) = path.strip_prefix("./") {
            path = format!("/{}/{}", parent.join("/"), relative);
        } else if path.starts_with("../") {
            while let Some(relative) = path.strip_prefix("../") {
                parent.pop();
                path = relative.to_owned();
            }
            path = format!("/{}/{}", parent.join("/"), path);
        } else {
            path = format!("/{}/{}", parent.join("/"), path);
        }
    }
    Ok(Some(
        path.split('/')
            .skip(1)
            .filter(|s| !s.is_empty() && !(s.starts_with('(') && s.ends_with(')')))
            .map(|s| {
                if s.contains(HOLE) {
                    "*".to_owned()
                } else {
                    decode(s)
                }
            })
            .collect(),
    ))
}

pub(crate) fn next(nodes: &[Node]) -> Result<Table<'_>> {
    let mut table = Table::default();
    for node in nodes.iter().filter(|n| n.kind == "route") {
        let Some(file) = paths::next(&node.file_path)? else {
            continue;
        };
        if file.kind != paths::Kind::Page || file.path != node.name {
            continue;
        }
        table.exact.insert(node.name.clone(), node);
        if node.name.contains(':') {
            table.dynamic.push((node, segments(&node.name)));
        }
        if !table.roots.contains(&file.root) {
            table.roots.push(file.root);
        }
    }
    Ok(table)
}
pub(crate) fn svelte(nodes: &[Node]) -> Result<Rooted<'_>> {
    let mut table = Rooted::new();
    let mut shortened = Vec::new();
    for node in nodes.iter().filter(|n| n.kind == "route") {
        if node.language != "svelte"
            || !node.file_path.ends_with("/+page.svelte")
            || !node.name.starts_with('/')
            || node.name.contains('*')
            || node.id != format!("route:{}:{}:1", node.file_path, node.name)
        {
            continue;
        }
        let root = app_root(&node.file_path)?;
        add(table.entry(root.clone()).or_default(), &node.name, node);
        let mut parts = segments(&node.name);
        while parts.len() > 1
            && parts
                .last()
                .is_some_and(|part| part.starts_with(':') && part.ends_with('?'))
        {
            parts.pop();
            shortened.push((root.clone(), format!("/{}", parts.join("/")), node));
        }
    }
    for (root, path, node) in shortened {
        if let Some(table) = table.get_mut(&root)
            && !table.exact.contains_key(&path)
        {
            add(table, &path, node);
        }
    }
    Ok(table)
}
pub(super) fn add<'a>(table: &mut Table<'a>, path: &str, node: &'a Node) {
    table.exact.entry(path.to_owned()).or_insert(node);
    if path.contains(':') {
        table.dynamic.push((node, segments(path)));
    }
}
fn segments(path: &str) -> Vec<String> {
    path.split('/').skip(1).map(str::to_owned).collect()
}
pub(crate) fn app_root(path: &str) -> Result<String> {
    Ok(regex(r"^((?:[^/]+/)*?)(?:src|pages|app|routes)/")?
        .captures(path)?
        .map_or_else(
            || {
                path.rsplit_once('/')
                    .map_or(String::new(), |(root, _)| format!("{root}/"))
            },
            |m| m[1].to_owned(),
        ))
}
pub(crate) fn for_file<'a, 'b>(tables: &'b Rooted<'a>, file: &str) -> Option<&'b Table<'a>> {
    tables
        .iter()
        .filter(|(root, _)| file.starts_with(root.as_str()))
        .max_by_key(|(root, _)| root.len())
        .map(|(_, table)| table)
}
pub(crate) fn destinations<'a, 'b>(
    href: &'b Href,
    table: &Table<'a>,
) -> Result<Vec<(&'a Node, &'b Href)>> {
    let absolute = regex(r"(?i)^(?:[a-z][a-z0-9+.-]*:)?//[^/]*(/.*)?$")?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for arm in href.arms() {
        let absolute = absolute.captures(&arm.path)?;
        let path = absolute
            .as_ref()
            .map_or(arm.path.as_str(), |m| m.get(1).map_or("/", |m| m.as_str()));
        if !path.starts_with('/') {
            continue;
        }
        let parts = path
            .split('/')
            .skip(1)
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s.contains(HOLE) {
                    "*".to_owned()
                } else {
                    decode(s)
                }
            })
            .collect::<Vec<_>>();
        if let Some(target) = matching(&parts, table)
            && seen.insert(&target.id)
        {
            output.push((target, arm));
        }
    }
    Ok(output)
}
pub(crate) fn matching<'a>(parts: &[String], table: &Table<'a>) -> Option<&'a Node> {
    if let Some(&exact) = table.exact.get(&format!("/{}", parts.join("/"))) {
        return Some(exact);
    }
    let mut best = None;
    let mut tied = false;
    for (node, route) in &table.dynamic {
        let Some(score) = score(parts, route) else {
            continue;
        };
        if best.is_none_or(|(_, known)| score > known) {
            best = Some((*node, score));
            tied = false;
        } else if best.is_some_and(|(_, known)| score == known) {
            tied = true;
        }
    }
    if tied {
        None
    } else {
        best.map(|(node, _)| node)
    }
}
fn score(href: &[String], route: &[String]) -> Option<usize> {
    let mut score = 0;
    let mut i = 0;
    for part in route {
        if part.starts_with("[...") && part.ends_with(']')
            || part.starts_with(':') && part.ends_with('*')
        {
            if i >= href.len() {
                return None;
            }
            score += href.len() - i;
            i = href.len();
            continue;
        }
        let value = href.get(i)?;
        if part.starts_with('[') && part.ends_with(']')
            || part.starts_with(':') && !part.ends_with('*')
        {
            score += 2;
        } else if value == part {
            score += 3;
        } else if value == "*" {
            score += 1;
        } else {
            return None;
        }
        i += 1;
    }
    (i == href.len()).then_some(score)
}
fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let Some(a) = bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16)) else {
                return value.to_owned();
            };
            let Some(b) = bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16)) else {
                return value.to_owned();
            };
            output.push((a * 16 + b) as u8);
            i += 3;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(output).unwrap_or_else(|_| value.to_owned())
}
