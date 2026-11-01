//! Laravel route groups, controller bindings, and composed route-file mounts.
use super::extractors::regex;
use anyhow::{Result, ensure};
use prometeu_extract::{ExtractionResult, Node, supplemental::Context};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
mod bindings;
pub(crate) use bindings::resolve;

struct Scan {
    work: Cell<usize>,
}
impl Scan {
    fn new() -> Self {
        Self { work: Cell::new(0) }
    }
    fn step(&self, count: usize) -> Result<()> {
        let work = self.work.get().saturating_add(count);
        ensure!(work <= 10_000_000, "PHP route scan exceeds work limit");
        self.work.set(work);
        Ok(())
    }
    fn close(&self, source: &str, open: usize) -> Result<Option<usize>> {
        let bytes = source.as_bytes();
        let mut at = open;
        let mut depth = 0i32;
        while at < bytes.len() {
            self.step(1)?;
            let byte = bytes[at];
            if matches!(byte, b'\'' | b'"') {
                at += 1;
                while at < bytes.len() && bytes[at] != byte {
                    self.step(1)?;
                    if bytes[at] == b'\\' {
                        at += 1
                    }
                    at += 1;
                }
                at += 1;
                continue;
            }
            if byte == b'(' {
                depth += 1
            } else if byte == b')' {
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(at));
                }
            }
            at += 1;
        }
        Ok(None)
    }
    fn args<'a>(&self, source: &'a str) -> Result<Vec<&'a str>> {
        let bytes = source.as_bytes();
        let mut depth = 0i32;
        let mut start = 0;
        let mut at = 0;
        let mut out = Vec::new();
        while at < bytes.len() {
            self.step(1)?;
            let byte = bytes[at];
            if matches!(byte, b'\'' | b'"') {
                at += 1;
                while at < bytes.len() && bytes[at] != byte {
                    self.step(1)?;
                    if bytes[at] == b'\\' {
                        at += 1
                    }
                    at += 1;
                }
                at += 1;
                continue;
            }
            if matches!(byte, b'(' | b'[' | b'{') {
                depth += 1
            } else if matches!(byte, b')' | b']' | b'}') {
                depth -= 1
            } else if byte == b',' && depth == 0 {
                out.push(&source[start..at]);
                start = at + 1;
            }
            at += 1;
        }
        let last = &source[start..];
        if !trim(last).is_empty() || !out.is_empty() {
            out.push(last);
        }
        Ok(out)
    }
}
fn trim(value: &str) -> &str {
    value.trim_matches(crate::search::query_whitespace)
}
fn join<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    format!(
        "/{}",
        parts
            .into_iter()
            .flat_map(|part| part.split('/'))
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    )
}
fn literal(value: &str) -> Result<String> {
    Ok(regex(r#"^\s*(['"])([^'"]*)\1\s*$"#)?
        .captures(value)?
        .map_or("{prefix}".into(), |found| found[2].into()))
}
fn controller_path(name: &str) -> Result<String> {
    Ok(regex(r"(?i)^App\\Http\\Controllers\\")?
        .replace(name.trim_start_matches('\\'), "")
        .into_owned())
}
fn handler(expression: &str) -> Result<Option<String>> {
    let value = trim(expression);
    let short = |name: &str| name.rsplit('\\').next().unwrap().to_owned();
    if let Some(found) = regex(r#"^\[\s*([A-Za-z_\\][\w\\]*)::class\s*,\s*['"]([^'"]+)['"]\s*\]"#)?
        .captures(value)?
    {
        return Ok(Some(format!("{}@{}", short(&found[1]), &found[2])));
    }
    if let Some(found) = regex(r#"^['"]([^'"@]+)@([^'"]+)['"]$"#)?.captures(value)? {
        return Ok(Some(format!(
            "{}@{}",
            controller_path(&found[1])?,
            &found[2]
        )));
    }
    if let Some(found) = regex(r"^([A-Za-z_\\][\w\\]*)::class")?.captures(value)? {
        return Ok(Some(short(&found[1])));
    }
    if let Some(found) = regex(r#"^['"]([A-Za-z_\\][\w\\]*)['"]$"#)?.captures(value)? {
        return Ok(Some(controller_path(&found[1])?));
    }
    Ok(None)
}
struct Group<'a> {
    open: usize,
    close: usize,
    prefix: String,
    args: Vec<&'a str>,
}
fn groups<'a>(scan: &Scan, safe: &'a str, original: &str) -> Result<Vec<Group<'a>>> {
    let mut out = Vec::new();
    for found in regex(r"\bRoute::(\w+)\s*\(")?.captures_iter(safe) {
        let found = found?;
        let mut name = found[1].to_owned();
        let mut open = found.get(0).unwrap().end() - 1;
        let mut prefixes = Vec::new();
        loop {
            let Some(close) = scan.close(safe, open)? else {
                break;
            };
            let args = scan.args(&safe[open + 1..close])?;
            if name == "prefix" {
                prefixes.push(literal(args.first().copied().unwrap_or(""))?);
            }
            if name == "group" {
                if prefixes.is_empty()
                    && let Some(first) = args.first().filter(|first| {
                        first
                            .trim_start_matches(crate::search::query_whitespace)
                            .starts_with('[')
                    })
                    && let Some(found) =
                        regex(r#"['"]prefix['"]\s*=>\s*([^,\]]+)"#)?.captures(first)?
                {
                    prefixes.push(literal(&found[1])?);
                }
                out.push(Group {
                    open,
                    close,
                    prefix: join(prefixes.iter().map(String::as_str)),
                    args,
                });
                break;
            }
            let end = super::rust::unit_end(original, close + 1, 199);
            let Some(next) = regex(r"^\s*->\s*(\w+)\s*\(")?.captures(&safe[close + 1..end])? else {
                break;
            };
            name = next[1].into();
            open = close + next.get(0).unwrap().end();
        }
    }
    Ok(out)
}
fn prefix(groups: &[Group<'_>], at: usize) -> String {
    let mut enclosing: Vec<_> = groups
        .iter()
        .filter(|group| group.open < at && at < group.close)
        .collect();
    enclosing.sort_by_key(|group| group.open);
    join(enclosing.iter().map(|group| group.prefix.as_str()))
}

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let ctx = Context::new(path, source, "php");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".php") {
        return Ok(out);
    }
    let safe = crate::synthesis::comments::strip_for_regex(source, "php");
    let scan = Scan::new();
    let groups = groups(&scan, &safe, source)?;
    for (pattern, resource) in [
        (
            r#"Route::(get|post|put|patch|delete|options|any)\s*\(\s*['"]([^'"]+)['"]\s*,\s*([^)]+)\)"#,
            false,
        ),
        (
            r#"Route::(resource|apiResource)\s*\(\s*['"]([^'"]+)['"]\s*(?:,\s*([^)]+))?\)"#,
            true,
        ),
    ] {
        for found in regex(pattern)?.captures_iter(&safe) {
            let found = found?;
            let text = found.get(0).unwrap();
            let line = ctx.position(text.start()).0;
            let method = if resource {
                "RESOURCE".into()
            } else {
                found[1].to_uppercase()
            };
            let route = &found[2];
            let in_file = join([prefix(&groups, text.start()).as_str(), route]);
            let mut node = super::routes::route(
                &ctx,
                line,
                &method,
                route,
                source[text.start()..text.end()].encode_utf16().count() as u32,
            );
            node.name = if resource {
                format!("resource:{}", &in_file[1..])
            } else {
                format!("{method} {in_file}")
            };
            node.qualified_name = format!("{path}::route:{in_file}");
            if let Some(expression) = found.get(3) {
                let args = if resource {
                    scan.args(expression.as_str())?
                } else {
                    Vec::new()
                };
                let expression = args.first().copied().unwrap_or(expression.as_str());
                if let Some(name) = handler(expression)? {
                    out.unresolved_references.push(super::routes::reference(
                        &node,
                        &name,
                        if resource { "imports" } else { "references" },
                    ));
                }
            }
            out.nodes.push(node);
        }
    }
    Ok(out)
}

