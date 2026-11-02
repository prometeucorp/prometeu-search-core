use super::*;

pub(super) fn visible(candidate: &Node, reference: &UnresolvedReference) -> Result<bool> {
    let from = reference.file_path.as_deref().unwrap_or_default();
    if candidate.file_path == from {
        return Ok(true);
    }
    if is_test_suite(&candidate.file_path)? && !crate::classify::is_test_path(from) {
        return Ok(false);
    }
    Ok(candidate.language != "markdown")
}
fn is_test_suite(path: &str) -> Result<bool> {
    if !crate::classify::is_test_path(path) {
        return Ok(false);
    }
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or_default();
    let original = path.rsplit('/').next().unwrap_or_default();
    Ok(name.starts_with("test_")
        || name == "conftest.py"
        || regex(
            r"[._-](?:test|tests|unittest|unittests)\.[a-z0-9]+$|[._](?:spec|specs)\.[a-z0-9]+$",
        )?
        .is_match(name)?
        || regex(r"(?:Test|Tests|TestCase)\.(?:java|kt|kts|swift|cs|scala|groovy|m|mm|vb|fs)$")?
            .is_match(original)?
        || regex(r"(?:^|/)(?:tests?|__tests__|specs?|e2e|unittests|[\w.]+[-_]unittests?)/")?
            .is_match(&lower)?
        || regex(r"(?:^|/)[A-Za-z0-9]*(?:Test|Tests|Spec)/")?.is_match(path)?)
}
impl<'a> Bindings<'a> {
    pub(super) fn nested_type(&self, node: &Node) -> bool {
        let Some((owner, _)) = node.qualified_name.rsplit_once("::") else {
            return false;
        };
        self.in_file(&node.file_path).iter().any(|node| {
            node.qualified_name == owner
                && matches!(
                    node.kind.as_str(),
                    "class" | "struct" | "interface" | "enum" | "trait" | "protocol" | "record"
                )
        })
    }
    pub(crate) fn lexically_reachable(
        &self,
        node: &'a Node,
        reference: &UnresolvedReference,
    ) -> Result<bool> {
        let cached = self.scopes.borrow().get(node.id.as_str()).copied();
        let scope = if let Some(scope) = cached {
            scope
        } else {
            let scope = self.scope(node)?;
            self.scopes.borrow_mut().insert(&node.id, scope);
            scope
        };
        Ok(scope.is_none_or(|(start, end)| {
            reference.file_path.as_deref() == Some(node.file_path.as_str())
                && reference.line >= start
                && reference.line <= end
        }))
    }
    fn scope(&self, node: &Node) -> Result<Option<(u32, u32)>> {
        if matches!(node.language.as_str(), "c" | "cpp") {
            return Ok(None);
        }
        if matches!(node.kind.as_str(), "variable" | "constant") {
            let mut scope: Option<(u32, u32)> = None;
            for &candidate in self.in_file(&node.file_path) {
                self.charge(1)?;
                if !matches!(candidate.kind.as_str(), "function" | "method")
                    || candidate.id == node.id
                    || candidate.start_line > node.start_line
                    || candidate.end_line < node.start_line
                    || candidate.start_line == candidate.end_line
                    || candidate.start_line == node.start_line
                        && candidate.start_column >= node.start_column
                {
                    continue;
                }
                if scope.is_none_or(|(start, end)| {
                    candidate.end_line - candidate.start_line < end - start
                }) {
                    scope = Some((candidate.start_line, candidate.end_line));
                }
            }
            return Ok(scope);
        }
        if !matches!(
            node.kind.as_str(),
            "function"
                | "method"
                | "class"
                | "struct"
                | "enum"
                | "interface"
                | "trait"
                | "type_alias"
        ) {
            return Ok(None);
        }
        let parent = node
            .qualified_name
            .rsplit_once("::")
            .map(|(parent, _)| parent);
        let mut owner = node.qualified_name.as_str();
        while let Some((prefix, _)) = owner.rsplit_once("::") {
            owner = prefix;
            for &candidate in self.qualified.get(owner).into_iter().flatten() {
                self.charge(1)?;
                if candidate.file_path != node.file_path
                    || !matches!(candidate.kind.as_str(), "function" | "method")
                    || candidate.start_line > node.start_line
                    || candidate.end_line < node.end_line
                {
                    continue;
                }
                if node.kind == "method" && Some(owner) == parent {
                    return Ok(None);
                }
                return Ok(Some((candidate.start_line, candidate.end_line)));
            }
        }
        Ok(None)
    }
}
