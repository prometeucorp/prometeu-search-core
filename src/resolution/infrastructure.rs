//! Directory-scoped infrastructure references and transaction ownership.
use super::super::Graph;
use crate::parser::Reference;
use anyhow::{Result, ensure};
use std::collections::{HashMap, HashSet};

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or(
        ".",
        |(parent, _)| if parent.is_empty() { "." } else { parent },
    )
}
fn parent(path: &str) -> Option<&str> {
    (!matches!(path, "" | ".")).then(|| directory(path))
}
fn normalize(base: &str, relative: &str) -> String {
    let combined = format!("{base}/{relative}");
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|part| *part != "..") {
                    parts.pop();
                } else {
                    parts.push(part);
                }
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

impl Graph {
    fn infrastructure_step(&self, amount: usize) -> bool {
        let work = self.infrastructure_work.get().saturating_add(amount);
        self.infrastructure_work.set(work);
        if work > 10_000_000 {
            self.resolution_truncated.set(true);
            false
        } else {
            true
        }
    }

    pub(in crate::graph) fn initialize_cics(&mut self) -> Result<()> {
        let mut modules = HashMap::new();
        let mut ordered = (0..self.nodes.len())
            .filter(|&index| self.metadata(index).is_some())
            .collect::<Vec<_>>();
        ordered.sort_by(|&a, &b| {
            let a = self.metadata(a).unwrap();
            let b = self.metadata(b).unwrap();
            (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
        });
        for &index in &ordered {
            let node = self.metadata(index).unwrap();
            if node.language == "cobol" && node.kind == "module" {
                modules.entry(node.file_path.clone()).or_insert(index);
            }
        }
        let value = regex::Regex::new(r#"(?i)\bVALUE\s+['"]([A-Za-z0-9$#@]{1,4})['"]"#)?;
        let mut transactions = HashMap::new();
        for kind in ["variable", "field", "constant"] {
            for &index in &ordered {
                let node = self.metadata(index).unwrap();
                if node.language != "cobol"
                    || node.kind != kind
                    || !node.name.to_ascii_uppercase().contains("TRAN")
                {
                    continue;
                }
                if let Some(value) = node
                    .signature
                    .as_deref()
                    .and_then(|signature| value.captures(signature))
                    && let Some(&module) = modules.get(&node.file_path)
                {
                    transactions
                        .entry(value[1].to_ascii_uppercase())
                        .or_insert(module);
                }
            }
        }
        self.cics_transactions = transactions;
        Ok(())
    }
    fn infrastructure_nodes(&self, name: &str) -> Vec<usize> {
        if !self.infrastructure_step(self.names.get(name).map_or(1, Vec::len)) {
            return Vec::new();
        }
        let mut nodes = self
            .names
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&index| {
                self.metadata(index)
                    .is_some_and(|node| node.qualified_name == name)
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|&a, &b| {
            let a = self.metadata(a).unwrap();
            let b = self.metadata(b).unwrap();
            (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
        });
        nodes
    }
    fn ancestor_node(&self, candidates: &[usize], from: &str) -> Option<usize> {
        let mut current = parent(from);
        while let Some(dir) = current {
            if let Some(&node) = candidates
                .iter()
                .find(|&&index| directory(&self.nodes[index].path) == dir)
            {
                return Some(node);
            }
            current = parent(dir);
        }
        None
    }
    fn span_match(&self, index: usize, pattern: &str) -> Result<Option<String>> {
        let node = self.metadata(index).unwrap();
        ensure!(
            self.infrastructure_step(self.files[&node.file_path].source.len()),
            "infrastructure source scan limit exceeded"
        );
        let expression = crate::frameworks::source_pattern(pattern)?;
        for line in self.files[&node.file_path]
            .source
            .split('\n')
            .skip(node.start_line.saturating_sub(1) as usize)
            .take(node.end_line.saturating_sub(node.start_line) as usize + 1)
        {
            if let Some(found) = expression.captures(line)? {
                return Ok(Some(found[1].into()));
            }
        }
        Ok(None)
    }
    fn module_attribute(&self, index: usize, name: &str) -> Result<Option<String>> {
        self.span_match(
            index,
            &format!(r#"^\s*{}\s*=\s*"([^"]+)""#, regex::escape(name)),
        )
    }
    pub(super) fn terraform_reference(
        &self,
        path: &str,
        name: &str,
    ) -> Result<Option<(usize, f64)>> {
        let dir = directory(path);
        if let Some(found) =
            crate::frameworks::source_pattern(r"^module\.([^.:\s]+):(.+)$")?.captures(name)?
        {
            let module = &found[1];
            let child = &found[2];
            let declarations = self
                .infrastructure_nodes(&format!("module.{module}"))
                .into_iter()
                .filter(|&index| self.nodes[index].symbol.kind == "module")
                .collect::<Vec<_>>();
            let Some(declaration) = declarations
                .iter()
                .copied()
                .find(|&index| directory(&self.nodes[index].path) == dir)
                .or_else(|| (declarations.len() == 1).then(|| declarations[0]))
            else {
                return Ok(None);
            };
            let Some(source) = self
                .module_attribute(declaration, "source")?
                .filter(|source| !source.is_empty())
            else {
                return Ok(None);
            };
            if let Some(output) = child.strip_prefix("remote-output.") {
                if !crate::frameworks::source_pattern(r"/remote-state(/|$)")?.is_match(&source)? {
                    return Ok(None);
                }
                let mut component = self.module_attribute(declaration, "component")?;
                if component.as_deref().is_none_or(str::is_empty)
                    && let Some(variable) = self.span_match(
                        declaration,
                        r"^\s*component\s*=\s*var\.([A-Za-z0-9_-]+)\s*$",
                    )?
                {
                    let declarations = self
                        .infrastructure_nodes(&format!("var.{variable}"))
                        .into_iter()
                        .filter(|&index| {
                            directory(&self.nodes[index].path)
                                == directory(&self.nodes[declaration].path)
                        })
                        .collect::<Vec<_>>();
                    if declarations.len() == 1 {
                        component =
                            self.span_match(declarations[0], r#"^\s*default\s*=\s*"([^"]+)""#)?;
                    }
                }
                let Some(component) = component.filter(|component| !component.is_empty()) else {
                    return Ok(None);
                };
                let suffix = format!("/{component}");
                let outputs = self
                    .infrastructure_nodes(&format!("output.{output}"))
                    .into_iter()
                    .filter(|&index| {
                        let dir = directory(&self.nodes[index].path);
                        dir == component || dir.ends_with(&suffix)
                    })
                    .collect::<Vec<_>>();
                let dirs = outputs
                    .iter()
                    .map(|&index| directory(&self.nodes[index].path))
                    .collect::<HashSet<_>>();
                return Ok((dirs.len() == 1)
                    .then(|| outputs.first().map(|&index| (index, 0.9)))
                    .flatten());
            }
            if !source.starts_with("./") && !source.starts_with("../") {
                return Ok(None);
            }
            let target = normalize(directory(&self.nodes[declaration].path), &source);
            if child == "file" {
                let mut files = self
                    .files
                    .keys()
                    .filter(|file| {
                        directory(file) == target
                            && (file.ends_with(".tf") || file.ends_with(".tofu"))
                    })
                    .collect::<Vec<_>>();
                files.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                let Some(file) = files
                    .iter()
                    .find(|file| file.ends_with("/main.tf") || file.as_str() == "main.tf")
                    .or_else(|| files.first())
                else {
                    return Ok(None);
                };
                let mut nodes = self.files[*file]
                    .nodes
                    .iter()
                    .copied()
                    .filter(|&index| self.metadata(index).is_some_and(|node| node.kind == "file"))
                    .collect::<Vec<_>>();
                nodes.sort_by(|&a, &b| {
                    let a = self.metadata(a).unwrap();
                    let b = self.metadata(b).unwrap();
                    (a.start_line, &a.id).cmp(&(b.start_line, &b.id))
                });
                return Ok(nodes.first().map(|&index| (index, 0.95)));
            }
            return Ok(self
                .infrastructure_nodes(child)
                .into_iter()
                .find(|&index| directory(&self.nodes[index].path) == target)
                .map(|index| (index, 0.95)));
        }
        let nodes = self.infrastructure_nodes(name);
        if let Some(index) = nodes
            .iter()
            .copied()
            .find(|&index| directory(&self.nodes[index].path) == dir)
        {
            return Ok(Some((index, 0.95)));
        }
        if path.ends_with(".tfvars") && name.starts_with("var.") {
            return Ok(self.ancestor_node(&nodes, dir).map(|index| (index, 0.9)));
        }
        if name.starts_with("provider.") {
            let nodes = nodes
                .into_iter()
                .filter(|&index| self.nodes[index].symbol.kind == "namespace")
                .collect::<Vec<_>>();
            return Ok(self.ancestor_node(&nodes, dir).map(|index| (index, 0.9)));
        }
        Ok(None)
    }
    pub(super) fn infrastructure_reference(
        &self,
        path: &str,
        reference: &Reference,
    ) -> Option<(Vec<usize>, &'static str)> {
        let name = reference
            .qualifier
            .as_ref()
            .map_or_else(
                || reference.target.clone(),
                |qualifier| format!("{qualifier}::{}", reference.target),
            )
            .replace("::", ".");
        match self.files[path].analysis.language.as_str() {
            "terraform" => {
                let target = match self.terraform_reference(path, &name) {
                    Ok(target) => target.map(|(index, _)| index),
                    Err(_) => {
                        self.resolution_truncated.set(true);
                        None
                    }
                };
                Some((target.into_iter().collect(), "framework_match"))
            }
            "cobol" => name.strip_prefix("cics-transid:").map(|transaction| {
                (
                    self.cics_transactions
                        .get(&transaction.to_ascii_uppercase())
                        .copied()
                        .into_iter()
                        .collect(),
                    "framework_match",
                )
            }),
            _ => None,
        }
    }
    pub(super) fn infrastructure_confidence(&self, from: usize, name: &str) -> Option<f64> {
        let path = &self.nodes[from].path;
        match self.files[path].analysis.language.as_str() {
            "cobol" => name.starts_with("cics-transid:").then_some(0.85),
            "terraform" => self
                .terraform_reference(path, name)
                .ok()
                .flatten()
                .map(|(_, confidence)| confidence),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use serde_json::{Value, json};
    #[test]
    fn infrastructure_scope_and_transactions_match_reference_and_reopen() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/infrastructure/cases.json"
        ))
        .unwrap();
        for project in corpus["projects"].as_array().unwrap() {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("project");
            let database = temporary.path().join("index.sqlite");
            for (path, source) in project["files"].as_object().unwrap() {
                let path = root.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let mut engine = Engine::open(&root, Some(&database)).unwrap();
            engine.sync().unwrap();
            for reopened in [false, true] {
                if reopened {
                    drop(engine);
                    for path in project["files"].as_object().unwrap().keys() {
                        std::fs::write(root.join(path), "changed live source").unwrap();
                    }
                    engine = Engine::open(&root, Some(&database)).unwrap();
                }
                let graph = Graph::load(&engine.connection, 1).unwrap();
                for query in project["queries"].as_array().unwrap() {
                    let reference = &query["reference"];
                    let path = reference["filePath"].as_str().unwrap();
                    let name = reference["referenceName"].as_str().unwrap();
                    let input = Reference {
                        column: None,
                        caller: None,
                        target: name.into(),
                        kind: reference["referenceKind"].as_str().unwrap().into(),
                        line: 1,
                        qualifier: None,
                        receiver_type: None,
                        shadowed: false,
                        owner: None,
                    };
                    let targets = graph
                        .infrastructure_reference(path, &input)
                        .map(|(targets, _)| targets)
                        .unwrap_or_default();
                    let actual=targets.first().map(|&target|{let origin=graph.files[path].nodes[0];json!({"id":graph.metadata(target).unwrap().id,"confidence":graph.infrastructure_confidence(origin,name).unwrap()})}).unwrap_or(Value::Null);
                    assert_eq!(
                        actual, query["expected"],
                        "{} {reference} reopen={reopened}",
                        project["name"]
                    );
                }
                let mut actual = graph
                    .canonical_edges
                    .iter()
                    .flatten()
                    .filter(|edge| {
                        edge.metadata.as_ref().is_some_and(|metadata| {
                            metadata["framework"] == "terraform" || metadata["framework"] == "cics"
                        })
                    })
                    .map(|edge| serde_json::to_value(edge).unwrap())
                    .collect::<Vec<_>>();
                let expected: Vec<prometeu_extract::Edge> =
                    serde_json::from_value(project["edges"].clone()).unwrap();
                let mut expected = expected
                    .iter()
                    .map(|edge| serde_json::to_value(edge).unwrap())
                    .collect::<Vec<_>>();
                let key = |edge: &Value| {
                    format!(
                        "{} {} {} {} {}",
                        edge["source"], edge["target"], edge["kind"], edge["line"], edge["column"]
                    )
                };
                actual.sort_by_key(&key);
                expected.sort_by_key(key);
                assert_eq!(
                    actual, expected,
                    "{} persisted edges reopen={reopened}",
                    project["name"]
                );
            }
        }
    }
}
