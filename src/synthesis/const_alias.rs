//! Module values that hand a callable binding on, retaining explicit import evidence.
use super::*;
use crate::frameworks::frontend::object::{matching, skip_string};
use std::borrow::Cow;
pub(super) mod object;

fn trim(source: &str) -> &str {
    source.trim_matches(crate::search::query_whitespace)
}
fn value(node: &Node) -> bool {
    matches!(node.kind.as_str(), "constant" | "variable")
}
fn callable(node: &Node) -> bool {
    matches!(node.kind.as_str(), "function" | "method" | "component")
}
fn binding_kind(node: &Node) -> bool {
    matches!(
        node.kind.as_str(),
        "function" | "component" | "class" | "constant" | "variable"
    )
}

struct Patterns {
    cast: fancy_regex::Regex,
    mock: fancy_regex::Regex,
    name: fancy_regex::Regex,
    non_name: fancy_regex::Regex,
    call: fancy_regex::Regex,
}
fn patterns() -> &'static Patterns {
    static PATTERNS: std::sync::LazyLock<Patterns> = std::sync::LazyLock::new(|| Patterns {
        cast: regex(r"^\s+(?:as|satisfies)\s").expect("valid reference pattern"),
        mock: regex(r"\bMock").expect("valid reference pattern"),
        name: regex(r"^[A-Za-z_$][\w$]*(?:\s*\??\.\s*[A-Za-z_$][\w$]*)*$")
            .expect("valid reference pattern"),
        non_name: regex(r#"^(?:[{['"`\d]|new\s)"#).expect("valid reference pattern"),
        call: regex(r"^[\w$.]+(?:<[^(]*>)?\s*\(").expect("valid reference pattern"),
    });
    &PATTERNS
}

