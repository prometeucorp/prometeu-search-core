//! Liquid and MyBatis extractors ported from the pinned upstream implementation.
//! SPDX-License-Identifier: MIT. Provenance: crates/prometeu-extract/UPSTREAM.md.
use anyhow::{Context, Result, bail};
use fancy_regex::Regex;
use prometeu_extract::{
    Edge, ExtractionResult, Node, NodeIdAllocator, UnresolvedReference, node_id,
};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "formats/json.rs"]
mod json;

// ECMAScript whitespace and word boundaries differ from Rust regex's Unicode classes.
const JS_SPACE: &str = r"[\x09-\x0D\x20\u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";

fn regex(pattern: &str) -> Result<Regex> {
    Ok(Regex::new(
        &super::javascript_regex::pattern(pattern)
            .replace(r"[\s\S]", "(?s:.)")
            .replace(r"\s", JS_SPACE),
    )?)
}

pub(super) fn extract(path: &Path, source: &str, language: &str) -> Result<ExtractionResult> {
    let path = path
        .to_str()
        .context("source path must be UTF-8")?
        .replace('\\', "/");
    match language {
        "liquid" => liquid(&path, source),
        "xml" => mybatis(&path, source),
        "yaml" | "twig" | "properties" => Ok(empty()),
        _ => bail!("unsupported custom format: {language}"),
    }
}

pub(super) fn empty() -> ExtractionResult {
    ExtractionResult {
        nodes: Vec::new(),
        edges: Vec::new(),
        unresolved_references: Vec::new(),
        errors: Vec::new(),
        duration_ms: 0.0,
    }
}

struct Positions {
    lines: Vec<usize>,
    units: Vec<u32>,
}

impl Positions {
    fn new(source: &str) -> Self {
        let mut lines = vec![0];
        let mut units = Vec::with_capacity(source.len() + 1);
        let mut count = 0;
        for (offset, character) in source.char_indices() {
            units.extend(std::iter::repeat_n(count, character.len_utf8()));
            count += character.len_utf16() as u32;
            if character == '\n' {
                lines.push(offset + 1);
            }
        }
        units.push(count);
        Self { lines, units }
    }

    fn at(&self, offset: usize) -> (u32, u32) {
        let line = self.lines.partition_point(|start| *start <= offset) - 1;
        (
            (line + 1) as u32,
            self.units[offset] - self.units[self.lines[line]],
        )
    }
}

