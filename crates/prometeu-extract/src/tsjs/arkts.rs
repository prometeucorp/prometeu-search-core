//! ArkTS-specific declarations and ArkUI calls, from the pinned language adapter.
use super::*;

impl<'t> Walker<'t> {
    pub(super) fn arkts_decorators(&mut self, node: Node<'t>) -> StrRef {
        if !matches!(
            node.kind(),
            "struct_declaration"
                | "public_field_definition"
                | "method_definition"
                | "function_declaration"
        ) {
            return NONE_STR;
        }
        fn name<'a>(walker: &Walker<'a>, decorator: Node<'a>) -> Option<String> {
            for child in decorator.named_children(&mut decorator.walk()) {
                if child.kind() == "identifier" {
                    return Some(walker.text(child).into());
                }
                if child.kind() == "call_expression" {
                    if let Some(function) = child
                        .child_by_field_name("function")
                        .filter(|n| n.kind() == "identifier")
                    {
                        return Some(walker.text(function).into());
                    }
                }
            }
            None
        }
        let mut names = Vec::new();
        for child in node
            .named_children(&mut node.walk())
            .filter(|n| n.kind() == "decorator")
        {
            if let Some(name) = name(self, child) {
                names.push(name);
            }
        }
        let mut sibling = node.prev_named_sibling();
        while let Some(decorator) = sibling.filter(|n| n.kind() == "decorator") {
            if let Some(name) = name(self, decorator) {
                names.insert(0, name);
            }
            sibling = decorator.prev_named_sibling();
        }
        if names.is_empty() {
            NONE_STR
        } else {
            self.arena.put(&names.join("\0"))
        }
    }

    pub(super) fn extract_arkts_struct(&mut self, node: Node<'t>) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        let name = self.extract_name(node);
        let extra = Extra {
            docstring: crate::docstring::preceding_docstring(node, self.src),
            visibility: self.visibility_of(node),
            is_exported: Some(self.is_exported(node)),
            ..Extra::default()
        };
        let Some(row) = self.create_node("struct", &name, node, extra) else {
            return;
        };
        self.extract_inheritance(node, row);
        self.stack.push(Scope {
            row,
            kind: "struct",
            name,
        });
        for child in body.named_children(&mut body.walk()) {
            self.visit_node(child);
        }
        self.stack.pop();
    }

    fn arkts_attribute(&mut self, node: Node<'t>) {
        let name = self.text(node);
        if !name.is_empty() {
            self.push_call_ref(&format!(".{name}"), node);
        }
    }

    fn arkts_handlers(&mut self, args: Option<Node<'t>>) {
        let Some(args) = args else {
            return;
        };
        for arg in args
            .named_children(&mut args.walk())
            .filter(|n| n.kind() == "member_expression")
        {
            if arg
                .child_by_field_name("object")
                .is_some_and(|n| n.kind() == "this")
            {
                if let Some(property) = arg.child_by_field_name("property") {
                    self.push_call_ref(self.text(property), arg);
                }
            }
        }
    }

    pub(super) fn extract_arkts_call(&mut self, node: Node<'t>) -> bool {
        let handler = |name: &str| {
            name.as_bytes().get(2).is_some_and(u8::is_ascii_uppercase) && name.starts_with("on")
        };
        if node.kind() == "arkui_component_expression" {
            if let Some(function) = node
                .child_by_field_name("function")
                .filter(|n| n.kind() == "identifier")
            {
                self.push_call_ref(self.text(function), node);
            }
            for i in 0..node.child_count() {
                let Some(property) = node.child(i).filter(|n| n.kind() == "property_identifier")
                else {
                    continue;
                };
                self.arkts_attribute(property);
                if handler(self.text(property)) {
                    let args = ((i + 1)..node.child_count())
                        .filter_map(|j| node.child(j))
                        .take_while(|n| n.kind() != "property_identifier")
                        .find(|n| n.kind() == "arguments");
                    self.arkts_handlers(args);
                }
            }
            return true;
        }
        if node.kind() == "call_expression" {
            if let Some(function) = node.child_by_field_name("function") {
                if matches!(
                    function.kind(),
                    "member_expression" | "arkui_dsl_decorator_member_expression"
                ) {
                    let object = function.child_by_field_name("object");
                    if function.kind() == "arkui_dsl_decorator_member_expression"
                        || object.is_some_and(|n| {
                            matches!(n.kind(), "call_expression" | "arkui_component_expression")
                        })
                    {
                        if let Some(property) = function.child_by_field_name("property") {
                            self.arkts_attribute(property);
                            if handler(self.text(property)) {
                                self.arkts_handlers(node.child_by_field_name("arguments"));
                            }
                            return true;
                        }
                    }
                }
                if function.kind() == "identifier" {
                    let mut parent = node.parent();
                    while let Some(current) = parent
                        .filter(|n| matches!(n.kind(), "member_expression" | "call_expression"))
                    {
                        parent = current.parent();
                    }
                    if parent.is_some_and(|n| n.kind() == "leading_dot_expression") {
                        self.arkts_attribute(function);
                        if handler(self.text(function)) {
                            self.arkts_handlers(node.child_by_field_name("arguments"));
                        }
                        return true;
                    }
                }
            }
        }
        if node.kind() == "leading_dot_expression" {
            if let Some(identifier) = node
                .named_child(0)
                .filter(|n| node.named_child_count() == 1 && n.kind() == "identifier")
            {
                self.arkts_attribute(identifier);
                if handler(self.text(identifier)) {
                    let args = node
                        .parent()
                        .and_then(|n| n.next_named_sibling())
                        .and_then(|n| n.named_child(0))
                        .filter(|n| n.kind() == "parenthesized_expression");
                    self.arkts_handlers(args);
                }
            }
            return true;
        }
        false
    }
}