fn normalize(value: &str) -> String {
    let rooted = value.starts_with('/');
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|part| *part != "..") {
                    parts.pop();
                } else if !rooted {
                    parts.push(part);
                }
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}
fn php_path(
    expression: &str,
    file: &str,
    files: &BTreeMap<String, String>,
) -> Result<Option<String>> {
    let value = trim(expression);
    let directory = file.rsplit_once('/').map_or("", |(directory, _)| directory);
    let parent = directory
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let mut path = None;
    for (pattern, base) in [
        (r#"^base_path\s*\(\s*(['"])([^'"]+)\1\s*\)$"#, None),
        (r#"^__DIR__\s*\.\s*(['"])([^'"]+)\1$"#, Some(directory)),
        (
            r#"^dirname\s*\(\s*__DIR__\s*\)\s*\.\s*(['"])([^'"]+)\1$"#,
            Some(parent),
        ),
        (r#"^(['"])([^'"]+\.php)\1$"#, None),
    ] {
        if let Some(found) = regex(pattern)?.captures(value)? {
            path = Some(base.map_or_else(
                || found[2].to_owned(),
                |base| format!("{base}/{}", &found[2]),
            ));
            break;
        }
    }
    let Some(path) = path.filter(|path| !path.is_empty()) else {
        return Ok(None);
    };
    let target = normalize(&path);
    Ok(
        (!target.starts_with("..") && target.ends_with(".php") && files.contains_key(&target))
            .then_some(target),
    )
}
struct Mount {
    source: String,
    target: String,
    prefix: String,
}
fn mounts(
    scan: &Scan,
    safe: &str,
    source: &str,
    file: &str,
    files: &BTreeMap<String, String>,
) -> Result<(Vec<Mount>, bool)> {
    let groups = groups(scan, safe, source)?;
    let mut out = Vec::new();
    let mut add = |expression: &str, prefix: String| -> Result<()> {
        if let Some(target) = php_path(expression, file, files)? {
            out.push(Mount {
                source: file.into(),
                target,
                prefix,
            });
        }
        Ok(())
    };
    for group in &groups {
        let body = group.args.last().copied().unwrap_or("");
        if !regex(r"^\s*(?:static\s+)?(?:function|fn)\b")?.is_match(body)? {
            add(body, prefix(&groups, group.open + 1))?;
        }
    }
    for found in
        regex(r"\b(?:require|include)(?:_once)?\b([^;]*);|\bloadRoutesFrom\s*\(([^;]*)\)\s*;")?
            .captures_iter(safe)
    {
        let found = found?;
        let mut expression = trim(found.get(1).or_else(|| found.get(2)).unwrap().as_str());
        if expression.starts_with('(') && scan.close(expression, 0)? == Some(expression.len() - 1) {
            expression = &expression[1..expression.len() - 1];
        }
        add(expression, prefix(&groups, found.get(0).unwrap().start()))?;
    }
    let mut custom = false;
    for found in regex(r"\bwithRouting\s*\(")?.find_iter(safe) {
        let found = found?;
        let Some(close) = scan.close(safe, found.end() - 1)? else {
            continue;
        };
        let mut named = HashMap::new();
        for arg in scan.args(&safe[found.end()..close])? {
            if let Some(found) = regex(r"^\s*(\w+)\s*:\s*([\s\S]*)$")?.captures(arg)? {
                named.insert(found[1].to_owned(), found[2].to_owned());
            }
        }
        custom |= named.contains_key("using");
        let api_prefix = named
            .get("apiPrefix")
            .map(|value| literal(value))
            .transpose()?
            .unwrap_or("api".into());
        for (key, prefix) in [("api", api_prefix.as_str()), ("web", "")] {
            let Some(value) = named.get(key).filter(|value| !value.is_empty()) else {
                continue;
            };
            let trimmed = trim(value);
            let items = if trimmed.starts_with('[') {
                let end = trimmed.char_indices().next_back().map_or(0, |(at, _)| at);
                scan.args(&trimmed[1..end])?
            } else {
                vec![value.as_str()]
            };
            for item in items {
                add(item, prefix.into())?;
            }
        }
    }
    Ok((out, custom))
}
pub(super) fn post_extract(nodes: &[Node], files: &BTreeMap<String, String>) -> Result<Vec<Node>> {
    let routes: Vec<_> = nodes
        .iter()
        .filter(|node| {
            node.kind == "route"
                && node.language == "php"
                && node.id.starts_with(&format!("route:{}:", node.file_path))
                && node
                    .qualified_name
                    .starts_with(&format!("{}::route:/", node.file_path))
        })
        .collect();
    if routes.is_empty() {
        return Ok(Vec::new());
    }
    let scan = Scan::new();
    let mut all = Vec::new();
    let mut custom = false;
    for (file, source) in files {
        if !file.ends_with(".php")
            || source.is_empty()
            || (!file.starts_with("routes/")
                && !source.contains("routes/")
                && !source.contains("withRouting("))
        {
            continue;
        }
        let safe = crate::synthesis::comments::strip_for_regex(source, "php");
        let (mounts, loader) = mounts(&scan, &safe, source, file, files)?;
        all.extend(mounts);
        custom |= loader;
    }
    let mut defaults = HashMap::new();
    if !custom
        && !all.iter().any(|mount| mount.target == "routes/api.php")
        && files.contains_key("routes/api.php")
    {
        defaults.insert("routes/api.php".to_owned(), "/api".to_owned());
    }
    let mut prefixes = defaults.clone();
    for _ in 0..8 {
        let mut next = defaults.clone();
        let mut ambiguous = HashSet::new();
        for mount in &all {
            if mount.source == mount.target {
                continue;
            }
            let full = join([
                prefixes.get(&mount.source).map_or("", String::as_str),
                &mount.prefix,
            ]);
            if next.get(&mount.target).is_some_and(|old| *old != full) {
                ambiguous.insert(mount.target.clone());
            } else {
                next.insert(mount.target.clone(), full);
            }
        }
        for path in ambiguous {
            next.remove(&path);
        }
        let changed = next != prefixes;
        prefixes = next;
        if !changed {
            break;
        }
    }
    let mut updates = Vec::new();
    for route in routes {
        let in_file = route.qualified_name.split_once("::route:").unwrap().1;
        let full = join([
            prefixes.get(&route.file_path).map_or("", String::as_str),
            in_file,
        ]);
        let name = if route.name.starts_with("resource:") {
            format!("resource:{}", &full[1..])
        } else {
            format!(
                "{} {full}",
                route.name.split_once(' ').map_or("", |(method, _)| method)
            )
        };
        if name != route.name {
            let mut node = route.clone();
            node.name = name;
            updates.push(node);
        }
    }
    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn records_and_bindings_match_independent_reference() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/laravel/cases.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let extracted = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut actual =
                json!({"nodes":extracted.nodes,"references":extracted.unresolved_references});
            for node in actual["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(
                actual, case["expected"],
                "{} {}",
                case["origin"], case["source"]
            );
        }
        for project in fixture["projects"].as_array().unwrap() {
            let mut nodes = project["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            let files: BTreeMap<String, String> =
                serde_json::from_value(project["files"].clone()).unwrap();
            let raw = files
                .iter()
                .filter(|(path, _)| path.ends_with(".php"))
                .flat_map(|(path, source)| extract(path, source).unwrap().nodes)
                .collect::<Vec<_>>();
            let updates = post_extract(&raw, &files).unwrap();
            for node in &raw {
                let node = updates
                    .iter()
                    .find(|update| update.id == node.id)
                    .unwrap_or(node);
                let expected = project["routes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|route| route["node"]["id"] == node.id)
                    .unwrap();
                assert_eq!(node.name, expected["node"]["name"], "{}", project["name"]);
            }
            for query in project["resolutions"].as_array().unwrap() {
                let reference = &query["reference"];
                let actual = resolve(
                    reference["fromNodeId"].as_str().unwrap(),
                    reference["referenceName"].as_str().unwrap(),
                    reference["referenceKind"].as_str().unwrap(),
                    |name| {
                        let mut found = nodes
                            .iter()
                            .filter(|node| node.name == name)
                            .collect::<Vec<_>>();
                        found.sort_by(|a, b| {
                            (&a.file_path, a.start_line, &a.id).cmp(&(
                                &b.file_path,
                                b.start_line,
                                &b.id,
                            ))
                        });
                        found
                    },
                    |path| {
                        let mut found = nodes
                            .iter()
                            .filter(|node| node.file_path == path)
                            .collect::<Vec<_>>();
                        found.sort_by(|a, b| (a.start_line, &a.id).cmp(&(b.start_line, &b.id)));
                        found
                    },
                )
                .unwrap();
                let actual = actual
                    .map(|(node, confidence)| json!({"id":node.id,"confidence":confidence}))
                    .unwrap_or(Value::Null);
                assert_eq!(
                    actual, query["expected"],
                    "{} {}",
                    project["name"], reference
                );
            }
        }
    }
}
