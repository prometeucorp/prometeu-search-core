//! CFML tag/script/query delegation from the pinned upstream implementation.
//! SPDX-License-Identifier: MIT. Provenance: crates/prometeu-extract/UPSTREAM.md.
use super::components::Positions;
use super::formats::{empty, node};
use super::preprocess::{captures, utf16_len};
use anyhow::{Context, Result};
use prometeu_extract::supplemental::children;
use prometeu_extract::{Edge, ExtractionResult, NodeIdAllocator, UnresolvedReference, node_id};
use std::collections::HashSet;
use std::path::Path;
use tree_sitter::Node as Syntax;

mod script;

const EXPRESSION_PARENTS: &[&str] = &[
    "cf_set_tag",
    "cf_if_tag",
    "cf_elseif_tag",
    "cf_return_tag",
    "hash_expression",
];
const EXPRESSIONS: &[&str] = &[
    "call_expression",
    "member_expression",
    "subscript_expression",
    "new_expression",
    "assignment_expression",
    "augmented_assignment_expression",
    "binary_expression",
    "unary_expression",
    "update_expression",
    "ternary_expression",
    "elvis_expression",
    "parenthesized_expression",
    "sequence_expression",
    "string",
    "array",
    "object",
    "ordered_struct",
    "function_expression",
    "arrow_function",
];

