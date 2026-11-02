//! Drupal routes, container definitions, hooks, and plugin discovery.
use super::{frontend::object, routes::reference};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, Node, UnresolvedReference, supplemental::Context};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
mod attributes;
mod yaml;

fn regex(pattern: &'static str) -> Result<Arc<fancy_regex::Regex>> {
    static CACHE: LazyLock<Mutex<HashMap<&'static str, Arc<fancy_regex::Regex>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut cache = CACHE.lock().expect("Drupal pattern cache");
    if let Some(pattern) = cache.get(pattern) {
        return Ok(pattern.clone());
    }
    let compiled = Arc::new(super::extractors::regex(pattern)?);
    cache.insert(pattern, compiled.clone());
    Ok(compiled)
}
fn trim(value: &str) -> &str {
    value.trim_matches(crate::search::query_whitespace)
}
fn hook_file(path: &str) -> bool {
    [".module", ".install", ".theme", ".inc"]
        .iter()
        .any(|extension| path.ends_with(extension))
}
fn node(ctx: &Context<'_>, kind: &str, name: &str, qualified: &str, id: String, line: u32) -> Node {
    let mut node = ctx.result.nodes[0].clone();
    node.id = id;
    node.kind = kind.into();
    node.name = name.into();
    node.qualified_name = format!("{}::{qualified}", ctx.path);
    node.start_line = line;
    node.end_line = line;
    node.start_column = 0;
    node.end_column = 0;
    node.is_exported = None;
    node
}
fn hook_reference(
    path: &str,
    kind: &str,
    name: &str,
    hook: &str,
    line: u32,
) -> UnresolvedReference {
    UnresolvedReference {
        from_node_id: prometeu_extract::node_id(path, kind, name, line),
        reference_name: hook.into(),
        reference_kind: "references".into(),
        line,
        column: 0,
        file_path: Some(path.into()),
        language: Some("php".into()),
        candidates: None,
    }
}
fn hooks(ctx: &Context<'_>) -> Result<Vec<UnresolvedReference>> {
    let mut functions = Vec::<(String, u32)>::new();
    let mut seen = HashSet::new();
    for capture in regex(r"(?m)^function\s+(\w+)\s*\(")?.captures_iter(ctx.source) {
        let capture = capture?;
        if seen.insert(capture[1].to_owned()) {
            functions.push((
                capture[1].into(),
                ctx.position(capture.get(0).unwrap().start()).0,
            ));
        }
    }
    let mut out = Vec::new();
    let mut matched = HashSet::new();
    for capture in regex(r"/\*\*[\s\S]*?(?:@|\*\s+)Implements\s+(hook_\w+)\s*\(\)[\s\S]*?\*/\s*\n(?:\s*\n)*function\s+(\w+)\s*\(")?.captures_iter(ctx.source) {
        let capture = capture?;
        if let Some((name, line)) = functions.iter().find(|(name, _)| name == &capture[2]) {
            out.push(hook_reference(ctx.path, "function", name, &capture[1], *line));
        }
        matched.insert(capture[2].to_owned());
    }
    if let Some(module) = regex(r"/([^/]+)\.[^./]+$")?.captures(ctx.path)? {
        let prefix = format!("{}_", &module[1]);
        for (name, line) in functions {
            if matched.contains(&name) {
                continue;
            }
            if let Some(suffix) = name
                .strip_prefix(&prefix)
                .filter(|suffix| !suffix.is_empty())
            {
                out.push(hook_reference(
                    ctx.path,
                    "function",
                    &name,
                    &format!("hook_{suffix}"),
                    line,
                ));
            }
        }
    }
    Ok(out)
}

pub(crate) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let language = if path.ends_with(".yml") {
        "yaml"
    } else {
        "php"
    };
    let ctx = Context::new(path, source, language);
    let mut result = ctx.result.clone();
    result.nodes.clear();
    if path.ends_with(".routing.yml") {
        yaml::routes(&ctx, &mut result)?;
    } else if path.ends_with(".services.yml") {
        yaml::services(&ctx, &mut result)?;
    } else if hook_file(path) || path.ends_with(".php") {
        result.unresolved_references = hooks(&ctx)?;
        let declarations = attributes::declarations(source)?;
        result
            .unresolved_references
            .extend(attributes::hooks(&ctx, &declarations)?);
        attributes::plugins(&ctx, &declarations, &mut result)?;
    }
    Ok(result)
}

