//! Type parameter shadowing and Scala member visibility across resolver strategies.
use crate::frameworks::source_pattern as regex;
use anyhow::{Result, ensure};
use prometeu_extract::{Node, UnresolvedReference};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) const LANGUAGES: &[&str] = &[
    "scala",
    "java",
    "kotlin",
    "csharp",
    "typescript",
    "tsx",
    "swift",
    "rust",
    "dart",
    "go",
    "cpp",
    "vbnet",
    "arkts",
];
const DECLARATIONS: &[&str] = &[
    "class",
    "struct",
    "interface",
    "trait",
    "protocol",
    "enum",
    "type_alias",
    "function",
    "method",
    "union",
    "module",
];
const SCALA_OWNERS: &[&str] = &["class", "trait", "module", "interface", "struct", "enum"];
const SCALA_TYPES: &[&str] = &[
    "class",
    "trait",
    "interface",
    "struct",
    "enum",
    "type_alias",
];

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$')
}
fn name_index(head: &str, name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    head.match_indices(name).find_map(|(at, _)| {
        (!at.checked_sub(1)
            .is_some_and(|at| word(head.as_bytes()[at]))
            && !head
                .as_bytes()
                .get(at + name.len())
                .is_some_and(|&c| word(c)))
        .then_some(at)
    })
}
fn opens(c: u8, angle: bool) -> bool {
    matches!(c, b'[' | b'(') || angle && c == b'<'
}
fn closes(c: u8, angle: bool, previous: u8) -> bool {
    matches!(c, b']' | b')') || angle && c == b'>' && !matches!(previous, b'-' | b'=')
}
fn matching(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let angle = bytes[open] == b'<';
    let closer = if angle {
        b'>'
    } else if bytes[open] == b'[' {
        b']'
    } else {
        b')'
    };
    let mut depth = 0i32;
    for at in open..bytes.len() {
        let c = bytes[at];
        if matches!(c, b'{' | b';') {
            return None;
        }
        if opens(c, angle) {
            depth += 1;
        } else if closes(c, angle, at.checked_sub(1).map_or(0, |i| bytes[i])) {
            depth -= 1;
            if depth == 0 {
                return (c == closer).then_some(at);
            }
        }
    }
    None
}
fn split(text: &str, angle: bool) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut from = 0;
    let mut parts = Vec::new();
    for (at, &c) in bytes.iter().enumerate() {
        if opens(c, angle) {
            depth += 1;
        } else if closes(c, angle, at.checked_sub(1).map_or(0, |i| bytes[i])) {
            depth -= 1;
        } else if c == b',' && depth == 0 {
            parts.push(&text[from..at]);
            from = at + 1;
        }
    }
    parts.push(&text[from..]);
    parts
}
fn without_annotations(text: &str) -> Result<String> {
    Ok(regex(r"^\s*(?:@[\w.]+(?:\([^()]*\))?\s*)+")?
        .replace(text, "")
        .into_owned())
}
fn collect(
    text: &str,
    open: usize,
    names: &mut BTreeMap<String, Option<String>>,
    last: bool,
) -> Result<()> {
    let Some(close) = matching(text, open) else {
        return Ok(());
    };
    for item in split(&text[open + 1..close], text.as_bytes()[open] == b'<') {
        if last {
            let (parameter, default) =
                item.split_once('=')
                    .map_or((item, None), |(name, default)| {
                        (
                            name,
                            Some(
                                default
                                    .trim_matches(crate::search::query_whitespace)
                                    .to_string(),
                            ),
                        )
                    });
            if let Some(found) = regex(r"([A-Za-z_]\w*)\s*$")?.captures(parameter)? {
                let name = &found[1];
                if !matches!(name, "typename" | "class" | "struct") {
                    names.insert(name.to_string(), default);
                }
            }
        } else {
            let bare = without_annotations(item)?;
            if let Some(found) = regex(r"^\s*(?:(?:in|out|reified|const|typename|class|struct)\s+|[+\-']\s*)*([A-Za-z_]\w*)")?.captures(&bare)? {
                let name = &found[1];
                if !matches!(name, "typename" | "class") { names.entry(name.to_string()).or_insert(None); }
            }
        }
    }
    Ok(())
}

