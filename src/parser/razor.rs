//! Razor and Blazor extraction ported from the pinned upstream implementation.
//! SPDX-License-Identifier: MIT. Provenance: crates/prometeu-extract/UPSTREAM.md.
use super::components::{Positions, SCRIPT};
use super::formats::{empty, node};
use super::preprocess::{captures, test, utf16_len};
use anyhow::{Context, Result};
use prometeu_extract::{ExtractionResult, UnresolvedReference, node_id};
use std::collections::HashSet;
use std::path::Path;

mod script;

const BUILTINS: &[&str] = &[
    "Router",
    "Found",
    "NotFound",
    "RouteView",
    "AuthorizeRouteView",
    "LayoutView",
    "CascadingValue",
    "CascadingAuthenticationState",
    "AuthorizeView",
    "Authorized",
    "NotAuthorized",
    "Authorizing",
    "EditForm",
    "DataAnnotationsValidator",
    "ValidationSummary",
    "ValidationMessage",
    "InputText",
    "InputNumber",
    "InputCheckbox",
    "InputSelect",
    "InputDate",
    "InputTextArea",
    "InputRadio",
    "InputRadioGroup",
    "InputFile",
    "PageTitle",
    "HeadContent",
    "HeadOutlet",
    "Virtualize",
    "DynamicComponent",
    "ErrorBoundary",
    "SectionContent",
    "SectionOutlet",
    "FocusOnNavigate",
    "NavLink",
    "Microsoft",
];