pub(crate) fn detect(manifests: &BTreeMap<&str, &str>, files: &[&str]) -> bool {
    if let Some(value) = manifests
        .get("composer.json")
        .and_then(|source| serde_json::from_str::<serde_json::Value>(source).ok())
        && (value["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("drupal/"))
            || value["type"]
                .as_str()
                .is_some_and(|name| name.starts_with("drupal-"))
            || ["require", "require-dev"].iter().any(|field| {
                value[field]
                    .as_object()
                    .is_some_and(|deps| deps.keys().any(|name| name.starts_with("drupal/")))
            }))
    {
        return true;
    }
    files.iter().any(|path| path.ends_with(".info.yml"))
        && files.iter().any(|path| {
            [".routing.yml", ".module", ".install", ".theme"]
                .iter()
                .any(|extension| path.ends_with(extension))
        })
}

pub(crate) fn claims(name: &str) -> Result<bool> {
    Ok(name.starts_with("hook_")
        || name.starts_with("drupal:")
        || name.contains('\\')
        || regex(r"^[A-Za-z_]\w*::?\w+$")?.is_match(name)?
        || regex(r"^(?:this->)?container->get\('([\w.\\-]+)'\)\.(\w+)$")?.is_match(name)?
        || regex(r"^\\?Drupal::service\(\)\.(\w+)$")?.is_match(name)?)
}

/// Borrowed lookup tables retain reference database order for each lookup.
pub(crate) struct Bindings<'a> {
    named: HashMap<&'a str, Vec<&'a Node>>,
    qualified: HashMap<&'a str, Vec<&'a Node>>,
    files: HashMap<&'a str, Vec<&'a Node>>,
    functions: Vec<&'a Node>,
    lines: HashMap<&'a str, Vec<&'a str>>,
}
impl<'a> Bindings<'a> {
    pub(crate) fn new(nodes: &'a [Node], sources: &BTreeMap<&'a str, &'a str>) -> Self {
        let mut out = Self {
            named: HashMap::new(),
            qualified: HashMap::new(),
            files: HashMap::new(),
            functions: Vec::new(),
            lines: sources
                .iter()
                .map(|(file, source)| (*file, source.split('\n').collect()))
                .collect(),
        };
        for node in nodes {
            out.named.entry(&node.name).or_default().push(node);
            out.qualified
                .entry(&node.qualified_name)
                .or_default()
                .push(node);
            out.files.entry(&node.file_path).or_default().push(node);
            if node.kind == "function" {
                out.functions.push(node);
            }
        }
        out
    }
    fn named(&self, name: &str) -> &[&'a Node] {
        self.named.get(name).map(Vec::as_slice).unwrap_or_default()
    }
    fn in_file(&self, file: &str) -> &[&'a Node] {
        self.files.get(file).map(Vec::as_slice).unwrap_or_default()
    }
    pub(crate) fn resolve(
        &self,
        reference: &UnresolvedReference,
        supertypes: impl Fn(&str) -> Vec<&'a Node>,
    ) -> Result<Option<(&'a Node, f64)>> {
        let name = &reference.reference_name;
        let class = |fqcn: &str| {
            let clean = fqcn.trim_start_matches('\\');
            let (namespace, short) = clean.rsplit_once('\\')?;
            if namespace.is_empty() {
                return None;
            }
            let qualified = format!("{namespace}::{short}");
            self.qualified
                .get(qualified.as_str())
                .and_then(|nodes| nodes.iter().copied().find(|node| node.kind == "class"))
        };
        let service = |id: &str| {
            let name = format!("service:{id}");
            self.named(&name)
                .iter()
                .copied()
                .find(|node| node.kind == "component")
        };
        if let Some(id) = name.strip_prefix("drupal:service:") {
            return Ok(service(id).map(|node| (node, 0.9)));
        }
        if let Some(fqcn) = name.strip_prefix("drupal:fqcn:") {
            return Ok(class(&fqcn.replacen(':', "", 1)).map(|node| (node, 0.9)));
        }
        if let Some(encoded) = name.strip_prefix("drupal:class:") {
            let mut parts = encoded.split('#');
            let path = parts.next().unwrap_or_default();
            let class_name = parts.next();
            return Ok(self
                .in_file(path)
                .iter()
                .copied()
                .find(|node| node.kind == "class" && Some(node.name.as_str()) == class_name)
                .map(|node| (node, 0.95)));
        }
        let on_container =
            regex(r"^(?:this->)?container->get\('([\w.\\-]+)'\)\.(\w+)$")?.captures(name)?;
        let on_drupal = if on_container.is_none() {
            regex(r"^\\?Drupal::service\(\)\.(\w+)$")?.captures(name)?
        } else {
            None
        };
        if on_container.is_some() || on_drupal.is_some() {
            let method = on_container
                .as_ref()
                .map_or_else(|| &on_drupal.as_ref().unwrap()[1], |capture| &capture[2]);
            let mut id = on_container.as_ref().map(|capture| capture[1].to_owned());
            if id.is_none() {
                let line = reference
                    .file_path
                    .as_deref()
                    .and_then(|path| self.lines.get(path))
                    .and_then(|lines| lines.get(reference.line.saturating_sub(1) as usize))
                    .copied()
                    .unwrap_or_default();
                for capture in
                    regex(r#"Drupal::service\(\s*['"]([\w.\\-]+)['"]\s*\)\s*->\s*(\w+)"#)?
                        .captures_iter(line)
                {
                    let capture = capture?;
                    if &capture[2] == method {
                        id = Some(capture[1].into());
                        break;
                    }
                }
            }
            let Some(class) = id
                .as_deref()
                .and_then(service)
                .and_then(|service| service.signature.as_deref())
                .and_then(class)
            else {
                return Ok(None);
            };
            let mut level = vec![class];
            let mut seen = HashSet::new();
            for _ in 0..6 {
                let mut next = Vec::new();
                for class in level {
                    if !seen.insert(&class.id) {
                        continue;
                    }
                    let prefix = format!("{}::", class.qualified_name);
                    if let Some(method) =
                        self.in_file(&class.file_path).iter().copied().find(|node| {
                            node.kind == "method"
                                && node.name == method
                                && node.qualified_name.starts_with(&prefix)
                        })
                    {
                        return Ok(Some((method, 0.9)));
                    }
                    next.extend(supertypes(&class.id));
                }
                level = next;
                if level.is_empty() {
                    break;
                }
            }
            return Ok(None);
        }
        if let Some(capture) =
            regex(r"^\\?(?:Drupal\\[^:]+\\)?([^\\:]+):{1,2}(\w+)$")?.captures(name)?
            && let Some(class) = self
                .named(&capture[1])
                .iter()
                .copied()
                .find(|node| node.kind == "class")
        {
            let method = self
                .in_file(&class.file_path)
                .iter()
                .copied()
                .find(|node| node.kind == "method" && node.name == capture[2]);
            return Ok(Some(method.map_or((class, 0.7), |node| (node, 0.9))));
        }
        if name.contains('\\') && !name.contains(':') {
            let short = trim(name.trim_start_matches('\\'))
                .rsplit_once('\\')
                .map(|(_, short)| short);
            if let Some(class) = class(name).or_else(|| {
                short.and_then(|short| {
                    self.named(short)
                        .iter()
                        .copied()
                        .find(|node| node.kind == "class")
                })
            }) {
                return Ok(Some((class, 0.85)));
            }
        }
        if let Some(suffix) = name.strip_prefix("hook_")
            && !reference.from_node_id.starts_with("method:")
        {
            let suffix = format!("_{suffix}");
            return Ok(self
                .functions
                .iter()
                .copied()
                .find(|node| node.name.ends_with(&suffix) && hook_file(&node.file_path))
                .map(|node| (node, 0.75)));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn drupal_extraction_detection_and_resolution_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/drupal/cases.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let path = case["path"].as_str().unwrap();
            let source = case["source"].as_str().unwrap();
            let result = extract(path, source).unwrap();
            let mut nodes = serde_json::to_value(result.nodes).unwrap();
            for node in nodes.as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(nodes, case["expected"]["nodes"], "{path}\n{source}");
            assert_eq!(
                serde_json::to_value(result.unresolved_references).unwrap(),
                case["expected"]["references"],
                "{path}\n{source}"
            );
        }
        for case in corpus["detections"].as_array().unwrap() {
            let manifests = case["manifests"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let files = case["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|path| path.as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                detect(&manifests, &files),
                case["expected"].as_bool().unwrap(),
                "{case}"
            );
        }
        for case in corpus["claims"].as_array().unwrap() {
            assert_eq!(
                claims(case["name"].as_str().unwrap()).unwrap(),
                case["expected"].as_bool().unwrap(),
                "{case}"
            );
        }
        for project in corpus["projects"].as_array().unwrap() {
            let nodes: Vec<Node> = project["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .cloned()
                .map(|mut node| {
                    node["updatedAt"] = 0.into();
                    serde_json::from_value(node).unwrap()
                })
                .collect();
            let sources = project["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let bindings = Bindings::new(&nodes, &sources);
            for case in project["resolutions"].as_array().unwrap() {
                let reference: UnresolvedReference =
                    serde_json::from_value(case["reference"].clone()).unwrap();
                let result = bindings
                    .resolve(&reference, |id| {
                        project["superclasses"][id]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|id| {
                                nodes
                                    .iter()
                                    .find(|node| Some(node.id.as_str()) == id.as_str())
                            })
                            .collect()
                    })
                    .unwrap();
                let result = result.map(
                    |(node, confidence)| serde_json::json!({"id":node.id,"confidence":confidence}),
                );
                assert_eq!(
                    serde_json::to_value(result).unwrap(),
                    case["expected"],
                    "{} {case}",
                    project["name"]
                );
            }
        }
    }
}
