//! Shared typed emission for the additional pinned AST walkers.
//! The upstream createNode, scope, and file-record contract; see ../UPSTREAM.md.
use crate::{Edge, ExtractionResult, Node, NodeIdAllocator, UnresolvedReference};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tree_sitter::Node as SyntaxNode;

pub fn children(node: SyntaxNode<'_>) -> Vec<SyntaxNode<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub fn docstring(node: SyntaxNode<'_>, source: &str) -> Option<String> {
    crate::docstring::preceding_docstring(node, source)
}

pub fn run(
    path: &str,
    source: &str,
    language: &str,
    walk: impl FnOnce(&mut Context<'_>, SyntaxNode<'_>),
) -> Result<ExtractionResult, String> {
    let grammar =
        crate::grammar_for(language).ok_or_else(|| format!("unsupported language: {language}"))?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|error| error.to_string())?;
    let tree = parser
        .parse(source, None)
        .ok_or("parser returned no tree")?;
    crate::stack::run_guarded(|| {
        let mut context = Context::new(path, source, language);
        walk(&mut context, tree.root_node());
        Ok(context.finish(tree.root_node().has_error()))
    })
}

pub struct Context<'a> {
    pub path: &'a str,
    pub source: &'a str,
    pub language: &'a str,
    pub result: ExtractionResult,
    pub scopes: Vec<usize>,
    ids: NodeIdAllocator,
    lines: Vec<usize>,
    units: Option<Vec<u32>>,
    updated_at: u64,
}

impl<'a> Context<'a> {
    pub fn new(path: &'a str, source: &'a str, language: &'a str) -> Self {
        let lines: Vec<_> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        let units = (!source.is_ascii()).then(|| {
            let mut units = Vec::with_capacity(source.len() + 1);
            let mut count = 0;
            for character in source.chars() {
                units.extend(std::iter::repeat_n(count, character.len_utf8()));
                count += character.len_utf16() as u32;
            }
            units.push(count);
            units
        });
        let mut context = Self {
            path,
            source,
            language,
            result: ExtractionResult {
                nodes: Vec::new(),
                edges: Vec::new(),
                unresolved_references: Vec::new(),
                errors: Vec::new(),
                duration_ms: 0.0,
            },
            scopes: vec![0],
            ids: NodeIdAllocator::default(),
            lines,
            units,
            updated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        let mut file = context.node("file", name, format!("file:{path}"), path.into());
        file.end_line = context.lines.len() as u32;
        file.is_exported = Some(false);
        context.result.nodes.push(file);
        context
    }

    fn node(&self, kind: &str, name: &str, id: String, qualified_name: String) -> Node {
        Node {
            id,
            kind: kind.into(),
            name: name.into(),
            qualified_name,
            file_path: self.path.into(),
            language: self.language.into(),
            start_line: 1,
            end_line: 1,
            start_column: 0,
            end_column: 0,
            updated_at: self.updated_at,
            docstring: None,
            signature: None,
            visibility: None,
            is_exported: None,
            is_async: None,
            is_static: None,
            is_abstract: None,
            decorators: None,
            type_parameters: None,
            return_type: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn text(&self, node: SyntaxNode<'_>) -> &'a str {
        &self.source[node.byte_range()]
    }

    pub fn position(&self, offset: usize) -> (u32, u32) {
        let line = self.lines.partition_point(|start| *start <= offset) - 1;
        let start = self.lines[line];
        let column = self
            .units
            .as_ref()
            .map_or((offset - start) as u32, |units| {
                units[offset] - units[start]
            });
        ((line + 1) as u32, column)
    }

    pub fn create_node(
        &mut self,
        kind: &str,
        name: &str,
        syntax: SyntaxNode<'_>,
        extra: impl FnOnce(&mut Node),
    ) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let (start_line, start_column) = self.position(syntax.start_byte());
        let (end_line, end_column) = self.position(syntax.end_byte());
        let id = self
            .ids
            .generate(self.path, kind, name, start_line, start_column);
        let qualified_name = self
            .scopes
            .iter()
            .map(|&index| &self.result.nodes[index])
            .filter(|node| node.kind != "file")
            .map(|node| node.name.as_str())
            .chain(std::iter::once(name))
            .collect::<Vec<_>>()
            .join("::");
        let mut node = self.node(kind, name, id.clone(), qualified_name);
        node.start_line = start_line;
        node.start_column = start_column;
        node.end_line = end_line;
        node.end_column = end_column;
        extra(&mut node);
        let index = self.result.nodes.len();
        self.result.nodes.push(node);
        if let Some(parent) = self.current() {
            self.result.edges.push(Edge {
                source: self.result.nodes[parent].id.clone(),
                target: id,
                kind: "contains".into(),
                line: None,
                column: None,
                provenance: None,
                metadata: None,
            });
        }
        Some(index)
    }

    pub fn current(&self) -> Option<usize> {
        self.scopes.last().copied()
    }

    pub fn push_scope(&mut self, node: usize) {
        self.scopes.push(node);
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn reference(&mut self, from: usize, name: &str, kind: &str, at: SyntaxNode<'_>) {
        let (line, column) = self.position(at.start_byte());
        self.reference_at(from, name, kind, line, column);
    }

    pub fn reference_at(&mut self, from: usize, name: &str, kind: &str, line: u32, column: u32) {
        self.result.unresolved_references.push(UnresolvedReference {
            from_node_id: self.result.nodes[from].id.clone(),
            reference_name: name.into(),
            reference_kind: kind.into(),
            line,
            column,
            file_path: None,
            language: None,
            candidates: None,
        });
    }

    pub fn finish(mut self, has_parse_error: bool) -> ExtractionResult {
        if has_parse_error && self.result.nodes.len() == 1 {
            self.result.errors.push(serde_json::json!({
                "message": format!("{}: parse produced no symbols (tree has errors) — the file is indexed but contributes nothing to the graph", self.path),
                "severity": "warning", "code": "parse_error"
            }));
        }
        self.result
    }
}