/// Read a declaration's first forty lines, plus the preceding C++ template head.
pub(crate) fn declared(
    node: &Node,
    head: &str,
    above: &str,
) -> Result<BTreeMap<String, Option<String>>> {
    let mut names = BTreeMap::new();
    if node.language != "cpp" {
        if let Some(at) = name_index(head, &node.name) {
            let mut open = at + node.name.len();
            while matches!(head.as_bytes().get(open), Some(b' ' | b'\t')) {
                open += 1;
            }
            if matches!(head.as_bytes().get(open), Some(b'<' | b'[')) {
                collect(head, open, &mut names, false)?;
            }
            for index in 0..at {
                if head.as_bytes()[index] == b'<'
                    && (index == 0
                        || head.as_bytes()[index - 1].is_ascii_whitespace()
                        || matches!(head.as_bytes()[index - 1], b'(' | b','))
                {
                    collect(&head[..at], index, &mut names, false)?;
                }
            }
        }
        if node.language == "go"
            && node.kind == "method"
            && let Some(receiver) =
                regex(r"^\s*func\s*\(\s*(?:[A-Za-z_]\w*\s+)?\*?\s*[A-Za-z_]\w*\s*\[")?.find(head)?
        {
            collect(head, receiver.end() - 1, &mut names, false)?;
        }
    } else {
        for found in regex(r"\btemplate\s*<")?.find_iter(above) {
            collect(above, found?.end() - 1, &mut names, true)?;
        }
    }
    Ok(names)
}
fn value_parameters(node: &Node, head: &str) -> Result<HashSet<String>> {
    let mut names = HashSet::new();
    let Some(at) = name_index(head, &node.name) else {
        return Ok(names);
    };
    let mut at = at + node.name.len();
    let blanks = |at: &mut usize| {
        while let Some(c) = head[*at..]
            .chars()
            .next()
            .filter(|c| crate::search::query_whitespace(*c))
        {
            *at += c.len_utf8();
        }
    };
    blanks(&mut at);
    if head.as_bytes().get(at) == Some(&b'[') {
        at = matching(head, at).map_or(head.len(), |end| end + 1);
    }
    loop {
        blanks(&mut at);
        if head.as_bytes().get(at) != Some(&b'(') {
            break;
        }
        let Some(close) = matching(head, at) else {
            break;
        };
        for item in split(&head[at + 1..close], false) {
            let bare = without_annotations(item)?;
            if let Some(found) = regex(r"^\s*(?:(?:implicit|using|val|var|override|private|protected|final|inline|erased)\s+)*([A-Za-z_]\w*)\s*:")?.captures(&bare)? { names.insert(found[1].to_string()); }
        }
        at = close + 1;
    }
    Ok(names)
}

