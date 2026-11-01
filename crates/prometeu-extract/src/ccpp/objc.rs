//! Objective-C adapter hooks; C declaration/type-reference traversal stays shared.
use super::*;

impl<'t> Walker<'t> {
    pub(super) fn dispatch_objc(&mut self, node: Node<'t>) -> bool {
        match node.kind() {
            "class_interface" | "protocol_declaration" => {
                let name = self.extract_name(node);
                let kind = if node.kind() == "protocol_declaration" {
                    "protocol"
                } else {
                    "class"
                };
                let Some(row) = self.create_node(
                    kind,
                    &name,
                    node,
                    Extra {
                        docstring: preceding_docstring(node, self.src),
                        ..Extra::default()
                    },
                ) else {
                    return true;
                };
                if node.kind() == "class_interface" {
                    if let Some(superclass) = node.child_by_field_name("superclass") {
                        self.push_ref_at(
                            row,
                            self.text(superclass),
                            edge_kind_index("extends").unwrap(),
                            superclass,
                        );
                    }
                    for args in node
                        .named_children(&mut node.walk())
                        .filter(|n| n.kind() == "parameterized_arguments")
                    {
                        for ty in args.named_children(&mut args.walk()) {
                            if let Some(name) = ty
                                .named_children(&mut ty.walk())
                                .find(|n| matches!(n.kind(), "identifier" | "type_identifier"))
                            {
                                self.push_ref_at(
                                    row,
                                    self.text(name),
                                    edge_kind_index("implements").unwrap(),
                                    name,
                                );
                            }
                        }
                    }
                }
                self.stack.push(Scope {
                    row,
                    kind,
                    name,
                    qualified: None,
                });
                for child in node.named_children(&mut node.walk()) {
                    self.visit_node(child);
                }
                self.stack.pop();
                true
            }
            "class_implementation" => {
                let Some(name_node) = node
                    .named_children(&mut node.walk())
                    .find(|n| n.kind() == "identifier")
                else {
                    return true;
                };
                let name = self.text(name_node).to_owned();
                let row = self
                    .nodes_meta
                    .iter()
                    .position(|n| n.kind == "class" && n.name == name)
                    .map(|n| n as u32)
                    .or_else(|| self.create_node("class", &name, node, Extra::default()));
                if let Some(row) = row {
                    self.stack.push(Scope {
                        row,
                        kind: "class",
                        name,
                        qualified: None,
                    });
                    for implementation in node
                        .named_children(&mut node.walk())
                        .filter(|n| n.kind() == "implementation_definition")
                    {
                        for child in implementation.named_children(&mut implementation.walk()) {
                            self.visit_node(child);
                        }
                    }
                    self.stack.pop();
                }
                self.scan_fn_ref_subtree(node, 0);
                true
            }
            "method_definition" => {
                let parameters = node
                    .named_children(&mut node.walk())
                    .any(|n| n.kind() == "method_parameter");
                let identifiers: Vec<_> = node
                    .named_children(&mut node.walk())
                    .filter(|n| n.kind() == "identifier")
                    .collect();
                let name = if parameters {
                    identifiers
                        .iter()
                        .map(|n| format!("{}:", self.text(*n)))
                        .collect::<String>()
                } else {
                    identifiers
                        .first()
                        .map(|n| self.text(*n))
                        .unwrap_or("<anonymous>")
                        .into()
                };
                let extra = Extra {
                    docstring: preceding_docstring(node, self.src),
                    is_static: Some(self.text(node).trim_start().starts_with('+')),
                    return_type: self.objc_return_type(node),
                    ..Extra::default()
                };
                if let Some(row) = self.create_node("method", &name, node, extra) {
                    self.stack.push(Scope {
                        row,
                        kind: "method",
                        name,
                        qualified: None,
                    });
                    if let Some(body) = node
                        .named_children(&mut node.walk())
                        .find(|n| n.kind() == "compound_statement")
                    {
                        self.visit_function_body(body);
                    }
                    self.stack.pop();
                }
                true
            }
            "property_declaration" => {
                let Some(declaration) = node
                    .named_children(&mut node.walk())
                    .find(|n| n.kind() == "struct_declaration")
                else {
                    return true;
                };
                let Some(mut current) = declaration
                    .named_children(&mut declaration.walk())
                    .find(|n| n.kind() == "struct_declarator")
                else {
                    return true;
                };
                let name = loop {
                    let inner = current.child_by_field_name("declarator").or_else(|| {
                        current
                            .named_children(&mut current.walk())
                            .find(|n| matches!(n.kind(), "identifier" | "pointer_declarator"))
                    });
                    let Some(inner) = inner else {
                        return true;
                    };
                    if inner.kind() == "identifier" {
                        break self.text(inner).to_owned();
                    }
                    current = inner;
                };
                let ty = node.named_child(0).map(|n| self.text(n)).unwrap_or("");
                let signature = if ty.is_empty() {
                    name.clone()
                } else {
                    format!("{ty} {name}")
                };
                self.create_node(
                    "property",
                    &name,
                    node,
                    Extra {
                        docstring: preceding_docstring(node, self.src),
                        signature: Some(signature),
                        is_static: Some(false),
                        ..Extra::default()
                    },
                );
                true
            }
            "message_expression" => {
                self.extract_objc_message(node);
                false
            }
            _ => false,
        }
    }

