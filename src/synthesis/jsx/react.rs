//! React framework bindings, sharing the source-proven JSX export readers.
use super::{components::Components, *};
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Resolution {
    pub target_node_id: String,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}
fn found(node: &Node, confidence: f64, layout: bool) -> Option<Resolution> {
    Some(Resolution {
        target_node_id: node.id.clone(),
        confidence,
        metadata: layout.then(|| json!({"layout":true})),
    })
}
fn react_route(reference: &UnresolvedReference) -> bool {
    matches!(reference.language.as_deref(), Some("tsx" | "jsx"))
        && reference.file_path.as_ref().is_some_and(|path| {
            reference
                .from_node_id
                .starts_with(&format!("route:{path}:"))
        })
}
fn picked(name: &str) -> Result<Option<(String, String)>> {
    Ok(regex(r"^import:(.+)#([A-Za-z_$][\w$]*)$")?
        .captures(name)?
        .map(|m| (m[1].into(), m[2].into())))
}
fn component<'a>(ctx: &Context<'a>, name: &str, file: &str) -> Option<&'a Node> {
    let nodes: Vec<_> = ctx
        .named(name)
        .filter(|node| components::kind(node))
        .collect();
    if let Some((dir, _)) = file.rsplit_once('/')
        && let Some(node) = nodes
            .iter()
            .find(|node| node.file_path.starts_with(&format!("{dir}/")))
    {
        return Some(node);
    }
    if let Some(node) = nodes.iter().find(|node| {
        [
            "/components/",
            "/src/components/",
            "/app/components/",
            "/pages/",
            "/src/pages/",
            "/views/",
            "/src/views/",
        ]
        .iter()
        .any(|part| node.file_path.contains(part))
    }) {
        return Some(node);
    }
    (nodes.len() == 1).then(|| nodes[0])
}
fn module(ctx: &Context<'_>, specifier: &str, file: &str) -> Option<String> {
    let language = components::language(file);
    (ctx.resolve_module)(specifier, file, language).or_else(|| {
        (language != "typescript")
            .then(|| (ctx.resolve_module)(specifier, file, "typescript"))
            .flatten()
    })
}
fn exported<'a>(
    ctx: &Context<'a>,
    components: &mut Components<'a, '_>,
    specifier: &str,
    name: &str,
    file: &str,
    lazy: bool,
) -> Result<Option<&'a Node>> {
    let Some(path) = module(ctx, specifier, file) else {
        return Ok(None);
    };
    let node = components.exported(&path, name)?;
    if node.is_none() && lazy {
        components.exported(&path, "Component")
    } else {
        Ok(node)
    }
}
fn layout<'a>(
    ctx: &Context<'a>,
    components: &mut Components<'a, '_>,
    name: &str,
    reference: &UnresolvedReference,
) -> Result<Option<&'a Node>> {
    let file = reference.file_path.as_deref().unwrap_or("");
    if let Some(specifier) = name.strip_prefix("lazy-import:") {
        return exported(ctx, components, specifier, "default", file, true);
    }
    if react_route(reference)
        && let Some((specifier, member)) = picked(name)?
    {
        return exported(ctx, components, &specifier, &member, file, false);
    }
    let Some(mapping) = ctx
        .file_imports
        .get(file)
        .into_iter()
        .flatten()
        .find(|m| m.local_name == name)
    else {
        return Ok(match components.declared(name, file)? {
            Some(own) => Some(own.component.unwrap_or(own.declaration)),
            None => component(ctx, name, file),
        });
    };
    let mut imported = reference.clone();
    imported.reference_name = name.to_owned();
    if let Some(id) = (ctx.resolve_import)(&imported)?
        && let Some(&index) = ctx.ids.get(id.as_str())
    {
        return Ok(Some(&ctx.nodes[index]));
    }
    if mapping.is_default || mapping.imported_name == "default" {
        return exported(ctx, components, &mapping.source, "default", file, true);
    }
    let Some(path) = (ctx.resolve_module)(
        &mapping.source,
        file,
        reference.language.as_deref().unwrap_or("typescript"),
    ) else {
        return Ok(None);
    };
    Ok(ctx
        .in_file(&path)
        .find(|node| node.name == mapping.imported_name && components::kind(node)))
}
fn resolve<'a>(
    ctx: &Context<'a>,
    components: &mut Components<'a, '_>,
    reference: &UnresolvedReference,
) -> Result<Option<Resolution>> {
    let language = reference.language.as_deref().unwrap_or("");
    if !matches!(language, "javascript" | "typescript" | "tsx" | "jsx") {
        return Ok(None);
    }
    let file = reference.file_path.as_deref().unwrap_or("");
    let name = &reference.reference_name;
    ctx.steps(ctx.named(name).count() + ctx.in_file(file).count() + 1)?;
    if let Some(specifier) = name.strip_prefix("lazy-import:") {
        return Ok(exported(ctx, components, specifier, "default", file, true)?
            .and_then(|n| found(n, 0.9, false)));
    }
    if let Some((specifier, member)) = picked(name)? {
        return Ok(if react_route(reference) {
            exported(ctx, components, &specifier, &member, file, false)?
                .and_then(|n| found(n, 0.9, false))
        } else {
            None
        });
    }
    if let Some(specifier) = name.strip_prefix("layout:") {
        return Ok(layout(ctx, components, specifier, reference)?.and_then(|n| found(n, 0.9, true)));
    }
    if ctx
        .file_imports
        .get(file)
        .into_iter()
        .flatten()
        .any(|m| m.local_name == *name)
    {
        return Ok(None);
    }
    const BUILTIN: &[&str] = &[
        "Array",
        "Boolean",
        "Date",
        "Error",
        "Function",
        "JSON",
        "Math",
        "Number",
        "Object",
        "Promise",
        "RegExp",
        "String",
        "Symbol",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "React",
        "Component",
        "Fragment",
        "Suspense",
        "StrictMode",
    ];
    if matches!(language, "tsx" | "jsx")
        && name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && name.bytes().all(|c| c.is_ascii_alphanumeric())
        && !BUILTIN.contains(&name.as_str())
    {
        let own: Vec<_> = ctx
            .in_file(file)
            .filter(|n| {
                n.name == *name
                    && n.qualified_name == *name
                    && !matches!(n.kind.as_str(), "import" | "export" | "file")
            })
            .collect();
        if reference.reference_kind == "implements"
            || reference.reference_kind == "references"
                && !reference.from_node_id.starts_with("route:")
        {
            return Ok(own
                .iter()
                .find(|n| matches!(n.kind.as_str(), "type_alias" | "interface"))
                .filter(|_| {
                    own.iter()
                        .any(|n| !matches!(n.kind.as_str(), "type_alias" | "interface" | "class"))
                })
                .and_then(|n| found(n, 0.8, false)));
        }
        if let Some(own) = components.declared(name, file)? {
            return Ok(own.component.and_then(|n| found(n, 0.9, false)));
        }
        if let Some(node) = own.iter().find(|n| components::kind(n)) {
            return Ok(found(node, 0.8, false));
        }
        if !own.is_empty() {
            return Ok(None);
        }
        if let Some(node) = component(ctx, name, file) {
            return Ok(found(node, 0.8, false));
        }
    }
    if name.starts_with("use") && name.len() > 3 {
        for node in ctx
            .named(name)
            .filter(|node| node.kind == "function" && node.name.starts_with("use"))
        {
            ctx.steps(ctx.in_file(&node.file_path).count())?;
            let nested = ctx.in_file(&node.file_path).any(|other| {
                other.id != node.id
                    && matches!(other.kind.as_str(), "function" | "method")
                    && other.start_line <= node.start_line
                    && other.end_line >= node.end_line
                    && (other.start_line < node.start_line || other.end_line > node.end_line)
            });
            if !nested && node.file_path == file {
                return Ok(found(node, 0.85, false));
            }
        }
    }
    if (name.ends_with("Context") || name.ends_with("Provider"))
        && let Some(node) = ctx.named(name).find(|node| node.file_path == file)
    {
        return Ok(found(node, 0.8, false));
    }
    Ok(None)
}
pub(crate) fn references<'a>(
    nodes: &'a [Node],
    sources: &'a BTreeMap<&'a str, &'a str>,
    references: &[UnresolvedReference],
    imports: &'a [ImportBinding],
    resolve_module: &'a ModuleResolver<'a>,
    resolve_import: &'a ImportResolver<'a>,
) -> Result<Vec<Option<Resolution>>> {
    let mut ctx = Context::new(nodes, &[], sources, &[]);
    ctx.resolve_module = resolve_module;
    ctx.resolve_import = resolve_import;
    for binding in imports {
        ctx.file_imports
            .entry(&binding.file)
            .or_default()
            .push(binding);
        ctx.imports
            .entry((&binding.file, &binding.local_name))
            .or_insert(binding);
    }
    let mut components = Components::new(&ctx);
    references
        .iter()
        .map(|reference| resolve(&ctx, &mut components, reference))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_reference_binding_answers_match() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/react-bindings.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let mut nodes = case["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = 0.into();
            }
            let mut nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            nodes.sort_by(|a, b| {
                (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
            });
            let sources: BTreeMap<_, _> = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(p, s)| (p.as_str(), s.as_str().unwrap()))
                .collect();
            let imports: Vec<ImportBinding> =
                serde_json::from_value(case["imports"].clone()).unwrap();
            let refs: Vec<UnresolvedReference> =
                serde_json::from_value(case["references"].clone()).unwrap();
            let module = |specifier: &str, from: &str, language: &str| {
                let answer = case["moduleAnswers"].as_array().unwrap().iter().find(|a| {
                    a["specifier"] == specifier && a["from"] == from && a["language"] == language
                });
                assert!(
                    answer.is_some(),
                    "{} missing module answer {specifier} from {from} ({language})",
                    case["id"]
                );
                answer.and_then(|a| a["answer"].as_str().map(str::to_owned))
            };
            let imported = |reference: &UnresolvedReference| {
                let answer = case["importAnswers"].as_array().unwrap().iter().find(|a| {
                    a["reference"]["fromNodeId"] == reference.from_node_id
                        && a["reference"]["referenceName"] == reference.reference_name
                        && a["reference"]["referenceKind"] == reference.reference_kind
                        && a["reference"]["line"] == reference.line
                });
                assert!(
                    answer.is_some(),
                    "{} missing import answer {reference:?}",
                    case["id"]
                );
                Ok(answer.and_then(|a| a["answer"].as_str().map(str::to_owned)))
            };
            let answers =
                references(&nodes, &sources, &refs, &imports, &module, &imported).unwrap();
            for ((reference, actual), expected) in refs
                .iter()
                .zip(answers)
                .zip(case["expected"].as_array().unwrap())
            {
                assert_eq!(
                    serde_json::to_value(actual).unwrap(),
                    *expected,
                    "{} {reference:?}",
                    case["id"]
                );
            }
        }
    }
}
