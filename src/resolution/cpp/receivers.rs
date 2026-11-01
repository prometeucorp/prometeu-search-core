//! Receiver declarations read from the stored C/C++ syntax tree.
use super::*;
use anyhow::Context;

#[derive(Clone)]
pub(super) struct Receiver {
    pub name: String,
    pub raw: String,
    pub file: String,
    pub owner: Option<String>,
    pub first: u32,
    pub last: u32,
    pub declared: u32,
}
fn declarator(mut node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    for _ in 0..32 {
        if matches!(node.kind(), "identifier" | "field_identifier") {
            return Some(node);
        }
        if node.kind() == "function_declarator" {
            return None;
        }
        node = node
            .child_by_field_name("declarator")
            .or_else(|| node.named_child(0))?;
    }
    None
}
fn owner<'a>(nodes: &'a [Node], file: &str, line: u32, column: u32) -> Option<&'a Node> {
    nodes
        .iter()
        .filter(|node| {
            node.file_path == file
                && matches!(
                    node.kind.as_str(),
                    "class" | "struct" | "union" | "function" | "method"
                )
                && (node.start_line, node.start_column) <= (line, column)
                && (node.end_line, node.end_column) >= (line, column)
        })
        .min_by_key(|node| {
            (
                node.end_line - node.start_line,
                std::cmp::Reverse(node.start_column),
            )
        })
}
pub(super) fn read(nodes: &[Node], sources: &BTreeMap<&str, &str>) -> Result<Vec<Receiver>> {
    let mut receivers = Vec::new();
    let mut work = 0usize;
    for (&file, &source) in sources {
        let Some(language) = nodes
            .iter()
            .find(|node| node.file_path == file && family(node))
            .map(|node| node.language.as_str())
        else {
            continue;
        };
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(
            &prometeu_extract::grammar_for(language).context("missing C/C++ grammar")?,
        )?;
        let tree = parser
            .parse(source, None)
            .context("C/C++ receiver parser did not return a tree")?;
        let mut pending = vec![(tree.root_node(), tree.root_node())];
        while let Some((node, mut scope)) = pending.pop() {
            work += 1;
            ensure!(
                work <= 2_000_000,
                "C/C++ receiver syntax exceeds graph budget"
            );
            if matches!(
                node.kind(),
                "compound_statement" | "function_definition" | "lambda_expression"
            ) {
                scope = node;
            }
            if matches!(
                node.kind(),
                "declaration"
                    | "field_declaration"
                    | "parameter_declaration"
                    | "optional_parameter_declaration"
            ) && let Some(ty) = node.child_by_field_name("type")
            {
                let mut cursor = node.walk();
                for decl in node.children_by_field_name("declarator", &mut cursor) {
                    let Some(name) = declarator(decl) else {
                        continue;
                    };
                    let mut raw = source[ty.byte_range()].to_string();
                    if source[ty.end_byte()..name.start_byte()].contains('*') {
                        raw.push('*');
                    }
                    if raw == "auto" || raw == "auto*" {
                        raw = decl
                            .child_by_field_name("value")
                            .and_then(|value| match value.kind() {
                                "call_expression" => value.child_by_field_name("function"),
                                "new_expression" => value.child_by_field_name("type"),
                                "compound_literal_expression" => value.child_by_field_name("type"),
                                _ => None,
                            })
                            .map_or(String::new(), |value| {
                                source[value.byte_range()].to_string()
                            });
                    }
                    let line = name.start_position().row as u32 + 1;
                    let column = source[source[..name.start_byte()].rfind('\n').map_or(0, |i| i + 1)
                        ..name.start_byte()]
                        .encode_utf16()
                        .count() as u32;
                    let owning = owner(nodes, file, line, column);
                    let class = owning
                        .filter(|node| CLASSES.contains(&node.kind.as_str()))
                        .map(|node| node.qualified_name.clone());
                    receivers.push(Receiver {
                        name: source[name.byte_range()].to_string(),
                        raw,
                        file: file.to_string(),
                        owner: class,
                        first: scope.start_position().row as u32 + 1,
                        last: scope.end_position().row as u32 + 1,
                        declared: line,
                    });
                }
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor).map(|child| (child, scope)));
        }
    }
    Ok(receivers)
}
