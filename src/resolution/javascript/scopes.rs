//! Source-contained object scopes and declaration-module merge guards.
use super::*;
fn offset(source: &str, line: u32, column: u32) -> usize {
    let start = source
        .split_inclusive('\n')
        .take(line.saturating_sub(1) as usize)
        .map(str::len)
        .sum::<usize>()
        .min(source.len());
    let text = source[start..].split('\n').next().unwrap_or("");
    start + byte(text, column)
}
fn extent<'a>(source: &'a str, node: &Node) -> &'a str {
    source
        .get(
            offset(source, node.start_line, node.start_column)
                ..offset(source, node.end_line, node.end_column),
        )
        .unwrap_or("")
}
fn owner(node: &Node) -> Result<bool> {
    Ok(matches!(node.kind.as_str(), "constant" | "variable")
        && family(&node.language, "typescript")
        && test(r"^=\s*\{", node.signature.as_deref().unwrap_or(""))?)
}
fn blocks(code: &str) -> Vec<(usize, usize)> {
    let mut open = Vec::new();
    let mut out = Vec::new();
    for (i, ch) in code.bytes().enumerate() {
        if ch == b'{' {
            open.push(i);
        } else if ch == b'}'
            && let Some(start) = open.pop()
        {
            out.push((start, i));
        }
    }
    out.extend(open.into_iter().map(|start| (start, code.len())));
    out
}
fn block_at(blocks: &[(usize, usize)], at: usize) -> Option<(usize, usize)> {
    blocks
        .iter()
        .copied()
        .filter(|(start, end)| *start < at && at <= *end)
        .max_by_key(|(start, _)| *start)
}
impl<'a> Context<'a> {
    pub(super) fn this_function_ref(
        &self,
        reference: &UnresolvedReference,
    ) -> Result<(Option<&'a Node>, f64)> {
        let Some(member) = reference
            .reference_name
            .strip_prefix("this.")
            .filter(|member| !member.is_empty())
        else {
            return Ok((None, 0.0));
        };
        let Some(&written) = self.ids.get(reference.from_node_id.as_str()) else {
            return Ok((None, 0.0));
        };
        let caller = self.this_caller(written)?;
        let owner_kind = |node: &Node| {
            matches!(
                node.kind.as_str(),
                "class"
                    | "struct"
                    | "union"
                    | "interface"
                    | "trait"
                    | "protocol"
                    | "enum"
                    | "type_alias"
                    | "module"
            )
        };
        let prefix = if owner_kind(caller) {
            caller.qualified_name.as_str()
        } else if let Some((prefix, _)) = caller
            .qualified_name
            .rsplit_once("::")
            .filter(|(prefix, _)| !prefix.is_empty())
        {
            prefix
        } else {
            return Ok((None, 0.0));
        };
        let qualified = format!("{prefix}::{member}");
        let file = reference.file_path.as_deref().unwrap_or("");
        self.steps(self.files.get(file).map_or(0, Vec::len))?;
        if let Some(target) = self
            .in_file(file)
            .filter(|node| {
                node.qualified_name == qualified
                    && matches!(node.kind.as_str(), "function" | "method")
                    && node.id != reference.from_node_id
            })
            .min_by_key(|node| node.start_line)
        {
            return Ok((Some(target), 0.95));
        }
        let class_name = prefix.rsplit("::").next().unwrap_or(prefix);
        let mut frontier: Vec<_> = self
            .named(class_name)
            .filter(|node| owner_kind(node) && node.file_path == file)
            .collect();
        if frontier.is_empty() {
            frontier = self
                .named(class_name)
                .filter(|node| {
                    owner_kind(node)
                        && family(&node.language, reference.language.as_deref().unwrap_or(""))
                })
                .collect();
        }
        let mut seen: std::collections::HashSet<_> =
            frontier.iter().map(|node| node.id.as_str()).collect();
        for _ in 0..5 {
            let mut next = Vec::new();
            for node in frontier {
                for &parent in self.supers.get(node.id.as_str()).into_iter().flatten() {
                    self.step()?;
                    if !owner_kind(parent) || !seen.insert(parent.id.as_str()) {
                        continue;
                    }
                    let qualified = format!("{}::{member}", parent.qualified_name);
                    self.steps(
                        self.files
                            .get(parent.file_path.as_str())
                            .map_or(0, Vec::len),
                    )?;
                    if let Some(target) = self.in_file(&parent.file_path).find(|node| {
                        node.qualified_name == qualified
                            && matches!(node.kind.as_str(), "function" | "method")
                            && family(&node.language, reference.language.as_deref().unwrap_or(""))
                    }) {
                        return Ok((Some(target), 0.85));
                    }
                    next.push(parent);
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        Ok((None, 0.0))
    }
    fn parent(&self, node: &Node) -> Option<&'a Node> {
        let (parent, _) = node.qualified_name.rsplit_once("::")?;
        if parent.is_empty() {
            return None;
        }
        self.in_file(&node.file_path).find(|candidate| {
            candidate.qualified_name == parent && candidate.id != node.id && within(node, candidate)
        })
    }
    fn literal_owner(&self, node: &Node) -> Result<Option<&'a Node>> {
        if node.kind != "function" || !family(&node.language, "typescript") {
            return Ok(None);
        }
        let Some(holder) = self.parent(node) else {
            return Ok(None);
        };
        if !owner(holder)? {
            return Ok(None);
        }
        let source = self.source(holder);
        let text = extent(source, holder);
        let code = crate::synthesis::comments::blank_string_contents(
            &crate::synthesis::comments::strip_for_regex(text, "typescript"),
        )?;
        self.steps(code.len() / 64 + 1)?;
        let blocks = blocks(&code);
        let at = offset(source, node.start_line, node.start_column).saturating_sub(offset(
            source,
            holder.start_line,
            holder.start_column,
        ));
        if let Some(block) = block_at(&blocks, at)
            && block_at(&blocks, block.0).is_some()
        {
            return Ok(None);
        }
        Ok(Some(holder))
    }
    pub(super) fn this_caller(&self, caller: &'a Node) -> Result<&'a Node> {
        if !family(&caller.language, "typescript") {
            return Ok(caller);
        }
        let mut current = caller;
        for _ in 0..8 {
            let holder = if owner(current)? {
                Some(current)
            } else if let Some(holder) = self.literal_owner(current)? {
                let mut lines = self.lines(&current.file_path)?;
                let line = lines
                    .nth(current.start_line.saturating_sub(1) as usize)
                    .unwrap_or("");
                test(
                    r"^(?:async\s*)?(?:[(<]|[A-Za-z_$][\w$]*\s*=>)",
                    &line[byte(line, current.start_column)..],
                )?
                .then_some(holder)
            } else {
                None
            };
            let Some(parent) = holder.and_then(|holder| self.parent(holder)) else {
                break;
            };
            current = parent;
        }
        Ok(current)
    }
    pub(super) fn merged(&self, node: &Node) -> Result<bool> {
        if node.qualified_name != node.name
            || !matches!(
                node.kind.as_str(),
                "function"
                    | "class"
                    | "interface"
                    | "type_alias"
                    | "enum"
                    | "constant"
                    | "variable"
                    | "component"
            )
            || !test(r"(?i)\.d\.[cm]?ts$", &node.file_path)?
        {
            return Ok(false);
        }
        let Some(module) = self.declaring_module(node)? else {
            return Ok(false);
        };
        if module.contains('*') {
            return Ok(false);
        }
        let mut count = 0;
        for candidate in self.named(&node.name).filter(|candidate| {
            candidate.qualified_name == candidate.name
                && matches!(
                    candidate.kind.as_str(),
                    "function"
                        | "class"
                        | "interface"
                        | "type_alias"
                        | "enum"
                        | "constant"
                        | "variable"
                        | "component"
                )
        }) {
            self.step()?;
            if test(r"(?i)\.d\.[cm]?ts$", &candidate.file_path)?
                && self.declaring_module(candidate)?.as_deref() == Some(&module)
            {
                count += 1;
                if count > 1 {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
