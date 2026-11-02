//! The literal's own property and lexical binding, excluding nested helpers.
use super::*;

fn within(inner: &Node, outer: &Node) -> bool {
    (inner.start_line, inner.start_column) >= (outer.start_line, outer.start_column)
        && (inner.end_line, inner.end_column) <= (outer.end_line, outer.end_column)
}
fn same_range(a: &Node, b: &Node) -> bool {
    (a.start_line, a.start_column, a.end_line, a.end_column)
        == (b.start_line, b.start_column, b.end_line, b.end_column)
}
fn offset(ctx: &Context<'_>, node: &Node) -> Option<usize> {
    let starts = ctx.lines(&node.file_path)?;
    let start = *starts.get(node.start_line.checked_sub(1)? as usize)? as usize;
    let line = ctx.line(&node.file_path, node.start_line)?;
    Some(start + byte_at(line, node.start_column))
}
enum Property {
    Unknown,
    Missing,
    Found {
        binding: Option<String>,
        start: usize,
        end: usize,
    },
}
fn property(ctx: &Context<'_>, container: &Node, member: &str) -> Result<Property> {
    let Some(source) = ctx.sources.get(container.file_path.as_str()) else {
        return Ok(Property::Unknown);
    };
    let Some(start) = offset(ctx, container) else {
        return Ok(Property::Unknown);
    };
    let Some(starts) = ctx.lines(&container.file_path) else {
        return Ok(Property::Unknown);
    };
    let Some(&line_start) = container
        .end_line
        .checked_sub(1)
        .and_then(|line| starts.get(line as usize))
    else {
        return Ok(Property::Unknown);
    };
    let line = ctx
        .line(&container.file_path, container.end_line)
        .unwrap_or("");
    let end = line_start as usize + byte_at(line, container.end_column);
    let Some(extent) = source.get(start..end) else {
        return Ok(Property::Unknown);
    };
    ctx.steps(extent.len() / 64 + 1)?;
    let extent = comments::strip_for_regex(extent, "typescript");
    let code = comments::blank_string_contents(&extent)?;
    let Some(open) =
        regex(r"^[^=]*=\s*(?:(?:Object\.(?:freeze|seal)\s*)?\(\s*)*\{")?.find(&code)?
    else {
        return Ok(Property::Unknown);
    };
    let mut parts = Vec::new();
    let mut depth = 0i64;
    let mut first = open.end();
    for (i, &ch) in code.as_bytes().iter().enumerate().skip(first) {
        match ch {
            b'{' | b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'}' => {
                if depth == 0 {
                    parts.push((first, i));
                    break;
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                parts.push((first, i));
                first = i + 1;
            }
            _ => {}
        }
    }
    let key = regex(
        r#"^(?:(?:async|get|set)\s+)?\*?\s*(?:([A-Za-z_$][\w$]*)|['"]([^'"\\]*)['"])(?=\s*(?:[:(<,=]|$))"#,
    )?;
    let binding = regex(r"^:\s*([A-Za-z_$][\w$]*)$")?;
    let mut selected = Property::Missing;
    for (from, to) in parts {
        ctx.step()?;
        let text = trim(&extent[from..to]);
        if text.starts_with("...") || text.starts_with('[') {
            selected = Property::Missing;
            continue;
        }
        let Some(key) = key.captures(text)? else {
            continue;
        };
        let name = key.get(1).or_else(|| key.get(2)).unwrap().as_str();
        if name != member {
            continue;
        }
        let value = trim(&text[key.get(0).unwrap().end()..]);
        let binding = if value.is_empty() {
            Some(member.to_owned())
        } else {
            binding.captures(value)?.map(|m| m[1].to_owned())
        };
        selected = Property::Found {
            binding,
            start: start + from,
            end: start + to,
        };
    }
    Ok(selected)
}

pub(super) fn member<'a>(
    ctx: &Context<'a>,
    container: &Node,
    name: &str,
    at: &UnresolvedReference,
) -> Result<Option<&'a Node>> {
    member_with_binding(ctx, container, name, at, true)
}

pub(in crate::synthesis) fn direct_member<'a>(
    ctx: &Context<'a>,
    container: &Node,
    name: &str,
    at: &UnresolvedReference,
) -> Result<Option<&'a Node>> {
    member_with_binding(ctx, container, name, at, false)
}

