//! Solidity declarations and calls from the pinned language adapter and shared walker.
use crate::supplemental::{children, docstring, Context};
use crate::ExtractionResult;
use tree_sitter::Node;

pub fn extract(path: &str, source: &str) -> Result<ExtractionResult, String> {
    crate::supplemental::run(path, source, "solidity", |context, root| {
        walk(context, root, false)
    })
}

fn visibility(context: &Context<'_>, node: Node<'_>) -> Option<String> {
    children(node)
        .into_iter()
        .find(|child| child.kind() == "visibility")
        .and_then(|child| match context.text(child).trim() {
            "public" | "external" => Some("public".into()),
            "private" => Some("private".into()),
            "internal" => Some("internal".into()),
            _ => None,
        })
}

fn signature(context: &Context<'_>, node: Node<'_>) -> String {
    let mut parameters = Vec::new();
    let mut returns = None;
    let mut visibility = None;
    let mut mutability = None;
    for i in 0..node.named_child_count() {
        let child = node.named_child(i).unwrap();
        let field = node.field_name_for_named_child(i as u32);
        if child.kind() == "parameter" && field != Some("return_type") {
            parameters.push(context.text(child));
        } else if child.kind() == "return_type_definition" || field == Some("return_type") {
            returns = Some(context.text(child));
        } else if child.kind() == "visibility" {
            visibility = Some(context.text(child));
        } else if child.kind() == "state_mutability" {
            mutability = Some(context.text(child));
        }
    }
    let mut result = format!("({})", parameters.join(", "));
    for part in [visibility, mutability, returns].into_iter().flatten() {
        result.push(' ');
        result.push_str(part);
    }
    result
}

fn name(context: &Context<'_>, node: Node<'_>) -> String {
    match node.kind() {
        "constructor_definition" => "constructor".into(),
        "fallback_receive_definition" => node
            .children(&mut node.walk())
            .map(|child| context.text(child))
            .find(|text| matches!(*text, "fallback" | "receive"))
            .unwrap_or("fallback")
            .into(),
        _ => node
            .child_by_field_name("name")
            .map(|child| context.text(child))
            .unwrap_or("<anonymous>")
            .into(),
    }
}

fn call(context: &mut Context<'_>, node: Node<'_>) {
    let Some(from) = context.current() else {
        return;
    };
    let Some(function) = node
        .child_by_field_name("function")
        .or_else(|| node.named_child(0))
    else {
        return;
    };
    let mut callee = context.text(function).to_owned();
    if function.kind() == "member_expression" {
        let Some(property) = function
            .child_by_field_name("property")
            .or_else(|| function.child_by_field_name("field"))
            .or_else(|| function.named_child(1))
        else {
            return;
        };
        let receiver = function
            .child_by_field_name("object")
            .or_else(|| function.child_by_field_name("operand"))
            .or_else(|| function.child_by_field_name("argument"))
            .or_else(|| function.named_child(0));
        if receiver.is_some_and(|node| {
            matches!(
                node.kind(),
                "string"
                    | "string_literal"
                    | "number"
                    | "number_literal"
                    | "true"
                    | "false"
                    | "array"
            )
        }) {
            return;
        }
        callee = match receiver.filter(|node| node.kind() == "identifier") {
            Some(receiver)
                if !matches!(context.text(receiver), "self" | "this" | "cls" | "super") =>
            {
                format!("{}.{}", context.text(receiver), context.text(property))
            }
            _ => context.text(property).into(),
        };
    }
    if !callee.is_empty() {
        context.reference(from, &callee, "calls", node);
    }
}

