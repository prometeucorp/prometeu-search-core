//! Single-file component extraction from the pinned upstream implementation.
//! SPDX-License-Identifier: MIT. See crates/prometeu-extract/UPSTREAM.md.
use super::formats::{empty, node};
use super::preprocess::{self, captures, replace, test, utf16_len};
use anyhow::{Context, Result, bail};
use prometeu_extract::{Edge, ExtractionResult, Node, UnresolvedReference, node_id};
use std::collections::{HashMap, HashSet};
use std::path::Path;

mod vue_options;
mod vue_template;

pub(super) const SCRIPT: &str = r"<script(\s[^>]*?)?(?:/>|>(?<content>[\s\S]*?)</script\s*>)";
const SCRIPT_STYLE: &str = r"<(script|style)(\s[^>]*?)?(?:/>|>[\s\S]*?</\1\s*>)";
const RUNES: &[&str] = &[
    "$props",
    "$state",
    "$derived",
    "$effect",
    "$bindable",
    "$inspect",
    "$host",
    "$snippet",
];
const VUE_BUILTINS: &[&str] = &[
    "Transition",
    "TransitionGroup",
    "KeepAlive",
    "Suspense",
    "Teleport",
    "Component",
    "Slot",
];

pub(super) fn extract(path: &Path, source: &str, language: &str) -> Result<ExtractionResult> {
    Ok(extract_projected(path, source, language)?.0)
}

/// Canonical records plus source columns for the engine's byte-span projection.
/// Upstream records retain script-local columns on an inline block's first line.
pub(super) fn extract_projected(
    path: &Path,
    source: &str,
    language: &str,
) -> Result<(ExtractionResult, Vec<(u32, u32)>)> {
    if !matches!(language, "svelte" | "vue" | "astro") {
        bail!("unsupported component format: {language}");
    }
    let path = path
        .to_str()
        .context("source path must be UTF-8")?
        .replace('\\', "/");
    let mut result = empty();
    let positions = Positions::new(source);
    let file_id = format!("file:{path}");
    let filename = path.rsplit('/').next().unwrap_or(&path);
    let mut file = node(&path, language, "file", filename, &path, file_id.clone());
    file.end_line = source.split('\n').count() as u32;
    file.is_exported = Some(false);
    result.nodes.push(file);
    let suffix = format!(".{language}");
    let name = filename.strip_suffix(&suffix).unwrap_or(filename);
    let component_id = node_id(&path, "component", name, 1);
    let mut component = node(
        &path,
        language,
        "component",
        name,
        &format!("{path}::{name}"),
        component_id.clone(),
    );
    component.end_line = source.split('\n').count() as u32;
    component.end_column = utf16_len(source.rsplit('\n').next().unwrap_or("")) as u32;
    component.is_exported = Some(true);
    result.nodes.push(component);
    let mut columns = result
        .nodes
        .iter()
        .map(|node| (node.start_column, node.end_column))
        .collect::<Vec<_>>();
    result
        .edges
        .push(edge(&file_id, &component_id, "contains", None, None));
    let extraction = (|| -> Result<()> {
        let frontmatter = if language == "astro" {
            astro_frontmatter(source)
        } else {
            None
        };
        if let Some(block) = &frontmatter {
            process_script(
                &path,
                block,
                language,
                name,
                &component_id,
                &mut result,
                &mut columns,
            )?;
        }
        for m in captures(SCRIPT, source)? {
            let Some(content) = m.name("content") else {
                continue;
            };
            let attrs = m.get(1).map_or("", |m| m.as_str());
            let script_language =
                if language == "astro" || test(r#"lang\s*=\s*["'](ts|typescript)["']"#, attrs)? {
                    "typescript"
                } else if language == "vue" && test(r#"lang\s*=\s*["']tsx["']"#, attrs)? {
                    "tsx"
                } else {
                    "javascript"
                };
            let instance = match language {
                "svelte" => !test(
                    r#"context\s*=\s*["']module["']|(?:^|\s)module(?=[\s=]|$)"#,
                    attrs,
                )?,
                "vue" => test(r"\bsetup\b", attrs)?,
                _ => true,
            };
            let (start_line, start_column) = positions.at(content.start());
            let block = Script {
                content: content.as_str().to_owned(),
                line_offset: start_line - 1,
                column_offset: start_column,
                end_line: 0,
                language: script_language,
                instance,
            };
            process_script(
                &path,
                &block,
                language,
                name,
                &component_id,
                &mut result,
                &mut columns,
            )?;
        }
        let mut covered = covered_ranges(source, &positions)?;
        if let Some(block) = &frontmatter {
            covered.insert(0, (block.line_offset - 1, block.end_line));
        }
        if language == "vue" {
            template_components(
                &path,
                source,
                language,
                &component_id,
                &covered,
                &mut result,
            )?;
            vue_calls(&path, source, name, &component_id, &positions, &mut result)?;
        } else {
            template_expressions(
                &path,
                source,
                language,
                &component_id,
                &covered,
                &mut result,
            )?;
            template_components(
                &path,
                source,
                language,
                &component_id,
                &covered,
                &mut result,
            )?;
        }
        if language == "svelte" {
            result
                .unresolved_references
                .retain(|reference| !RUNES.contains(&reference.reference_name.as_str()));
        }
        Ok(())
    })();
    if let Err(error) = extraction {
        let label = match language {
            "svelte" => "Svelte",
            "vue" => "Vue",
            _ => "Astro",
        };
        let mut error = serde_json::json!({"message":format!("{label} extraction error: {error}"),"severity":"error"});
        if language != "vue" {
            error["code"] = "parse_error".into();
        }
        result.errors.push(error);
    }
    Ok((result, columns))
}

