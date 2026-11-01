use super::*;

fn shared_depth(from: &str, to: &str) -> usize {
    let from = from.rsplit_once('/').map_or("", |(directory, _)| directory);
    let to = to.rsplit_once('/').map_or("", |(directory, _)| directory);
    if from.is_empty() || to.is_empty() {
        return 0;
    }
    from.split('/')
        .zip(to.split('/'))
        .take_while(|(a, b)| a == b)
        .count()
}
fn closest_copy<'a>(reference: &UnresolvedReference, candidates: &[&'a Node]) -> Option<&'a Node> {
    if candidates.len() < 2 {
        return None;
    }
    let first = candidates[0];
    let mut best = None;
    let mut depth = 0;
    let mut unique = false;
    for &node in candidates {
        if node.qualified_name != first.qualified_name
            || node.kind != first.kind
            || node.file_path.rsplit('/').next() != first.file_path.rsplit('/').next()
        {
            return None;
        }
        let distance = shared_depth(
            reference.file_path.as_deref().unwrap_or_default(),
            &node.file_path,
        );
        if best.is_none() || distance > depth {
            best = Some(node);
            depth = distance;
            unique = true;
        } else if distance == depth {
            unique = false;
        }
    }
    if unique { best } else { None }
}
impl<'a> Bindings<'a> {
    fn decisive(&self, node: &Node, reference: &UnresolvedReference, candidates: &[&Node]) -> bool {
        if Some(node.file_path.as_str()) != reference.file_path.as_deref()
            || node.id == reference.from_node_id
            || candidates
                .iter()
                .any(|candidate| candidate.id != node.id && candidate.file_path == node.file_path)
        {
            return false;
        }
        let Some((owner, _)) = node
            .qualified_name
            .rsplit_once("::")
            .filter(|(owner, _)| !owner.is_empty())
        else {
            return node.kind != "method";
        };
        if node.kind != "method"
            && self
                .in_file(&node.file_path)
                .iter()
                .filter(|candidate| candidate.qualified_name == owner)
                .all(|candidate| matches!(candidate.kind.as_str(), "function" | "method"))
        {
            return true;
        }
        self.ids
            .get(reference.from_node_id.as_str())
            .is_some_and(|caller| {
                caller.qualified_name == owner
                    || caller.qualified_name.starts_with(&format!("{owner}::"))
            })
    }
    fn enclosing_member(
        &self,
        reference: &UnresolvedReference,
        candidates: &[&'a Node],
        environment: &Environment<'a>,
    ) -> Result<Option<&'a Node>> {
        if reference.reference_kind != "calls" {
            return Ok(None);
        }
        let is_super = guards::super_receiver(reference, environment)?;
        if !is_super && !guards::self_receiver(reference, environment)? {
            return Ok(None);
        }
        let Some(caller) = self.ids.get(reference.from_node_id.as_str()) else {
            return Ok(None);
        };
        let from = reference.file_path.as_deref().unwrap_or_default();
        let mut scope = if caller.kind == "class" {
            Some(caller.qualified_name.as_str())
        } else {
            caller
                .qualified_name
                .rsplit_once("::")
                .map(|(owner, _)| owner)
        };
        while let Some(owner) = scope {
            let types: Vec<_> = self
                .qualified
                .get(owner)
                .into_iter()
                .flatten()
                .copied()
                .filter(|node| node.language == "python" && node.kind == "class")
                .collect();
            scope = owner.rsplit_once("::").map(|(owner, _)| owner);
            if types.is_empty() {
                continue;
            }
            if !is_super
                && owner != reference.reference_name
                && !owner.ends_with(&format!("::{}", reference.reference_name))
            {
                let mut members: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|node| {
                        node.qualified_name == format!("{owner}::{}", reference.reference_name)
                    })
                    .collect();
                members.sort_by_key(|node| !matches!(node.kind.as_str(), "method" | "function"));
                if let Some(node) = members
                    .iter()
                    .copied()
                    .find(|node| node.file_path == from)
                    .or_else(|| {
                        (types.len() == 1)
                            .then(|| members.first().copied())
                            .flatten()
                    })
                {
                    return Ok(Some(node));
                }
            }
            let declaration = types
                .iter()
                .copied()
                .rfind(|node| {
                    node.file_path == from
                        && node.start_line <= reference.line
                        && node.end_line >= reference.line
                })
                .or_else(|| (types.len() == 1).then_some(types[0]));
            if let Some(declaration) = declaration {
                let mut frontier = vec![declaration];
                let mut seen = std::collections::HashSet::from([declaration.id.as_str()]);
                let mut declared = None;
                for _ in 0..4 {
                    let mut next = Vec::new();
                    for node in frontier {
                        for parent in (environment.supertypes)(&node.id) {
                            if !seen.insert(parent.id.as_str()) {
                                continue;
                            }
                            next.push(parent);
                            if parent.name == reference.reference_name {
                                continue;
                            }
                            let qualified =
                                format!("{}::{}", parent.qualified_name, reference.reference_name);
                            let mut members: Vec<_> = candidates
                                .iter()
                                .copied()
                                .filter(|node| {
                                    node.file_path == parent.file_path
                                        && node.qualified_name == qualified
                                        && !(is_super && node.is_abstract == Some(true))
                                })
                                .collect();
                            members.sort_by_key(|node| {
                                !matches!(node.kind.as_str(), "method" | "function")
                            });
                            if let Some(member) = members.first().copied() {
                                if member.is_abstract != Some(true) {
                                    return Ok(Some(member));
                                }
                                declared.get_or_insert(member);
                            }
                        }
                    }
                    frontier = next;
                    if frontier.is_empty() {
                        break;
                    }
                }
                if declared.is_some() {
                    return Ok(declared);
                }
            }
            return Ok(None);
        }
        Ok(None)
    }
    pub(crate) fn exact_name(
        &self,
        reference: &UnresolvedReference,
        environment: &Environment<'a>,
    ) -> Result<Option<Answer<'a>>> {
        let shape = guards::shape(reference, environment)?;
        let mut candidates = Vec::new();
        for &node in self.named(&reference.reference_name) {
            self.charge(1)?;
            if self.hidden(node, reference)
                || node.kind == "import"
                || matches!(
                    reference.reference_kind.as_str(),
                    "references" | "function_ref"
                ) && node.language != "python"
                || matches!(reference.reference_kind.as_str(), "extends" | "implements")
                    && !matches!(
                        node.kind.as_str(),
                        "class"
                            | "struct"
                            | "interface"
                            | "trait"
                            | "type_alias"
                            | "protocol"
                            | "enum"
                            | "union"
                            | "component"
                            | "module"
                            | "namespace"
                    )
                || reference.reference_kind == "imports"
                    && matches!(
                        node.kind.as_str(),
                        "property" | "field" | "method" | "enum_member" | "parameter"
                    )
                || !self.lexically_reachable(node, reference)?
                || !self.fits_shape(node, reference, shape.as_ref(), environment)?
            {
                continue;
            }
            candidates.push(node);
        }
        if candidates.is_empty() {
            return Ok(None);
        }
        let answer = |node: &'a Node, confidence| Answer {
            node,
            confidence,
            resolved_by: "exact-match",
            framework: None,
        };
        if candidates.len() == 1 {
            let node = candidates[0];
            return Ok(
                if node.language == "python" || scope::visible(node, reference)? {
                    Some(answer(
                        node,
                        if node.language == "python" { 0.9 } else { 0.5 },
                    ))
                } else {
                    None
                },
            );
        }
        if let Some(member) = self.enclosing_member(reference, &candidates, environment)? {
            let members: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|node| {
                    node.qualified_name == member.qualified_name
                        && node.file_path == member.file_path
                })
                .collect();
            if members.len() <= 1 {
                return Ok(Some(answer(member, 0.9)));
            }
            candidates = members;
        }
        if candidates.len() > 500 {
            return Ok(None);
        }
        let from = reference.file_path.as_deref().unwrap_or_default();
        let same_language = candidates.iter().any(|node| node.language == "python");
        let weighed: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|node| !same_language || node.language == "python")
            .collect();
        let mut best = None;
        let mut score = -1.0;
        let mut tied = Vec::new();
        for &node in &weighed {
            let same_file = node.file_path == from;
            let mut value = if same_file { 100.0 } else { 0.0 }
                + (shared_depth(from, &node.file_path) * 15).min(80) as f64
                + if node.language == "python" {
                    51.0
                } else {
                    -80.0
                };
            match reference.reference_kind.as_str() {
                "calls" if matches!(node.kind.as_str(), "function" | "method") => value += 25.0,
                "instantiates"
                    if matches!(
                        node.kind.as_str(),
                        "class" | "struct" | "union" | "interface"
                    ) =>
                {
                    value += 25.0
                }
                "decorates" if matches!(node.kind.as_str(), "function" | "method") => value += 25.0,
                "decorates" if matches!(node.kind.as_str(), "class" | "interface") => value += 15.0,
                _ => {}
            }
            if node.is_exported == Some(true) {
                value += 10.0;
            }
            if same_file && node.start_line > 0 {
                value +=
                    (20.0 - f64::from(node.start_line.abs_diff(reference.line)) / 10.0).max(0.0);
            }
            if value > score {
                score = value;
                best = Some(node);
                tied = vec![node];
            } else if value == score {
                tied.push(node);
            }
        }
        let Some(best) = closest_copy(reference, &tied).or(best) else {
            return Ok(None);
        };
        if best.language != "python" && !scope::visible(best, reference)? {
            return Ok(None);
        }
        let confidence = if self.decisive(best, reference, &candidates) {
            0.9
        } else if best.id != reference.from_node_id
            && closest_copy(reference, &weighed).is_some_and(|copy| copy.id == best.id)
        {
            0.8
        } else if shared_depth(from, &best.file_path) * 15 >= 30 {
            0.7
        } else {
            0.4
        };
        Ok(Some(answer(best, confidence)))
    }
    /// Apply the original strategy order: ties retain the first candidate.
    pub(crate) fn choose(
        &self,
        reference: &UnresolvedReference,
        frameworks: &[String],
        environment: &Environment<'a>,
        imported: Option<Answer<'a>>,
    ) -> Result<Option<Answer<'a>>> {
        let prior = self.mockless.replace(true);
        let result = self.choose_inner(reference, frameworks, environment, imported);
        self.mockless.set(prior);
        result
    }
    fn choose_inner(
        &self,
        reference: &UnresolvedReference,
        frameworks: &[String],
        environment: &Environment<'a>,
        imported: Option<Answer<'a>>,
    ) -> Result<Option<Answer<'a>>> {
        if reference.reference_kind == "function_ref" {
            return Ok(None);
        }
        let mut best = None;
        for framework in ["django", "flask", "fastapi"] {
            if !frameworks.iter().any(|active| active == framework) {
                continue;
            }
            if let Some((node, confidence)) = self.resolve(reference, framework)?
                && node.language == "python"
            {
                let candidate = Answer {
                    node,
                    confidence,
                    resolved_by: "framework",
                    framework: Some(framework),
                };
                if best.is_none_or(|best: Answer<'_>| confidence > best.confidence) {
                    best = Some(candidate);
                }
            }
        }
        if let Some(imported) = imported
            && !guards::imported_self_call(reference, environment)?
        {
            if imported.confidence >= 0.9 {
                return Ok(Some(imported));
            }
            if best.is_none_or(|best| imported.confidence > best.confidence) {
                best = Some(imported);
            }
        }
        let from = reference.file_path.as_deref().unwrap_or_default();
        let outside_import = reference.reference_kind == "imports"
            && self
                .imported_from(from, &reference.reference_name, environment)?
                .is_some_and(|module| {
                    !module.starts_with('.') && (environment.module)(&module, from).is_none()
                });
        if !outside_import
            && let Some(named) = self.exact_name(reference, environment)?
            && scope::visible(named.node, reference)?
            && best.is_none_or(|best| named.confidence > best.confidence)
        {
            best = Some(named);
        }
        Ok(best)
    }
}
