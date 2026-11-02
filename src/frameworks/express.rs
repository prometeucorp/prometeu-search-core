//! Express registrations, inline handler effects, and composed router mounts.
use super::routes::{reference, route};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, Node, supplemental::Context};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
mod handlers;
mod scan;

fn regex(pattern: &'static str) -> Result<Arc<fancy_regex::Regex>> {
    static CACHE: LazyLock<Mutex<HashMap<&'static str, Arc<fancy_regex::Regex>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut cache = CACHE.lock().expect("Express pattern cache");
    if let Some(compiled) = cache.get(pattern) {
        return Ok(compiled.clone());
    }
    let compiled = Arc::new(super::extractors::regex(pattern)?);
    cache.insert(pattern, compiled.clone());
    Ok(compiled)
}
fn trim(value: &str) -> &str {
    value.trim_matches(crate::search::query_whitespace)
}
fn language(path: &str) -> &'static str {
    if path.ends_with(".ts") || path.ends_with(".tsx") {
        "typescript"
    } else {
        "javascript"
    }
}
fn supported(path: &str) -> Result<bool> {
    Ok(regex(r"\.(m?js|tsx?|cjs)$")?.is_match(path)?)
}

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let ctx = Context::new(path, source, language(path));
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !supported(path)? {
        return Ok(out);
    }
    let safe = crate::synthesis::comments::strip_for_regex(source, language(path));
    for capture in regex(
        r#"\b(app|router)\s*\.\s*(get|post|put|patch|delete|all|use)\s*\(\s*['"]([^'"]+)['"]\s*,"#,
    )?
    .captures_iter(&safe)
    {
        let capture = capture?;
        let method = &capture[2];
        let endpoint = &capture[3];
        if method == "use" && !endpoint.starts_with('/') {
            continue;
        }
        let whole = capture.get(0).unwrap();
        let line = ctx.position(whole.start()).0;
        let node = route(
            &ctx,
            line,
            &method.to_uppercase(),
            endpoint,
            source[whole.start()..whole.end()].encode_utf16().count() as u32,
        );
        let open = whole.start() + safe[whole.start()..].find('(').unwrap();
        let call = scan::call(&safe, open)?;
        let args = call
            .as_ref()
            .map_or("", |(_, structure)| &structure[1..structure.len() - 1]);
        out.unresolved_references
            .extend(handlers::references(&ctx, &safe, open + 1, args, &node)?);
        out.nodes.push(node);
    }
    for capture in
        regex(r#"\b(?:app|router)\s*\.\s*route\s*\(\s*['"]([^'"]+)['"]\s*\)"#)?.captures_iter(&safe)
    {
        let capture = capture?;
        let endpoint = &capture[1];
        let mut at = capture.get(0).unwrap().end();
        loop {
            let Some(link) = regex(r"^\s*\.\s*(get|post|put|patch|delete|all)\s*\(")?
                .captures(scan::prefix(&safe[at..], 64))?
            else {
                break;
            };
            let open = at + link.get(0).unwrap().end() - 1;
            let Some((close, structure)) = scan::call(&safe, open)? else {
                break;
            };
            let line = ctx.position(open).0;
            let method = link[1].to_uppercase();
            let node = route(
                &ctx,
                line,
                &method,
                endpoint,
                source[at..open + 1].encode_utf16().count() as u32,
            );
            out.unresolved_references.extend(handlers::references(
                &ctx,
                &safe,
                open + 1,
                &structure[1..structure.len() - 1],
                &node,
            )?);
            out.nodes.push(node);
            at = close + 1;
        }
    }
    Ok(out)
}

fn join_paths(prefix: &str, path: &str) -> String {
    let a = prefix.trim_end_matches('/');
    let b = path.trim_start_matches('/');
    let joined = if b.is_empty() {
        a.into()
    } else {
        format!("{a}/{b}")
    };
    if joined.is_empty() {
        "/".into()
    } else {
        joined
    }
}