struct Script {
    content: String,
    line_offset: u32,
    column_offset: u32,
    end_line: u32,
    language: &'static str,
    instance: bool,
}
pub(super) struct Positions {
    lines: Vec<(usize, usize)>,
    unicode: Vec<(usize, usize)>,
}
impl Positions {
    pub(super) fn new(source: &str) -> Self {
        let mut lines = vec![(0, 0)];
        let mut unicode = Vec::new();
        let mut units = 0;
        let mut extra_bytes = 0;
        for (at, ch) in source.char_indices() {
            units += ch.len_utf16();
            if ch == '\n' {
                lines.push((at + 1, units));
            }
            if !ch.is_ascii() {
                extra_bytes += ch.len_utf8() - ch.len_utf16();
                unicode.push((at + ch.len_utf8(), extra_bytes));
            }
        }
        Self { lines, unicode }
    }
    pub(super) fn at(&self, at: usize) -> (u32, u32) {
        let line = self.lines.partition_point(|(start, _)| *start <= at) - 1;
        let extra = self.unicode.partition_point(|(end, _)| *end <= at);
        let extra_bytes = extra
            .checked_sub(1)
            .map_or(0, |index| self.unicode[index].1);
        (
            line as u32 + 1,
            (at - extra_bytes - self.lines[line].1) as u32,
        )
    }
}
fn edge(from: &str, target: &str, kind: &str, line: Option<u32>, column: Option<u32>) -> Edge {
    Edge {
        source: from.into(),
        target: target.into(),
        kind: kind.into(),
        line,
        column,
        provenance: None,
        metadata: None,
    }
}
fn reference(
    result: &mut ExtractionResult,
    path: &str,
    language: &str,
    from: &str,
    name: &str,
    kind: &str,
    position: (u32, u32),
) {
    result.unresolved_references.push(UnresolvedReference {
        from_node_id: from.into(),
        reference_name: name.into(),
        reference_kind: kind.into(),
        line: position.0,
        column: position.1,
        file_path: Some(path.into()),
        language: Some(language.into()),
        candidates: None,
    });
}
fn astro_frontmatter(source: &str) -> Option<Script> {
    let lines: Vec<_> = source.split('\n').collect();
    let open = lines.iter().position(|line| !line.trim().is_empty())?;
    if lines[open].trim() != "---" {
        return None;
    }
    let close = open
        + 1
        + lines[open + 1..]
            .iter()
            .position(|line| line.trim() == "---")?;
    Some(Script {
        content: lines[open + 1..close].join("\n"),
        line_offset: open as u32 + 1,
        column_offset: 0,
        end_line: close as u32,
        language: "typescript",
        instance: true,
    })
}
fn process_script(
    path: &str,
    block: &Script,
    language: &str,
    component_name: &str,
    component_id: &str,
    result: &mut ExtractionResult,
    columns: &mut Vec<(u32, u32)>,
) -> Result<()> {
    let prepared = preprocess::prepare(Path::new(path), &block.content, block.language)?;
    let mut extracted = prometeu_extract::extract_recovering(path, &prepared, block.language)
        .map_err(anyhow::Error::msg)?;
    preprocess::finish(
        Path::new(path),
        &block.content,
        block.language,
        &mut extracted,
    )?;
    if language == "vue" && !block.instance {
        add_options_members(path, block, component_name, &mut extracted)?;
    }
    fold_script(
        path,
        block,
        language,
        component_id,
        extracted,
        result,
        columns,
    );
    Ok(())
}
fn fold_script(
    path: &str,
    block: &Script,
    language: &str,
    component_id: &str,
    extracted: ExtractionResult,
    sink: &mut ExtractionResult,
    columns: &mut Vec<(u32, u32)>,
) {
    let file_id = format!("file:{path}");
    let parented: HashSet<_> = extracted
        .edges
        .iter()
        .filter(|edge| edge.kind == "contains" && edge.source != file_id)
        .map(|edge| edge.target.clone())
        .collect();
    let instance_values: HashSet<_> = extracted
        .nodes
        .iter()
        .filter(|node| {
            block.instance
                && matches!(node.kind.as_str(), "constant" | "variable")
                && !parented.contains(&node.id)
        })
        .map(|node| node.id.clone())
        .collect();
    let runs_as_component = |id: &str| id == file_id || instance_values.contains(id);
    for mut node in extracted.nodes {
        if node.kind == "file" {
            continue;
        }
        columns.push((
            node.start_column
                + if node.start_line == 1 {
                    block.column_offset
                } else {
                    0
                },
            node.end_column
                + if node.end_line == 1 {
                    block.column_offset
                } else {
                    0
                },
        ));
        node.start_line += block.line_offset;
        node.end_line += block.line_offset;
        node.language = language.into();
        if !parented.contains(&node.id) {
            sink.edges
                .push(edge(component_id, &node.id, "contains", None, None));
        }
        sink.nodes.push(node);
    }
    for mut edge in extracted.edges {
        if edge.kind == "contains" && edge.source == file_id {
            continue;
        }
        if let Some(line) = edge.line.as_mut().filter(|line| **line != 0) {
            *line += block.line_offset;
        }
        if block.instance
            && runs_as_component(&edge.source)
            && !matches!(edge.kind.as_str(), "imports" | "contains")
        {
            edge.source = component_id.into();
        }
        sink.edges.push(edge);
    }
    for mut reference in extracted.unresolved_references {
        reference.line += block.line_offset;
        reference.file_path = Some(path.into());
        reference.language = Some(language.into());
        if block.instance
            && runs_as_component(&reference.from_node_id)
            && matches!(
                reference.reference_kind.as_str(),
                "calls" | "references" | "instantiates" | "function_ref"
            )
        {
            reference.from_node_id = component_id.into();
        }
        sink.unresolved_references.push(reference);
    }
    for mut error in extracted.errors {
        if let Some(line) = error
            .get("line")
            .and_then(serde_json::Value::as_u64)
            .filter(|line| *line != 0)
        {
            error["line"] = (line + u64::from(block.line_offset)).into();
        }
        sink.errors.push(error);
    }
}
fn add_options_members(
    path: &str,
    block: &Script,
    owner: &str,
    result: &mut ExtractionResult,
) -> Result<()> {
    let mut created = Vec::new();
    let positions = Positions::new(&block.content);
    for member in vue_options::members(&block.content)? {
        let (start_line, start_column) = positions.at(member.start);
        let (end_line, end_column) = positions.at(member.end);
        let id = node_id(
            path,
            "method",
            &format!("{owner}.{}", member.name),
            start_line + block.line_offset,
        );
        let mut method = node(
            path,
            "vue",
            "method",
            &member.name,
            &format!("{owner}::{}", member.name),
            id,
        );
        method.start_line = start_line;
        method.end_line = end_line;
        method.start_column = start_column;
        method.end_column = end_column;
        created.push(method);
    }
    let member_at = |line: u32| -> Option<&Node> {
        let mut best: Option<&Node> = None;
        for node in &created {
            if node.start_line <= line
                && node.end_line >= line
                && best.is_none_or(|best| node.start_line >= best.start_line)
            {
                best = Some(node);
            }
        }
        best
    };
    let file_node = result
        .nodes
        .iter()
        .find(|node| node.kind == "file")
        .map(|node| node.id.clone());
    let narrower: HashSet<_> = result
        .nodes
        .iter()
        .filter(|node| node.kind != "file")
        .map(|node| node.id.clone())
        .collect();
    let file_level = |id: &str| {
        file_node
            .as_ref()
            .map_or_else(|| !narrower.contains(id), |file| id == file)
    };
    for reference in &mut result.unresolved_references {
        if file_level(&reference.from_node_id)
            && let Some(member) = member_at(reference.line)
        {
            reference.from_node_id = member.id.clone();
        }
    }
    for edge in &mut result.edges {
        if edge.kind != "contains"
            && file_level(&edge.source)
            && let Some(member) = edge.line.filter(|line| *line != 0).and_then(member_at)
        {
            edge.source = member.id.clone();
        }
    }
    result.nodes.extend(created);
    Ok(())
}
fn covered_ranges(source: &str, positions: &Positions) -> Result<Vec<(u32, u32)>> {
    let mut ranges = Vec::new();
    for m in captures(SCRIPT_STYLE, source)? {
        let whole = m.get(0).unwrap();
        if whole.as_str().ends_with("/>") {
            continue;
        }
        ranges.push((
            positions.at(whole.start()).0 - 1,
            positions.at(whole.end()).0 - 1,
        ));
    }
    Ok(ranges)
}
fn template_expressions(
    path: &str,
    source: &str,
    language: &str,
    component_id: &str,
    covered: &[(u32, u32)],
    result: &mut ExtractionResult,
) -> Result<()> {
    let pattern = if language == "svelte" {
        r"\{([^}#/:@][^}]*)\}"
    } else {
        r"\{([^}/][^}]*)\}"
    };
    for (index, line) in source.split('\n').enumerate() {
        if covered
            .iter()
            .any(|(start, end)| index as u32 >= *start && index as u32 <= *end)
        {
            continue;
        }
        let mut expressions = Vec::new();
        for m in captures(pattern, line)? {
            expressions.push((
                m[1].to_owned(),
                utf16_len(&line[..m.get(0).unwrap().start()]),
            ));
        }
        if language == "astro" {
            let without_complete = replace(line, pattern, |_| String::new())?;
            for m in captures(r"\{([^}/][^}]*)$", &without_complete)? {
                let start = line.rfind('{').unwrap_or(0);
                expressions.push((m[1].to_owned(), utf16_len(&line[..start])));
            }
        }
        for (expression, offset) in expressions {
            for m in captures(r"\b([a-zA-Z_$][\w$.]*)\s*\(", &expression)? {
                let name = &m[1];
                if language == "svelte"
                    && (RUNES.contains(&name) || matches!(name, "if" | "else" | "each" | "await"))
                    || language == "astro" && matches!(name, "if" | "await" | "function")
                {
                    continue;
                }
                let column = offset + utf16_len(&expression[..m.get(0).unwrap().start()]);
                reference(
                    result,
                    path,
                    language,
                    component_id,
                    name,
                    "calls",
                    (index as u32 + 1, column as u32),
                );
            }
        }
    }
    Ok(())
}
fn template_components(
    path: &str,
    source: &str,
    language: &str,
    component_id: &str,
    covered: &[(u32, u32)],
    result: &mut ExtractionResult,
) -> Result<()> {
    let pattern = if language == "vue" {
        r"<([A-Za-z][A-Za-z0-9_-]*)\b"
    } else {
        r"<([A-Z][a-zA-Z0-9_$]*)\b"
    };
    for (index, line) in source.split('\n').enumerate() {
        if covered
            .iter()
            .any(|(start, end)| index as u32 >= *start && index as u32 <= *end)
        {
            continue;
        }
        for m in captures(pattern, line)? {
            let raw = &m[1];
            let name = if language == "vue" && !raw.as_bytes()[0].is_ascii_uppercase() {
                if !raw.contains('-') {
                    continue;
                }
                raw.split('-')
                    .map(|part| {
                        let mut chars = part.chars();
                        chars.next().map_or_else(String::new, |first| {
                            first.to_uppercase().chain(chars).collect()
                        })
                    })
                    .collect::<String>()
            } else {
                raw.to_owned()
            };
            if language == "vue" && VUE_BUILTINS.contains(&name.as_str())
                || language == "astro" && ["Fragment", "Code", "Debug"].contains(&name.as_str())
            {
                continue;
            }
            let column = utf16_len(&line[..m.get(0).unwrap().start()]) + 1;
            reference(
                result,
                path,
                language,
                component_id,
                &name,
                "references",
                (index as u32 + 1, column as u32),
            );
        }
    }
    Ok(())
}
fn vue_calls(
    path: &str,
    source: &str,
    name: &str,
    component_id: &str,
    positions: &Positions,
    result: &mut ExtractionResult,
) -> Result<()> {
    let own: HashMap<_, _> = result
        .nodes
        .iter()
        .filter(|node| {
            node.kind == "method" && node.qualified_name == format!("{name}::{}", node.name)
        })
        .map(|node| (node.name.clone(), node.id.clone()))
        .collect();
    for (name, offset) in vue_template::calls(source)? {
        let (line, column) = positions.at(offset);
        if !name.contains('.')
            && let Some(method) = own.get(&name)
        {
            result.edges.push(edge(
                component_id,
                method,
                "calls",
                Some(line),
                Some(column),
            ));
        } else {
            reference(
                result,
                path,
                "vue",
                component_id,
                &name,
                "calls",
                (line, column),
            );
        }
    }
    Ok(())
}
