//! VB.NET AST extraction using the pinned grammar and typed record contract.
use crate::supplemental::{children, docstring, Context};
use crate::ExtractionResult;
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node as SyntaxNode;

macro_rules! re {
    ($pattern:literal) => {{
        static VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).unwrap());
        &*VALUE
    }};
}
fn child<'t>(node: SyntaxNode<'t>, kind: &str) -> Option<SyntaxNode<'t>> {
    children(node).into_iter().find(|c| c.kind() == kind)
}
fn modifier(node: SyntaxNode<'_>, source: &str, wanted: &str) -> bool {
    (0..node.child_count())
        .filter_map(|i| node.child(i))
        .any(|n| {
            n.kind() == "member_modifier" && source[n.byte_range()].eq_ignore_ascii_case(wanted)
        })
}
fn visibility(node: SyntaxNode<'_>, source: &str) -> String {
    if modifier(node, source, "private") {
        "private"
    } else if modifier(node, source, "protected") {
        "protected"
    } else if modifier(node, source, "friend") {
        "internal"
    } else {
        "public"
    }
    .into()
}
fn is_call(node: SyntaxNode<'_>) -> bool {
    matches!(
        node.kind(),
        "invocation_expression" | "array_access_expression" | "generic_invocation_expression"
    )
}
fn is_type(node: SyntaxNode<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "module_declaration"
            | "interface_declaration"
            | "structure_declaration"
            | "enum_declaration"
    )
}
fn non_type(name: &str) -> bool {
    re!(r"(?i)^(?:Global|System|Microsoft|My|Boolean|Byte|Char|Date|Decimal|Double|Integer|Long|Object|SByte|Short|Single|String|UInteger|ULong|UShort)$").is_match(name)
}
struct Walker<'s> {
    context: Context<'s>,
}

pub(crate) fn extract(path: &str, source: &str) -> Result<ExtractionResult, String> {
    let prepared = if source.ends_with('\n') {
        source.to_owned()
    } else {
        format!("{source}\n")
    };
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&crate::grammar_for("vbnet").ok_or("missing VB.NET grammar")?)
        .map_err(|e| e.to_string())?;
    let tree = parser
        .parse(&prepared, None)
        .ok_or("VB.NET parser returned no tree")?;
    let mut w = Walker {
        context: Context::new(path, &prepared, "vbnet"),
    };
    if let Some(ns) = child(tree.root_node(), "namespace_declaration") {
        if let Some(name) = ns.child_by_field_name("name") {
            if let Some(index) =
                w.context
                    .create_node("namespace", w.context.text(name), ns, |_| {})
            {
                w.context.push_scope(index);
            }
        }
    }
    crate::stack::run_guarded(|| {
        w.visit(tree.root_node());
        Ok(())
    })?;
    Ok(w.context.finish(tree.root_node().has_error()))
}

