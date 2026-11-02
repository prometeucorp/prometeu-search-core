//! Python framework name preferences and the declarations each caller can reach.
use anyhow::{Result, ensure};
use prometeu_extract::{Node, UnresolvedReference};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex};
mod guards;
mod scope;

pub(crate) fn regex(pattern: &str) -> Result<Arc<fancy_regex::Regex>> {
    static CACHE: LazyLock<Mutex<HashMap<String, Arc<fancy_regex::Regex>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut cache = CACHE.lock().expect("Python framework pattern cache");
    if let Some(compiled) = cache.get(pattern) {
        return Ok(compiled.clone());
    }
    let compiled = Arc::new(super::extractors::regex(pattern)?);
    if cache.len() < 512 && pattern.len() <= 4096 {
        cache.insert(pattern.into(), compiled.clone());
    }
    Ok(compiled)
}
mod scoring;

#[derive(Clone, Copy)]
pub(crate) struct Answer<'a> {
    pub node: &'a Node,
    pub confidence: f64,
    pub resolved_by: &'static str,
    pub framework: Option<&'static str>,
}

pub(crate) struct Environment<'a> {
    pub sources: &'a BTreeMap<&'a str, &'a str>,
    pub module: &'a dyn Fn(&str, &str) -> Option<String>,
    pub supertypes: &'a dyn Fn(&str) -> Vec<&'a Node>,
    lines: HashMap<&'a str, Vec<&'a str>>,
}
impl<'a> Environment<'a> {
    pub(crate) fn new(
        sources: &'a BTreeMap<&'a str, &'a str>,
        module: &'a dyn Fn(&str, &str) -> Option<String>,
        supertypes: &'a dyn Fn(&str) -> Vec<&'a Node>,
    ) -> Self {
        Self {
            sources,
            module,
            supertypes,
            lines: sources
                .iter()
                .map(|(path, source)| {
                    (
                        *path,
                        source
                            .split('\n')
                            .map(|line| line.strip_suffix('\r').unwrap_or(line))
                            .collect(),
                    )
                })
                .collect(),
        }
    }
    fn lines(&self, path: &str) -> &[&'a str] {
        self.lines.get(path).map(Vec::as_slice).unwrap_or_default()
    }
}

