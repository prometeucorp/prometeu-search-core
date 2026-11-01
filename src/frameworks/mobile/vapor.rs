use super::*;
const TYPES: &[&str] = &["class", "struct", "enum", "protocol"];
fn owner<'a>(node: &'a Node, method: &str) -> &'a str {
    // The declaration suffix is an ASCII handler name and two separators.
    node.qualified_name
        .get(..node.qualified_name.len().saturating_sub(method.len() + 2))
        .unwrap_or("")
}
fn one_owner<'a>(ctx: &Context<'a>, nodes: &[&'a Node], method: &str) -> Result<Option<&'a Node>> {
    let Some(first) = nodes.first() else {
        return Ok(None);
    };
    ctx.step(nodes.len())?;
    if nodes
        .iter()
        .any(|node| owner(node, method) != owner(first, method))
    {
        return Ok(None);
    }
    if nodes.len() == 1 {
        return Ok(Some(first));
    }
    let request = regex(r"\(\s*(?:\w+\s+)?\w+\s*:\s*Request\b")?;
    let mut takes = Vec::new();
    for node in nodes {
        let end = node.end_line.min(node.start_line.saturating_add(2));
        let head = (node.start_line.saturating_sub(1)..end)
            .map(|line| {
                ctx.line(&node.file_path, line as usize)
                    .trim_end_matches('\r')
            })
            .collect::<Vec<_>>()
            .join(" ");
        ctx.step(head.len() / 32 + 1)?;
        if request.is_match(&head)? {
            takes.push(*node)
        }
    }
    Ok(if takes.is_empty() { nodes } else { &takes }
        .iter()
        .copied()
        .min_by_key(|node| node.start_line))
}
fn handler<'a>(
    ctx: &Context<'a>,
    mut ty: Option<String>,
    method: &str,
    r: &UnresolvedReference,
) -> Result<Option<&'a Node>> {
    let callables: Vec<_> = ctx
        .named(method)
        .filter(|node| {
            matches!(node.kind.as_str(), "method" | "function") && node.language == "swift"
        })
        .collect();
    ctx.step(callables.len())?;
    if callables.is_empty() {
        return Ok(None);
    }
    let file = r.file_path.as_deref().unwrap_or("");
    if ty.is_none() {
        ctx.step(ctx.by_file.get(file).map_or(0, Vec::len))?;
        let enclosing = ctx
            .in_file(file)
            .filter(|node| {
                TYPES.contains(&node.kind.as_str())
                    && node.start_line <= r.line
                    && node.end_line >= r.line
            })
            .max_by_key(|node| node.start_line);
        let Some(enclosing) = enclosing else {
            let functions: Vec<_> = callables
                .iter()
                .copied()
                .filter(|node| node.kind == "function")
                .collect();
            let same: Vec<_> = functions
                .iter()
                .copied()
                .filter(|node| node.file_path == file)
                .collect();
            return if !same.is_empty() {
                one_owner(ctx, &same, method)
            } else {
                Ok((functions.len() == 1).then(|| functions[0]))
            };
        };
        let own: Vec<_> = callables
            .iter()
            .copied()
            .filter(|node| {
                node.file_path == file
                    && node.start_line >= enclosing.start_line
                    && node.end_line <= enclosing.end_line
            })
            .collect();
        if !own.is_empty() {
            return one_owner(ctx, &own, method);
        }
        ty = Some(enclosing.qualified_name.replace("::", "."));
    }
    let ty = ty.unwrap();
    let parts: Vec<_> = ty.split('.').collect();
    let last = parts.last().copied().unwrap_or("");
    let full = parts.join("::");
    let exact: Vec<_> = callables
        .iter()
        .copied()
        .filter(|node| owner(node, method) == full)
        .collect();
    if !exact.is_empty() {
        return one_owner(ctx, &exact, method);
    }
    let suffix = format!("::{full}");
    let nested: Vec<_> = callables
        .iter()
        .copied()
        .filter(|node| owner(node, method).ends_with(&suffix))
        .collect();
    if !nested.is_empty() {
        return one_owner(ctx, &nested, method);
    }
    if parts.len() > 1 {
        let declaration = regex(&format!(
            r"\bextension\s+{}\b",
            parts
                .iter()
                .map(|part| regex::escape(part))
                .collect::<Vec<_>>()
                .join(r"\s*\.\s*")
        ))?;
        let mut extended = Vec::new();
        for node in &callables {
            if owner(node, method) != last {
                continue;
            }
            ctx.step(ctx.by_file.get(node.file_path.as_str()).map_or(0, Vec::len))?;
            let Some(enclosing) = ctx
                .in_file(&node.file_path)
                .filter(|parent| {
                    TYPES.contains(&parent.kind.as_str())
                        && parent.name == last
                        && parent.start_line <= node.start_line
                        && parent.end_line >= node.end_line
                })
                .max_by_key(|node| node.start_line)
            else {
                continue;
            };
            let head = (enclosing.start_line.saturating_sub(1)
                ..enclosing.start_line.saturating_add(2))
                .map(|line| {
                    ctx.line(&node.file_path, line as usize)
                        .trim_end_matches('\r')
                })
                .collect::<Vec<_>>()
                .join(" ");
            ctx.step(head.len() / 32 + 1)?;
            if declaration.is_match(&head)? {
                extended.push(*node)
            }
        }
        return one_owner(ctx, &extended, method);
    }
    let suffix = format!("::{last}");
    let nested: Vec<_> = callables
        .iter()
        .copied()
        .filter(|node| owner(node, method).ends_with(&suffix))
        .collect();
    one_owner(ctx, &nested, method)
}
pub(super) fn resolve(ctx: &Context<'_>, r: &UnresolvedReference) -> Result<Option<Binding>> {
    if r.language.as_deref() != Some("swift") {
        return Ok(None);
    }
    let name = &r.reference_name;
    if let Some(parts) =
        regex(r"^((?:[A-Za-z_]\w*\.)*[A-Za-z_]\w*)?@([A-Za-z_]\w*)$")?.captures(name)?
    {
        return Ok(handler(
            ctx,
            parts.get(1).map(|m| m.as_str().to_owned()),
            &parts[2],
            r,
        )?
        .map(|node| binding(node, 0.9)));
    }
    for (matches, kinds, dirs, confidence) in [
        (
            name.ends_with("Controller"),
            &["class", "struct"][..],
            &["/Controllers/", "/Controller/", "/Routes/"][..],
            0.85,
        ),
        (
            regex(r"^[A-Z][a-zA-Z]+$")?.is_match(name)?,
            &["class"][..],
            &["/Models/", "/Model/", "/Entities/", "/Database/"][..],
            0.75,
        ),
        (
            name.ends_with("Middleware"),
            &["class", "struct"][..],
            &["/Middleware/", "/Middlewares/"][..],
            0.8,
        ),
    ] {
        if matches && let Some(node) = swift::pick(ctx, r, kinds, dirs)? {
            return Ok(Some(binding(node, confidence)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_vapor_handler_and_naming_answers() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/frameworks/vapor.json"
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
            let resolve_module = |_: &str, _: &str, _: &str| Ok(None);
            let mut bindings = Bindings::new(&nodes, &sources, &[], &resolve_module).unwrap();
            let references: Vec<UnresolvedReference> =
                serde_json::from_value(case["references"].clone()).unwrap();
            for (reference, expected) in references.iter().zip(case["expected"].as_array().unwrap())
            {
                let actual =
                    serde_json::to_value(bindings.resolve("vapor", reference).unwrap()).unwrap();
                if actual != *expected {
                    differences.push(json!({"origin":case["origin"],"reference":reference,"expected":expected,"actual":actual}));
                }
            }
        }
        if !differences.is_empty() {
            std::fs::write(
                "/tmp/prometeu-vapor-answer-differences.json",
                serde_json::to_string_pretty(&differences).unwrap(),
            )
            .unwrap();
        }
        assert!(
            differences.is_empty(),
            "{} Vapor answers differ; see /tmp/prometeu-vapor-answer-differences.json",
            differences.len()
        );
    }
}