pub(super) struct Parameters<'a> {
    files: HashMap<&'a str, Vec<&'a Node>>,
    names: HashMap<&'a str, Vec<&'a Node>>,
    lines: HashMap<&'a str, Vec<&'a str>>,
    sources: &'a BTreeMap<&'a str, &'a str>,
    types: HashMap<&'a str, BTreeMap<String, Option<String>>>,
    values: HashMap<&'a str, HashSet<String>>,
    parents: HashMap<&'a str, Vec<&'a str>>,
    work: Cell<usize>,
}
impl<'a> Parameters<'a> {
    pub(super) fn new(
        nodes: &'a [Node],
        sources: &'a BTreeMap<&'a str, &'a str>,
        supers: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self> {
        let mut files: HashMap<_, Vec<_>> = HashMap::new();
        let mut names: HashMap<_, Vec<_>> = HashMap::new();
        for node in nodes {
            files.entry(node.file_path.as_str()).or_default().push(node);
            names.entry(node.name.as_str()).or_default().push(node);
        }
        let mut lines = HashMap::new();
        let mut count = 0usize;
        for (&path, &source) in sources {
            let mut file = Vec::new();
            for line in source.split('\n') {
                count += 1;
                ensure!(
                    count <= 2_000_000,
                    "type parameter source lines exceed graph budget"
                );
                file.push(line.trim_end_matches('\r'));
            }
            lines.insert(path, file);
        }
        let mut parents: HashMap<_, Vec<_>> = HashMap::new();
        for (child, parent) in supers {
            parents.entry(child).or_default().push(parent);
        }
        Ok(Self {
            files,
            names,
            lines,
            sources,
            types: HashMap::new(),
            values: HashMap::new(),
            parents,
            work: Cell::new(0),
        })
    }
    fn steps(&self, count: usize) -> Result<()> {
        let used = self.work.get().saturating_add(count);
        ensure!(
            used <= 10_000_000,
            "type parameter visibility work exceeds graph budget"
        );
        self.work.set(used);
        Ok(())
    }
    fn enclosing(&self, reference: &UnresolvedReference) -> Vec<&'a Node> {
        self.files
            .get(reference.file_path.as_deref().unwrap_or(""))
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| {
                DECLARATIONS.contains(&node.kind.as_str())
                    && node.start_line <= reference.line
                    && node.end_line >= reference.line
            })
            .collect()
    }
    fn head(&self, node: &Node) -> String {
        self.lines
            .get(node.file_path.as_str())
            .into_iter()
            .flatten()
            .skip(node.start_line.saturating_sub(1) as usize)
            .take((node.end_line.saturating_sub(node.start_line) as usize + 1).min(40))
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn types(&mut self, node: &'a Node) -> Result<&BTreeMap<String, Option<String>>> {
        if !self.types.contains_key(node.id.as_str()) {
            let above = self
                .lines
                .get(node.file_path.as_str())
                .into_iter()
                .flatten()
                .skip(node.start_line.saturating_sub(3) as usize)
                .take(node.start_line.min(3) as usize)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            let head = self.head(node);
            self.steps((head.len() + above.len()) / 64 + 1)?;
            let parameters = declared(node, &head, &above)?;
            self.types.insert(&node.id, parameters);
        }
        Ok(&self.types[node.id.as_str()])
    }
    fn scala_scope(&self, target: &Node, reference: &UnresolvedReference) -> Result<bool> {
        if reference.reference_kind != "references"
            || !regex(r"^[A-Za-z_]\w*$")?.is_match(&reference.reference_name)?
        {
            return Ok(true);
        }
        let file = reference.file_path.as_deref().unwrap_or("");
        let line = self
            .lines
            .get(file)
            .and_then(|lines| lines.get(reference.line.saturating_sub(1) as usize));
        let Some(line) = line else {
            return Ok(true);
        };
        let at = utf16_byte(line, reference.column);
        if !line[at..].starts_with(&reference.reference_name)
            || regex(r"\.\s*$")?.is_match(&line[..at])?
        {
            return Ok(true);
        }
        if target.language != "scala" {
            return Ok(true);
        }
        let Some((qualified, _)) = target.qualified_name.rsplit_once("::") else {
            return Ok(true);
        };
        self.steps(
            self.files
                .get(target.file_path.as_str())
                .map_or(0, Vec::len),
        )?;
        let containers: Vec<_> = self
            .files
            .get(target.file_path.as_str())
            .into_iter()
            .flatten()
            .copied()
            .filter(|node| {
                node.qualified_name == qualified
                    && node.start_line <= target.start_line
                    && node.end_line >= target.end_line
            })
            .collect();
        let inside = |node: &Node| {
            node.file_path == file
                && node.start_line <= reference.line
                && node.end_line >= reference.line
        };
        if let Some(function) = containers
            .iter()
            .find(|node| matches!(node.kind.as_str(), "function" | "method"))
        {
            return Ok(inside(function));
        }
        let Some(owner) = containers
            .iter()
            .find(|node| SCALA_OWNERS.contains(&node.kind.as_str()))
        else {
            return Ok(true);
        };
        if inside(owner) {
            return Ok(true);
        }
        for enclosing in self
            .enclosing(reference)
            .into_iter()
            .filter(|node| SCALA_OWNERS.contains(&node.kind.as_str()))
        {
            let mut frontier = vec![enclosing.name.as_str()];
            let mut visited = HashSet::new();
            while let Some(name) = frontier.pop() {
                if name == owner.name {
                    return Ok(true);
                }
                if !visited.insert(name) {
                    continue;
                }
                frontier.extend(self.parents.get(name).into_iter().flatten().copied());
            }
        }
        if !regex(r"^[A-Za-z_]\w*$")?.is_match(&owner.name)?
            || !regex(r"^[A-Za-z_]\w*$")?.is_match(&target.name)?
        {
            return Ok(true);
        }
        let source = self.sources.get(file).copied().unwrap_or("");
        regex(&format!(
            r"\bimport\s+[\w.]*\b{}\.(?:_|\*|given\b|\{{[^}}]*\b(?:_|\*|{})\b[^}}]*\}}|{}\b)",
            ::regex::escape(&owner.name),
            ::regex::escape(&target.name),
            ::regex::escape(&target.name)
        ))?
        .is_match(source)
        .map_err(Into::into)
    }
    pub(super) fn gate(
        &mut self,
        reference: &UnresolvedReference,
        mut target: &'a Node,
    ) -> Result<Option<&'a Node>> {
        if !applies(reference)? {
            return Ok(Some(target));
        }
        self.steps(
            self.files
                .get(reference.file_path.as_deref().unwrap_or(""))
                .map_or(0, Vec::len),
        )?;
        let language = reference.language.as_deref().unwrap_or("");
        let name = &reference.reference_name;
        let enclosing = self.enclosing(reference);
        if language == "scala" {
            if matches!(reference.reference_kind.as_str(), "calls" | "references")
                && regex(r"^[A-Za-z_]\w*$")?.is_match(name)?
            {
                for node in enclosing
                    .iter()
                    .copied()
                    .filter(|node| matches!(node.kind.as_str(), "function" | "method"))
                {
                    if !self.values.contains_key(node.id.as_str()) {
                        self.values
                            .insert(&node.id, value_parameters(node, &self.head(node))?);
                    }
                    if self.values[node.id.as_str()].contains(name) {
                        return Ok(None);
                    }
                }
            }
            if !self.scala_scope(target, reference)? {
                if !SCALA_TYPES.contains(&target.kind.as_str()) {
                    return Ok(None);
                }
                let mut candidates = Vec::new();
                for &node in self.names.get(name.as_str()).into_iter().flatten() {
                    if node.id != target.id
                        && node.language == "scala"
                        && SCALA_TYPES.contains(&node.kind.as_str())
                        && self.scala_scope(node, reference)?
                    {
                        candidates.push(node);
                    }
                }
                let chosen = if candidates.len() == 1 {
                    Some(candidates[0])
                } else {
                    candidates.into_iter().find(|node| {
                        Some(node.file_path.as_str()) == reference.file_path.as_deref()
                    })
                };
                let Some(chosen) = chosen else {
                    return Ok(None);
                };
                target = chosen;
            }
        }
        if language == "cpp"
            && let Some(qualifier) = regex(r"^([A-Za-z_]\w*)::")?.captures(name)?
        {
            for node in &enclosing {
                if let Some(default) = self.types(node)?.get(&qualifier[1]) {
                    let pattern = regex(r"([A-Za-z_]\w*)\s*(?:<[\s\S]*>)?\s*$")?;
                    let owner = default
                        .as_deref()
                        .map(|default| pattern.captures(default).map_err(anyhow::Error::from))
                        .transpose()?
                        .flatten()
                        .map(|capture| capture[1].to_string());
                    let actual = target
                        .qualified_name
                        .rsplit_once("::")
                        .map(|(owner, _)| owner.rsplit("::").next().unwrap_or(""));
                    return Ok((owner.as_deref().is_some() && actual == owner.as_deref())
                        .then_some(target));
                }
            }
        }
        if LANGUAGES.contains(&language)
            && matches!(
                reference.reference_kind.as_str(),
                "references" | "type_of" | "returns" | "instantiates"
            )
            && regex(r"^[A-Z][A-Za-z0-9_]*$")?.is_match(name)?
        {
            for node in enclosing {
                if self.types(node)?.contains_key(name) {
                    return Ok(None);
                }
            }
        }
        Ok(Some(target))
    }
}
fn utf16_byte(text: &str, column: u32) -> usize {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        if units >= column {
            return at;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}

pub(super) fn applies(reference: &UnresolvedReference) -> Result<bool> {
    let language = reference.language.as_deref().unwrap_or("");
    Ok(language == "scala"
        || language == "cpp" && reference.reference_name.contains("::")
        || LANGUAGES.contains(&language)
            && matches!(
                reference.reference_kind.as_str(),
                "references" | "type_of" | "returns" | "instantiates"
            )
            && regex(r"^[A-Z][A-Za-z0-9_]*$")?.is_match(&reference.reference_name)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn type_parameters_and_scala_scope_match_independent_resolver() -> Result<()> {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/type-parameters/cases.json"
        ))?;
        for case in corpus["cases"].as_array().unwrap() {
            let nodes: Vec<Node> = case["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| {
                    let mut node = node.clone();
                    node["updatedAt"] = json!(0);
                    serde_json::from_value(node)
                })
                .collect::<std::result::Result<_, _>>()?;
            let ids: HashMap<_, _> = nodes.iter().map(|node| (node.id.as_str(), node)).collect();
            let sources: BTreeMap<_, _> = case["sources"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let supers = case["allEdges"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|edge| matches!(edge["kind"].as_str(), Some("extends" | "implements")))
                .filter_map(|edge| {
                    Some((
                        ids.get(edge["source"].as_str()?)?.name.as_str(),
                        ids.get(edge["target"].as_str()?)?.name.as_str(),
                    ))
                });
            let mut parameters = Parameters::new(&nodes, &sources, supers)?;
            for head in case["heads"].as_array().into_iter().flatten() {
                let node = ids[head["node"].as_str().unwrap()];
                let expected: BTreeMap<String, Option<String>> = head["types"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|pair| {
                        (
                            pair[0].as_str().unwrap().to_string(),
                            pair[1].as_str().map(str::to_string),
                        )
                    })
                    .collect();
                assert_eq!(
                    *parameters.types(node)?,
                    expected,
                    "{} type parameters {}",
                    case["origin"],
                    node.qualified_name
                );
                let actual: std::collections::BTreeSet<_> =
                    value_parameters(node, &parameters.head(node))?
                        .into_iter()
                        .collect();
                let expected: std::collections::BTreeSet<_> = head["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap().to_string())
                    .collect();
                assert_eq!(
                    actual, expected,
                    "{} value parameters {}",
                    case["origin"], node.qualified_name
                );
            }
            for answer in case["answers"].as_array().unwrap() {
                let reference: UnresolvedReference =
                    serde_json::from_value(answer["reference"].clone())?;
                let actual = if let Some(id) = answer["input"]["targetNodeId"].as_str() {
                    parameters
                        .gate(&reference, ids[id])?
                        .map(|node| node.id.as_str())
                } else {
                    None
                };
                assert_eq!(
                    actual,
                    answer["expected"]["targetNodeId"].as_str(),
                    "{} {}",
                    case["origin"],
                    answer["reference"]
                );
            }
        }
        Ok(())
    }

    #[test]
    fn generic_parameter_decisions_survive_indexing_and_reopening() -> Result<()> {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/type-parameters/cases.json"
        ))?;
        let mut differences = Vec::new();
        for case in corpus["cases"].as_array().unwrap() {
            let mut selected = BTreeMap::new();
            for answer in case["answers"].as_array().unwrap() {
                if answer["input"].is_null()
                    || answer["input"]["targetNodeId"] == answer["expected"]["targetNodeId"]
                {
                    continue;
                }
                let raw = &answer["reference"];
                selected.insert(
                    (
                        raw["fromNodeId"].as_str().unwrap().to_string(),
                        raw["referenceName"].as_str().unwrap().to_string(),
                        raw["line"].as_u64().unwrap() as u32,
                        raw["column"].as_u64().unwrap() as u32,
                    ),
                    answer["expected"]["targetNodeId"]
                        .as_str()
                        .map(str::to_string),
                );
            }
            if selected.is_empty() {
                continue;
            }
            let temporary = tempfile::tempdir()?;
            let root = temporary.path().join("project");
            let database = temporary.path().join("index.sqlite");
            for (file, source) in case["sources"].as_object().unwrap() {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, source.as_str().unwrap())?;
            }
            let mut engine = crate::Engine::open(&root, Some(&database))?;
            engine.sync()?;
            for reopened in [false, true] {
                if reopened {
                    drop(engine);
                    for file in case["sources"].as_object().unwrap().keys() {
                        std::fs::write(root.join(file), "modified live source")?;
                    }
                    engine = crate::Engine::open(&root, Some(&database))?;
                }
                let graph = crate::graph::Graph::load(&engine.connection, 1)?;
                for ((from, name, line, column), expected) in &selected {
                    let actual: std::collections::BTreeSet<_> = graph
                        .canonical_edges
                        .iter()
                        .flatten()
                        .filter(|edge| {
                            edge.source == *from
                                && edge.line == Some(*line)
                                && edge.column == Some(*column)
                                && edge
                                    .metadata
                                    .as_ref()
                                    .is_some_and(|metadata| metadata["refName"] == *name)
                        })
                        .map(|edge| edge.target.as_str())
                        .collect();
                    let wanted: std::collections::BTreeSet<_> =
                        expected.as_deref().into_iter().collect();
                    if actual != wanted {
                        differences.push(json!({"case":case["origin"], "reopened":reopened, "from":from,"name":name,"line":line,"column":column,"actual":actual,"expected":wanted}));
                    }
                }
            }
        }
        let path = std::env::temp_dir().join("prometeu-generic-parameter-differences.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&differences)?)?;
        ensure!(
            differences.is_empty(),
            "{} generic parameter decisions differ; records in {}",
            differences.len(),
            path.display()
        );
        Ok(())
    }
}