struct Top<'a> {
    operands: Vec<&'a str>,
    cast: Option<usize>,
}
fn top(code: &str) -> Result<Option<Top<'_>>> {
    let mut operands = Vec::new();
    let mut question = None;
    let mut nested = 0;
    let mut start = 0;
    let mut cast = None;
    let mut i = 0;
    let cast_at = &patterns().cast;
    let bytes = code.as_bytes();
    while i < bytes.len() && cast.is_none() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                let Some(end) = skip_string(code, i)? else {
                    return Ok(None);
                };
                i = end;
            }
            b'(' | b'[' | b'{' => {
                let Some(end) = matching(code, i)? else {
                    return Ok(None);
                };
                i = end;
            }
            b')' | b']' | b'}' => return Ok(None),
            b'?' => {
                if bytes.get(i + 1) == Some(&b'?') {
                    if question.is_none() {
                        operands.push(&code[start..i]);
                        start = i + 2;
                    }
                    i += 1;
                } else if bytes.get(i + 1) != Some(&b'.')
                    || bytes.get(i + 2).is_some_and(u8::is_ascii_digit)
                {
                    if question.is_none() {
                        question = Some(i);
                    } else {
                        nested += 1;
                    }
                }
            }
            b':' if question.is_some() => {
                if nested == 0 {
                    return Ok(Some(Top {
                        operands: vec![&code[question.unwrap() + 1..i], &code[i + 1..]],
                        cast: None,
                    }));
                }
                nested -= 1;
            }
            b'|' if bytes.get(i + 1) == Some(&b'|') => {
                if question.is_none() {
                    operands.push(&code[start..i]);
                    start = i + 2;
                }
                i += 1;
            }
            _ if question.is_none() => {
                if bytes[i] == b'=' && bytes.get(i + 1) == Some(&b'>') && operands.is_empty() {
                    return Ok(None);
                }
                if code.is_char_boundary(i) && cast_at.is_match(&code[i..])? {
                    cast = Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    if question.is_some() {
        return Ok(None);
    }
    if !operands.is_empty() {
        operands.push(&code[start..]);
        cast = None;
    }
    Ok(Some(Top { operands, cast }))
}
fn paths_in(code: &str, nesting: usize, out: &mut Vec<String>) -> Result<()> {
    let code = trim(code);
    if code.is_empty() || nesting > 8 {
        return Ok(());
    }
    let Some(top) = top(code)? else {
        return Ok(());
    };
    if !top.operands.is_empty() {
        for operand in top.operands {
            paths_in(operand, nesting + 1, out)?;
        }
        return Ok(());
    }
    if let Some(cast) = top.cast {
        if !patterns().mock.is_match(&code[cast..])? {
            paths_in(&code[..cast], nesting + 1, out)?;
        }
        return Ok(());
    }
    if let Some(inner) = code.strip_suffix('!') {
        return paths_in(inner, nesting + 1, out);
    }
    if code.starts_with('(') {
        if matching(code, 0)? == Some(code.len() - 1) {
            paths_in(&code[1..code.len() - 1], nesting + 1, out)?;
        }
        return Ok(());
    }
    if !patterns().name.is_match(code)? {
        return Ok(());
    }
    let path: String = code
        .chars()
        .filter(|&ch| ch != '?' && !crate::search::query_whitespace(ch))
        .collect();
    if ![
        "true",
        "false",
        "null",
        "undefined",
        "this",
        "super",
        "NaN",
        "Infinity",
    ]
    .contains(&path.split('.').next().unwrap())
        && !out.contains(&path)
    {
        out.push(path);
    }
    Ok(())
}
fn aliased_paths(initializer: &str) -> Result<Vec<String>> {
    let code = if initializer.contains("/*") || initializer.contains("//") {
        Cow::Owned(comments::strip_for_regex(initializer, "typescript"))
    } else {
        Cow::Borrowed(initializer)
    };
    let mut out = Vec::new();
    paths_in(&code, 0, &mut out)?;
    Ok(out)
}

fn byte_at(source: &str, units: u32) -> usize {
    let mut seen = 0;
    for (at, ch) in source.char_indices() {
        if seen >= units {
            return at;
        }
        seen += ch.len_utf16() as u32;
    }
    source.len()
}
fn declaration(node: &Node, ctx: &Context<'_>) -> Result<String> {
    let mut lines = Vec::new();
    for line in node.start_line..=node.end_line {
        ctx.step()?;
        let Some(line) = ctx.line(&node.file_path, line) else {
            break;
        };
        lines.push(line.strip_suffix('\r').unwrap_or(line));
    }
    if let Some(last) = lines.last_mut()
        && node.end_column > 0
    {
        *last = &last[..byte_at(last, node.end_column)];
    }
    if let Some(first) = lines.first_mut() {
        *first = &first[byte_at(first, node.start_column)..];
    }
    Ok(lines.join("\n"))
}
fn initializer_at(code: &str, at: usize) -> Result<Option<usize>> {
    let bytes = code.as_bytes();
    let mut i = at;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                let Some(end) = skip_string(code, i)? else {
                    return Ok(None);
                };
                i = end;
            }
            b'{' | b'(' | b'[' => {
                let Some(end) = matching(code, i)? else {
                    return Ok(None);
                };
                i = end;
            }
            b'=' => {
                if matches!(bytes.get(i + 1), Some(b'>' | b'=')) {
                    i += 1;
                } else {
                    return Ok(Some(i + 1));
                }
            }
            b';' | b',' | b'}' | b')' => return Ok(None),
            _ => {}
        }
        i += 1;
    }
    Ok(None)
}
fn initializer<'a>(node: &'a Node, ctx: &Context<'_>) -> Result<Option<Cow<'a, str>>> {
    if node.qualified_name != node.name {
        return Ok(None);
    }
    let Some(kept) = node
        .signature
        .as_deref()
        .and_then(|signature| signature.strip_prefix("= "))
    else {
        return Ok(None);
    };
    if patterns().non_name.is_match(kept)? {
        return Ok(None);
    }
    if kept.encode_utf16().count() < 100 || !kept.ends_with("...") {
        return Ok(Some(Cow::Borrowed(kept)));
    }
    if let Some(call) = patterns().call.find(kept)?
        && matching(kept, call.end() - 1)?.is_none()
    {
        return Ok(None);
    }
    let code = comments::strip_for_regex(&declaration(node, ctx)?, "typescript");
    Ok(initializer_at(&code, node.name.len())?.map(|at| Cow::Owned(code[at..].to_owned())))
}
fn own<'a>(ctx: &Context<'a>, name: &str, file: &str) -> Option<&'a Node> {
    ctx.in_file(file)
        .find(|node| node.name == name && node.qualified_name == name && binding_kind(node))
}
fn reference(alias: &Node, name: &str) -> UnresolvedReference {
    UnresolvedReference {
        from_node_id: alias.id.clone(),
        reference_name: name.into(),
        reference_kind: "calls".into(),
        line: alias.start_line,
        column: alias.start_column,
        file_path: Some(alias.file_path.clone()),
        language: Some(alias.language.clone()),
        candidates: None,
    }
}
fn imported<'a>(ctx: &Context<'a>, at: &UnresolvedReference) -> Result<Option<&'a Node>> {
    Ok((ctx.resolve_import)(at)?.and_then(|id| ctx.node(&id)))
}
fn target<'a>(ctx: &Context<'a>, path: &str, alias: &Node) -> Result<Option<&'a Node>> {
    let mut members = path.split('.');
    let root = members.next().unwrap();
    let mut at = reference(alias, path);
    let mut target = if let Some(binding) = ctx.imports.get(&(alias.file_path.as_str(), root)) {
        at.reference_name = if binding.is_namespace {
            let Some(member) = members.next() else {
                return Ok(None);
            };
            format!("{root}.{member}")
        } else {
            root.into()
        };
        // The callback follows explicit exports only, including stated defaults.
        imported(ctx, &at)?
    } else {
        own(ctx, root, &alias.file_path)
    };
    for member in members {
        ctx.step()?;
        let Some(owner) = target else {
            return Ok(None);
        };
        target = if owner.kind == "class" {
            ctx.qualified(&format!("{}::{member}", owner.qualified_name))
                .find(|node| {
                    node.file_path == owner.file_path
                        && node.kind == "method"
                        && node.is_static == Some(true)
                })
        } else {
            object::member(ctx, owner, member, &at)?
        };
    }
    Ok(target)
}