pub(super) fn extract(path: &Path, source: &str, language: &str) -> Result<ExtractionResult> {
    Ok(extract_projected(path, source, language)?.0)
}
pub(super) fn extract_projected(
    path: &Path,
    source: &str,
    language: &str,
) -> Result<(ExtractionResult, Vec<(u32, u32)>)> {
    let path = path
        .to_str()
        .context("source path must be UTF-8")?
        .replace('\\', "/");
    if language == "cfquery" {
        let result = script::extract(&path, source, language)?;
        let columns = result
            .nodes
            .iter()
            .map(|node| (node.start_column, node.end_column))
            .collect();
        return Ok((result, columns));
    }
    let mut walker = Walker {
        path: &path,
        source,
        language,
        result: empty(),
        columns: Vec::new(),
        positions: Positions::new(source),
        ids: NodeIdAllocator::default(),
        expressions: Vec::new(),
    };
    let extracted = if is_bare_script(source) {
        walker.bare_script()
    } else {
        walker.tag_based()
    };
    if let Err(error) = extracted {
        walker.result.errors.push(serde_json::json!({"message":format!("CFML extraction error: {error}"),"severity":"error","code":"parse_error"}));
    }
    Ok((walker.result, walker.columns))
}
fn is_bare_script(source: &str) -> bool {
    let mut at = 0;
    while at < source.len() {
        let tail = &source[at..];
        if let Some(ch) = tail
            .chars()
            .next()
            .filter(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{feff}'))
        {
            at += ch.len_utf8();
        } else if tail.starts_with("//") {
            at += tail.find('\n').map_or(tail.len(), |end| end + 1);
        } else if let Some(comment) = tail.strip_prefix("/*") {
            at += comment.find("*/").map_or(tail.len(), |end| end + 4);
        } else {
            return !tail.starts_with('<');
        }
    }
    true
}
struct Expression {
    start: usize,
    end: usize,
    row: u32,
    column: u32,
    text_column: u32,
    scope: String,
}
struct Walker<'a> {
    path: &'a str,
    source: &'a str,
    language: &'a str,
    result: ExtractionResult,
    columns: Vec<(u32, u32)>,
    positions: Positions,
    ids: NodeIdAllocator,
    expressions: Vec<Expression>,
}
impl Walker<'_> {
    fn text(&self, node: Syntax<'_>) -> &str {
        &self.source[node.byte_range()]
    }
    fn component_name(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or(self.path);
        if [".cfc", ".cfm", ".cfs"]
            .iter()
            .any(|extension| name.to_ascii_lowercase().ends_with(extension))
        {
            name[..name.len() - 4].into()
        } else {
            name.into()
        }
    }
    fn bare_script(&mut self) -> Result<()> {
        let mut result = script::extract(self.path, self.source, "cfscript")?;
        let name = self.component_name();
        for node in &mut result.nodes {
            node.language = self.language.into();
            if node.name == "<anonymous>" && matches!(node.kind.as_str(), "class" | "interface") {
                node.name = name.clone();
                node.qualified_name = format!("{}::{name}", self.path);
            } else if node.qualified_name == "<anonymous>"
                || node.qualified_name.starts_with("<anonymous>::")
            {
                node.qualified_name = format!("{name}{}", &node.qualified_name[11..]);
            }
        }
        for reference in &mut result.unresolved_references {
            reference.language = Some(self.language.into());
        }
        self.columns = result
            .nodes
            .iter()
            .map(|node| (node.start_column, node.end_column))
            .collect();
        self.result = result;
        Ok(())
    }
    fn add(
        &mut self,
        kind: &str,
        name: &str,
        syntax: Syntax<'_>,
        qualified: String,
        owner: Option<usize>,
    ) -> usize {
        let (start_line, start_column) = self.positions.at(syntax.start_byte());
        let (end_line, end_column) = self.positions.at(syntax.end_byte());
        let id = self
            .ids
            .generate(self.path, kind, name, start_line, start_column);
        let mut record = node(self.path, self.language, kind, name, &qualified, id.clone());
        record.start_line = start_line;
        record.start_column = start_column;
        record.end_line = end_line;
        record.end_column = end_column;
        let index = self.result.nodes.len();
        self.result.nodes.push(record);
        self.columns.push((start_column, end_column));
        if let Some(owner) = owner {
            self.contains(self.result.nodes[owner].id.clone(), id);
        }
        index
    }
    fn contains(&mut self, source: String, target: String) {
        self.result.edges.push(Edge {
            source,
            target,
            kind: "contains".into(),
            line: None,
            column: None,
            provenance: None,
            metadata: None,
        });
    }
    fn reference(&mut self, owner: usize, name: &str, kind: &str, at: Syntax<'_>) {
        let (line, column) = self.positions.at(at.start_byte());
        self.result.unresolved_references.push(UnresolvedReference {
            from_node_id: self.result.nodes[owner].id.clone(),
            reference_name: name.into(),
            reference_kind: kind.into(),
            line,
            column,
            file_path: Some(self.path.into()),
            language: Some(self.language.into()),
            candidates: None,
        });
    }
    fn tag_based(&mut self) -> Result<()> {
        let grammar = prometeu_extract::grammar_for("cfml").context("cfml grammar not loaded")?;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&grammar)?;
        let tree = parser
            .parse(self.source, None)
            .context("Failed to parse CFML source")?;
        let filename = self.path.rsplit('/').next().unwrap_or(self.path);
        let mut file = node(
            self.path,
            self.language,
            "file",
            filename,
            self.path,
            node_id(self.path, "file", self.path, 1),
        );
        (file.end_line, file.end_column) = self.positions.at(self.source.len());
        self.columns.push((0, file.end_column));
        self.result.nodes.push(file);
        let root = tree.root_node();
        let mut child = root.named_child(0);
        while let Some(node) = child {
            if node.kind() == "cf_component_open_tag" {
                child = self.component(node, 0)?.next_sibling();
            } else {
                self.visit(node, root.kind(), 0, None, true)?;
                child = node.next_sibling();
            }
        }
        self.tag_expressions()
    }
    fn component<'a>(&mut self, open: Syntax<'a>, owner: usize) -> Result<Syntax<'a>> {
        let name = self
            .attribute(open, "name")
            .unwrap_or_else(|| self.component_name());
        let index = self.add(
            "class",
            &name,
            open,
            format!("{}::{name}", self.path),
            Some(owner),
        );
        self.result.nodes[index].is_exported = Some(true);
        self.result.nodes[index].end_line = self.result.nodes[index].start_line;
        if let Some(base) = self
            .attribute(open, "extends")
            .filter(|value| !value.is_empty())
        {
            self.reference(index, &base, "extends", open);
        }
        self.inheritance(
            index,
            self.attribute(open, "implements"),
            "implements",
            open,
        );
        let mut sibling = open.next_sibling();
        let mut last = open;
        while let Some(node) = sibling {
            last = node;
            if node.kind() == "cf_component_close_tag" {
                break;
            }
            self.visit(
                node,
                open.parent().map_or("", |parent| parent.kind()),
                index,
                Some(name.clone()),
                true,
            )?;
            sibling = node.next_sibling();
        }
        let (end_line, end_column) = self.positions.at(last.end_byte());
        self.result.nodes[index].end_line = end_line;
        self.columns[index].1 = end_column;
        Ok(last)
    }
    fn inheritance(&mut self, owner: usize, list: Option<String>, kind: &str, tag: Syntax<'_>) {
        if let Some(list) = list {
            for name in list
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                self.reference(owner, name, kind, tag);
            }
        }
    }
    fn visit(
        &mut self,
        first: Syntax<'_>,
        parent: &str,
        owner: usize,
        class: Option<String>,
        declarations: bool,
    ) -> Result<()> {
        let mut tasks = vec![(first, parent.to_owned(), owner, class, declarations)];
        while let Some((node, parent, owner, class, declarations)) = tasks.pop() {
            let mut next_owner = owner;
            let mut next_class = class;
            let mut next_declarations = declarations && node.kind() != "ERROR";
            match node.kind() {
                "cf_script_tag" => {
                    self.script_tag(node, owner, next_class.as_deref())?;
                    continue;
                }
                "cf_query_tag" => self.query_tag(node, owner)?,
                "cf_function_tag" => {
                    if !declarations {
                        continue;
                    }
                    let Some(name) = self.attribute(node, "name").filter(|name| !name.is_empty())
                    else {
                        continue;
                    };
                    let kind = if next_class.is_some() {
                        "method"
                    } else {
                        "function"
                    };
                    let qualified =
                        format!("{}::{name}", next_class.as_deref().unwrap_or(self.path));
                    let index = self.add(kind, &name, node, qualified, Some(owner));
                    self.result.nodes[index].visibility = self
                        .attribute(node, "access")
                        .filter(|access| !access.is_empty())
                        .map(|access| {
                            match access.as_str() {
                                "private" => "private",
                                "package" => "internal",
                                _ => "public",
                            }
                            .into()
                        });
                    self.result.nodes[index].return_type = self.attribute(node, "returntype");
                    next_owner = index;
                    next_class = None;
                    next_declarations = false;
                }
                "cf_selfclose_tag" => {
                    let end = (node.start_byte() + 64).min(self.source.len());
                    let mut boundary = end;
                    while !self.source.is_char_boundary(boundary) {
                        boundary -= 1;
                    }
                    if captures(
                        r"(?i)^<cf([A-Za-z_]\w*)",
                        &self.source[node.start_byte()..boundary],
                    )?
                    .first()
                    .is_some_and(|m| m[1].eq_ignore_ascii_case("include"))
                    {
                        self.include(node, owner);
                    }
                }
                "cf_tag" => {
                    let start = self.start_tag(node).unwrap_or(node);
                    let name = self
                        .start_tag(node)
                        .and_then(|start| {
                            children(start)
                                .into_iter()
                                .find(|child| child.kind() == "cf_tag_name")
                        })
                        .map(|name| self.text(name).to_lowercase());
                    if name.as_deref() == Some("interface") && declarations && next_class.is_none()
                    {
                        let name = self
                            .attribute(start, "name")
                            .unwrap_or_else(|| self.component_name());
                        let index = self.add(
                            "interface",
                            &name,
                            node,
                            format!("{}::{name}", self.path),
                            Some(owner),
                        );
                        self.result.nodes[index].is_exported = Some(true);
                        self.inheritance(index, self.attribute(start, "extends"), "extends", start);
                        next_owner = index;
                        next_class = Some(name);
                        next_declarations = true;
                    } else if name.as_deref() == Some("loop")
                        && let Some(Some(condition)) =
                            self.attribute_value(start, "condition", false)
                    {
                        self.expression(condition, owner);
                    }
                }
                _ => {
                    if EXPRESSION_PARENTS.contains(&parent.as_str())
                        && EXPRESSIONS.contains(&node.kind())
                    {
                        self.expression(node, owner);
                        continue;
                    }
                }
            }
            tasks.extend(children(node).into_iter().rev().map(|child| {
                (
                    child,
                    node.kind().into(),
                    next_owner,
                    next_class.clone(),
                    next_declarations,
                )
            }));
        }
        Ok(())
    }
    fn start_tag<'a>(&self, tag: Syntax<'a>) -> Option<Syntax<'a>> {
        children(tag)
            .into_iter()
            .find(|child| matches!(child.kind(), "cf_start_tag" | "cf_start_tag_with_selfclose"))
    }
    fn attribute(&self, tag: Syntax<'_>, name: &str) -> Option<String> {
        self.attribute_value(tag, name, false)
            .map(|value| value.map_or_else(String::new, |value| self.text(value).into()))
    }
    fn attribute_value<'a>(
        &self,
        tag: Syntax<'a>,
        name: &str,
        whole: bool,
    ) -> Option<Option<Syntax<'a>>> {
        let mut attributes = Vec::new();
        for child in children(tag) {
            if child.kind() == "cf_attribute" {
                attributes.push(child);
            } else if child.kind() == "cf_tag_attributes" {
                attributes.extend(
                    children(child)
                        .into_iter()
                        .filter(|child| child.kind() == "cf_attribute"),
                );
            }
        }
        for attribute in attributes {
            let parts = children(attribute);
            let Some(label) = parts
                .iter()
                .find(|child| child.kind() == "cf_attribute_name")
            else {
                continue;
            };
            if !self.text(*label).eq_ignore_ascii_case(name) {
                continue;
            }
            let wrapper = parts.iter().find(|child| {
                matches!(
                    child.kind(),
                    "quoted_cf_attribute_value" | "cf_attribute_value"
                )
            });
            if whole && wrapper.is_none_or(|wrapper| wrapper.named_child_count() != 1) {
                return Some(None);
            }
            return Some(wrapper.and_then(|wrapper| {
                children(*wrapper)
                    .into_iter()
                    .find(|child| child.kind() == "attribute_value")
            }));
        }
        None
    }
    fn include(&mut self, tag: Syntax<'_>, owner: usize) {
        let Some(Some(value)) = self.attribute_value(tag, "template", true) else {
            return;
        };
        let template = self.text(value).trim().to_owned();
        if template.is_empty() {
            return;
        }
        let scope = &self.result.nodes[owner];
        let prefix = match scope.kind.as_str() {
            "file" => "",
            "method" => &scope.qualified_name,
            _ => &scope.name,
        };
        let qualified = if prefix.is_empty() {
            template.clone()
        } else {
            format!("{prefix}::{template}")
        };
        let index = self.add("import", &template, tag, qualified, Some(owner));
        self.result.nodes[index].signature = Some(self.text(tag).trim().into());
        self.reference(owner, &template, "imports", tag);
    }
    fn script_tag(&mut self, tag: Syntax<'_>, owner: usize, class: Option<&str>) -> Result<()> {
        let Some(content) = children(tag)
            .into_iter()
            .find(|child| child.kind() == "cf_script_content")
        else {
            return Ok(());
        };
        let (line, column) = self.positions.at(content.start_byte());
        let offset = line - 1;
        let mut result = script::extract(self.path, self.text(content), "cfscript")?;
        super::components::remap_script_ids(&mut result, &self.result, offset, column);
        let inner_file = result
            .nodes
            .iter()
            .find(|node| node.kind == "file")
            .map(|node| node.id.clone());
        let top: HashSet<_> = result
            .edges
            .iter()
            .filter(|edge| edge.kind == "contains" && Some(&edge.source) == inner_file.as_ref())
            .map(|edge| edge.target.clone())
            .collect();
        let locals: HashSet<_> = result
            .nodes
            .iter()
            .filter(|node| {
                top.contains(&node.id) && matches!(node.kind.as_str(), "variable" | "constant")
            })
            .map(|node| node.id.clone())
            .collect();
        for mut node in result.nodes {
            if node.kind == "file" {
                continue;
            }
            self.columns.push((
                node.start_column + if node.start_line == 1 { column } else { 0 },
                node.end_column + if node.end_line == 1 { column } else { 0 },
            ));
            node.start_line += offset;
            node.end_line += offset;
            node.language = self.language.into();
            if let Some(class) = class {
                if node.kind == "function" && top.contains(&node.id) {
                    node.kind = "method".into();
                }
                node.qualified_name = format!("{class}::{}", node.qualified_name);
            }
            self.contains(self.result.nodes[owner].id.clone(), node.id.clone());
            self.result.nodes.push(node);
        }
        for mut edge in result.edges {
            if Some(&edge.source) == inner_file.as_ref()
                || Some(&edge.target) == inner_file.as_ref()
            {
                continue;
            }
            if let Some(line) = edge.line.as_mut().filter(|line| **line != 0) {
                *line += offset;
            }
            self.result.edges.push(edge);
        }
        for mut reference in result.unresolved_references {
            reference.line += offset;
            reference.file_path = Some(self.path.into());
            reference.language = Some(self.language.into());
            if reference.from_node_id.is_empty()
                || Some(&reference.from_node_id) == inner_file.as_ref()
                || locals.contains(&reference.from_node_id)
            {
                reference.from_node_id = self.result.nodes[owner].id.clone();
            }
            self.result.unresolved_references.push(reference);
        }
        self.block_errors(result.errors, tag, "cfscript", offset);
        Ok(())
    }
    fn query_tag(&mut self, tag: Syntax<'_>, owner: usize) -> Result<()> {
        let Some(content) = children(tag)
            .into_iter()
            .find(|child| child.kind() == "cf_query_content")
        else {
            return Ok(());
        };
        let offset = self.positions.at(content.start_byte()).0 - 1;
        let result = script::extract(self.path, self.text(content), "cfquery")?;
        let inner_file = result
            .nodes
            .iter()
            .find(|node| node.kind == "file")
            .map(|node| node.id.clone());
        for mut reference in result.unresolved_references {
            reference.line += offset;
            reference.file_path = Some(self.path.into());
            reference.language = Some(self.language.into());
            if reference.from_node_id.is_empty()
                || Some(&reference.from_node_id) == inner_file.as_ref()
            {
                reference.from_node_id = self.result.nodes[owner].id.clone();
            }
            self.result.unresolved_references.push(reference);
        }
        self.block_errors(result.errors, tag, "cfquery", offset);
        Ok(())
    }
    fn block_errors(
        &mut self,
        errors: Vec<serde_json::Value>,
        tag: Syntax<'_>,
        name: &str,
        offset: u32,
    ) {
        for mut error in errors {
            if error["severity"] == "warning" && error["code"] == "parse_error" {
                let line = self.positions.at(tag.start_byte()).0;
                self.result.errors.push(serde_json::json!({"message":format!("{}: <{name}> at line {line} has a syntax error — the rest of the file is indexed, but calls inside that block may be missing from the graph",self.path),"line":line,"severity":"warning","code":"partial_parse"}));
                continue;
            }
            if let Some(line) = error["line"].as_u64().filter(|line| *line != 0) {
                error["line"] = (line + u64::from(offset)).into();
            }
            self.result.errors.push(error);
        }
    }
    fn expression(&mut self, node: Syntax<'_>, owner: usize) {
        if node.end_byte() <= node.start_byte() {
            return;
        }
        let (line, column) = self.positions.at(node.start_byte());
        self.expressions.push(Expression {
            start: node.start_byte(),
            end: node.end_byte(),
            row: line - 1,
            column,
            text_column: 0,
            scope: self.result.nodes[owner].id.clone(),
        });
    }
    fn tag_expressions(&mut self) -> Result<()> {
        if self.expressions.is_empty() {
            return Ok(());
        }
        self.expressions.sort_by_key(|expression| expression.start);
        let mut synthetic = String::new();
        let mut pos = 0;
        let mut column = 0;
        for expression in &mut self.expressions {
            if pos > 0 {
                synthetic.push(';');
                column += 1;
            }
            let breaks = self.source[pos.min(expression.start)..expression.start]
                .bytes()
                .filter(|ch| *ch == b'\n')
                .count();
            if breaks > 0 {
                synthetic.extend(std::iter::repeat_n('\n', breaks));
                column = 0;
            }
            expression.text_column = column;
            let text = &self.source[expression.start..expression.end];
            synthetic.push_str(text);
            column = if let Some(at) = text.rfind('\n') {
                utf16_len(&text[at + 1..]) as u32
            } else {
                column + utf16_len(text) as u32
            };
            pos = expression.end;
        }
        let result = script::extract(self.path, &synthetic, "cfscript")?;
        for mut reference in result.unresolved_references {
            let at = self
                .expressions
                .partition_point(|expression| {
                    expression.row < reference.line - 1
                        || expression.row == reference.line - 1
                            && expression.text_column <= reference.column
                })
                .saturating_sub(1);
            let expression = &self.expressions[at];
            if reference.line - 1 == expression.row {
                reference.column = (i64::from(reference.column) + i64::from(expression.column)
                    - i64::from(expression.text_column))
                .try_into()
                .context("invalid CF expression column")?;
            }
            reference.from_node_id = expression.scope.clone();
            reference.file_path = Some(self.path.into());
            reference.language = Some(self.language.into());
            self.result.unresolved_references.push(reference);
        }
        self.result.errors.extend(
            result
                .errors
                .into_iter()
                .filter(|error| error["severity"] == "error"),
        );
        Ok(())
    }
}