pub(super) fn extract(path: &Path, source: &str) -> Result<ExtractionResult> {
    let path = path
        .to_str()
        .context("source path must be UTF-8")?
        .replace('\\', "/");
    let filename = path.rsplit('/').next().unwrap_or(&path);
    let name = if filename.to_ascii_lowercase().ends_with(".razor") {
        &filename[..filename.len() - 6]
    } else if filename.to_ascii_lowercase().ends_with(".cshtml") {
        &filename[..filename.len() - 7]
    } else {
        filename
    };
    let mut result = empty();
    let id = node_id(&path, "component", name, 1);
    let mut component = node(
        &path,
        "razor",
        "component",
        name,
        &format!("{path}::{name}"),
        id.clone(),
    );
    component.end_line = source.split('\n').count() as u32;
    component.end_column = utf16_len(source.rsplit('\n').next().unwrap_or("")) as u32;
    component.is_exported = Some(true);
    result.nodes.push(component);
    let extracted = (|| -> Result<()> {
        directives(&path, source, &id, &mut result)?;
        if path.to_ascii_lowercase().ends_with(".razor") {
            component_tags(&path, source, &id, &mut result)?;
        }
        code_blocks(&path, source, &id, &mut result)?;
        script_blocks(&path, source, &id, &mut result)?;
        Ok(())
    })();
    if let Err(error) = extracted {
        result.errors.push(serde_json::json!({"message":format!("Razor extraction error: {error}"),"severity":"error","code":"parse_error"}));
    }
    Ok(result)
}
fn reference(
    path: &str,
    id: &str,
    name: &str,
    line: u32,
    column: u32,
    result: &mut ExtractionResult,
) {
    result.unresolved_references.push(UnresolvedReference {
        from_node_id: id.into(),
        reference_name: name.into(),
        reference_kind: "references".into(),
        line,
        column,
        file_path: Some(path.into()),
        language: Some("razor".into()),
        candidates: None,
    });
}
fn type_names(expression: &str) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for m in captures(r"[^<>,\s]+", expression)? {
        let name = m[0].rsplit('.').next().unwrap();
        if test(r"^[A-Z][A-Za-z0-9_]*$", name)? {
            names.push(name.into());
        }
    }
    Ok(names)
}
fn directives(path: &str, source: &str, id: &str, result: &mut ExtractionResult) -> Result<()> {
    for (i, line) in source.split('\n').enumerate() {
        for pattern in [
            r"^\s*@(?:model|inherits)\s+([A-Za-z_][\w.]*(?:\s*<[^>]+>)?)",
            r"^\s*@inject\s+([A-Za-z_][\w.]*(?:\s*<[^>]+>)?)\s+[A-Za-z_]",
        ] {
            if let Some(m) = captures(pattern, line)?.first() {
                for name in type_names(&m[1])? {
                    reference(path, id, &name, i as u32 + 1, 0, result);
                }
            }
        }
        for m in captures(r"@typeof\(\s*([A-Za-z_][\w.]*)\s*\)", line)? {
            let name = m[1].rsplit('.').next().unwrap();
            if test(r"^[A-Z]", name)? {
                reference(
                    path,
                    id,
                    name,
                    i as u32 + 1,
                    utf16_len(&line[..m.get(0).unwrap().start()]) as u32,
                    result,
                );
            }
        }
    }
    Ok(())
}
fn component_tags(path: &str, source: &str, id: &str, result: &mut ExtractionResult) -> Result<()> {
    for (i, line) in source.split('\n').enumerate() {
        for m in captures(r"<([A-Z][A-Za-z0-9_]*)\b([^>]*)>", line)? {
            if BUILTINS.contains(&&m[1]) {
                continue;
            }
            reference(
                path,
                id,
                &m[1],
                i as u32 + 1,
                utf16_len(&line[..m.get(0).unwrap().start()]) as u32 + 1,
                result,
            );
            for t in captures(r#"\bT[A-Za-z]*\s*=\s*"([A-Za-z_][\w.]*)""#, &m[2])? {
                let name = t[1].rsplit('.').next().unwrap();
                if test(r"^[A-Z]", name)? {
                    reference(path, id, name, i as u32 + 1, 0, result);
                }
            }
        }
    }
    Ok(())
}
fn matching_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = source[i..].find('\n').map_or(bytes.len(), |end| i + end);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = source[i + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| i + end + 3);
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
fn code_blocks(path: &str, source: &str, id: &str, result: &mut ExtractionResult) -> Result<()> {
    let positions = Positions::new(source);
    let mut resume = 0;
    for m in captures(r"@(?:code|functions)\b\s*\{|@\{", source)? {
        let whole = m.get(0).unwrap();
        if whole.start() < resume {
            continue;
        }
        let open = whole.end() - 1;
        let Some(close) = matching_brace(source, open) else {
            continue;
        };
        resume = close;
        let content = &source[open + 1..close];
        if content.trim().is_empty() {
            continue;
        }
        let wrapped = format!("class __RazorCode__ {{\n{content}\n}}");
        let Ok(extracted) = super::native::extract_as(Path::new(path), &wrapped, "csharp") else {
            continue;
        };
        let line_offset = positions.at(open + 1).0 - 1;
        for mut reference in extracted.unresolved_references {
            reference.from_node_id = id.into();
            reference.line = reference.line + line_offset - 1;
            reference.file_path = Some(path.into());
            reference.language = Some("razor".into());
            result.unresolved_references.push(reference);
        }
    }
    Ok(())
}
fn script_blocks(path: &str, source: &str, id: &str, result: &mut ExtractionResult) -> Result<()> {
    let source = script::blank_comments(source)?;
    let positions = Positions::new(&source);
    let mut blocks = Vec::new();
    for m in captures(SCRIPT, &source)? {
        let Some(content) = m
            .name("content")
            .filter(|content| !content.as_str().trim().is_empty())
        else {
            continue;
        };
        let attrs = m.get(1).map_or("", |attrs| attrs.as_str());
        if !script::is_javascript(attrs)? {
            continue;
        }
        let language = if test(r#"lang\s*=\s*["'](ts|typescript)["']"#, attrs)? {
            "typescript"
        } else {
            "javascript"
        };
        let prepared = script::blank_in_script(content.as_str())?;
        if let Ok(extracted) = super::native::extract_as(Path::new(path), &prepared, language) {
            blocks.push((extracted, language, positions.at(content.start()).0 - 1));
        }
    }
    let own: HashSet<_> = blocks
        .iter()
        .flat_map(|(result, _, _)| result.nodes.iter())
        .filter(|node| !matches!(node.kind.as_str(), "file" | "import"))
        .map(|node| node.name.clone())
        .collect();
    for (extracted, language, line_offset) in blocks {
        for mut reference in extracted.unresolved_references {
            if own.contains(reference.reference_name.split('.').next().unwrap()) {
                continue;
            }
            reference.from_node_id = id.into();
            reference.line += line_offset;
            reference.file_path = Some(path.into());
            reference.language = Some(language.into());
            result.unresolved_references.push(reference);
        }
    }
    Ok(())
}
