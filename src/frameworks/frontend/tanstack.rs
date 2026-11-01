//! TanStack's file and code route declarations and literal destinations.
use super::{href, object, regex, tables};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, Node, supplemental::Context};
use std::collections::{HashMap, HashSet};

pub(crate) fn path(raw: &str) -> Option<String> {
    if !raw.starts_with('/') {
        return None;
    }
    let segments: Vec<_> = raw
        .split('/')
        .filter(|s| {
            !s.is_empty() && !s.starts_with('_') && !(s.starts_with('(') && s.ends_with(')'))
        })
        .map(|s| {
            let bare = s.strip_suffix('_').unwrap_or(s);
            if bare == "$" {
                ":splat*".to_owned()
            } else if let Some(parameter) = bare.strip_prefix('$') {
                format!(":{parameter}")
            } else {
                bare.to_owned()
            }
        })
        .collect();
    Some(format!("/{}", segments.join("/")))
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    path: String,
    component: Option<String>,
    index: bool,
    file_based: bool,
    line: u32,
}
struct Declaration {
    path: Option<String>,
    parent: Option<String>,
    component: Option<String>,
    root: bool,
    at: usize,
}

pub(super) fn extract(file: &str, content: &str) -> Result<ExtractionResult> {
    let ctx = Context::new(file, content, super::language(file, false));
    let mut result = ctx.result.clone();
    result.nodes.clear();
    let layout = regex(r"(?:^|\.)route\.(?:tsx|ts|jsx|js)$")?
        .is_match(file.rsplit('/').next().unwrap_or(file))?
        || regex(r"<Outlet\b")?.is_match(content)?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut by_path: HashMap<String, usize> = HashMap::new();
    for entry in parse(content)? {
        if entry.file_based && layout {
            continue;
        }
        if let Some(&i) = by_path.get(&entry.path) {
            if entry.index && !entries[i].index {
                entries[i] = entry;
            }
        } else {
            by_path.insert(entry.path.clone(), entries.len());
            entries.push(entry);
        }
    }
    for entry in entries {
        let mut node = super::page_node(&ctx, &entry.path);
        node.id = format!("route:{file}:{}:{}:tanstack", entry.line, entry.path);
        node.start_line = entry.line;
        node.end_line = entry.line;
        node.is_exported = None;
        if let Some(component) = entry.component {
            let mut reference = crate::frameworks::routes::reference(&node, &component, "calls");
            reference.candidates = Some(vec![component]);
            result.unresolved_references.push(reference);
        }
        result.nodes.push(node);
    }
    Ok(result)
}