pub(super) fn post_extract(
    nodes: &[Node],
    files: &BTreeMap<String, String>,
    resolve_module: impl Fn(&str, &str, &str) -> Result<Option<String>>,
) -> Result<Vec<Node>> {
    let mut mounts: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (file, source) in files {
        if !supported(file)? || !source.contains(".use(") {
            continue;
        }
        let lang = language(file);
        let safe = crate::synthesis::comments::strip_for_regex(source, lang);
        let imports = crate::graph::js_import_mappings(source)?;
        for capture in
            regex(r#"\b[A-Za-z_$][\w$]*\.use\s*\(\s*(['"`])(/[^'"`]*)\1\s*,"#)?.captures_iter(&safe)
        {
            let capture = capture?;
            let at = capture.get(0).unwrap().start();
            let open = at + safe[at..].find('(').unwrap();
            let Some((close, structure)) = scan::call(&safe, open)? else {
                continue;
            };
            let args = scan::arguments(&structure[1..structure.len() - 1])?;
            let last = args
                .last()
                .map_or("", |part| trim(&safe[open + 1 + part.start..close]));
            let specifier = if let Some(inline) =
                regex(r#"^require\s*\(\s*(['"])([^'"]+)\1\s*\)(?:\.\w+)?$"#)?.captures(last)?
            {
                Some(inline[2].to_owned())
            } else if let Some(ident) =
                regex(r"^([A-Za-z_$][\w$]*)(?:\.(?:default|router|routes))?$")?.captures(last)?
            {
                let name = &ident[1];
                if let Some(mapping) = imports.iter().find(|mapping| mapping.local_name == name) {
                    Some(mapping.source.clone())
                } else {
                    let pattern = format!(
                        r#"\b(?:const|let|var)\s+{}\s*=\s*require\s*\(\s*(['"])([^'"]+)\1\s*\)"#,
                        name.replace('$', r"\$")
                    );
                    super::extractors::regex(&pattern)?
                        .captures(&safe)?
                        .map(|matched| matched[2].into())
                }
            } else {
                None
            };
            let Some(specifier) = specifier else { continue };
            let Some(target) = resolve_module(file, &specifier, lang)? else {
                continue;
            };
            if target != *file {
                mounts
                    .entry(file.clone())
                    .or_default()
                    .push((capture[2].into(), target));
            }
        }
    }
    let mut prefixes: BTreeMap<String, String> = BTreeMap::new();
    for _ in 0..8 {
        let mut next = BTreeMap::new();
        let mut ambiguous = HashSet::new();
        for (file, mounts) in &mounts {
            let base = prefixes.get(file).map_or("", String::as_str);
            for (prefix, target) in mounts {
                let full = join_paths(base, prefix);
                if next.get(target).is_some_and(|old| old != &full) {
                    ambiguous.insert(target);
                } else {
                    next.insert(target.clone(), full);
                }
            }
        }
        for ambiguous in ambiguous {
            next.remove(ambiguous);
        }
        if next == prefixes {
            break;
        }
        prefixes = next;
    }
    let mut updates = Vec::new();
    for node in nodes.iter().filter(|node| {
        node.kind == "route" && matches!(node.language.as_str(), "typescript" | "javascript")
    }) {
        let Some((_, tail)) = node.qualified_name.split_once("::") else {
            continue;
        };
        let Some((method, path)) = tail.split_once(':') else {
            continue;
        };
        if !path.starts_with('/')
            || !matches!(
                method,
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "ALL" | "USE"
            )
        {
            continue;
        }
        // Recompute names from canonical registration paths when a mount changes or disappears.
        let path = prefixes
            .get(&node.file_path)
            .filter(|prefix| !matches!(prefix.as_str(), "" | "/"))
            .map_or_else(|| path.to_owned(), |prefix| join_paths(prefix, path));
        let name = format!("{method} {path}");
        if name != node.name {
            let mut updated = node.clone();
            updated.name = name;
            updates.push(updated);
        }
    }
    Ok(updates)
}

/// Original framework patterns only accept declarations in the referencing file.
pub(crate) fn resolve<'a>(
    path: &str,
    name: &str,
    named: impl Fn(&str) -> Vec<&'a Node>,
    in_file: impl Fn(&str) -> Vec<&'a Node>,
) -> Result<Option<(&'a Node, f64)>> {
    let callable = |node: &&Node| matches!(node.kind.as_str(), "method" | "function");
    let middleware = |node: &&Node| {
        matches!(
            node.kind.as_str(),
            "function" | "method" | "variable" | "constant" | "class"
        )
    };
    if regex(r"(?i)^(?:auth|authenticate|authorization|cors|helmet|logger|errorHandler|notFound)$|^(?:validate|sanitize|rateLimit)|Middleware$")?.is_match(name)? {
        let mut candidates = named(name).into_iter().filter(middleware).collect::<Vec<_>>();
        candidates.sort_by_key(|node| node.file_path != path);
        let base = regex(r"(?i)Middleware$")?.replace(name, "");
        let found = candidates.into_iter().find(|node| node.name.to_lowercase() == name.to_lowercase() || node.name.to_lowercase() == base.to_lowercase()).or_else(|| {
            if base == name { return None; }
            let candidates = named(&base).into_iter().filter(middleware).collect::<Vec<_>>();
            candidates.iter().copied().find(|node| ["/middleware/", "/middlewares/"].iter().any(|dir| node.file_path.contains(dir))).or_else(|| candidates.first().copied())
        });
        if let Some(found) = found.filter(|node| node.file_path == path) { return Ok(Some((found, 0.8))); }
    }
    if let Some(capture) = regex(r"^(\w+)Controller\.(\w+)$")?.captures(name)? {
        let controller = &capture[1];
        let method = &capture[2];
        let found = named(method)
            .into_iter()
            .filter(callable)
            .find(|node| {
                node.file_path
                    .to_lowercase()
                    .contains(&controller.to_lowercase())
            })
            .or_else(|| {
                named(&format!("{controller}Controller"))
                    .into_iter()
                    .find_map(|controller| {
                        in_file(&controller.file_path)
                            .into_iter()
                            .filter(callable)
                            .find(|node| node.name == method)
                    })
            });
        if let Some(found) = found.filter(|node| node.file_path == path) {
            return Ok(Some((found, 0.85)));
        }
    }
    if let Some(capture) = regex(r"^(\w+)(Service|Helper|Utils?)\.(\w+)$")?.captures(name)? {
        let stripped = capture[1].to_lowercase();
        if let Some(found) = named(&capture[3])
            .into_iter()
            .filter(callable)
            .find(|node| node.file_path.to_lowercase().contains(&stripped))
            .filter(|node| node.file_path == path)
        {
            return Ok(Some((found, 0.8)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn corpus() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/express/cases.json"
        ))
        .unwrap()
    }
    #[test]
    fn extraction_and_same_file_resolution_match_reference() {
        let corpus = corpus();
        for case in corpus["cases"].as_array().unwrap() {
            let result = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut nodes = serde_json::to_value(result.nodes).unwrap();
            for node in nodes.as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(
                nodes, case["expected"]["nodes"],
                "{} {}",
                case["origin"], case["source"]
            );
            assert_eq!(
                serde_json::to_value(result.unresolved_references).unwrap(),
                case["expected"]["references"],
                "{} {}",
                case["origin"],
                case["source"]
            );
        }
        let mut records = corpus["resolver_nodes"].clone();
        for node in records.as_array_mut().unwrap() {
            node["updatedAt"] = 0.into();
        }
        let nodes: Vec<Node> = serde_json::from_value(records).unwrap();
        for case in corpus["resolutions"].as_array().unwrap() {
            let reference = &case["reference"];
            let actual = resolve(
                reference["filePath"].as_str().unwrap(),
                reference["referenceName"].as_str().unwrap(),
                |name| nodes.iter().filter(|node| node.name == name).collect(),
                |path| nodes.iter().filter(|node| node.file_path == path).collect(),
            )
            .unwrap()
            .map(|(node, confidence)| json!({"id":node.id,"confidence":confidence}));
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "{reference}"
            );
        }
    }
}
