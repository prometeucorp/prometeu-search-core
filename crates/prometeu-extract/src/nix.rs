//! Pinned Nix visitNode hook over its native AST. Provenance: ../UPSTREAM.md.
use crate::supplemental::{children, Context};
use crate::textutil::{slice_utf16, utf16_len};
use crate::ExtractionResult;
use tree_sitter::Node;

pub fn extract(path: &str, source: &str) -> Result<ExtractionResult, String> {
    crate::supplemental::run(path, source, "nix", walk)
}

fn field_or<'tree>(node: Node<'tree>, field: &str, index: usize) -> Option<Node<'tree>> {
    node.child_by_field_name(field)
        .or_else(|| node.named_child(index))
}

fn unwrap_variable(node: Node<'_>) -> Node<'_> {
    if node.kind() == "variable_expression" {
        node.named_child(0).unwrap_or(node)
    } else {
        node
    }
}

fn callee<'a>(mut node: Node<'a>) -> Node<'a> {
    while node.kind() == "apply_expression" {
        let Some(function) = field_or(node, "function", 0) else {
            break;
        };
        node = function;
    }
    unwrap_variable(node)
}

fn static_path(text: &str) -> bool {
    (text.starts_with("./") || text.starts_with("../"))
        && !text.chars().any(|ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '{' | '}' | '(' | ')' | '[' | ']' | ';' | '"' | '\'' | '<' | '>' | '$'
                )
        })
}

fn import_path<'a>(mut node: Node<'_>, context: &Context<'a>) -> Option<&'a str> {
    while node.kind() == "parenthesized_expression" {
        let Some(inner) = node.named_child(0) else {
            break;
        };
        node = inner;
    }
    let mut text = context.text(node).trim();
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        text = &text[1..text.len() - 1];
    }
    static_path(text).then_some(text)
}

fn returned_attrset(node: Node<'_>) -> bool {
    let mut current = node;
    let mut returned = false;
    while let Some(parent) = current.parent() {
        if parent.kind() == "let_expression" {
            let body = parent
                .child_by_field_name("body")
                .or_else(|| parent.child_by_field_name("expression"));
            if body != Some(current) {
                return false;
            }
        }
        if (parent.kind() == "binding" && current != node)
            || matches!(parent.kind(), "formal_parameters" | "formals")
        {
            return false;
        }
        if matches!(
            parent.kind(),
            "attrset" | "rec_attrset" | "attrset_expression" | "rec_attrset_expression"
        ) {
            returned = true;
        }
        current = parent;
    }
    returned
}

fn function_body<'tree>(
    mut node: Node<'tree>,
    context: &Context<'_>,
) -> (Vec<String>, Option<Node<'tree>>) {
    let mut params = Vec::new();
    while node.kind() == "function_expression" && node.named_child_count() > 0 {
        let body = node.named_child(node.named_child_count() - 1).unwrap();
        let parameter = context.source[node.start_byte()..body.start_byte()].trim();
        let parameter = parameter.strip_suffix(':').unwrap_or(parameter).trim();
        if !parameter.is_empty() {
            params.push(parameter.into());
        }
        if body.kind() == "function_expression" {
            node = body;
        } else {
            return (params, Some(body));
        }
    }
    (
        params,
        node.named_child_count()
            .checked_sub(1)
            .and_then(|last| node.named_child(last)),
    )
}

fn emit_import(context: &mut Context<'_>, path: &str, anchor: Node<'_>) {
    let signature = slice_utf16(context.text(anchor).trim(), 100).0;
    if context
        .create_node("import", path, anchor, |node| {
            node.signature = Some(signature)
        })
        .is_some()
    {
        if let Some(owner) = context.current() {
            context.reference(owner, path, "imports", anchor);
        }
    }
}

