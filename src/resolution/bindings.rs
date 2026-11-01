//! Typed local receiver evidence from the stored source snapshot.
use anyhow::{Context, Result};
use std::cell::Cell;
use std::collections::HashMap;
use tree_sitter::Node;

#[derive(Default)]
pub(in crate::graph) struct Bindings {
    names: HashMap<String, Vec<Binding>>,
    pub(super) imports: Vec<(Option<String>, String)>,
    pub(super) package: Option<String>,
    truncated: Cell<bool>,
}
struct Binding {
    first: usize,
    last: usize,
    declared: usize,
    receiver: Option<String>,
}

impl Bindings {
    pub(in crate::graph) fn read(language: &str, source: &str) -> Result<Self> {
        let mut result = Self::default();
        if !matches!(language, "java" | "csharp" | "python" | "php" | "go") {
            return Ok(result);
        }
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(
            &prometeu_extract::grammar_for(language).context("missing receiver grammar")?,
        )?;
        let tree = parser
            .parse(source, None)
            .context("receiver parser did not return a tree")?;
        let mut pending = vec![(tree.root_node(), tree.root_node())];
        while let Some((node, mut scope)) = pending.pop() {
            if matches!(
                node.kind(),
                "function_definition"
                    | "function_declaration"
                    | "method_declaration"
                    | "constructor_declaration"
                    | "block"
                    | "lambda_expression"
            ) {
                scope = node;
            }
            let text = |node: Node<'_>| source[node.byte_range()].to_owned();
            if language == "go" && node.kind() == "package_clause" {
                result.package = node.named_child(0).map(text);
            }
            if language == "go"
                && node.kind() == "import_spec"
                && let Some(path) = node.child_by_field_name("path")
            {
                result.imports.push((
                    node.child_by_field_name("name").map(text),
                    source[path.byte_range()].trim_matches(['"', '`']).into(),
                ));
            }
            let pair = match node.kind() {
                "variable_declarator" => {
                    let name = node.child_by_field_name("name");
                    let declared = node
                        .parent()
                        .and_then(|parent| parent.child_by_field_name("type"));
                    let receiver = declared
                        .filter(|ty| {
                            !matches!(
                                &source[ty.byte_range()],
                                "var" | "dynamic" | "object" | "Object"
                            )
                        })
                        .and_then(|node| type_name(node, source))
                        .or_else(|| {
                            node.child_by_field_name("value")
                                .and_then(|value| constructed_type(value, source, language))
                        });
                    name.map(|name| (name, receiver))
                }
                "assignment" | "assignment_expression" => {
                    node.child_by_field_name("left").map(|name| {
                        (
                            name,
                            node.child_by_field_name("right")
                                .and_then(|value| constructed_type(value, source, language)),
                        )
                    })
                }
                "formal_parameter" | "parameter" | "simple_parameter" | "parameter_declaration" => {
                    node.child_by_field_name("name").map(|name| {
                        (
                            name,
                            node.child_by_field_name("type")
                                .and_then(|node| type_name(node, source)),
                        )
                    })
                }
                "var_spec" => node.child_by_field_name("name").map(|name| {
                    (
                        name,
                        node.child_by_field_name("type")
                            .and_then(|node| type_name(node, source)),
                    )
                }),
                "identifier"
                    if node
                        .parent()
                        .is_some_and(|parent| parent.kind() == "parameters") =>
                {
                    Some((node, None))
                }
                _ => None,
            };
            let mut pairs = pair.into_iter().collect::<Vec<_>>();
            if language == "go"
                && node.kind() == "short_var_declaration"
                && let Some(left) = node.child_by_field_name("left")
            {
                let right = node.child_by_field_name("right");
                let mut cursor = left.walk();
                pairs.extend(
                    left.named_children(&mut cursor)
                        .enumerate()
                        .map(|(index, name)| {
                            (
                                name,
                                right
                                    .and_then(|right| right.named_child(index))
                                    .and_then(|value| constructed_type(value, source, language)),
                            )
                        }),
                );
            }
            for (name, receiver) in pairs
                .into_iter()
                .filter(|(name, _)| matches!(name.kind(), "identifier" | "variable_name"))
            {
                result
                    .names
                    .entry(source[name.byte_range()].trim_start_matches('$').into())
                    .or_default()
                    .push(Binding {
                        first: scope.start_position().row + 1,
                        last: scope.end_position().row + 1,
                        declared: node.start_position().row + 1,
                        receiver,
                    });
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor).map(|child| (child, scope)));
        }
        for bindings in result.names.values_mut() {
            bindings.sort_by_key(|binding| binding.declared);
        }
        Ok(result)
    }

    pub(super) fn receiver(&self, name: &str, line: usize) -> Option<Option<&str>> {
        let bindings = self.names.get(name.trim_start_matches('$'))?;
        let end = bindings.partition_point(|binding| binding.declared <= line);
        let mut nearest: Option<&Binding> = None;
        for (visited, binding) in bindings[..end].iter().rev().enumerate() {
            if let Some(found) = nearest
                && binding.declared < found.declared
            {
                return Some(found.receiver.as_deref());
            }
            if visited == 256 {
                self.truncated.set(true);
                return Some(None);
            }
            if line < binding.first || line > binding.last {
                continue;
            }
            if let Some(found) = nearest {
                // Distinct writes on one source line need a call-site column.
                if binding.receiver != found.receiver {
                    return Some(None);
                }
            } else {
                nearest = Some(binding);
            }
        }
        nearest.map(|binding| binding.receiver.as_deref())
    }
    pub(super) fn has(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }
    pub(in crate::graph) fn truncated(&self) -> bool {
        self.truncated.get()
    }
}

fn constructed_type(mut node: Node<'_>, source: &str, language: &str) -> Option<String> {
    while language == "go" && node.kind() == "unary_expression" {
        node = node.child_by_field_name("operand")?;
    }
    let target = if matches!(
        node.kind(),
        "object_creation_expression" | "new_expression" | "composite_literal"
    ) {
        node.child_by_field_name("type")
            .or_else(|| node.child_by_field_name("class"))
            .or_else(|| {
                if language == "php" {
                    node.named_child(0)
                } else {
                    None
                }
            })
    } else if language == "python" && node.kind() == "call" {
        node.child_by_field_name("function")
    } else {
        None
    }?;
    type_name(target, source)
}

fn type_name(mut node: Node<'_>, source: &str) -> Option<String> {
    while matches!(
        node.kind(),
        "pointer_type" | "generic_type" | "generic_name" | "nullable_type"
    ) {
        node = node.named_child(0)?;
    }
    if matches!(
        node.kind(),
        "identifier"
            | "type_identifier"
            | "scoped_type_identifier"
            | "qualified_name"
            | "attribute"
            | "name"
            | "qualified_type"
    ) {
        Some(source[node.byte_range()].replace(['.', '\\'], "::"))
    } else {
        None
    }
}
