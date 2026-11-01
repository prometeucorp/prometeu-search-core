//! Pinned Terraform/OpenTofu AST extraction. Provenance: ../UPSTREAM.md.
use crate::supplemental::{children, Context};
use crate::ExtractionResult;
use std::collections::VecDeque;
use tree_sitter::Node;

pub fn extract(path: &str, source: &str) -> Result<ExtractionResult, String> {
    crate::supplemental::run(path, source, "terraform", walk)
}

fn child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    children(node).into_iter().find(|node| node.kind() == kind)
}

fn string_value<'a>(context: &Context<'a>, node: Node<'_>) -> &'a str {
    child(node, "template_literal").map_or("", |node| context.text(node))
}

fn string_literal(node: Node<'_>) -> Option<Node<'_>> {
    let mut pending = VecDeque::from([node]);
    while let Some(node) = pending.pop_front() {
        if node.kind() == "string_lit" {
            return Some(node);
        }
        pending.extend(children(node));
    }
    None
}

fn string_attribute<'a>(context: &Context<'a>, body: Node<'_>, name: &str) -> Option<&'a str> {
    for attribute in children(body) {
        if attribute.kind() != "attribute" {
            continue;
        }
        if child(attribute, "identifier").is_some_and(|node| context.text(node) == name) {
            return child(attribute, "expression")
                .and_then(string_literal)
                .map(|node| string_value(context, node));
        }
    }
    None
}

fn qualified_references(head: &str, attrs: &[&str]) -> Vec<String> {
    let first = attrs.first().copied().unwrap_or("");
    if first.is_empty() {
        return Vec::new();
    }
    match head {
        "var" | "local" => vec![format!("{head}.{first}")],
        "data" => attrs
            .get(1)
            .filter(|next| !next.is_empty())
            .map_or_else(Vec::new, |next| vec![format!("data.{first}.{next}")]),
        "module" => {
            let mut names = vec![format!("module.{first}")];
            if let Some(second) = attrs.get(1).filter(|next| !next.is_empty()) {
                names.push(format!("module.{first}:output.{second}"));
                if *second == "outputs" {
                    if let Some(third) = attrs.get(2).filter(|next| !next.is_empty()) {
                        names.push(format!("module.{first}:remote-output.{third}"));
                    }
                }
            }
            names
        }
        _ => vec![format!("{head}.{first}")],
    }
}

fn collect_references(
    context: &mut Context<'_>,
    expression: Node<'_>,
    owner: usize,
    suppress_scoped: bool,
) {
    let mut pending = VecDeque::from([expression]);
    while let Some(node) = pending.pop_front() {
        if node.kind() == "variable_expr" {
            if let Some(identifier) = child(node, "identifier") {
                let head = context.text(identifier);
                if !matches!(
                    head,
                    "each" | "count" | "self" | "path" | "terraform" | "null" | "true" | "false"
                ) {
                    let mut attrs = Vec::new();
                    let mut next = node.next_named_sibling();
                    while let Some(part) = next {
                        if part.kind() == "get_attr" {
                            let Some(identifier) = child(part, "identifier") else {
                                break;
                            };
                            attrs.push(context.text(identifier));
                        } else if !matches!(
                            part.kind(),
                            "index"
                                | "new_index"
                                | "legacy_index"
                                | "splat"
                                | "attr_splat"
                                | "full_splat"
                        ) {
                            break;
                        }
                        next = part.next_named_sibling();
                    }
                    for name in qualified_references(head, &attrs) {
                        if !suppress_scoped || !name.contains(':') {
                            context.reference(owner, &name, "references", node);
                        }
                    }
                }
            }
        }
        pending.extend(children(node));
    }
}

fn body_references(
    context: &mut Context<'_>,
    body: Node<'_>,
    owner: usize,
    suppress_scoped: bool,
    skip: &str,
) {
    let mut pending: VecDeque<_> = children(body)
        .into_iter()
        .filter(|node| {
            node.kind() != "attribute"
                || !child(*node, "identifier").is_some_and(|name| context.text(name) == skip)
        })
        .collect();
    while let Some(node) = pending.pop_front() {
        if node.kind() == "expression" {
            collect_references(context, node, owner, suppress_scoped);
        } else {
            pending.extend(children(node));
        }
    }
}

fn provider_selection(context: &Context<'_>, expression: Node<'_>) -> Option<String> {
    let mut pending = VecDeque::from([expression]);
    while let Some(node) = pending.pop_front() {
        if node.kind() == "variable_expr" {
            let head = context.text(child(node, "identifier")?);
            return match node.next_named_sibling() {
                Some(next) if next.kind() == "get_attr" && next.next_named_sibling().is_none() => {
                    Some(format!(
                        "{head}.{}",
                        context.text(child(next, "identifier")?)
                    ))
                }
                None => Some(head.into()),
                _ => None,
            };
        }
        if matches!(node.kind(), "function_call" | "conditional" | "for_expr") {
            return None;
        }
        pending.extend(children(node));
    }
    None
}

fn provider_references(context: &mut Context<'_>, body: Node<'_>, owner: usize, module: bool) {
    let name = if module { "providers" } else { "provider" };
    for attribute in children(body) {
        if attribute.kind() != "attribute"
            || !child(attribute, "identifier")
                .is_some_and(|identifier| context.text(identifier) == name)
        {
            continue;
        }
        if !module {
            if let Some(selection) = child(attribute, "expression")
                .and_then(|expression| provider_selection(context, expression))
            {
                context.reference(
                    owner,
                    &format!("provider.{selection}"),
                    "references",
                    attribute,
                );
            }
        } else {
            let mut pending = VecDeque::from([attribute]);
            while let Some(node) = pending.pop_front() {
                if node.kind() == "object_elem" {
                    if let Some(selection) = node
                        .child_by_field_name("val")
                        .and_then(|value| provider_selection(context, value))
                    {
                        context.reference(
                            owner,
                            &format!("provider.{selection}"),
                            "references",
                            node,
                        );
                    }
                } else {
                    pending.extend(children(node));
                }
            }
        }
        return;
    }
}

fn module_wiring(
    context: &mut Context<'_>,
    name: &str,
    block: Node<'_>,
    body: Node<'_>,
    owner: usize,
) {
    for attribute in children(body) {
        if attribute.kind() != "attribute" {
            continue;
        }
        let Some(identifier) = child(attribute, "identifier") else {
            continue;
        };
        let attr = context.text(identifier);
        if attr == "source" {
            if let Some(literal) = child(attribute, "expression").and_then(string_literal) {
                let source = string_value(context, literal);
                if source.starts_with("./") || source.starts_with("../") {
                    context.reference(owner, &format!("module.{name}:file"), "imports", block);
                }
            }
        } else if !matches!(
            attr,
            "version" | "count" | "for_each" | "providers" | "depends_on"
        ) {
            context.reference(
                owner,
                &format!("module.{name}:var.{attr}"),
                "references",
                attribute,
            );
        }
    }
}

fn locals(context: &mut Context<'_>, body: Node<'_>) {
    for attribute in children(body) {
        if attribute.kind() != "attribute" {
            continue;
        }
        let Some(identifier) = child(attribute, "identifier") else {
            continue;
        };
        let name = context.text(identifier);
        let Some(owner) = context.create_node("constant", name, attribute, |node| {
            node.qualified_name = format!("local.{name}");
            node.signature = Some(format!("local.{name}"));
        }) else {
            continue;
        };
        if let Some(expression) = child(attribute, "expression") {
            context.push_scope(owner);
            collect_references(context, expression, owner, false);
            context.pop_scope();
        }
    }
}

fn walk(context: &mut Context<'_>, node: Node<'_>) {
    stack_guard!();
    if node.kind() != "block" {
        if node.kind() == "attribute"
            && context.path.ends_with(".tfvars")
            && node.parent().is_some_and(|parent| {
                parent.kind() == "body"
                    && parent
                        .parent()
                        .is_some_and(|grandparent| grandparent.kind() == "config_file")
            })
        {
            if let Some(identifier) = child(node, "identifier") {
                context.reference(
                    0,
                    &format!("var.{}", context.text(identifier)),
                    "references",
                    node,
                );
            }
            return;
        }
    } else if let Some(first) = node
        .named_child(0)
        .filter(|first| first.kind() == "identifier")
    {
        let kind = context.text(first);
        let labels: Vec<_> = children(node)
            .into_iter()
            .skip(1)
            .take_while(|node| matches!(node.kind(), "string_lit" | "identifier"))
            .map(|node| {
                if node.kind() == "string_lit" {
                    string_value(context, node)
                } else {
                    context.text(node)
                }
            })
            .collect();
        let body = child(node, "body");
        if labels.is_empty() {
            match kind {
                "locals" => {
                    if let Some(body) = body {
                        locals(context, body);
                    }
                    return;
                }
                "terraform" => return,
                "moved" | "import" | "removed" | "assert" => {
                    if let Some(body) = body {
                        body_references(context, body, 0, true, "");
                    }
                    return;
                }
                _ => {}
            }
        }
        let first = labels.first().copied().unwrap_or("");
        let second = labels.get(1).copied().unwrap_or("");
        let declaration = match kind {
            "resource" | "data" if !first.is_empty() && !second.is_empty() => {
                let name = format!("{first}.{second}");
                let qualified = if kind == "data" {
                    format!("data.{name}")
                } else {
                    name.clone()
                };
                Some((
                    "class",
                    name,
                    qualified,
                    format!("{kind} \"{first}\" \"{second}\""),
                ))
            }
            "module" | "variable" | "output" | "provider" if !first.is_empty() => {
                let node_kind = match kind {
                    "module" => "module",
                    "provider" => "namespace",
                    _ => "variable",
                };
                let prefix = if kind == "variable" { "var" } else { kind };
                let alias = if kind == "provider" {
                    body.and_then(|body| string_attribute(context, body, "alias"))
                        .filter(|alias| !alias.is_empty())
                } else {
                    None
                };
                let name =
                    alias.map_or_else(|| first.to_string(), |alias| format!("{first}.{alias}"));
                let signature = alias.map_or_else(
                    || format!("{kind} \"{first}\""),
                    |alias| format!("provider \"{first}\" alias=\"{alias}\""),
                );
                Some((
                    node_kind,
                    name.clone(),
                    format!("{prefix}.{name}"),
                    signature,
                ))
            }
            _ => None,
        };
        if let Some((node_kind, name, qualified, signature)) = declaration {
            if let Some(owner) = context.create_node(node_kind, &name, node, |node| {
                node.qualified_name = qualified;
                node.signature = Some(signature);
                node.is_exported = Some(node_kind == "variable");
            }) {
                if let Some(body) = body {
                    context.push_scope(owner);
                    let skip = match kind {
                        "resource" | "data" => {
                            provider_references(context, body, owner, false);
                            "provider"
                        }
                        "module" => {
                            provider_references(context, body, owner, true);
                            "providers"
                        }
                        _ => "",
                    };
                    body_references(context, body, owner, false, skip);
                    if kind == "module" {
                        module_wiring(context, first, node, body, owner);
                    }
                    context.pop_scope();
                }
            }
            return;
        }
    }
    for child in children(node) {
        walk(context, child);
    }
}