impl<'s> Walker<'s> {
    fn text(&self, node: SyntaxNode<'_>) -> &'s str {
        self.context.text(node)
    }
    fn name(&self, node: SyntaxNode<'_>) -> &'s str {
        node.child_by_field_name("name")
            .or_else(|| {
                children(node)
                    .into_iter()
                    .find(|c| matches!(c.kind(), "identifier" | "type_identifier"))
            })
            .map(|n| self.text(n))
            .unwrap_or("<anonymous>")
    }
    fn in_class(&self) -> bool {
        self.context.scopes.iter().any(|&i| {
            matches!(
                self.context.result.nodes[i].kind.as_str(),
                "class" | "struct" | "interface" | "enum" | "trait"
            )
        })
    }
    fn reference(&mut self, name: &str, kind: &str, node: SyntaxNode<'_>) {
        if let Some(from) = self.context.current() {
            self.context.reference(from, name, kind, node);
        }
    }
    fn visit(&mut self, node: SyntaxNode<'_>) {
        stack_guard!();
        match node.kind() {
            "event_declaration" | "custom_event_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    if let Some(index) =
                        self.context
                            .create_node("field", self.text(name), node, |_| {})
                    {
                        if node.kind() == "custom_event_declaration" {
                            self.context.push_scope(index);
                            self.body(node);
                            self.context.pop_scope();
                        }
                    }
                }
                return;
            }
            "constructor_declaration" => {
                if let Some(index) = self.context.create_node("method", "New", node, |_| {}) {
                    self.context.push_scope(index);
                    self.body(node);
                    self.context.pop_scope();
                }
                return;
            }
            "class_declaration"
            | "module_declaration"
            | "interface_declaration"
            | "structure_declaration"
            | "enum_declaration" => {
                self.aggregate(node);
                return;
            }
            "method_declaration"
            | "external_method_declaration"
            | "interface_method_declaration"
            | "abstract_method_declaration" => {
                self.method(node);
                return;
            }
            "property_declaration"
            | "interface_property_declaration"
            | "abstract_property_declaration"
                if self.in_class() =>
            {
                self.property(node);
                return;
            }
            "field_declaration" if self.in_class() => {
                self.field(node);
                return;
            }
            "delegate_declaration" => {
                let doc = docstring(node, self.context.source);
                self.context
                    .create_node("type_alias", self.name(node), node, |n| n.docstring = doc);
            }
            "imports_statement" => {
                if let Some(name) = children(node).into_iter().rev().find(|n| {
                    matches!(
                        n.kind(),
                        "qualified_name" | "simple_name" | "global_qualified_name" | "identifier"
                    )
                }) {
                    let text = self.text(name);
                    let signature = self.text(node).trim().to_owned();
                    self.context
                        .create_node("import", text, node, |n| n.signature = Some(signature));
                    self.reference(text, "imports", node);
                }
                return;
            }
            "declaration_statement"
                if !self.context.scopes.iter().any(|&i| {
                    self.context.result.nodes[i].kind != "file"
                        && self.context.result.nodes[i].kind != "namespace"
                }) =>
            {
                let constant = modifier(node, self.context.source, "const")
                    || modifier(node, self.context.source, "shared")
                        && modifier(node, self.context.source, "readonly");
                let doc = docstring(node, self.context.source);
                for c in children(node)
                    .into_iter()
                    .filter(|n| matches!(n.kind(), "identifier" | "variable_declarator"))
                {
                    let name = if c.kind() == "identifier" {
                        self.text(c)
                    } else {
                        self.name(c)
                    };
                    if name != "<anonymous>" {
                        self.context.create_node(
                            if constant { "constant" } else { "variable" },
                            name,
                            c,
                            |n| {
                                n.docstring = doc.clone();
                                n.is_exported = Some(false);
                            },
                        );
                    }
                }
            }
            _ => {}
        }
        self.expression(node, false);
        for c in children(node) {
            self.visit(c);
        }
    }
    fn aggregate(&mut self, node: SyntaxNode<'_>) {
        let kind = match node.kind() {
            "interface_declaration" => "interface",
            "structure_declaration" => "struct",
            "enum_declaration" => "enum",
            _ => "class",
        };
        let doc = docstring(node, self.context.source);
        let visibility = visibility(node, self.context.source);
        let Some(index) = self.context.create_node(kind, self.name(node), node, |n| {
            n.docstring = doc;
            if kind != "interface" {
                n.visibility = Some(visibility);
            }
        }) else {
            return;
        };
        for c in children(node)
            .into_iter()
            .filter(|n| matches!(n.kind(), "inherits_statement" | "implements_statement"))
        {
            for reference in children(c).into_iter().filter(|n| {
                matches!(
                    n.kind(),
                    "simple_name" | "qualified_name" | "generic_name" | "global_qualified_name"
                )
            }) {
                let text = re!(r"(?i)\(\s*Of\b[^)]*\)").replace_all(self.text(reference), "");
                let name = text.rsplit('.').next().unwrap().trim();
                if !name.is_empty() {
                    self.context.reference(
                        index,
                        name,
                        if c.kind() == "inherits_statement" {
                            "extends"
                        } else {
                            "implements"
                        },
                        reference,
                    );
                }
            }
        }
        if kind == "class" {
            self.decorators(node, index);
        }
        self.context.push_scope(index);
        for c in children(node) {
            if kind == "enum" && c.kind() == "enum_member_declaration" {
                if let Some(name) = c.child_by_field_name("name") {
                    self.context
                        .create_node("enum_member", self.text(name), c, |_| {});
                }
            } else {
                self.visit(c);
            }
        }
        self.context.pop_scope();
    }
    fn method(&mut self, node: SyntaxNode<'_>) {
        let kind = if self.in_class() {
            "method"
        } else {
            "function"
        };
        let name = self.name(node);
        if kind == "function" && name == "<anonymous>" {
            self.body(node);
            return;
        }
        let doc = docstring(node, self.context.source);
        let visibility = visibility(node, self.context.source);
        let is_static = modifier(node, self.context.source, "shared");
        let is_async = modifier(node, self.context.source, "async");
        let return_type = child(node, "as_clause")
            .and_then(|n| n.child_by_field_name("declared_type"))
            .filter(|n| !matches!(n.kind(), "predefined_type" | "array_type"))
            .and_then(|n| {
                let text = self.text(n).trim().trim_end_matches('?');
                let simple = re!(r"(?i)\(\s*Of\b[^)]*\)").replace_all(text, "");
                let last = simple.rsplit('.').next()?.trim();
                re!(r"^[A-Za-z_][A-Za-z0-9_]*$")
                    .is_match(last)
                    .then(|| last.to_owned())
            });
        let Some(index) = self.context.create_node(kind, name, node, |n| {
            n.docstring = doc;
            n.visibility = Some(visibility);
            n.is_static = Some(is_static);
            n.is_async = Some(is_async);
            n.return_type = return_type;
        }) else {
            return;
        };
        self.decorators(node, index);
        self.context.push_scope(index);
        self.body(node);
        self.context.pop_scope();
    }
    fn property(&mut self, node: SyntaxNode<'_>) {
        let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| child(node, "identifier"))
        else {
            return;
        };
        let name = self.text(name);
        let doc = docstring(node, self.context.source);
        let visibility = visibility(node, self.context.source);
        let shared = modifier(node, self.context.source, "shared");
        let ty = children(node)
            .into_iter()
            .find(|n| {
                !matches!(
                    n.kind(),
                    "modifier"
                        | "modifiers"
                        | "identifier"
                        | "accessor_list"
                        | "accessors"
                        | "equals_value_clause"
                )
            })
            .map(|n| re!(r"^:\s*").replace(self.text(n), "").into_owned());
        let signature = ty
            .filter(|s| !s.is_empty())
            .map_or_else(|| name.to_owned(), |ty| format!("{ty} {name}"));
        if let Some(index) = self.context.create_node("property", name, node, |n| {
            n.docstring = doc;
            n.signature = Some(signature);
            n.visibility = Some(visibility);
            n.is_static = Some(shared);
        }) {
            self.decorators(node, index);
            self.context.push_scope(index);
            self.body(node);
            self.context.pop_scope();
        }
    }
    fn field(&mut self, node: SyntaxNode<'_>) {
        let doc = docstring(node, self.context.source);
        let visibility = visibility(node, self.context.source);
        let shared = modifier(node, self.context.source, "shared");
        let mut declarators: Vec<_> = children(node)
            .into_iter()
            .filter(|n| n.kind() == "variable_declarator")
            .collect();
        let var = child(node, "variable_declaration");
        if declarators.is_empty() {
            if let Some(var) = var {
                declarators = children(var)
                    .into_iter()
                    .filter(|n| n.kind() == "variable_declarator")
                    .collect();
            }
        }
        if declarators.is_empty() {
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| child(node, "identifier"))
            {
                self.context
                    .create_node("field", self.text(name), node, |n| {
                        n.docstring = doc;
                        n.visibility = Some(visibility);
                        n.is_static = Some(shared);
                    });
            }
            return;
        }
        let ty = children(var.unwrap_or(node))
            .into_iter()
            .find(|n| {
                !matches!(
                    n.kind(),
                    "modifiers"
                        | "modifier"
                        | "variable_declarator"
                        | "variable_declaration"
                        | "marker_annotation"
                        | "annotation"
                )
            })
            .map(|n| self.text(n));
        for declaration in declarators {
            let Some(name) = declaration
                .child_by_field_name("name")
                .or_else(|| child(declaration, "identifier"))
            else {
                continue;
            };
            let name = self.text(name);
            let signature = ty.map_or_else(|| name.to_owned(), |ty| format!("{ty} {name}"));
            if let Some(index) = self.context.create_node("field", name, declaration, |n| {
                n.docstring = doc.clone();
                n.signature = Some(signature);
                n.visibility = Some(visibility.clone());
                n.is_static = Some(shared);
            }) {
                self.decorators(node, index);
                self.context.push_scope(index);
                self.body(declaration);
                self.context.pop_scope();
            }
        }
    }
    fn decorators(&mut self, node: SyntaxNode<'_>, from: usize) {
        for n in children(node).into_iter().flat_map(|n| {
            if n.kind() == "modifiers" {
                let mut nodes = vec![n];
                nodes.extend(children(n));
                nodes
            } else {
                vec![n]
            }
        }) {
            if !matches!(
                n.kind(),
                "decorator" | "annotation" | "marker_annotation" | "attribute"
            ) {
                continue;
            }
            let target = children(n).into_iter().find_map(|c| {
                if c.kind() == "call_expression" {
                    c.child_by_field_name("function")
                        .or_else(|| c.named_child(0))
                } else if matches!(
                    c.kind(),
                    "identifier"
                        | "member_expression"
                        | "scoped_identifier"
                        | "navigation_expression"
                        | "user_type"
                        | "type_identifier"
                ) {
                    Some(c)
                } else {
                    None
                }
            });
            if let Some(target) = target {
                let name = self
                    .text(target)
                    .split('<')
                    .next()
                    .unwrap()
                    .rsplit(['.', ':'])
                    .next()
                    .unwrap()
                    .trim();
                if !name.is_empty() {
                    self.context.reference(from, name, "decorates", n);
                }
            }
        }
    }
    fn body(&mut self, node: SyntaxNode<'_>) {
        stack_guard!();
        self.expression(node, true);
        if is_type(node) {
            self.aggregate(node);
            return;
        }
        for c in children(node) {
            self.body(c);
        }
    }
    fn expression(&mut self, node: SyntaxNode<'_>, body: bool) {
        if is_call(node) {
            self.call(node);
        } else if node.kind() == "object_creation_expression"
            || node.kind() == "array_creation_expression"
                && node
                    .child_by_field_name("type")
                    .is_some_and(|n| !matches!(n.kind(), "predefined_type" | "array_type"))
                && child(node, "array_initializer").is_none()
        {
            if let Some(ty) = node
                .child_by_field_name("constructor")
                .or_else(|| node.child_by_field_name("type"))
                .or_else(|| node.child_by_field_name("name"))
                .or_else(|| node.named_child(0))
            {
                let text = self.text(ty);
                let name = text
                    .split(['<', '('])
                    .next()
                    .unwrap()
                    .rsplit(['.', ':'])
                    .next()
                    .unwrap()
                    .trim();
                if !name.is_empty() {
                    self.reference(name, "instantiates", node);
                }
            }
        }
        if body {
            self.member_read(node);
        }
    }
    fn call(&mut self, node: SyntaxNode<'_>) {
        let Some(function) = node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("array"))
        else {
            return;
        };
        let name = match function.kind() {
            "identifier" => self.text(function).to_owned(),
            "member_access_expression" => {
                let Some(member) = function.child_by_field_name("member") else {
                    return;
                };
                let name = self.text(member);
                let receiver = function.child_by_field_name("object");
                if let Some(receiver) = receiver.filter(|n| {
                    n.kind() == "identifier"
                        && !["me", "mybase", "myclass"]
                            .contains(&self.text(*n).to_lowercase().as_str())
                }) {
                    format!("{}.{name}", self.text(receiver))
                } else {
                    name.into()
                }
            }
            "with_member_access_expression" => {
                let Some(member) = function.named_child(0).filter(|n| n.kind() == "identifier")
                else {
                    return;
                };
                let Some(receiver) = self.with_target(function).and_then(|n| self.receiver(n, 0))
                else {
                    return;
                };
                if non_type(receiver.split('.').next().unwrap()) {
                    return;
                }
                if ["me", "mybase", "myclass"].contains(&receiver.to_lowercase().as_str()) {
                    self.text(member).into()
                } else {
                    format!("{receiver}.{}", self.text(member))
                }
            }
            _ => return,
        };
        if !name.is_empty() {
            self.reference(&name, "calls", node);
        }
    }
    fn split_new(&self, node: SyntaxNode<'_>) -> bool {
        node.kind() == "object_creation_expression"
            && node.child_by_field_name("arguments").is_none()
            && node.child_by_field_name("initializer").is_none()
            && !node
                .child_by_field_name("type")
                .is_some_and(|n| n.kind() == "array_type")
    }
    fn with_target<'t>(&self, mut node: SyntaxNode<'t>) -> Option<SyntaxNode<'t>> {
        while let Some(parent) = node.parent() {
            match parent.kind() {
                "with_statement" => {
                    let expression = parent.child_by_field_name("expression")?;
                    if expression.byte_range() != node.byte_range() {
                        return Some(expression);
                    }
                }
                "object_initializer" => {
                    return parent
                        .parent()
                        .filter(|n| n.kind() == "object_creation_expression");
                }
                "anonymous_object_creation_expression" => return None,
                _ => {}
            }
            node = parent;
        }
        None
    }
    fn receiver(&self, node: SyntaxNode<'_>, depth: usize) -> Option<String> {
        if depth > 6 {
            return None;
        }
        match node.kind() {
            "identifier" => Some(self.text(node).into()),
            "me_expression" => Some("Me".into()),
            "myclass_expression" => Some("MyClass".into()),
            "mybase_expression" => Some("MyBase".into()),
            "parenthesized_expression" => self.receiver(node.named_child(0)?, depth + 1),
            "member_access_expression" | "with_member_access_expression" => {
                let through = node.kind() == "member_access_expression";
                let member = if through {
                    node.child_by_field_name("member")
                } else {
                    node.named_child(0)
                }?;
                if member.kind() != "identifier" {
                    return None;
                }
                let object = if through {
                    node.child_by_field_name("object")
                } else {
                    self.with_target(node)
                }?;
                if through && self.split_new(object) {
                    return None;
                }
                Some(format!(
                    "{}.{}",
                    self.receiver(object, depth + 1)?,
                    self.text(member)
                ))
            }
            "cast_expression" | "object_creation_expression" => {
                let mut ty = node.child_by_field_name("type")?;
                if node.kind() == "object_creation_expression" && ty.kind() == "array_type" {
                    let rank = child(ty, "array_rank_specifier")?;
                    if !re!(r"^\(\s*\)$").is_match(self.text(rank)) {
                        return None;
                    }
                    ty = ty.child_by_field_name("element_type")?;
                }
                if matches!(ty.kind(), "predefined_type" | "array_type") {
                    return None;
                }
                let written = re!(r"\s+").replace_all(self.text(ty), " ");
                (!re!(r"(?i)^(?:System|Microsoft|My)\.").is_match(&written))
                    .then(|| format!("{{{written}}}"))
            }
            _ => None,
        }
    }
    fn member_read(&mut self, node: SyntaxNode<'_>) {
        let (member, receiver) = match node.kind() {
            "member_access_expression" => {
                let object = node.child_by_field_name("object");
                (
                    node.child_by_field_name("member"),
                    object
                        .filter(|n| !self.split_new(*n))
                        .and_then(|n| self.receiver(n, 0)),
                )
            }
            "with_member_access_expression" => (
                node.named_child(0),
                self.with_target(node).and_then(|n| self.receiver(n, 0)),
            ),
            "member_initializer" => (
                node.child_by_field_name("name"),
                node.parent()
                    .and_then(|n| n.parent())
                    .filter(|n| n.kind() == "object_creation_expression")
                    .and_then(|n| self.receiver(n, 0)),
            ),
            _ => return,
        };
        let Some(member) = member.filter(|n| n.kind() == "identifier") else {
            return;
        };
        let Some(receiver) = receiver.filter(|r| !non_type(r.split('.').next().unwrap())) else {
            return;
        };
        if node.kind() != "member_initializer" {
            if let Some(parent) = node.parent().filter(|n| is_call(*n)) {
                let callee = parent
                    .child_by_field_name("function")
                    .or_else(|| parent.child_by_field_name("method"))
                    .or_else(|| parent.named_child(0));
                if callee.is_some_and(|n| n.start_byte() == node.start_byte()) {
                    return;
                }
            }
        }
        self.reference(
            &format!("{receiver}.{}", self.text(member)),
            "references",
            node,
        );
    }
}