fn walk(context: &mut Context<'_>, node: Node<'_>, in_body: bool) {
    stack_guard!();
    let kind = node.kind();
    match kind {
        "contract_declaration"
        | "library_declaration"
        | "interface_declaration"
        | "struct_declaration"
        | "enum_declaration" => {
            let name = name(context, node);
            let node_kind = match kind {
                "interface_declaration" => "interface",
                "struct_declaration" => "struct",
                "enum_declaration" => "enum",
                _ => "class",
            };
            let Some(index) = context.create_node(node_kind, &name, node, |_| {}) else {
                return;
            };
            for ancestor in children(node)
                .into_iter()
                .filter(|child| child.kind() == "inheritance_specifier")
            {
                if let Some(ancestor) = ancestor.child_by_field_name("ancestor") {
                    let mut stack = vec![ancestor];
                    let mut names = Vec::new();
                    while let Some(child) = stack.pop() {
                        if child.kind() == "identifier" {
                            names.push(context.text(child));
                        }
                        stack.extend(children(child).into_iter().rev());
                    }
                    if let Some(ancestor) = names.last() {
                        context.reference(index, ancestor, "extends", node);
                    }
                }
            }
            context.push_scope(index);
            let body = node.child_by_field_name("body").unwrap_or(node);
            for child in children(body) {
                walk(context, child, false);
            }
            context.pop_scope();
            return;
        }
        "enum_value" => {
            let name = context.text(node);
            context.create_node("enum_member", name, node, |_| {});
            return;
        }
        "event_definition" | "error_declaration" => {
            let name = name(context, node);
            let text = context.text(node).trim();
            let signature = crate::textutil::slice_utf16(text, 200).0;
            context.create_node("field", &name, node, |node| {
                node.signature = Some(signature)
            });
            return;
        }
        "function_definition"
        | "modifier_definition"
        | "constructor_definition"
        | "fallback_receive_definition" => {
            let name = name(context, node);
            let signature = signature(context, node);
            let visibility = visibility(context, node);
            let docstring = docstring(node, context.source);
            let method = context.scopes.iter().any(|&index| {
                matches!(
                    context.result.nodes[index].kind.as_str(),
                    "class" | "interface" | "struct" | "enum"
                )
            });
            let Some(index) = context.create_node(
                if method { "method" } else { "function" },
                &name,
                node,
                |node| {
                    node.signature = Some(signature);
                    node.docstring = docstring;
                    node.visibility = visibility;
                },
            ) else {
                return;
            };
            context.push_scope(index);
            for child in children(node) {
                walk(context, child, true);
            }
            context.pop_scope();
            return;
        }
        "state_variable_declaration" | "constant_variable_declaration" | "struct_member"
            if !in_body =>
        {
            let name = name(context, node);
            let docstring = docstring(node, context.source);
            let visibility = visibility(context, node);
            let member = context.scopes.iter().any(|&i| {
                matches!(
                    context.result.nodes[i].kind.as_str(),
                    "class" | "struct" | "interface" | "enum"
                )
            });
            let kind = if member {
                "field"
            } else if kind == "constant_variable_declaration" {
                "constant"
            } else {
                "variable"
            };
            let extent = if member {
                node
            } else {
                node.child_by_field_name("name").unwrap_or(node)
            };
            context.create_node(kind, &name, extent, |node| {
                node.docstring = docstring;
                node.visibility = visibility;
                if member {
                    node.is_static = Some(false);
                } else {
                    node.is_exported = Some(false);
                }
            });
            return;
        }
        "user_defined_type_definition" => {
            let name = name(context, node);
            let docstring = docstring(node, context.source);
            context.create_node("type_alias", &name, node, |node| node.docstring = docstring);
            return;
        }
        "import_directive" => {
            if let Some(source) = node.child_by_field_name("source") {
                let module = context.text(source).trim_matches(['\'', '"']).trim();
                let signature = context.text(node).trim().to_owned();
                if !module.is_empty() {
                    context.create_node("import", module, node, |node| {
                        node.signature = Some(signature)
                    });
                    if let Some(from) = context.current() {
                        context.reference(from, module, "imports", node);
                    }
                }
            }
            return;
        }
        "call_expression" | "emit_statement" | "revert_statement" | "modifier_invocation" => {
            call(context, node)
        }
        "new_expression" => {
            if let Some(constructor) = node
                .child_by_field_name("name")
                .or_else(|| node.child_by_field_name("type"))
                .or_else(|| node.named_child(0))
            {
                if let Some(from) = context.current() {
                    context.reference(from, context.text(constructor), "instantiates", node);
                }
            }
        }
        _ => {}
    }
    for child in children(node) {
        walk(context, child, in_body);
    }
}