fn walk(context: &mut Context<'_>, node: Node<'_>) {
    stack_guard!();
    match node.kind() {
        "binding" => {
            let Some(attribute) = field_or(node, "attrpath", 0) else {
                return;
            };
            let name = context.text(attribute).trim();
            let Some(value) = node
                .child_by_field_name("expression")
                .or_else(|| node.child_by_field_name("value"))
                .or_else(|| node.named_child(1))
            else {
                return;
            };
            if name.is_empty() {
                return;
            }
            let exported = returned_attrset(node);
            if value.kind() == "function_expression" {
                let (params, body) = function_body(value, context);
                let signature = match params.as_slice() {
                    [] => "()".into(),
                    [one] if one.starts_with('(') || one.contains('{') || one.contains('@') => {
                        one.clone()
                    }
                    [one] => format!("({one})"),
                    _ => params.join(" : "),
                };
                if let Some(index) = context.create_node("function", name, node, |node| {
                    node.signature = Some(signature);
                    node.is_exported = Some(exported);
                }) {
                    context.push_scope(index);
                    if let Some(body) = body {
                        walk(context, body);
                    }
                    context.pop_scope();
                }
            } else {
                let initial = slice_utf16(context.text(value), 100).0;
                context.create_node("variable", name, node, |node| {
                    if !initial.is_empty() {
                        node.signature = Some(format!(
                            "= {initial}{}",
                            if utf16_len(&initial) >= 100 {
                                "..."
                            } else {
                                ""
                            }
                        ));
                    }
                    node.is_exported = Some(exported);
                });
                if matches!(name.rsplit('.').next(), Some("imports" | "modules"))
                    && value.kind() == "list_expression"
                {
                    for child in children(value) {
                        let path = context.text(child).trim();
                        if child.kind() == "path_expression" && static_path(path) {
                            emit_import(context, path, child);
                        }
                    }
                }
                walk(context, value);
            }
            return;
        }
        "function_expression" => {
            if let Some(body) = node
                .named_child_count()
                .checked_sub(1)
                .and_then(|last| node.named_child(last))
            {
                walk(context, body);
            }
            return;
        }
        "inherit" | "inherit_from" => {
            for child in children(node) {
                if child.kind() == "inherited_attrs" {
                    for attribute in children(child) {
                        let name = context.text(attribute).trim();
                        let exported = returned_attrset(attribute);
                        context.create_node("variable", name, attribute, |node| {
                            node.is_exported = Some(exported)
                        });
                    }
                }
            }
            for child in children(node) {
                if child.kind() != "inherited_attrs" {
                    walk(context, child);
                }
            }
            return;
        }
        "apply_expression" => {
            let direct = field_or(node, "function", 0)
                .map(unwrap_variable)
                .map(|function| context.text(function).trim());
            let is_import = matches!(direct, Some("import" | "builtins.import"));
            let is_parent_callee = node
                .parent()
                .filter(|parent| parent.kind() == "apply_expression")
                .and_then(|parent| field_or(parent, "function", 0))
                == Some(node);
            if !is_parent_callee || is_import {
                if is_import {
                    if let Some(path) = field_or(node, "argument", 1)
                        .and_then(|argument| import_path(argument, context))
                    {
                        emit_import(context, path, node);
                    }
                } else {
                    let function = callee(node);
                    if matches!(function.kind(), "identifier" | "select_expression") {
                        let name = context.text(function).trim();
                        if !name.is_empty() && !matches!(name, "import" | "builtins.import") {
                            if let Some(owner) = context.current() {
                                context.reference(owner, name, "calls", node);
                            }
                        }
                        if matches!(name, "callPackage" | "callPackages")
                            || name.ends_with(".callPackage")
                            || name.ends_with(".callPackages")
                        {
                            let mut inner = node;
                            while let Some(function) = field_or(inner, "function", 0)
                                .filter(|function| function.kind() == "apply_expression")
                            {
                                inner = function;
                            }
                            if let Some(path) = field_or(inner, "argument", 1)
                                .and_then(|argument| import_path(argument, context))
                            {
                                emit_import(context, path, node);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    for child in children(node) {
        walk(context, child);
    }
}