fn member_with_binding<'a>(
    ctx: &Context<'a>,
    container: &Node,
    name: &str,
    at: &UnresolvedReference,
    follow_binding: bool,
) -> Result<Option<&'a Node>> {
    if !value(container)
        || ![
            "typescript",
            "tsx",
            "javascript",
            "jsx",
            "arkts",
            "vue",
            "svelte",
            "astro",
        ]
        .contains(&container.language.as_str())
    {
        return Ok(None);
    }
    let property = property(ctx, container, name)?;
    if matches!(property, Property::Missing) {
        return Ok(None);
    }
    if let Property::Found {
        binding: Some(binding),
        ..
    } = property
    {
        return if follow_binding {
            lexical_binding(ctx, container, &binding, at)
        } else {
            Ok(None)
        };
    }
    let mut candidates = Vec::new();
    for node in ctx.named(name).filter(|node| {
        node.file_path == container.file_path
            && node.id != container.id
            && within(node, container)
            && matches!(node.kind.as_str(), "function" | "method")
    }) {
        ctx.step()?;
        if let Property::Found { start, end, .. } = property
            && !offset(ctx, node).is_some_and(|at| at >= start && at < end)
        {
            continue;
        }
        let mut nested = false;
        for body in ctx.in_file(&container.file_path).filter(|body| {
            body.id != container.id
                && matches!(body.kind.as_str(), "function" | "method")
                && within(body, container)
        }) {
            ctx.step()?;
            if body.id != node.id && !same_range(body, node) && within(node, body) {
                nested = true;
                break;
            }
        }
        if !nested {
            candidates.push(node);
        }
    }
    candidates.sort_by_key(|node| (node.start_line, node.start_column));
    Ok(candidates.first().copied())
}

fn parameter_binding(code: &str, name: &str) -> Result<bool> {
    let escaped = ::regex::escape(name);
    if regex(&format!(r"(?<![\w$]){escaped}\s*=>"))?.is_match(code)? {
        return Ok(true);
    }
    let contains = regex(&format!(r"(?<![\w$]){escaped}(?![\w$])"))?;
    let control = regex(r"\b(?:if|while|for|switch|with)\s*$")?;
    let body = regex(r"^\s*(?::[^=;{]*)?(?:=>|\{)")?;
    for (i, &ch) in code.as_bytes().iter().enumerate() {
        if ch != b'(' || control.is_match(&code[..i])? {
            continue;
        }
        let mut depth = 1;
        let mut j = i + 1;
        while j < code.len() && depth > 0 {
            match code.as_bytes()[j] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        if depth == 0 && contains.is_match(&code[i + 1..j - 1])? && body.is_match(&code[j..])? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn scope(code: &str, end: usize) -> Vec<usize> {
    let mut scope = Vec::new();
    for (i, &ch) in code.as_bytes().iter().enumerate().take(end) {
        match ch {
            b'{' => scope.push(i),
            b'}' => {
                scope.pop();
            }
            _ => {}
        }
    }
    scope
}
fn lexical_binding<'a>(
    ctx: &Context<'a>,
    container: &Node,
    name: &str,
    at: &UnresolvedReference,
) -> Result<Option<&'a Node>> {
    for function in ctx.in_file(&container.file_path).filter(|node| {
        matches!(node.kind.as_str(), "function" | "method") && within(container, node)
    }) {
        ctx.step()?;
        if let Some(signature) = &function.signature
            && parameter_binding(&format!("{signature} {{"), name)?
        {
            return Ok(None);
        }
    }
    let Some(source) = ctx.sources.get(container.file_path.as_str()) else {
        return Ok(None);
    };
    ctx.steps(source.len() / 64 + 1)?;
    let code = comments::blank_string_contents(&comments::strip_for_regex(source, "typescript"))?;
    let enclosing = scope(&code, offset(ctx, container).unwrap_or(code.len()));
    let mut locals = Vec::new();
    for node in ctx.named(name).filter(|node| {
        node.file_path == container.file_path && node.id != container.id && binding_kind(node)
    }) {
        ctx.step()?;
        let own = scope(&code, offset(ctx, node).unwrap_or(code.len()));
        if enclosing.starts_with(&own) {
            locals.push((own.len(), node));
        }
    }
    locals.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
    if let Some((_, local)) = locals.first() {
        return Ok(
            matches!(local.kind.as_str(), "function" | "method" | "class").then_some(*local),
        );
    }
    let mut reference = at.clone();
    reference.from_node_id = container.id.clone();
    reference.reference_name = name.into();
    reference.file_path = Some(container.file_path.clone());
    reference.language = Some(container.language.clone());
    reference.line = container.start_line;
    reference.column = container.start_column;
    Ok(imported(ctx, &reference)?
        .filter(|node| matches!(node.kind.as_str(), "function" | "method" | "class")))
}