pub(super) fn node(
    path: &str,
    language: &str,
    kind: &str,
    name: &str,
    qualified: &str,
    id: String,
) -> Node {
    Node {
        id,
        kind: kind.into(),
        name: name.into(),
        qualified_name: qualified.into(),
        file_path: path.into(),
        language: language.into(),
        start_line: 1,
        end_line: 1,
        start_column: 0,
        end_column: 0,
        updated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
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

fn file(path: &str, language: &str, source: &str, positions: &Positions) -> Node {
    let mut node = node(
        path,
        language,
        "file",
        path.rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(path),
        path,
        node_id(path, "file", path, 1),
    );
    (node.end_line, node.end_column) = positions.at(source.len());
    node
}

fn contains(result: &mut ExtractionResult, file: &str, child: Node) {
    result.edges.push(Edge {
        source: file.into(),
        target: child.id.clone(),
        kind: "contains".into(),
        line: None,
        column: None,
        provenance: None,
        metadata: None,
    });
    result.nodes.push(child);
}

fn reference(result: &mut ExtractionResult, from: &str, name: String, line: u32, column: u32) {
    result.unresolved_references.push(UnresolvedReference {
        from_node_id: from.into(),
        reference_name: name,
        reference_kind: "references".into(),
        line,
        column,
        file_path: None,
        language: None,
        candidates: None,
    });
}

fn attribute(source: &str, name: &str) -> Result<Option<String>> {
    let pattern = regex(&format!(r#"\b{name}\s*=\s*(["'])([^"']+)\1"#))?;
    Ok(pattern
        .captures(source)?
        .and_then(|captures| captures.get(2).map(|value| value.as_str().to_owned())))
}

fn strip_xml_comments(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut offset = 0;
    while offset < source.len() {
        if source[offset..].starts_with("<![CDATA[") {
            let end = source[offset + 9..]
                .find("]]>")
                .map_or(source.len(), |end| offset + 9 + end + 3);
            result.push_str(&source[offset..end]);
            offset = end;
        } else if source[offset..].starts_with("<!--") {
            let end = source[offset + 4..]
                .find("-->")
                .map_or(source.len(), |end| offset + 4 + end + 3);
            for character in source[offset..end].chars() {
                if character == '\n' {
                    result.push('\n');
                } else {
                    result.extend(std::iter::repeat_n(' ', character.len_utf16()));
                }
            }
            offset = end;
        } else {
            let character = source[offset..].chars().next().unwrap();
            result.push(character);
            offset += character.len_utf8();
        }
    }
    result
}

fn mybatis(path: &str, source: &str) -> Result<ExtractionResult> {
    let source = strip_xml_comments(source);
    let positions = Positions::new(&source);
    let mut result = empty();
    let file = file(path, "xml", &source, &positions);
    let file_id = file.id.clone();
    result.nodes.push(file);
    let mut root = None;
    for (tag, namespace_required) in [("mapper", true), ("sqlMap", false)] {
        if let Some(captures) = regex(&format!(r"<{tag}\b([^>]*)>"))?.captures(&source)? {
            let namespace = attribute(captures.get(1).unwrap().as_str(), "namespace")?;
            if namespace_required && namespace.is_none() {
                continue;
            }
            let start = captures.get(0).unwrap().end();
            let end = source[start..]
                .find(&format!("</{tag}>"))
                .map_or(source.len(), |offset| start + offset);
            root = Some((namespace.unwrap_or_default(), tag == "sqlMap", start, end));
            break;
        }
    }
    let Some((namespace, ibatis, start, end)) = root else {
        return Ok(result);
    };
    let verbs = if ibatis {
        "select|insert|update|delete|sql|statement|procedure"
    } else {
        "select|insert|update|delete|sql"
    };
    let statements = regex(&format!(r"<({verbs})\b([^>]*)>([\s\S]*?)</\1\s*>"))?;
    let includes = regex(r#"<include\b[^>]*\brefid\s*=\s*(["'])([^"']+)\1"#)?;
    let tags = regex(r"<[^>]+>")?;
    let spaces = regex(r"\s+")?;
    for captures in statements.captures_iter(&source[start..end]) {
        let captures = captures?;
        let whole = captures.get(0).unwrap();
        let verb = captures.get(1).unwrap().as_str();
        let attributes = captures.get(2).unwrap().as_str();
        let body = captures.get(3).unwrap();
        let Some(id) = attribute(attributes, "id")? else {
            continue;
        };
        let absolute = start + whole.start();
        let (qualified, name) = if !namespace.is_empty() {
            (format!("{namespace}::{id}"), id.as_str())
        } else if let Some((prefix, name)) = id.rsplit_once('.') {
            (format!("{prefix}::{name}"), name)
        } else {
            (id.clone(), id.as_str())
        };
        let id = node_id(path, "method", &qualified, positions.units[absolute]);
        let mut statement = node(path, "xml", "method", name, &qualified, id.clone());
        statement.start_line = positions.at(absolute).0;
        statement.end_line = positions.at(start + whole.end()).0;
        let mut signature = vec![verb.to_ascii_uppercase()];
        for (key, label) in [
            ("parameterType", "param"),
            ("resultType", "result"),
            ("databaseId", "databaseId"),
        ] {
            if let Some(value) = attribute(attributes, key)? {
                signature.push(format!("{label}={value}"));
            }
        }
        statement.signature = Some(if verb == "sql" {
            "<sql>".into()
        } else {
            signature.join(" ")
        });
        let plain = tags.replace_all(body.as_str(), " ");
        let plain = spaces.replace_all(&plain, " ");
        // JavaScript slice(0, 200) counts UTF-16 code units.
        statement.docstring = Some(String::from_utf16_lossy(
            &plain
                .trim_matches(' ')
                .encode_utf16()
                .take(200)
                .collect::<Vec<_>>(),
        ));
        contains(&mut result, &file_id, statement);
        for included in includes.captures_iter(body.as_str()) {
            let included = included?;
            let name = included.get(2).unwrap().as_str();
            let name = if let Some((prefix, suffix)) = name.rsplit_once('.') {
                format!("{prefix}::{suffix}")
            } else if !namespace.is_empty() {
                format!("{namespace}::{name}")
            } else {
                name.into()
            };
            let offset = start + body.start() + included.get(0).unwrap().start();
            reference(&mut result, &id, name, positions.at(offset).0, 0);
        }
    }
    Ok(result)
}

struct Tag {
    full: String,
    groups: Vec<Option<String>>,
    offset: usize,
}

fn silent_regions(source: &str) -> Result<Vec<(usize, usize)>> {
    let tags = regex(r"\{%[-]?\s*(\w+|#)([\s\S]*?)[-]?%\}")?;
    let mut regions = Vec::new();
    let mut open: Option<(String, usize)> = None;
    for captures in tags.captures_iter(source) {
        let captures = captures?;
        let tag = captures.get(0).unwrap();
        let name = captures.get(1).unwrap().as_str();
        if let Some((opening, start)) = &open {
            if name == format!("end{opening}") {
                regions.push((*start, tag.end()));
                open = None;
            }
        } else if matches!(name, "comment" | "raw") {
            open = Some((name.into(), tag.start()));
        }
    }
    if let Some((_, start)) = open {
        regions.push((start, source.len()));
    }
    Ok(regions)
}

fn silent_end(regions: &[(usize, usize)], index: usize) -> Option<usize> {
    regions
        .iter()
        .find_map(|&(start, end)| (index >= start && index < end).then_some(end))
}

fn occurrences(
    source: &str,
    regions: &[(usize, usize)],
    tag: &str,
    argument: &str,
) -> Result<Vec<Tag>> {
    let tags = regex(r"\{%[-]?\s*(\w+|#)([\s\S]*?)[-]?%\}")?;
    let braced = regex(&format!(r"^\{{%[-]?\s*({tag})\s+{argument}"))?;
    let bare = regex(&format!(r"^[ \t]*({tag})[ \t]+{argument}"))?;
    let line_name = regex(r"^[ \t]*(\w+)")?;
    let mut found = Vec::new();
    let mut blocks = Vec::new();
    let mut cursor = 0;
    while let Some(captures) = tags.captures_from_pos(source, cursor)? {
        let whole = captures.get(0).unwrap();
        cursor = whole.end();
        if let Some(end) = silent_end(regions, whole.start()) {
            cursor = end;
            continue;
        }
        if captures.get(1).unwrap().as_str() == "liquid" {
            let body = captures.get(2).unwrap();
            blocks.push((body.start(), body.as_str()));
            continue;
        }
        if let Some(matched) = braced.captures(whole.as_str())? {
            found.push(Tag {
                full: matched.get(0).unwrap().as_str().into(),
                groups: (1..matched.len())
                    .map(|index| matched.get(index).map(|m| m.as_str().to_owned()))
                    .collect(),
                offset: whole.start(),
            });
        }
    }
    for (mut offset, body) in blocks {
        let mut suppressed: Option<String> = None;
        for line in body.split('\n') {
            let name = line_name
                .captures(line)?
                .and_then(|captures| captures.get(1).map(|name| name.as_str()));
            if let Some(open) = &suppressed {
                if name == Some(format!("end{open}").as_str()) {
                    suppressed = None;
                }
            } else if matches!(name, Some("comment" | "raw")) {
                suppressed = name.map(str::to_owned);
            } else if let Some(matched) = bare.captures(line)? {
                let full = matched.get(0).unwrap().as_str();
                let trimmed = full.trim_start_matches([' ', '\t']);
                found.push(Tag {
                    full: trimmed.into(),
                    groups: (1..matched.len())
                        .map(|index| matched.get(index).map(|m| m.as_str().to_owned()))
                        .collect(),
                    offset: offset + full.len() - trimmed.len(),
                });
            }
            offset += line.len() + 1;
        }
    }
    found.sort_by_key(|tag| tag.offset);
    Ok(found)
}

fn liquid(path: &str, source: &str) -> Result<ExtractionResult> {
    let positions = Positions::new(source);
    let mut result = empty();
    let file = file(path, "liquid", source, &positions);
    let file_id = file.id.clone();
    result.nodes.push(file);
    if path.ends_with(".json") {
        if let Some(tree) = json::parse(source, true)
            && let Some(sections) = tree
                .property(0, "sections", false)
                .filter(|&index| matches!(tree.nodes[index].kind, json::Kind::Object))
        {
            let mut seen = HashSet::new();
            for (_, section) in &tree.nodes[sections].children {
                if let Some(kind) = tree.property(*section, "type", false)
                    && let Some(name) = tree.nodes[kind].string()
                    && seen.insert(name)
                {
                    let (line, column) = positions.at(tree.nodes[kind].offset);
                    reference(
                        &mut result,
                        &file_id,
                        format!("sections/{name}.liquid"),
                        line,
                        column,
                    );
                }
            }
        }
        return Ok(result);
    }
    let regions = silent_regions(source)?;
    let mut ids = NodeIdAllocator::default();
    for (pattern, directory) in [("render|include", "snippets"), ("sections?", "sections")] {
        for tag in occurrences(source, &regions, pattern, r#"['"]([^'"]+)['"]"#)? {
            let kind = tag.groups[0].as_deref().unwrap();
            let name = tag.groups[1].as_deref().unwrap();
            let (line, column) = positions.at(tag.offset);
            let end = column + tag.full.encode_utf16().count() as u32;
            let mut import = node(
                path,
                "liquid",
                "import",
                name,
                &format!("{path}::import:{name}"),
                ids.generate(path, "import", name, line, column),
            );
            (
                import.start_line,
                import.end_line,
                import.start_column,
                import.end_column,
            ) = (line, line, column, end);
            import.signature = Some(tag.full.clone());
            contains(&mut result, &file_id, import);
            let mut component = node(
                path,
                "liquid",
                "component",
                name,
                &format!("{path}::{kind}:{name}"),
                ids.generate(path, "component", &format!("{kind}:{name}"), line, column),
            );
            (
                component.start_line,
                component.end_line,
                component.start_column,
                component.end_column,
            ) = (line, line, column, end);
            contains(&mut result, &file_id, component);
            let extension = if kind == "sections" { "json" } else { "liquid" };
            reference(
                &mut result,
                &file_id,
                format!("{directory}/{name}.{extension}"),
                line,
                column,
            );
        }
    }
    let schemas = regex(r"\{%[-]?\s*schema\s*[-]?%\}([\s\S]*?)\{%[-]?\s*endschema\s*[-]?%\}")?;
    let mut cursor = 0;
    while let Some(schema) = schemas.captures_from_pos(source, cursor)? {
        let whole = schema.get(0).unwrap();
        cursor = whole.end();
        if let Some(end) = silent_end(&regions, whole.start()) {
            cursor = end;
            continue;
        }
        let name = json::schema_name(schema.get(1).unwrap().as_str());
        let (line, column) = positions.at(whole.start());
        let mut schema = node(
            path,
            "liquid",
            "constant",
            &name,
            &format!("{path}::schema:{name}"),
            ids.generate(path, "constant", &format!("schema:{name}"), line, column),
        );
        schema.start_line = line;
        schema.end_line = positions.at(whole.end()).0;
        schema.start_column = column;
        contains(&mut result, &file_id, schema);
    }
    for (kind, argument) in [
        ("assign", r"(\w[\w-]*)\s*="),
        (
            "capture",
            r#"(?:'(\w[\w-]*)'|"(\w[\w-]*)"|(\w[\w-]*?))(?=\s|-?%\}|$)"#,
        ),
    ] {
        for tag in occurrences(source, &regions, kind, argument)? {
            let name = tag
                .groups
                .iter()
                .skip(1)
                .find_map(|name| name.as_deref())
                .unwrap();
            let (line, column) = positions.at(tag.offset);
            let mut variable = node(
                path,
                "liquid",
                "variable",
                name,
                &format!("{path}::{name}"),
                ids.generate(path, "variable", name, line, column),
            );
            (
                variable.start_line,
                variable.end_line,
                variable.start_column,
                variable.end_column,
            ) = (
                line,
                line,
                column,
                column + tag.full.encode_utf16().count() as u32,
            );
            contains(&mut result, &file_id, variable);
        }
    }
    Ok(result)
}