    fn objc_return_type(&self, node: Node<'t>) -> Option<String> {
        let method_type = node
            .named_children(&mut node.walk())
            .find(|n| n.kind() == "method_type")?;
        let mut stack = vec![method_type];
        while let Some(child) = stack.pop() {
            if child.kind() == "type_identifier" {
                let name = self.text(child).trim();
                if !matches!(
                    name,
                    "nonnull"
                        | "nullable"
                        | "null_unspecified"
                        | "null_resettable"
                        | "_Nonnull"
                        | "_Nullable"
                        | "_Null_unspecified"
                        | "__nonnull"
                        | "__nullable"
                        | "const"
                        | "volatile"
                        | "strong"
                        | "weak"
                        | "copy"
                        | "assign"
                        | "retain"
                        | "oneway"
                        | "__strong"
                        | "__weak"
                        | "__unsafe_unretained"
                        | "__autoreleasing"
                        | "__kindof"
                ) {
                    return (!matches!(name, "void" | "id" | "instancetype")
                        && util::simple_name().is_match(name))
                    .then(|| name.to_owned());
                }
            }
            let children: Vec<_> = child.named_children(&mut child.walk()).collect();
            stack.extend(children.into_iter().rev());
        }
        None
    }

    fn objc_selector(&self, node: Node<'t>) -> Option<String> {
        let names: Vec<_> = (0..node.named_child_count())
            .filter(|&i| node.field_name_for_named_child(i as u32) == Some("method"))
            .filter_map(|i| node.named_child(i))
            .map(|n| self.text(n))
            .collect();
        if names.is_empty() {
            return None;
        }
        if node.children(&mut node.walk()).any(|n| n.kind() == ":") {
            Some(names.iter().map(|name| format!("{name}:")).collect())
        } else {
            Some(names[0].into())
        }
    }

    pub(super) fn extract_objc_message(&mut self, node: Node<'t>) {
        if self.stack.is_empty() {
            return;
        }
        let Some(selector) = self.objc_selector(node) else {
            return;
        };
        let from = self.top_row();
        let receiver = node.child_by_field_name("receiver");
        let mut callee = selector.clone();
        if let Some(receiver) = receiver {
            if receiver.kind() != "message_expression" {
                let name = self.text(receiver);
                if !name.is_empty() && !matches!(name, "self" | "super") {
                    callee = format!("{name}.{selector}");
                    if capitalized_re().is_match(name) {
                        self.push_ref_at(
                            from,
                            name,
                            edge_kind_index("references").unwrap(),
                            receiver,
                        );
                    }
                }
            } else if selector
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                if let Some(inner_receiver) = receiver
                    .child_by_field_name("receiver")
                    .filter(|n| n.kind() == "identifier")
                {
                    let inner_name = self.text(inner_receiver);
                    if inner_name
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_uppercase)
                    {
                        if let Some(inner_selector) = self.objc_selector(receiver) {
                            callee = format!("{inner_name}.{inner_selector}().{selector}");
                        }
                    }
                }
            }
        }
        self.push_ref_at(from, &callee, edge_kind_index("calls").unwrap(), node);
    }
}
