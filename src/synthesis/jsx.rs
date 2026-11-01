//! JSX rendering edges from explicit bindings, wrappers, and owned source spans.
use super::*;
use crate::frameworks::frontend::jsx_code;
use std::rc::Rc;
mod components;
pub(crate) mod react;
mod scope;
use components::{Components, kind};
struct Span {
    tags: Vec<(String, u32)>,
    bare: HashSet<String>,
    source: String,
    bindings: scope::Bindings,
}
fn jsx(language: &str) -> bool {
    matches!(language, "tsx" | "jsx" | "javascript")
}
fn parent(node: &Node) -> bool {
    matches!(node.kind.as_str(), "method" | "function" | "component")
}
fn imported<'a>(ctx: &'a Context<'_>, name: &str, file: &str) -> Option<&'a ImportBinding> {
    ctx.imports.get(&(file, name)).copied()
}
fn project<'a>(ctx: &'a Context<'_>, name: &str, file: &str) -> Option<(&'a str, &'a str)> {
    let binding = imported(ctx, name, file)?;
    if binding.is_namespace {
        return None;
    }
    let module = binding
        .mapped_path
        .as_deref()
        .or(binding.resolved_path.as_deref())?;
    ctx.in_file(module).any(|n| n.kind == "file").then_some((
        module,
        if binding.is_default || binding.imported_name == "default" {
            "default"
        } else {
            &binding.imported_name
        },
    ))
}
fn child<'a>(
    ctx: &Context<'a>,
    components: &mut Components<'a, '_>,
    name: &str,
    file: &str,
) -> Result<Option<&'a Node>> {
    if let Some(own) = components.declared(name, file)? {
        return Ok(Some(own.component.unwrap_or(own.declaration)));
    }
    let candidates = ctx
        .named(name)
        .filter(|n| kind(n) && (!manual_mock(&n.file_path) || crate::classify::is_test_path(file)))
        .collect::<Vec<_>>();
    if let Some(local) = candidates.iter().find(|n| n.file_path == file) {
        return Ok(Some(local));
    }
    if imported(ctx, name, file).is_some_and(|binding| binding.external) {
        return Ok(None);
    }
    if let Some((module, exported)) = project(ctx, name, file) {
        if let Some(exported) = components.exported(module, exported)? {
            return Ok(Some(exported));
        }
        if let Some(in_module) = candidates.iter().find(|n| n.file_path == module) {
            return Ok(Some(in_module));
        }
        if !candidates.is_empty() {
            return Ok(None);
        }
    }
    let from = imported(ctx, name, file).and_then(|binding| {
        binding
            .mapped_path
            .as_deref()
            .or(binding.resolved_path.as_deref())
    });
    if candidates.is_empty() {
        let from = if imported(ctx, name, file).is_some_and(|binding| binding.is_default) {
            from
        } else {
            None
        };
        let components = from
            .map(|from| {
                ctx.in_file(from)
                    .filter(|n| n.kind == "component")
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return Ok((components.len() == 1).then(|| components[0]));
    }
    if candidates.len() == 1 {
        return Ok(Some(candidates[0]));
    }
    if let Some(from) = from
        && let Some(imported) = candidates.iter().find(|n| n.file_path == from)
    {
        return Ok(Some(imported));
    }
    Ok(candidates
        .iter()
        .copied()
        .find(|n| {
            JS.contains(&n.language.as_str()) || matches!(n.language.as_str(), "vue" | "svelte")
        })
        .or_else(|| candidates.first().copied()))
}
fn wrapped_child<'a>(
    ctx: &Context<'a>,
    components: &mut Components<'a, '_>,
    name: &str,
    file: &str,
) -> Result<Option<&'a Node>> {
    if components.declared(name, file)?.is_none()
        && !ctx.in_file(file).any(|n| n.name == name && kind(n))
        && project(ctx, name, file).is_none()
    {
        return Ok(None);
    }
    child(ctx, components, name, file)
}
fn inside<'a>(ctx: &Context<'a>, name: &str, parent: &Node, child: &'a Node) -> Option<&'a Node> {
    let inside = |n: &Node| {
        n.id != parent.id
            && n.file_path == parent.file_path
            && n.start_line >= parent.start_line
            && n.start_line <= parent.end_line
    };
    if inside(child) {
        Some(child)
    } else {
        ctx.named(name).find(|n| kind(n) && inside(n))
    }
}
fn bare(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(at, _)| {
        let bytes = source.as_bytes();
        let before = at.checked_sub(1).and_then(|i| bytes.get(i)).copied();
        let after = bytes.get(at + name.len()).copied();
        !before.is_some_and(id)
            && !after.is_some_and(id)
            && before != Some(b'<')
            && !(before == Some(b'/') && at >= 2 && bytes[at - 2] == b'<')
    })
}
fn id(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$')
}
fn tags(source: String, start: u32) -> Result<Span> {
    let mut tags = Vec::<(String, u32)>::new();
    let mut names = HashMap::<String, usize>::new();
    let mut bare_names = HashSet::new();
    if source.contains("</") || source.contains("/>") {
        let mut line = start;
        let mut counted = 0;
        for found in regex(r"<([A-Z][A-Za-z0-9_]*)(?=[\s/><])")?.captures_iter(&source) {
            let found = found?;
            let at = found.get(0).unwrap().start();
            line += source[counted..at].bytes().filter(|&b| b == b'\n').count() as u32;
            counted = at;
            let name = &found[1];
            let opens = !regex(r"^\s+extends\s")?.is_match(
                crate::frameworks::frontend::object::prefix(&source[at + 1 + name.len()..], 40),
            )? && (!at.checked_sub(1).is_some_and(|i| id(source.as_bytes()[i]))
                || regex(r"\breturn$")?.is_match(&source[at.saturating_sub(7)..at])?);
            if let Some(&index) = names.get(name) {
                if tags[index].1 == 0 {
                    tags[index].1 = if opens { line } else { 0 };
                }
            } else {
                names.insert(name.to_owned(), tags.len());
                tags.push((name.into(), if opens { line } else { 0 }));
            }
        }
        for (name, line) in &tags {
            if *line > 0 && bare(&source, name) {
                bare_names.insert(name.clone());
            }
        }
    }
    Ok(Span {
        bindings: scope::Bindings::new(&source),
        source,
        tags,
        bare: bare_names,
    })
}
pub(super) fn code_binds(ctx: &Context<'_>, node: &Node, name: &str, line: u32) -> Result<bool> {
    let source = ctx
        .sources
        .get(node.file_path.as_str())
        .copied()
        .unwrap_or("");
    let text = source
        .split('\n')
        .skip(node.start_line.saturating_sub(1) as usize)
        .take(node.end_line.saturating_sub(node.start_line) as usize + 1)
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    let stripped = comments::strip_for_regex(&text.join("\n"), "javascript");
    let end = text
        .iter()
        .take(line.saturating_sub(node.start_line) as usize + 1)
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .saturating_sub(1)
        .min(stripped.len());
    scope::code_binds(&stripped[..end], name)
}
fn scope_binds(ctx: &Context<'_>, span: &Span, node: &Node, name: &str, line: u32) -> Result<bool> {
    if !span.bare.contains(name) {
        return Ok(false);
    }
    if code_binds(ctx, node, name, line)? {
        return Ok(true);
    }
    let end = span
        .source
        .split_inclusive('\n')
        .take(line.saturating_sub(node.start_line) as usize + 1)
        .map(str::len)
        .sum::<usize>();
    let end = if end > 0 && span.source.as_bytes().get(end - 1) == Some(&b'\n') {
        end - 1
    } else {
        end
    };
    span.bindings
        .destructures(name, span.source[..end].encode_utf16().count())
}
pub(super) fn edges(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut components = Components::new(ctx);
    for (&file, &content) in ctx.sources {
        ctx.step()?;
        if content.is_empty() {
            continue;
        }
        let has_tags = content.contains("</") || content.contains("/>");
        let wraps = regex(r"\b(?:styled|memo|forwardRef)\s*\(\s*[A-Z]")?.is_match(content)?;
        if !has_tags && !wraps {
            continue;
        }
        let in_file = ctx.in_file(file).collect::<Vec<_>>();
        let mut add = |parent: &Node, child: &Node, name: &str, wrapped: bool| -> Result<bool> {
            if parent.id == child.id
                || super::vue::crosses_web_boundary(&child.language)
                || !seen.insert((parent.id.clone(), child.id.clone()))
            {
                return Ok(false);
            }
            let mut metadata = json!({"synthesizedBy":"jsx-render","via":name});
            if wrapped {
                metadata["wrapped"] = json!(true);
            }
            ctx.emit(
                &mut out,
                Edge {
                    source: parent.id.clone(),
                    target: child.id.clone(),
                    kind: "calls".into(),
                    line: Some(parent.start_line),
                    column: None,
                    provenance: Some("heuristic".into()),
                    metadata: Some(metadata),
                },
            )?;
            Ok(true)
        };
        if wraps {
            for wrapper in &in_file {
                ctx.step()?;
                if wrapper.kind != "component" || !JS.contains(&wrapper.language.as_str()) {
                    continue;
                }
                if let Some(name) = components.wrapped_name(wrapper)?
                    && let Some(child) = wrapped_child(ctx, &mut components, &name, file)?
                {
                    add(wrapper, child, &name, true)?;
                }
            }
        }
        if !has_tags {
            continue;
        }
        let fns = in_file
            .iter()
            .copied()
            .filter(|n| parent(n) && jsx(&n.language))
            .collect::<Vec<_>>();
        let mut file_node = in_file
            .iter()
            .copied()
            .find(|n| n.kind == "file" && jsx(&n.language));
        let code = jsx_code::code(content);
        let lines = code
            .as_deref()
            .unwrap_or(content)
            .split('\n')
            .collect::<Vec<_>>();
        let mut owns = HashMap::<&str, Vec<bool>>::new();
        let mut values = Vec::new();
        if let Some(file_node_value) = file_node {
            ensure!(
                file_node_value.end_line <= 1_000_000,
                "JSX source line limit exceeded"
            );
            let length = file_node_value.end_line as usize + 2;
            let mut held = vec![0u8; length];
            let mut values_on = vec![0u8; length];
            for n in &in_file {
                if matches!(n.kind.as_str(), "file" | "import" | "export" | "route")
                    || n.start_line == 0
                    || n.end_line == 0
                {
                    continue;
                }
                let mark = if parent(n) || n.kind == "class" { 1 } else { 2 };
                for held in held
                    .iter_mut()
                    .take(n.end_line as usize + 1)
                    .skip(n.start_line as usize)
                {
                    ctx.step()?;
                    if *held != 1 {
                        *held = mark;
                    }
                }
            }
            values = in_file
                .iter()
                .copied()
                .filter(|n| {
                    matches!(n.kind.as_str(), "constant" | "variable")
                        && jsx(&n.language)
                        && held.get(n.start_line as usize) != Some(&1)
                })
                .collect();
            for v in &values {
                for count in values_on
                    .iter_mut()
                    .take(v.end_line as usize + 1)
                    .skip(v.start_line as usize)
                {
                    ctx.step()?;
                    *count = (*count + 1).min(2);
                }
                let before = lines
                    .get(v.start_line.saturating_sub(1) as usize)
                    .copied()
                    .unwrap_or("");
                let before = &before[..components::byte(before, v.start_column)];
                let after = lines
                    .get(v.end_line.saturating_sub(1) as usize)
                    .copied()
                    .unwrap_or("");
                let after = &after[components::byte(after, v.end_column)..];
                if !regex(r"^\s*(?:(?:export|declare|default|const|let|var)\s+)*$")?
                    .is_match(before)?
                    && let Some(value) = values_on.get_mut(v.start_line as usize)
                {
                    *value = 2;
                }
                if !regex(r"^[\s;]*$")?.is_match(after)?
                    && let Some(value) = values_on.get_mut(v.end_line as usize)
                {
                    *value = 2;
                }
            }
            for v in &values {
                owns.insert(
                    &v.id,
                    (v.start_line..=v.end_line)
                        .map(|line| {
                            held.get(line as usize) != Some(&1)
                                && values_on.get(line as usize) == Some(&1)
                        })
                        .collect(),
                );
            }
            if held[1..file_node_value.end_line as usize + 1].contains(&0) {
                owns.insert(
                    &file_node_value.id,
                    (file_node_value.start_line..=file_node_value.end_line)
                        .map(|line| held.get(line as usize) == Some(&0))
                        .collect(),
                );
            } else {
                file_node = None;
            }
        }
        let mut parents = fns.clone();
        parents.extend(values);
        parents.extend(file_node);
        if parents.is_empty() {
            continue;
        }
        let mut spans = HashMap::<(u32, u32, String), Rc<Span>>::new();
        let mut span_of = |node: &Node| -> Result<Rc<Span>> {
            let own = owns.get(node.id.as_str());
            let key = (
                node.start_line,
                node.end_line,
                if own.is_some() {
                    node.id.clone()
                } else {
                    String::new()
                },
            );
            if let Some(span) = spans.get(&key) {
                return Ok(span.clone());
            }
            let slice = lines
                .iter()
                .skip(node.start_line.saturating_sub(1) as usize)
                .take(node.end_line.saturating_sub(node.start_line) as usize + 1)
                .enumerate()
                .map(|(i, &line)| {
                    if own.is_none_or(|own| own.get(i) == Some(&true)) {
                        line
                    } else {
                        ""
                    }
                })
                .collect::<Vec<_>>();
            let bytes = slice.iter().map(|line| line.len()).sum::<usize>();
            ctx.work.set(ctx.work.get().saturating_add(bytes));
            ctx.step()?;
            let span = Rc::new(tags(slice.join("\n"), node.start_line)?);
            spans.insert(key, span.clone());
            Ok(span)
        };
        let top = code.as_deref().map(scope::Bindings::new);
        let patterns = top
            .as_ref()
            .map(scope::Bindings::top_patterns)
            .transpose()?
            .unwrap_or_default();
        let mut bound_top = HashMap::new();
        for parent in parents {
            ctx.step()?;
            if parent.start_line == 0 || parent.end_line == 0 {
                continue;
            }
            let names = span_of(parent)?;
            let mut around: Option<Vec<&Node>> = None;
            let mut added = 0;
            for (name, line) in &names.tags {
                if added >= 30 {
                    break;
                }
                if *line == 0 {
                    continue;
                }
                let mut child = child(ctx, &mut components, name, file)?;
                if let Some(current) = child {
                    let top_binds = if let Some(&bound) = bound_top.get(name) {
                        bound
                    } else {
                        let bound = if let Some(top) = &top {
                            top.patterns_bind(&patterns, name)?
                        } else {
                            false
                        };
                        bound_top.insert(name.clone(), bound);
                        bound
                    };
                    if owns.contains_key(parent.id.as_str()) {
                        if names.bare.contains(name)
                            && regex(&format!(
                                r"\b(?:const|let|var)\s+{}\b(?!\s*[,\]}}])",
                                regex::escape(name)
                            ))?
                            .is_match(&names.source)?
                            || top_binds
                        {
                            child = inside(ctx, name, parent, current);
                        }
                    } else {
                        let mut binder = if scope_binds(ctx, &names, parent, name, *line)? {
                            Some(parent)
                        } else {
                            None
                        };
                        if binder.is_none() {
                            let neighbours = around.get_or_insert_with(|| {
                                let mut list = fns
                                    .iter()
                                    .copied()
                                    .filter(|n| {
                                        n.start_line <= parent.start_line
                                            && n.end_line >= parent.end_line
                                            && (n.start_line != parent.start_line
                                                || n.end_line != parent.end_line)
                                    })
                                    .collect::<Vec<_>>();
                                list.sort_by_key(|n| (std::cmp::Reverse(n.start_line), n.end_line));
                                list
                            });
                            for node in neighbours.iter() {
                                if scope_binds(ctx, span_of(node)?.as_ref(), node, name, *line)? {
                                    binder = Some(node);
                                    break;
                                }
                            }
                        }
                        if let Some(binder) = binder {
                            child = inside(ctx, name, binder, current);
                        } else if top_binds {
                            child = inside(
                                ctx,
                                name,
                                around
                                    .as_ref()
                                    .and_then(|around| around.last().copied())
                                    .unwrap_or(parent),
                                current,
                            );
                        }
                    }
                }
                if let Some(child) = child
                    && add(parent, child, name, false)?
                {
                    added += 1;
                }
            }
        }
    }
    Ok(out)
}
