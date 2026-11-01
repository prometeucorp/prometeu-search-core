//! Go convention candidates are read only from a bare name's own package.
use super::source_pattern as regex;
use crate::synthesis::{GoPackageResolver, GoScopes};
mod chains;
mod indexed;
mod parameters;
mod qualified;
mod selection;
use anyhow::{Result, ensure};
use prometeu_extract::{Node, UnresolvedReference};
pub(crate) use selection::Answer;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
};
fn dir(path: &str) -> &str {
    path.rfind('/').map_or("", |at| &path[..at])
}
fn word(ch: u16) -> bool {
    ch <= 127 && (ch as u8).is_ascii_alphanumeric() || ch == b'_' as u16
}
fn whitespace(ch: u16) -> bool {
    char::from_u32(ch as u32).is_some_and(crate::search::query_whitespace)
}
fn spelling(line: &str, r: &UnresolvedReference) -> Result<bool> {
    let name = &r.reference_name;
    if r.language.as_deref() != Some("go") || !regex(r"^[A-Za-z_]\w*$")?.is_match(name)? {
        return Ok(false);
    }
    let units: Vec<u16> = line.encode_utf16().collect();
    let name_units: Vec<u16> = name.encode_utf16().collect();
    let column = r.column as usize;
    let starts = units
        .get(column..)
        .is_some_and(|s| s.starts_with(&name_units));
    if r.reference_kind == "calls" {
        if starts {
            let mut tail = column + name_units.len();
            while units.get(tail).copied().is_some_and(whitespace) {
                tail += 1
            }
            if units.get(tail..tail + 2) == Some(&[b'?' as u16, b'.' as u16]) {
                tail += 2;
                while units.get(tail).copied().is_some_and(whitespace) {
                    tail += 1
                }
            }
            if units.get(tail).is_some_and(|ch| matches!(*ch, 40 | 60)) {
                let mut end = column.min(units.len());
                while end > 0 && whitespace(units[end - 1]) {
                    end -= 1
                }
                if end == 0
                    || !(word(units[end - 1]) || matches!(units[end - 1], 46 | 36 | 93 | 41))
                {
                    return Ok(true);
                }
                let mut start = end;
                while start > 0 && word(units[start - 1]) {
                    start -= 1
                }
                let before = String::from_utf16_lossy(&units[start..end]);
                if [
                    "return",
                    "await",
                    "yield",
                    "typeof",
                    "void",
                    "new",
                    "else",
                    "case",
                    "throw",
                    "in",
                    "of",
                    "instanceof",
                    "go",
                    "defer",
                    "if",
                    "switch",
                    "for",
                    "range",
                    "do",
                ]
                .contains(&before.as_str())
                {
                    return Ok(true);
                }
            }
        }
        return Ok(units.get(column) == Some(&(b'(' as u16))
            && regex(&format!(r"^\(\s*\*?\s*{}\s*\)\s*\(", regex::escape(name)))?.is_match(
                &String::from_utf16_lossy(units.get(column..).unwrap_or_default()),
            )?);
    }
    if starts {
        let mut end = column;
        while end > 0 && whitespace(units[end - 1]) {
            end -= 1
        }
        return Ok(end == 0
            || units[end - 1] != b'.' as u16
            || (end >= 3 && units[end - 3..end] == [46, 46, 46]));
    }
    let code = regex(r#""(?:[^"\\]|\\.)*"|`[^`]*`"#)?.replace_all(line, " ");
    Ok(regex(&format!(r"(?<![\w.]){}\b", regex::escape(name)))?.is_match(&code)?)
}
pub(crate) struct Bindings<'a> {
    sources: &'a BTreeMap<&'a str, &'a str>,
    by_name: HashMap<&'a str, Vec<&'a Node>>,
    by_file: HashMap<&'a str, Vec<&'a Node>>,
    qualified: HashMap<&'a str, Vec<&'a Node>>,
    checkpoints: HashMap<&'a str, Vec<usize>>,
    packages: HashMap<&'a str, Option<String>>,
    imports: HashMap<&'a str, Vec<(crate::synthesis::ImportBinding, Option<String>)>>,
    dot_packages: HashMap<&'a str, Vec<String>>,
    work: Cell<usize>,
    scopes: RefCell<HashMap<&'a str, GoScopes>>,
    chain_sources: RefCell<HashMap<&'a str, std::rc::Rc<String>>>,
    parameters: RefCell<HashMap<&'a str, parameters::Parameters>>,
    by_id: HashMap<&'a str, &'a Node>,
}
impl<'a> Bindings<'a> {
    pub(crate) fn new(
        nodes: &'a [Node],
        sources: &'a BTreeMap<&'a str, &'a str>,
        package: &GoPackageResolver<'_>,
    ) -> Result<Self> {
        let mut out = Self {
            sources,
            by_name: HashMap::new(),
            by_file: HashMap::new(),
            qualified: HashMap::new(),
            checkpoints: HashMap::new(),
            packages: HashMap::new(),
            imports: HashMap::new(),
            dot_packages: HashMap::new(),
            work: Cell::new(0),
            scopes: RefCell::new(HashMap::new()),
            chain_sources: RefCell::new(HashMap::new()),
            parameters: RefCell::new(HashMap::new()),
            by_id: HashMap::new(),
        };
        for node in nodes {
            out.by_id.insert(&node.id, node);
            out.by_name.entry(&node.name).or_default().push(node);
            out.by_file.entry(&node.file_path).or_default().push(node);
            out.qualified
                .entry(&node.qualified_name)
                .or_default()
                .push(node);
        }
        let package_pattern = regex(r"^\s*package\s+([A-Za-z_]\w*)")?;
        let comment_open = regex(r"^\s*/\*")?;
        let declaration = regex(r"^(?:func|type|var|const)\b")?;
        let dot = regex(r#"^\s*(?:import\s+)?\.\s+"([^"]+)""#)?;
        for (&file, &source) in sources {
            let mut points = vec![0];
            let mut count = 0;
            for (at, ch) in source.bytes().enumerate() {
                if ch == b'\n' {
                    count += 1;
                    if count % 64 == 0 {
                        points.push(at + 1)
                    }
                }
            }
            out.checkpoints.insert(file, points);
            if !file.ends_with(".go") {
                continue;
            }
            out.imports.insert(
                file,
                crate::synthesis::go_imports(file, source)?
                    .into_iter()
                    .map(|binding| {
                        let directory = package(file, &binding.source)
                            .map(|dir| if dir == "." { String::new() } else { dir });
                        (binding, directory)
                    })
                    .collect(),
            );
            let mut clause = None;
            let mut comment = false;
            for line in source.split('\n').take(500) {
                if comment {
                    if line.contains("*/") {
                        comment = false
                    }
                    continue;
                }
                if comment_open.is_match(line)? && !line.contains("*/") {
                    comment = true;
                    continue;
                }
                if let Some(m) = package_pattern.captures(line)? {
                    clause = Some(m[1].to_owned());
                    break;
                }
            }
            out.packages.insert(file, clause);
            let mut dirs = Vec::new();
            for line in source.split('\n') {
                if declaration.is_match(line)? {
                    break;
                }
                if let Some(m) = dot.captures(line)?
                    && let Some(dir) = package(file, &m[1])
                {
                    dirs.push(if dir == "." { String::new() } else { dir })
                }
            }
            out.dot_packages.insert(file, dirs);
        }
        Ok(out)
    }
    fn step(&self, n: usize) -> Result<()> {
        let n = self.work.get().saturating_add(n);
        ensure!(n <= 10_000_000, "Go convention work limit exceeded");
        self.work.set(n);
        Ok(())
    }
    fn line(&self, file: &str, line: usize) -> Option<&'a str> {
        let source = self.sources.get(file)?;
        let start = self.checkpoints.get(file)?.get(line / 64)?;
        source[*start..]
            .split('\n')
            .nth(line % 64)
            .map(|line| line.trim_end_matches('\r'))
    }
    fn reachable(&self, node: &Node, r: &UnresolvedReference) -> Result<bool> {
        let file = r.file_path.as_deref().unwrap_or("");
        let mut qn = node.qualified_name.as_str();
        if !matches!(node.language.as_str(), "c" | "cpp") {
            while let Some((parent, _)) = qn.rsplit_once("::") {
                qn = parent;
                self.step(self.qualified.get(parent).map_or(0, Vec::len))?;
                if let Some(scope) = self.qualified.get(parent).into_iter().flatten().find(|p| {
                    p.file_path == node.file_path
                        && matches!(p.kind.as_str(), "function" | "method")
                        && p.start_line <= node.start_line
                        && p.end_line >= node.end_line
                }) {
                    if file != node.file_path
                        || r.line < scope.start_line
                        || r.line > scope.end_line
                    {
                        return Ok(false);
                    }
                    break;
                }
            }
        }
        if node.file_path == file {
            return Ok(true);
        }
        if let Some((parent, _)) = node.qualified_name.rsplit_once("::") {
            self.step(
                self.by_file
                    .get(node.file_path.as_str())
                    .map_or(0, Vec::len),
            )?;
            if self
                .by_file
                .get(node.file_path.as_str())
                .into_iter()
                .flatten()
                .any(|p| {
                    p.qualified_name == parent
                        && matches!(
                            p.kind.as_str(),
                            "class"
                                | "struct"
                                | "interface"
                                | "enum"
                                | "trait"
                                | "protocol"
                                | "record"
                        )
                })
            {
                return Ok(false);
            }
        }
        if crate::classify::is_test_suite_path(&node.file_path)?
            && !crate::classify::is_test_path(file)
        {
            return Ok(false);
        }
        if node.language == "go" {
            let own = self.packages.get(file).and_then(Option::as_deref);
            let theirs = self
                .packages
                .get(node.file_path.as_str())
                .and_then(Option::as_deref);
            if own.is_some() && theirs.is_some() && own != theirs {
                return Ok(node
                    .name
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_uppercase)
                    && self
                        .dot_packages
                        .get(file)
                        .is_some_and(|dirs| dirs.iter().any(|pkg| pkg == dir(&node.file_path))));
            }
        } else if matches!(
            node.language.as_str(),
            "swift" | "kotlin" | "java" | "csharp" | "scala" | "php"
        ) && node.visibility.as_deref() == Some("private")
        {
            return Ok(false);
        }
        Ok(true)
    }
    pub(crate) fn resolve(&self, r: &UnresolvedReference) -> Result<Option<(&'a Node, f64)>> {
        if matches!(r.reference_kind.as_str(), "extends" | "implements") {
            return Ok(None);
        }
        let file = r.file_path.as_deref().unwrap_or("");
        let Some(line) = self.line(file, r.line.saturating_sub(1) as usize) else {
            return Ok(None);
        };
        self.step(line.len() / 32 + 1)?;
        if !spelling(line, r)? {
            return Ok(None);
        }
        let name = &r.reference_name;
        let patterns = [
            (
                name.ends_with("Handler") || name.starts_with("Handle"),
                &["function"][..],
                0.8,
            ),
            (
                name.ends_with("Service")
                    || name.ends_with("Repository")
                    || name.ends_with("Store"),
                &["struct", "interface"][..],
                0.8,
            ),
            (
                name.ends_with("Middleware") || name.starts_with("Auth") || name.starts_with("Log"),
                &["function"][..],
                0.75,
            ),
            (
                regex(r"^[A-Z][a-zA-Z]+$")?.is_match(name)?,
                &["struct", "interface", "type_alias"][..],
                0.7,
            ),
        ];
        for (matches, kinds, confidence) in patterns {
            if !matches {
                continue;
            }
            let mut first = None;
            for &node in self.by_name.get(name.as_str()).into_iter().flatten() {
                self.step(1)?;
                if !kinds.contains(&node.kind.as_str())
                    || dir(&node.file_path) != dir(file)
                    || !self.reachable(node, r)?
                {
                    continue;
                }
                if node.file_path == file {
                    return Ok(Some((node, confidence)));
                }
                first = first.or(Some(node));
            }
            if let Some(node) = first {
                return Ok(Some((node, confidence)));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn go_spelling_and_conventions_match_pinned_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/frameworks/go-bindings.json"
        ))
        .unwrap();
        let mut differences = Vec::new();
        for case in corpus["spelling"].as_array().unwrap() {
            let r = serde_json::from_value(case["reference"].clone()).unwrap();
            let actual = spelling(case["source"].as_str().unwrap(), &r).unwrap();
            if actual != case["expected"].as_bool().unwrap() {
                differences.push(json!({"spelling":case,"actual":actual}));
            }
        }
        for case in corpus["cases"].as_array().unwrap() {
            let mut raw = case["nodes"].clone();
            for node in raw.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes: Vec<Node> = serde_json::from_value(raw).unwrap();
            let sources: BTreeMap<&str, &str> = case["sources"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let package = |file: &str, source: &str| {
                case["packageAnswers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["file"] == file && a["source"] == source)
                    .and_then(|a| a["answer"].as_str())
                    .map(str::to_owned)
            };
            let bindings = Bindings::new(&nodes, &sources, &package).unwrap();
            let references: Vec<UnresolvedReference> =
                serde_json::from_value(case["references"].clone()).unwrap();
            for (r, expected) in references.iter().zip(case["expected"].as_array().unwrap()) {
                let actual=bindings.resolve(r).unwrap().map(|(node,confidence)|json!({"targetNodeId":node.id,"confidence":confidence})).unwrap_or(Value::Null);
                if actual != *expected {
                    differences.push(json!({"origin":case["origin"],"reference":r,"actual":actual,"expected":expected}));
                }
            }
        }
        if !differences.is_empty() {
            std::fs::write(
                "/tmp/prometeu-go-bindings-differences.json",
                serde_json::to_string_pretty(&differences).unwrap(),
            )
            .unwrap();
        }
        assert!(
            differences.is_empty(),
            "{} Go convention answers differ; see /tmp/prometeu-go-bindings-differences.json",
            differences.len()
        );
    }
}