pub(super) fn edges(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut aliases: Vec<_> = ctx
        .nodes
        .iter()
        .filter(|node| value(node) && JS.contains(&node.language.as_str()))
        .collect();
    aliases.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.start_line.cmp(&b.start_line))
            .then(a.id.cmp(&b.id))
    });
    let mut named = Vec::new();
    let mut targets = HashMap::new();
    for alias in aliases {
        ctx.step()?;
        let Some(initializer) = initializer(alias, ctx)? else {
            continue;
        };
        let paths = aliased_paths(&initializer)?;
        if paths.is_empty() || prometeu_extract::is_generated_file(&alias.file_path) {
            continue;
        }
        let mut links = Vec::new();
        for via in paths {
            ctx.step()?;
            let key = (alias.file_path.clone(), via.clone());
            if !targets.contains_key(&key) {
                targets.insert(key.clone(), target(ctx, &via, alias)?);
            }
            if let Some(target) = targets[&key]
                && target.id != alias.id
                && (callable(target) || value(target))
            {
                links.push((target, via));
            }
        }
        if !links.is_empty() {
            named.push((alias, links));
        }
    }
    // Reverse reachability is equivalent to the reference fixed point without
    // repeatedly scanning a long chain of aliases.
    let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut live = HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    for (alias, links) in &named {
        for (target, _) in links {
            if value(target) {
                reverse.entry(&target.id).or_default().push(&alias.id);
            } else if live.insert(alias.id.as_str()) {
                queue.push_back(alias.id.as_str());
            }
        }
    }
    while let Some(id) = queue.pop_front() {
        ctx.step()?;
        for parent in reverse.get(id).into_iter().flatten() {
            if live.insert(*parent) {
                queue.push_back(*parent);
            }
        }
    }
    let mut output = Vec::new();
    for (alias, links) in named {
        for (target, via) in links {
            if value(target) && !live.contains(target.id.as_str()) {
                continue;
            }
            let mut edge = call(alias, target, "const-alias", &via);
            edge.metadata.as_mut().unwrap()["registeredAt"] =
                format!("{}:{}", alias.file_path, alias.start_line).into();
            ctx.emit(&mut output, edge)?;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alias_paths_and_complete_edge_records_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/const-alias.json"
        ))
        .unwrap();
        assert_eq!(
            corpus["revision"],
            "6aa358ccbae30fbf27318f79ee183212f726f9ba"
        );
        for case in corpus["paths"].as_array().unwrap() {
            assert_eq!(
                serde_json::to_value(aliased_paths(case["source"].as_str().unwrap()).unwrap())
                    .unwrap(),
                case["expected"],
                "{}",
                case["source"]
            );
        }
        for case in corpus["cases"].as_array().unwrap() {
            let mut raw = case["nodes"].clone();
            for node in raw.as_array_mut().unwrap() {
                node.as_object_mut()
                    .unwrap()
                    .insert("updatedAt".into(), 0.into());
            }
            let nodes: Vec<Node> = serde_json::from_value(raw).unwrap();
            let base = serde_json::from_value::<Vec<Edge>>(case["baseEdges"].clone()).unwrap();
            let sources: BTreeMap<&str, &str> = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(file, source)| (file.as_str(), source.as_str().unwrap()))
                .collect();
            let imports: Vec<ImportBinding> =
                serde_json::from_value(case["imports"].clone()).unwrap();
            let answers: Vec<(UnresolvedReference, Option<String>)> = case["answers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|answer| {
                    (
                        serde_json::from_value(answer["reference"].clone()).unwrap(),
                        answer["target"].as_str().map(str::to_owned),
                    )
                })
                .collect();
            let resolve = |reference: &UnresolvedReference| -> Result<Option<String>> {
                Ok(answers
                    .iter()
                    .find(|(expected, _)| {
                        expected.from_node_id == reference.from_node_id
                            && expected.reference_name == reference.reference_name
                            && expected.file_path == reference.file_path
                            && expected.line == reference.line
                            && expected.column == reference.column
                    })
                    .unwrap_or_else(|| {
                        panic!("unrecorded import lookup in {}: {reference:?}", case["id"])
                    })
                    .1
                    .clone())
            };
            let mut ctx = Context::new(&nodes, &base, &sources, &[]);
            ctx.resolve_import = &resolve;
            for binding in &imports {
                ctx.imports
                    .entry((&binding.file, &binding.local_name))
                    .or_insert(binding);
            }
            let actual = edges(&ctx).unwrap_or_else(|error| panic!("{}: {error:#}", case["id"]));
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "{}: {}",
                case["id"],
                case["origin"]
            );
        }
    }
}
