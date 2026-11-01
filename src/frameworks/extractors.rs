//! Framework-specific canonical declarations, preserving the pinned source contract.
use anyhow::Result;
use prometeu_extract::{Node, supplemental::Context};
use std::collections::HashSet;

pub(crate) fn regex(pattern: &str) -> Result<fancy_regex::Regex> {
    Ok(fancy_regex::Regex::new(
        &crate::parser::javascript_regex::pattern(pattern),
    )?)
}
fn node(template: &Node, kind: &str, name: &str, id: String, qualified: String, line: u32) -> Node {
    let mut node = template.clone();
    node.id = id;
    node.kind = kind.into();
    node.name = name.into();
    node.qualified_name = qualified;
    node.start_line = line;
    node.end_line = line;
    node.start_column = 0;
    node.end_column = name.encode_utf16().count() as u32;
    node.is_exported = Some(true);
    node
}

const EXPO_DECL: &str = r#"\b(Function|AsyncFunction|Property|Constants)\s*(?:<[^(]*>)?\s*\(\s*["']([A-Za-z_][A-Za-z0-9_]*)["']"#;
const EXPO_CLASS: &str = r"\bclass\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*Module\b";
pub(super) fn is_expo(source: &str) -> Result<bool> {
    Ok(regex(EXPO_CLASS)?.is_match(source)? && regex(EXPO_DECL)?.is_match(source)?)
}