pub(crate) struct Bindings<'a> {
    named: HashMap<&'a str, Vec<&'a Node>>,
    qualified: HashMap<&'a str, Vec<&'a Node>>,
    files: HashMap<&'a str, Vec<&'a Node>>,
    scopes: RefCell<HashMap<&'a str, Option<(u32, u32)>>>,
    ids: HashMap<&'a str, &'a Node>,
    imports: RefCell<HashMap<String, HashMap<String, String>>>,
    local_bindings: RefCell<HashMap<(String, String, String, u32), bool>>,
    safe_file: RefCell<Option<(String, Vec<String>)>>,
    work: Cell<usize>,
    mockless: Cell<bool>,
}
impl<'a> Bindings<'a> {
    pub(crate) fn handles(reference: &UnresolvedReference, frameworks: &[String]) -> Result<bool> {
        if reference.reference_kind == "function_ref"
            || reference.language.as_deref() != Some("python")
            || !regex(r"^[A-Za-z_]\w*$")?.is_match(&reference.reference_name)?
        {
            return Ok(false);
        }
        let name = &reference.reference_name;
        Ok(frameworks.iter().any(|framework| match framework.as_str() {
            "django" => {
                name == "_iterable_class"
                    || name.ends_with("Model")
                    || name.ends_with("View")
                    || name.ends_with("ViewSet")
                    || name.ends_with("Form")
                    || (name.starts_with(|ch: char| ch.is_ascii_uppercase())
                        && name[1..].chars().all(|ch| ch.is_ascii_lowercase())
                        && name.len() > 1)
            }
            "flask" => name.ends_with("_bp") || name.ends_with("_blueprint"),
            "fastapi" => {
                name == "router"
                    || name.ends_with("_router")
                    || name.starts_with("get_")
                    || name.starts_with("Depends")
            }
            _ => false,
        }))
    }
    pub(crate) fn new(nodes: &'a [Node]) -> Self {
        let mut out = Self {
            named: HashMap::new(),
            qualified: HashMap::new(),
            files: HashMap::new(),
            scopes: RefCell::new(HashMap::new()),
            ids: HashMap::new(),
            imports: RefCell::new(HashMap::new()),
            local_bindings: RefCell::new(HashMap::new()),
            safe_file: RefCell::new(None),
            work: Cell::new(0),
            mockless: Cell::new(false),
        };
        for node in nodes {
            out.ids.insert(&node.id, node);
            out.named.entry(&node.name).or_default().push(node);
            out.qualified
                .entry(&node.qualified_name)
                .or_default()
                .push(node);
            out.files.entry(&node.file_path).or_default().push(node);
        }
        for candidates in out
            .named
            .values_mut()
            .chain(out.qualified.values_mut())
            .chain(out.files.values_mut())
        {
            candidates.sort_by(|a, b| {
                (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
            });
        }
        out
    }
    fn hidden(&self, node: &Node, reference: &UnresolvedReference) -> bool {
        self.mockless.get()
            && !crate::classify::is_test_path(reference.file_path.as_deref().unwrap_or_default())
            && node
                .file_path
                .replace('\\', "/")
                .to_lowercase()
                .split('/')
                .any(|part| part == "__mocks__")
    }
    fn charge(&self, count: usize) -> Result<()> {
        let work = self.work.get().saturating_add(count);
        ensure!(
            work <= 2_000_000,
            "Python framework candidate work limit exceeded"
        );
        self.work.set(work);
        Ok(())
    }
    fn named(&self, name: &str) -> &[&'a Node] {
        self.named.get(name).map(Vec::as_slice).unwrap_or_default()
    }
    fn in_file(&self, path: &str) -> &[&'a Node] {
        self.files.get(path).map(Vec::as_slice).unwrap_or_default()
    }
    fn pick(
        &self,
        reference: &UnresolvedReference,
        kinds: &[&str],
        directories: &[&str],
    ) -> Result<Option<&'a Node>> {
        let from = reference.file_path.as_deref().unwrap_or_default();
        let directory = from.rfind('/').map_or("", |at| &from[..at + 1]);
        let inheritance = matches!(reference.reference_kind.as_str(), "extends" | "implements");
        let mut same_directory = None;
        let mut preferred = None;
        let mut first = None;
        for &candidate in self.named(&reference.reference_name) {
            self.charge(1)?;
            if self.hidden(candidate, reference)
                || !kinds.contains(&candidate.kind.as_str())
                || inheritance && candidate.id == reference.from_node_id
                || !self.lexically_reachable(candidate, reference)?
                || candidate.file_path != from
                    && (self.nested_type(candidate) || !scope::visible(candidate, reference)?)
            {
                continue;
            }
            if candidate.file_path == from {
                return Ok(Some(candidate));
            }
            first.get_or_insert(candidate);
            if candidate.file_path.starts_with(directory)
                && !candidate.file_path[directory.len()..].contains('/')
            {
                same_directory.get_or_insert(candidate);
            }
            if directories
                .iter()
                .any(|directory| candidate.file_path.contains(directory))
            {
                preferred.get_or_insert(candidate);
            }
        }
        Ok(same_directory.or(preferred).or(first))
    }
    pub(crate) fn resolve(
        &self,
        reference: &UnresolvedReference,
        framework: &str,
    ) -> Result<Option<(&'a Node, f64)>> {
        let name = &reference.reference_name;
        match framework {
            "django" => {
                if (name.ends_with("Model") || regex(r"^[A-Z][a-z]+$")?.is_match(name)?)
                    && let Some(node) = self.pick(
                        reference,
                        &["class"],
                        &["models", "app/models", "src/models"],
                    )?
                {
                    return Ok(Some((node, 0.8)));
                }
                if (name.ends_with("View") || name.ends_with("ViewSet"))
                    && let Some(node) = self.pick(
                        reference,
                        &["class", "function"],
                        &["views", "app/views", "src/views", "api/views"],
                    )?
                {
                    return Ok(Some((node, 0.8)));
                }
                if name.ends_with("Form")
                    && let Some(node) =
                        self.pick(reference, &["class"], &["forms", "app/forms", "src/forms"])?
                {
                    return Ok(Some((node, 0.8)));
                }
                if name == "_iterable_class"
                    && let Some(class) = self
                        .named("ModelIterable")
                        .iter()
                        .find(|node| node.kind == "class" && !self.hidden(node, reference))
                {
                    return Ok(self
                        .named("__iter__")
                        .iter()
                        .copied()
                        .find(|node| {
                            node.file_path == class.file_path
                                && node.start_line >= class.start_line
                                && node.start_line <= class.end_line
                        })
                        .map(|node| (node, 0.7)));
                }
            }
            "flask" => {
                if name.ends_with("_bp") || name.ends_with("_blueprint") {
                    return Ok(self
                        .pick(reference, &["variable"], &[])?
                        .map(|node| (node, 0.8)));
                }
            }
            "fastapi" => {
                if (name.ends_with("_router") || name == "router")
                    && let Some(node) = self.pick(
                        reference,
                        &["variable"],
                        &["/routers/", "/api/", "/routes/", "/endpoints/"],
                    )?
                {
                    return Ok(Some((node, 0.8)));
                }
                if name.starts_with("get_") || name.starts_with("Depends") {
                    return Ok(self
                        .pick(
                            reference,
                            &["function"],
                            &["/dependencies/", "/deps/", "/core/"],
                        )?
                        .map(|node| (node, 0.75)));
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn value(answer: Option<Answer<'_>>) -> Value {
        answer.map_or(Value::Null, |answer| json!({"target":answer.node.id,"confidence":answer.confidence,"resolved_by":answer.resolved_by,"framework":answer.framework}))
    }
    #[test]
    fn python_framework_candidates_and_competing_strategies_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/python-frameworks/cases.json"
        ))
        .unwrap();
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
            let module = |specifier: &str, from: &str| {
                project["modules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry["specifier"] == specifier && entry["from"] == from)
                    .and_then(|entry| entry["target"].as_str())
                    .map(str::to_owned)
            };
            let supers = |id: &str| {
                project["supertypes"][id]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|id| {
                        nodes
                            .iter()
                            .find(|node| Some(node.id.as_str()) == id.as_str())
                    })
                    .collect()
            };
            let environment = Environment::new(&sources, &module, &supers);
            let bindings = Bindings::new(&nodes);
            let frameworks: Vec<String> =
                serde_json::from_value(project["frameworks"].clone()).unwrap();
            for query in project["queries"].as_array().unwrap() {
                let reference: UnresolvedReference =
                    serde_json::from_value(query["reference"].clone()).unwrap();
                for framework in ["django", "flask", "fastapi"] {
                    let actual = bindings.resolve(&reference, framework).unwrap().map(
                        |(node, confidence)| Answer {
                            node,
                            confidence,
                            resolved_by: "framework",
                            framework: None,
                        },
                    );
                    assert_eq!(
                        value(actual),
                        query["pure"][framework],
                        "{} {framework} {}",
                        project["name"],
                        query["reference"]
                    );
                }
                if !Bindings::handles(&reference, &frameworks).unwrap() {
                    continue;
                }
                let imported = query["imported"]["target"].as_str().map(|id| Answer {
                    node: nodes.iter().find(|node| node.id == id).unwrap(),
                    confidence: query["imported"]["confidence"].as_f64().unwrap(),
                    resolved_by: "import",
                    framework: None,
                });
                let actual = bindings
                    .choose(&reference, &frameworks, &environment, imported)
                    .unwrap();
                assert_eq!(
                    value(actual),
                    query["expected"],
                    "{} {}",
                    project["name"],
                    query["reference"]
                );
            }
        }
    }
}
