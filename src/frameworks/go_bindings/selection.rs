use super::*;
#[derive(Debug)]
pub(crate) struct Answer<'a> {
    pub node: Option<&'a Node>,
    pub confidence: f64,
    pub resolved_by: &'static str,
}
fn depth(a: &str, b: &str) -> usize {
    let a: Vec<_> = a.split('/').collect();
    let b: Vec<_> = b.split('/').collect();
    a[..a.len() - 1]
        .iter()
        .zip(&b[..b.len() - 1])
        .take_while(|(a, b)| a == b)
        .count()
}
fn closest_copy<'a>(r: &UnresolvedReference, nodes: &[&'a Node]) -> Option<&'a Node> {
    let first = *nodes.first()?;
    if nodes.len() < 2
        || nodes.iter().any(|n| {
            n.qualified_name != first.qualified_name
                || n.kind != first.kind
                || n.file_path.rsplit('/').next() != first.file_path.rsplit('/').next()
        })
    {
        return None;
    }
    let file = r.file_path.as_deref().unwrap_or("");
    let deepest = nodes.iter().map(|n| depth(file, &n.file_path)).max()?;
    let mut nearest = nodes
        .iter()
        .filter(|n| depth(file, &n.file_path) == deepest);
    let one = nearest.next().copied();
    if nearest.next().is_none() { one } else { None }
}
impl<'a> Bindings<'a> {
    fn lexical(&self, node: &Node, r: &UnresolvedReference) -> Result<bool> {
        if matches!(node.language.as_str(), "c" | "cpp")
            || !matches!(
                node.kind.as_str(),
                "function"
                    | "method"
                    | "class"
                    | "struct"
                    | "interface"
                    | "enum"
                    | "trait"
                    | "type_alias"
            )
        {
            return Ok(true);
        }
        let parent = node.qualified_name.rsplit_once("::").map(|(p, _)| p);
        let mut qn = node.qualified_name.as_str();
        while let Some((outer, _)) = qn.rsplit_once("::") {
            qn = outer;
            self.step(self.qualified.get(qn).map_or(0, Vec::len))?;
            if let Some(scope) = self.qualified.get(qn).into_iter().flatten().find(|p| {
                p.file_path == node.file_path
                    && matches!(p.kind.as_str(), "function" | "method")
                    && p.start_line <= node.start_line
                    && p.end_line >= node.end_line
            }) {
                if node.kind == "method" && Some(qn) == parent {
                    break;
                }
                return Ok(r.file_path.as_deref() == Some(node.file_path.as_str())
                    && r.line >= scope.start_line
                    && r.line <= scope.end_line);
            }
        }
        Ok(true)
    }
    fn go_visible(&self, node: &Node, r: &UnresolvedReference) -> bool {
        let file = r.file_path.as_deref().unwrap_or("");
        if dir(&node.file_path) != dir(file) {
            return node
                .name
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_uppercase);
        }
        if node.file_path == file
            || matches!(
                node.kind.as_str(),
                "method" | "property" | "field" | "enum_member"
            )
            || r.reference_name.contains('.')
        {
            return true;
        }
        let own = self.packages.get(file).and_then(Option::as_deref);
        let theirs = self
            .packages
            .get(node.file_path.as_str())
            .and_then(Option::as_deref);
        own.is_none()
            || theirs.is_none()
            || own == theirs
            || (node
                .name
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_uppercase)
                && self
                    .dot_packages
                    .get(file)
                    .is_some_and(|dirs| dirs.iter().any(|p| p == dir(&node.file_path))))
    }
    pub(super) fn locally_bound(&self, r: &UnresolvedReference) -> Result<bool> {
        Ok(self.local_declaration(r)?.is_some())
    }
    pub(super) fn local_declaration(
        &self,
        r: &UnresolvedReference,
    ) -> Result<Option<crate::synthesis::GoDeclaration>> {
        let file = r.file_path.as_deref().unwrap_or("");
        let Some((&path, &source)) = self.sources.get_key_value(file) else {
            return Ok(None);
        };
        let mut scopes = self.scopes.borrow_mut();
        if !scopes.contains_key(path) {
            self.step(source.len() / 32 + 1)?;
            if scopes.len() >= 64 {
                scopes.clear()
            }
            scopes.insert(path, GoScopes::new(source)?);
        }
        let Some(line) = self.line(file, r.line.saturating_sub(1) as usize) else {
            return Ok(None);
        };
        let mut units = 0;
        let mut column = line.len();
        for (at, ch) in line.char_indices() {
            if units >= r.column {
                column = at;
                break;
            }
            units += ch.len_utf16() as u32;
        }
        let at = line.as_ptr() as usize - source.as_ptr() as usize + column;
        scopes[path].lookup(&r.reference_name, at)
    }
    fn exact(&self, r: &UnresolvedReference) -> Result<Option<(&'a Node, f64)>> {
        let file = r.file_path.as_deref().unwrap_or("");
        let line = self
            .line(file, r.line.saturating_sub(1) as usize)
            .unwrap_or("");
        // For a conversion `(*T)(x)`, no receiver-less call starts at its column.
        let receiverless = r.reference_kind == "calls"
            && String::from_utf16_lossy(
                &line
                    .encode_utf16()
                    .skip(r.column as usize)
                    .collect::<Vec<_>>(),
            )
            .starts_with(&r.reference_name);
        let mut candidates = Vec::new();
        for &node in self
            .by_name
            .get(r.reference_name.as_str())
            .into_iter()
            .flatten()
        {
            self.step(1)?;
            if node.kind == "import"
                || (matches!(r.reference_kind.as_str(), "references" | "function_ref")
                    && node.language != "go")
                || (receiverless
                    && matches!(
                        node.kind.as_str(),
                        "method" | "field" | "property" | "enum_member"
                    ))
                || !self.lexical(node, r)?
                || (node.language == "go" && !self.go_visible(node, r))
            {
                continue;
            }
            candidates.push(node)
        }
        if candidates.len() == 1 {
            return Ok(Some((
                candidates[0],
                if candidates[0].language == "go" {
                    0.9
                } else {
                    0.5
                },
            )));
        }
        if candidates.is_empty() || candidates.len() > 500 {
            return Ok(None);
        }
        let native = candidates.iter().any(|n| n.language == "go");
        let weighed: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|n| !native || n.language == "go")
            .collect();
        let mut best = None;
        let mut score = -1.;
        let mut tied = Vec::new();
        for node in &weighed {
            let mut value = (depth(file, &node.file_path) * 15).min(80) as f64
                + if node.language == "go" { 51. } else { -80. };
            if node.file_path == file {
                value += 100.;
                if node.start_line > 0 {
                    value += (20. - node.start_line.abs_diff(r.line) as f64 / 10.).max(0.)
                }
            }
            if (r.reference_kind == "calls" && matches!(node.kind.as_str(), "function" | "method"))
                || (r.reference_kind == "instantiates"
                    && matches!(
                        node.kind.as_str(),
                        "class" | "struct" | "union" | "interface"
                    ))
            {
                value += 25.
            }
            if node.is_exported == Some(true) {
                value += 10.
            }
            if value > score {
                score = value;
                best = Some(*node);
                tied = vec![*node]
            } else if value == score {
                tied.push(*node)
            }
        }
        let Some(best) = closest_copy(r, &tied).or(best) else {
            return Ok(None);
        };
        let own: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|n| n.file_path == file)
            .collect();
        let decisive = if best.file_path != file || best.id == r.from_node_id || own.len() != 1 {
            false
        } else if let Some((owner, _)) = best.qualified_name.rsplit_once("::") {
            let scope_function = best.kind != "method"
                && self
                    .by_file
                    .get(file)
                    .into_iter()
                    .flatten()
                    .filter(|n| n.qualified_name == owner)
                    .all(|n| matches!(n.kind.as_str(), "function" | "method"));
            scope_function
                || self.by_id.get(r.from_node_id.as_str()).is_some_and(|from| {
                    from.qualified_name == owner
                        || from.qualified_name.starts_with(&format!("{owner}::"))
                })
        } else {
            best.kind != "method"
        };
        let confidence = if decisive {
            0.9
        } else if best.id != r.from_node_id
            && closest_copy(r, &weighed).is_some_and(|node| node.id == best.id)
        {
            0.8
        } else if depth(file, &best.file_path) * 15 >= 30 {
            0.7
        } else {
            0.4
        };
        Ok(Some((best, confidence)))
    }
    /// None leaves another strategy in control; an answer without a node is a final veto.
    pub(crate) fn select(
        &self,
        r: &UnresolvedReference,
        framework_enabled: bool,
    ) -> Result<Option<super::Answer<'a>>> {
        if r.reference_kind == "function_ref" {
            return if r.reference_name.contains('.') {
                self.chained_method(r)
            } else {
                Ok(None)
            };
        }
        if self.indexed_no_project_method(r)? {
            return Ok(Some(Answer {
                node: None,
                confidence: 0.,
                resolved_by: "instance-method",
            }));
        }
        if let Some(answer) = self.indexed_project_method(r)? {
            return Ok(Some(answer));
        }
        if let Some(answer) = self.chained_method(r)? {
            return Ok(Some(answer));
        }
        if let Some(answer) = self.qualified(r)? {
            return Ok(Some(answer));
        }
        let framework = if framework_enabled {
            self.resolve(r)?
        } else {
            None
        };
        if framework.is_none() {
            let line = self
                .line(
                    r.file_path.as_deref().unwrap_or(""),
                    r.line.saturating_sub(1) as usize,
                )
                .unwrap_or("");
            if !spelling(line, r)?
                || crate::synthesis::GO_PREDECLARED.contains(&r.reference_name.as_str())
            {
                return Ok(None);
            }
        }
        if (r.reference_kind == "calls" && self.locally_bound(r)?) || self.parameter_bound(r)? {
            return Ok(Some(Answer {
                node: None,
                confidence: 0.,
                resolved_by: "framework",
            }));
        }
        let mut answer = framework.map(|(node, confidence)| Answer {
            node: Some(node),
            confidence,
            resolved_by: "framework",
        });
        if let Some((node, score)) = self.exact(r)?
            && answer
                .as_ref()
                .is_none_or(|answer| score > answer.confidence)
        {
            answer = Some(Answer {
                node: Some(node),
                confidence: score,
                resolved_by: "exact-match",
            });
        }
        let Some(mut answer) = answer else {
            return Ok(None);
        };
        let node = answer.node.unwrap();
        if node.language != "go" {
            answer.node = None;
            return Ok(Some(answer));
        }
        if node.file_path != r.file_path.as_deref().unwrap_or("")
            && crate::classify::is_test_suite_path(&node.file_path)?
            && !crate::classify::is_test_path(r.file_path.as_deref().unwrap_or(""))
        {
            answer.node = None;
            return Ok(Some(answer));
        }
        if (r.reference_kind == "instantiates"
            || (r.reference_kind == "references"
                && self
                    .by_id
                    .get(r.from_node_id.as_str())
                    .is_none_or(|n| n.kind != "route")))
            && let Some((framework, _)) = framework
            && matches!(
                framework.kind.as_str(),
                "struct" | "interface" | "type_alias"
            )
            && (!matches!(node.kind.as_str(), "struct" | "interface" | "type_alias")
                || dir(&node.file_path) != dir(r.file_path.as_deref().unwrap_or("")))
        {
            answer.node = Some(framework)
        }
        Ok(Some(answer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn proven_go_convention_winners_match_full_reference_pipeline() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/frameworks/go-bindings.json"
        ))
        .unwrap();
        let mut differences = Vec::new();
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
            for (index, r) in references.iter().enumerate() {
                if r.reference_kind == "function_ref" {
                    continue;
                }
                let answer = bindings.select(r, true).unwrap();
                if !case["expected"][index].is_null()
                    || case["selected"][index]["resolvedBy"] == "import"
                {
                    assert!(
                        answer.is_some(),
                        "a convention candidate must make an explicit decision"
                    );
                }
                let Some(answer) = answer else { continue };
                let actual=answer.node.map(|node|json!({"targetNodeId":node.id,"confidence":answer.confidence,"resolvedBy":answer.resolved_by})).unwrap_or(Value::Null);
                if actual != case["selected"][index] {
                    differences.push(json!({"origin":case["origin"],"reference":r,"actual":actual,"expected":case["selected"][index]}));
                }
            }
        }
        if !differences.is_empty() {
            std::fs::write(
                "/tmp/prometeu-go-selection-differences.json",
                serde_json::to_string_pretty(&differences).unwrap(),
            )
            .unwrap();
        }
        assert!(
            differences.is_empty(),
            "{} Go selected answers differ; see /tmp/prometeu-go-selection-differences.json",
            differences.len()
        );
    }
}