pub(super) fn expo(path: &str, source: &str) -> Result<Vec<Node>> {
    if !is_expo(source)? {
        return Ok(Vec::new());
    }
    let class = regex(EXPO_CLASS)?;
    let class = class.captures(source)?.unwrap();
    let names = regex(r#"\bName\s*\(\s*["']([A-Za-z_][A-Za-z0-9_]*)["']"#)?;
    let name = names.captures(source)?;
    let module = name.as_ref().map_or(&class[1], |name| &name[1]);
    let language = if path.ends_with(".kt") {
        "kotlin"
    } else {
        "swift"
    };
    let positions = Context::new(path, source, language);
    let template = &positions.result.nodes[0];
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();
    for matched in regex(EXPO_DECL)?.captures_iter(source) {
        let matched = matched?;
        let kind = &matched[1];
        let name = &matched[2];
        let (line, column) = positions.position(matched.get(0).unwrap().start());
        if !seen.insert((name.to_owned(), line)) {
            continue;
        }
        let mut node = node(
            template,
            "method",
            name,
            format!("expo-module:{path}:{module}:{name}:{line}"),
            format!("{path}::{module}.{name}"),
            line,
        );
        node.start_column = column;
        node.end_column = column + kind.len() as u32 + 2 + name.len() as u32 + 2;
        node.docstring = Some(format!("Expo Modules {kind}(\"{name}\") in {module}"));
        node.signature = Some(format!("{kind}(\"{name}\")"));
        nodes.push(node);
    }
    Ok(nodes)
}

fn component_name(class: &str) -> &str {
    let class = class.strip_prefix("RCT").unwrap_or(class);
    class
        .strip_suffix("ViewManager")
        .or_else(|| class.strip_suffix("Manager"))
        .unwrap_or(class)
}

pub(super) fn fabric(path: &str, source: &str) -> Result<Vec<Node>> {
    if regex(r"\.(?:[cm]?[jt]s|[jt]sx)$")?.is_match(path)? {
        return fabric_script(path, source);
    }
    if path.ends_with(".m") || path.ends_with(".mm") {
        return fabric_manager(path, source, false);
    }
    if path.ends_with(".java") || path.ends_with(".kt") {
        return fabric_manager(path, source, true);
    }
    Ok(Vec::new())
}
fn fabric_manager(path: &str, source: &str, jvm: bool) -> Result<Vec<Node>> {
    if jvm {
        if !source.contains("@ReactProp") {
            return Ok(Vec::new());
        }
    } else if ![
        "RCT_EXPORT_VIEW_PROPERTY",
        "RCT_CUSTOM_VIEW_PROPERTY",
        "RCT_REMAP_VIEW_PROPERTY",
    ]
    .iter()
    .any(|name| source.contains(name))
    {
        return Ok(Vec::new());
    }
    let classes = regex(if jvm {
        r"\bclass\s+([A-Za-z_][A-Za-z0-9_]*)\b"
    } else {
        r"@implementation\s+([A-Za-z_][A-Za-z0-9_]*)"
    })?;
    let Some(class) = classes.captures(source)? else {
        return Ok(Vec::new());
    };
    let class_name = &class[1];
    if !class_name.ends_with("Manager") {
        return Ok(Vec::new());
    }
    let name = component_name(class_name);
    if name.is_empty() {
        return Ok(Vec::new());
    }
    let language = if !jvm {
        "objc"
    } else if path.ends_with(".kt") {
        "kotlin"
    } else {
        "java"
    };
    let positions = Context::new(path, source, language);
    let template = &positions.result.nodes[0];
    let line = positions.position(class.get(0).unwrap().start()).0;
    let mut component = node(
        template,
        "component",
        name,
        format!("fabric-component:{path}:{name}:{line}"),
        format!("{path}::{name}"),
        line,
    );
    component.docstring = Some(if jvm {
        format!("Android view-manager component '{name}' (from class {class_name})")
    } else {
        format!("Legacy Paper ViewManager component '{name}' (from @implementation {class_name})")
    });
    component.signature = Some(if jvm {
        format!("class {class_name} : ViewManager")
    } else {
        format!("RCT_EXPORT_MODULE() // ViewManager: {class_name}")
    });
    let mut nodes = vec![component];
    let mut seen = HashSet::new();
    let props = regex(if jvm {
        r#"@ReactProp\s*\(\s*(?:name\s*=\s*)?"([^"]+)""#
    } else {
        r"\bRCT_(?:EXPORT|CUSTOM|REMAP)_VIEW_PROPERTY\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)"
    })?;
    for matched in props.captures_iter(source) {
        let matched = matched?;
        let prop = &matched[1];
        if !seen.insert(prop.to_owned()) {
            continue;
        }
        let line = positions.position(matched.get(0).unwrap().start()).0;
        let mut property = node(
            template,
            "property",
            prop,
            format!("fabric-prop:{path}:{prop}:{line}"),
            format!("{path}::{name}.{prop}"),
            line,
        );
        property.docstring = Some(if jvm {
            format!("Android @ReactProp prop '{prop}' on {name}")
        } else {
            format!("Legacy Paper view prop '{prop}' on {name}")
        });
        nodes.push(property);
    }
    Ok(nodes)
}
fn fabric_script(path: &str, source: &str) -> Result<Vec<Node>> {
    if !source.contains("codegenNativeComponent") && !source.contains("requireNativeComponent") {
        return Ok(Vec::new());
    }
    let language = if path.ends_with(".tsx") {
        "tsx"
    } else if regex(r"\.[cm]?ts$")?.is_match(path)? {
        "typescript"
    } else if path.ends_with(".jsx") {
        "jsx"
    } else {
        "javascript"
    };
    let positions = Context::new(path, source, language);
    let template = &positions.result.nodes[0];
    let mut nodes = Vec::new();
    for matched in regex(r#"\b(codegenNativeComponent|requireNativeComponent)\s*(?:<[^>]+>)?\s*\(\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]"#)?.captures_iter(source) {
        let matched = matched?; let declaration = &matched[1]; let name = &matched[2];
        let (line,column) = positions.position(matched.get(0).unwrap().start());
        let mut component = node(template,"component",name,format!("fabric-component:{path}:{name}:{line}"),format!("{path}::{name}"),line);
        component.start_column = column; component.end_column = column + declaration.len() as u32;
        component.docstring = Some(if declaration=="codegenNativeComponent" {format!("Fabric/Codegen native component '{name}'")}else{format!("Native component '{name}' (requireNativeComponent)")});
        component.signature = Some(if declaration=="codegenNativeComponent" {format!("codegenNativeComponent<NativeProps>('{name}')")}else{format!("requireNativeComponent('{name}')")});nodes.push(component);
    }
    if let Some(body) =
        regex(r"export\s+interface\s+NativeProps\b[^{]*\{([\s\S]*?)\n\}")?.captures(source)?
    {
        let body = &body[1];
        let start = source.find(body).unwrap();
        for matched in regex(r"(?m)^\s*([A-Za-z_][A-Za-z0-9_]*)\??\s*:")?.captures_iter(body) {
            let matched = matched?;
            let end = matched.get(0).unwrap().end();
            let after = &body[end..];
            let after = after
                .char_indices()
                .scan(0, |units, (at, ch)| {
                    *units += ch.len_utf16();
                    Some((at, *units))
                })
                .find(|(_, units)| *units > 80)
                .map_or(after, |(at, _)| &after[..at]);
            if regex(r"^\s*\(")?.is_match(after)? {
                continue;
            }
            let name = &matched[1];
            let at = source[start..].find(name).map(|at| at + start);
            let line = at.map_or(1, |at| positions.position(at).0);
            let mut property = node(
                template,
                "property",
                name,
                format!("fabric-prop:{path}:{name}:{line}"),
                format!("{path}::NativeProps.{name}"),
                line,
            );
            property.language = if path.ends_with(".tsx") {
                "tsx"
            } else {
                "typescript"
            }
            .into();
            property.docstring = Some(format!("Fabric NativeProps prop '{name}'"));
            nodes.push(property);
        }
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framework_declarations_match_independent_reference() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/frameworks/cases.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let path = case["path"].as_str().unwrap();
            let source = case["source"].as_str().unwrap();
            let mut references = Vec::new();
            let nodes = match case["framework"].as_str().unwrap() {
                "expo-modules" => expo(path, source).unwrap(),
                "fabric-view" => fabric(path, source).unwrap(),
                framework => {
                    let extracted = if framework == "spring" {
                        super::super::spring::extract(path, source).unwrap()
                    } else if framework == "nuxt" {
                        super::super::nuxt::extract(path, source).unwrap()
                    } else if framework == "astro" {
                        super::super::astro::extract(path, source).unwrap()
                    } else if framework == "vapor" {
                        super::super::vapor::extract(path, source).unwrap()
                    } else if framework == "rust" {
                        super::super::rust::extract(path, source).unwrap()
                    } else if framework == "goframe" {
                        super::super::routes::goframe(path, source).unwrap()
                    } else if framework == "go" {
                        super::super::routes::go(path, source).unwrap()
                    } else {
                        super::super::python::extract(path, source, framework).unwrap()
                    };
                    references = extracted.unresolved_references;
                    extracted.nodes
                }
            };
            let mut nodes = serde_json::to_value(nodes).unwrap();
            for node in nodes.as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(nodes, case["expected"]["nodes"], "{}", case["id"]);
            assert_eq!(
                serde_json::to_value(references).unwrap(),
                case["expected"]["references"],
                "{}",
                case["id"]
            );
        }
    }
}