fn parse(content: &str) -> Result<Vec<Entry>> {
    if !regex(r"\bcreate(?:File|Lazy(?:File)?|Root)?Route\s*\(")?.is_match(content)? {
        return Ok(Vec::new());
    }
    let safe = crate::synthesis::comments::strip_for_regex(content, "typescript");
    let line = |at: usize| 1 + safe[..at].bytes().filter(|&b| b == b'\n').count() as u32;
    let mut out = Vec::new();
    for found in regex(r"\bcreate(?:Lazy)?FileRoute\s*\(")?.find_iter(&safe) {
        let found = found?;
        let open = found.end() - 1;
        let Some(close) = object::matching(&safe, open)? else {
            continue;
        };
        let Some(raw) = href::read_string(safe[open + 1..close].trim_start(), 0)? else {
            continue;
        };
        let Some(path) = path(&raw) else { continue };
        let index = raw.len() > 1 && raw.ends_with('/');
        if !index
            && raw
                .split('/')
                .rfind(|s| !s.is_empty())
                .is_some_and(|last| last.starts_with('_'))
        {
            continue;
        }
        out.push(Entry {
            path,
            component: component(chain(&safe, close + 1)?)?,
            index,
            file_based: true,
            line: line(found.start()),
        });
    }
    let mut declarations: Vec<(String, Declaration)> = Vec::new();
    let mut by_name = HashMap::new();
    for found in regex(
        r"\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=]*?)?=\s*create(Root)?Route\s*\(\s*\{",
    )?
    .captures_iter(&safe)
    {
        let found = found?;
        let whole = found.get(0).unwrap();
        let brace = whole.end() - 1;
        let Some(end) = object::matching(&safe, brace)? else {
            continue;
        };
        let fields = object::fields(&safe, brace, end)?;
        let root = found.get(2).is_some();
        let path = if root {
            Some("/".to_owned())
        } else {
            fields
                .get("path")
                .map(|f| href::read_string(f.text.trim_start(), 0))
                .transpose()?
                .flatten()
        };
        let parent = fields
            .get("getParentRoute")
            .map(|f| {
                regex(r"=>\s*([A-Za-z_$][\w$]*)")?
                    .captures(f.text)
                    .map_err(anyhow::Error::from)
                    .map(|found| found.map(|found| found[1].to_owned()))
            })
            .transpose()?
            .flatten();
        let component = fields
            .get("component")
            .map(|f| component(f.text))
            .transpose()?
            .flatten();
        let declaration = Declaration {
            path,
            parent,
            component,
            root,
            at: whole.start(),
        };
        if let Some(&i) = by_name.get(&found[1]) {
            declarations[i] = (found[1].to_owned(), declaration);
        } else {
            by_name.insert(found[1].to_owned(), declarations.len());
            declarations.push((found[1].to_owned(), declaration));
        }
    }
    let wraps: HashSet<_> = declarations
        .iter()
        .filter(|(_, d)| d.path.as_deref() == Some("/"))
        .filter_map(|(_, d)| d.parent.as_deref())
        .collect();
    for (name, decl) in &declarations {
        if decl.path.is_none() || decl.root || wraps.contains(name.as_str()) {
            continue;
        }
        let Some(full) = compose(name, &declarations, &by_name) else {
            continue;
        };
        let Some(path) = path(&full) else { continue };
        out.push(Entry {
            path,
            component: decl.component.clone(),
            index: decl.path.as_deref() == Some("/"),
            file_based: false,
            line: line(decl.at),
        });
    }
    Ok(out)
}
fn compose(
    name: &str,
    declarations: &[(String, Declaration)],
    names: &HashMap<String, usize>,
) -> Option<String> {
    let mut segments = Vec::new();
    let mut current = Some(name);
    for _ in 0..24 {
        let Some(name) = current else { break };
        let declaration = &declarations[*names.get(name)?].1;
        if let Some(path) = &declaration.path {
            let mut own: Vec<_> = path.split('/').filter(|s| !s.is_empty()).collect();
            own.extend(segments);
            segments = own;
        }
        current = declaration.parent.as_deref();
    }
    Some(format!("/{}", segments.join("/")))
}
fn chain(source: &str, at: usize) -> Result<&str> {
    let mut i = at;
    let space = regex(r"^\s+")?;
    let member = regex(r"^[\w$]+")?;
    for _ in 0..8 {
        if let Some(found) = space.find(&source[i..])? {
            i += found.end();
        }
        if source.as_bytes().get(i) == Some(&b'.') {
            i += 1;
            if let Some(found) = member.find(&source[i..])? {
                i += found.end();
            }
            if let Some(found) = space.find(&source[i..])? {
                i += found.end();
            }
        }
        if source.as_bytes().get(i) != Some(&b'(') {
            break;
        }
        let Some(close) = object::matching(source, i)? else {
            break;
        };
        i = close + 1;
    }
    Ok(&source[at..i])
}
fn component(text: &str) -> Result<Option<String>> {
    if let Some(lazy) = regex(r#"\bimport\s*\(\s*['"`]([^'"`]+)['"`]"#)?.captures(text)? {
        let name = lazy[1].rsplit('/').next().unwrap_or("");
        let name = regex(r"\.\w+$")?.replace(name, "").into_owned();
        return Ok((!name.is_empty()).then_some(name));
    }
    for pattern in [
        r"(?:^|[^\w$])component\s*:\s*([A-Z][A-Za-z0-9_]*)",
        r"^\s*([A-Z][A-Za-z0-9_]*)\s*$",
    ] {
        if let Some(found) = regex(pattern)?.captures(text)? {
            return Ok(Some(found[1].to_owned()));
        }
    }
    Ok(None)
}

pub(crate) fn table(nodes: &[Node]) -> Result<tables::Rooted<'_>> {
    let mut table = tables::Rooted::new();
    for node in nodes.iter().filter(|n| n.kind == "route") {
        if node.id
            != format!(
                "route:{}:{}:{}:tanstack",
                node.file_path, node.start_line, node.name
            )
        {
            continue;
        }
        tables::add(
            table.entry(tables::app_root(&node.file_path)?).or_default(),
            &node.name,
            node,
        );
    }
    Ok(table)
}
pub(crate) fn destination(expression: &str) -> Result<Option<href::Href>> {
    let args = expression.trim();
    let literal = if args.starts_with('{') {
        let Some(end) = object::matching(args, 0)? else {
            return Ok(None);
        };
        object::fields(args, 0, end)?
            .get("to")
            .map(|f| href::read_string(f.text.trim_start(), 0))
            .transpose()?
            .flatten()
    } else {
        href::read_string(args, 0)?
    };
    let Some(literal) = literal else {
        return Ok(None);
    };
    Ok(match path(&literal) {
        Some(path) => href::to_href(&path),
        None => href::parse(args)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn tanstack_matches_original_extraction_and_helpers() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/frontend/tanstack.json"
        ))
        .unwrap();
        for case in corpus["paths"].as_array().unwrap() {
            assert_eq!(
                json!(path(case["source"].as_str().unwrap())),
                case["expected"],
                "{case}"
            );
        }
        for case in corpus["parse"].as_array().unwrap() {
            assert_eq!(
                json!(parse(case["source"].as_str().unwrap()).unwrap()),
                case["expected"],
                "{case}"
            );
        }
        for case in corpus["destinations"].as_array().unwrap() {
            assert_eq!(
                json!(destination(case["source"].as_str().unwrap()).unwrap()),
                case["expected"],
                "{case}"
            );
        }
        for case in corpus["extraction"].as_array().unwrap() {
            let result = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut actual =
                json!({"nodes":result.nodes,"references":result.unresolved_references});
            for node in actual["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(actual, case["expected"], "{case}");
        }
    }
}
