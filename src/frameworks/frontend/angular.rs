//! Angular route declarations and their committed cross-file context.
use super::{href, object, regex};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, Node, UnresolvedReference, supplemental::Context};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
pub(crate) mod components;
mod context;
mod routing;
pub(crate) use routing::AngularTable;
mod strings;
pub(crate) mod template;
pub(crate) use context::AngularContext;
pub(crate) use strings::{commands_href, names_somewhere, static_string};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ComponentRef {
    pub name: Option<String>,
    pub spec: Option<String>,
}
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Route {
    pub path: String,
    pub line: u32,
    pub component: Option<ComponentRef>,
    pub layouts: Vec<ComponentRef>,
}
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Mount {
    pub prefix: String,
    pub spec: String,
    pub line: u32,
}
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Redirect {
    pub from: String,
    pub to: String,
    pub absolute: bool,
}
#[derive(Default, Debug, Deserialize, Serialize)]
pub(crate) struct Parsed {
    pub routes: Vec<Route>,
    pub mounts: Vec<Mount>,
    pub redirects: Vec<Redirect>,
}
fn trim(value: &str) -> &str {
    value.trim_matches(crate::search::query_whitespace)
}
fn join(parts: &[String]) -> String {
    format!("/{}", parts.join("/"))
}
fn segments(value: &str) -> Vec<String> {
    value
        .split('/')
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect()
}
fn lazy_import(value: &str) -> Result<Option<ComponentRef>> {
    let Some(c) = regex(r#"\bimport\s*\(\s*(['"`])([^'"`]+)\1\s*\)"#)?.captures(value)? else {
        return Ok(None);
    };
    let after = &value[c.get(0).unwrap().end()..];
    let then=regex(r"^\s*\.then\s*\(\s*\(?\s*([A-Za-z_$][\w$]*)\s*\)?\s*=>\s*\(?\s*\1\s*\.\s*([A-Za-z_$][\w$]*)")?.captures(after)?;
    let awaited = regex(r"^\s*\)\s*\.\s*([A-Za-z_$][\w$]*)")?.captures(after)?;
    Ok(Some(ComponentRef {
        spec: Some(c[2].into()),
        name: then
            .map(|m| m[2].into())
            .or_else(|| awaited.map(|m| m[1].into())),
    }))
}
fn component_ref(field: &str, text: &str) -> Result<Option<ComponentRef>> {
    if field == "component" {
        Ok(regex(r"^\s*([A-Z][\w$]*)\s*$")?
            .captures(text)?
            .map(|c| ComponentRef {
                name: Some(c[1].into()),
                spec: None,
            }))
    } else {
        lazy_import(text)
    }
}
struct Walker<'a> {
    source: &'a str,
    context: Context<'a>,
    out: Parsed,
    seen: HashSet<String>,
    walked: HashSet<usize>,
}
impl Walker<'_> {
    fn walk(
        &mut self,
        open: usize,
        close: usize,
        prefix: &[String],
        depth: usize,
        layouts: &[ComponentRef],
    ) -> Result<()> {
        if depth > 12 || !self.walked.insert(open) {
            return Ok(());
        }
        for (start, end) in object::objects(self.source, open + 1, close)? {
            let fields = object::fields(self.source, start, end)?;
            if ![
                "component",
                "loadComponent",
                "children",
                "loadChildren",
                "redirectTo",
            ]
            .iter()
            .any(|key| fields.contains_key(*key))
                || fields.contains_key("matcher")
            {
                continue;
            }
            let path = fields.get("path");
            let redirect = fields.get("redirectTo");
            if let (Some(path), Some(redirect)) = (path, redirect)
                && static_string(path.text)?.as_deref() == Some("**")
            {
                if let Some(target) =
                    static_string(redirect.text)?.filter(|target| !target.contains("**"))
                {
                    let absolute = target.starts_with('/');
                    let mut from = prefix.to_vec();
                    from.push("**".into());
                    let mut to = if absolute {
                        Vec::new()
                    } else {
                        prefix.to_vec()
                    };
                    to.extend(segments(&target));
                    self.out.redirects.push(Redirect {
                        from: join(&from),
                        to: join(&to),
                        absolute,
                    });
                }
                continue;
            }
            let own = path
                .map(|path| strings::path_segments(path.text))
                .transpose()?
                .unwrap_or(Some(Vec::new()));
            let Some(own) = own else { continue };
            let mut parts = prefix.to_vec();
            parts.extend(own);
            let line = self.context.position(path.map_or(start, |path| path.at)).0;
            if let Some(redirect) = redirect {
                if let Some(target) =
                    static_string(redirect.text)?.filter(|target| !target.contains("**"))
                {
                    let absolute = target.starts_with('/');
                    let mut to = if absolute {
                        Vec::new()
                    } else {
                        prefix.to_vec()
                    };
                    to.extend(segments(&target));
                    self.out.redirects.push(Redirect {
                        from: join(&parts),
                        to: join(&to),
                        absolute,
                    });
                }
                continue;
            }
            if let Some(load) = fields.get("loadChildren") {
                if let Some(lazy) = lazy_import(load.text)? {
                    self.out.mounts.push(Mount {
                        prefix: join(&parts),
                        spec: lazy.spec.unwrap(),
                        line,
                    });
                }
                continue;
            }
            let field = if fields.contains_key("component") {
                Some("component")
            } else if fields.contains_key("loadComponent") {
                Some("loadComponent")
            } else {
                None
            };
            let component = field
                .map(|field| component_ref(field, fields[field].text))
                .transpose()?
                .flatten();
            let children = fields.get("children");
            let mut child_claims = false;
            if let Some(children) = children
                && let Some(child_open) = self.source[children.at..]
                    .find('[')
                    .map(|at| at + children.at)
                && let Some(child_close) =
                    object::matching(self.source, child_open)?.filter(|&end| end > child_open)
            {
                for (start, end) in object::objects(self.source, child_open + 1, child_close)? {
                    let fields = object::fields(self.source, start, end)?;
                    let child_path = fields.get("path");
                    if child_path.is_none()
                        || child_path
                            .map(|path| strings::path_segments(path.text))
                            .transpose()?
                            .flatten()
                            .is_some_and(|parts| parts.is_empty())
                    {
                        child_claims = true;
                    }
                }
                let mut next = layouts.to_vec();
                if let Some(component) = &component {
                    next.push(component.clone());
                }
                self.walk(child_open, child_close, &parts, depth + 1, &next)?;
            }
            if field.is_none() || (children.is_some() && child_claims) {
                continue;
            }
            let path = join(&parts);
            if !self.seen.insert(path.clone()) {
                continue;
            }
            self.out.routes.push(Route {
                path,
                line,
                component,
                layouts: layouts.to_vec(),
            });
        }
        Ok(())
    }
}
pub(crate) fn parse(source: &str) -> Result<Parsed> {
    if !regex(r#"['"]@angular/router['"]"#)?.is_match(source)? {
        return Ok(Parsed::default());
    }
    let safe = crate::synthesis::comments::strip_for_regex(source, "typescript");
    let mut walker = Walker {
        source: &safe,
        context: Context::new("", source, "typescript"),
        out: Parsed::default(),
        seen: HashSet::new(),
        walked: HashSet::new(),
    };
    for found in regex(r"(?::\s*(?:Routes|Route\s*\[\s*\])\s*=\s*|\bRouterModule\s*\.\s*for(?:Root|Child)\s*\(\s*|\bprovideRouter\s*\(\s*|\bexport\s+default\s+(?:<\s*Routes\s*>\s*)?|\b(?:const|let)\s+[A-Za-z_$][\w$]*\s*=\s*)\[")?.find_iter(&safe){
        let found=found?;let open=found.end()-1;let Some(close)=object::matching(&safe,open)? else{continue};
        if regex(r"^\b(?:const|let)\b")?.is_match(found.as_str())?&&!regex(r"^\]\s*(?:as|satisfies)\s+Routes\b")?.is_match(object::prefix(&safe[close..],32))?{continue;}
        walker.walk(open,close,&[],0,&[])?;
    }
    Ok(walker.out)
}
pub(crate) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let ctx = Context::new(path, source, "typescript");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !regex(r"\.[cm]?ts$")?.is_match(path)? || path.ends_with(".d.ts") {
        return Ok(out);
    }
    for route in parse(source)?.routes {
        let mut node = ctx.result.nodes[0].clone();
        node.id = format!("route:{path}:{}:{}:angular", route.line, route.path);
        node.kind = "route".into();
        node.name = route.path.clone();
        node.qualified_name = format!("{path}::route:{}", route.path);
        node.is_exported = None;
        node.start_line = route.line;
        node.end_line = route.line;
        for (component, layout) in route
            .component
            .into_iter()
            .map(|component| (component, false))
            .chain(route.layouts.into_iter().map(|layout| (layout, true)))
        {
            let name = if let Some(spec) = component.spec {
                format!(
                    "import:{spec}#{}",
                    component.name.unwrap_or_else(|| "default".into())
                )
            } else {
                component.name.unwrap_or_default()
            };
            out.unresolved_references.push(UnresolvedReference {
                from_node_id: node.id.clone(),
                reference_name: format!("{}{name}", if layout { "layout:" } else { "" }),
                reference_kind: "references".into(),
                line: route.line,
                column: 0,
                file_path: Some(path.into()),
                language: Some("typescript".into()),
                candidates: None,
            });
        }
        out.nodes.push(node);
    }
    Ok(out)
}
pub(crate) fn in_file_path(node: &Node) -> &str {
    node.qualified_name
        .split_once("::route:")
        .map_or(&node.name, |(_, path)| path)
}
pub(crate) fn is_route(node: &Node) -> bool {
    node.kind == "route"
        && node.id.ends_with(":angular")
        && node.id
            == format!(
                "route:{}:{}:{}:angular",
                node.file_path,
                node.start_line,
                in_file_path(node)
            )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ComponentClass {
    pub name: String,
    pub is_default: bool,
    pub decorator_at: usize,
}
pub(crate) fn component_classes(source: &str) -> Result<Vec<ComponentClass>> {
    let safe = crate::synthesis::comments::strip_for_regex(source, "typescript");
    let mut out = Vec::new();
    let mut at = 0;
    let decorator = regex(r"@Component\s*\(")?;
    while let Some(found) = decorator.find_from_pos(&safe, at)? {
        let open = found.end() - 1;
        at = found.end();
        let Some(close) = object::matching(&safe, open)? else {
            continue;
        };
        if let Some(class) =
            regex(r"^\s*(export\s+)?(default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)")?
                .captures(object::prefix(&safe[close + 1..], 399))?
        {
            out.push(ComponentClass {
                name: class[3].into(),
                is_default: class.get(2).is_some(),
                decorator_at: found.start(),
            });
        }
        at = close;
    }
    Ok(out)
}
pub(crate) fn template_path(file: &str, url: &str) -> String {
    let parent = file.rsplit_once('/').map_or("", |(parent, _)| parent);
    let combined = if parent.is_empty() {
        url.to_owned()
    } else {
        format!("{parent}/{url}")
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|part| *part != "..") {
                    parts.pop();
                } else {
                    parts.push(part);
                }
            }
            part => parts.push(part),
        }
    }
    let path = parts.join("/");
    if combined.starts_with('/') {
        format!("/{path}")
    } else if path.is_empty() {
        ".".into()
    } else {
        path
    }
}
/// Explicit non-indexed inputs used by component template synthesis.
pub(crate) fn template_targets(file: &str, source: &str, nodes: &[Node]) -> Result<Vec<String>> {
    if !components::eligible(file, source)? {
        return Ok(Vec::new());
    }
    Ok(components::definitions(source)?
        .into_iter()
        .filter(|definition| {
            nodes
                .iter()
                .any(|node| node.kind == "class" && node.name == definition.name)
        })
        .filter_map(|definition| definition.url.map(|url| template_path(file, &url)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn angular_source_records_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/angular/cases.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let parsed = parse(source).unwrap();
            assert_eq!(
                serde_json::to_value(parsed).unwrap(),
                case["parsed"],
                "{source}"
            );
            let classes = component_classes(source).unwrap();
            let mut classes = serde_json::to_value(classes).unwrap();
            for class in classes.as_array_mut().unwrap() {
                let at = class["decoratorAt"].as_u64().unwrap() as usize;
                class["decoratorAt"] = source[..at].encode_utf16().count().into();
            }
            assert_eq!(classes, case["classes"], "{source}");
            let out = extract(case["path"].as_str().unwrap(), source).unwrap();
            let mut nodes = serde_json::to_value(out.nodes).unwrap();
            for node in nodes.as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(nodes, case["expected"]["nodes"], "{source}");
            assert_eq!(
                serde_json::to_value(out.unresolved_references).unwrap(),
                case["expected"]["references"],
                "{source}"
            );
        }
        for case in corpus["values"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            assert_eq!(
                serde_json::to_value(static_string(text).unwrap()).unwrap(),
                case["static"],
                "{text}"
            );
            assert_eq!(
                serde_json::to_value(strings::localize_default(text).unwrap()).unwrap(),
                case["localize"],
                "{text}"
            );
        }
        for case in corpus["commands"].as_array().unwrap() {
            assert_eq!(
                serde_json::to_value(
                    commands_href(
                        case["text"].as_str().unwrap(),
                        case["fromRoot"].as_bool().unwrap()
                    )
                    .unwrap()
                )
                .unwrap(),
                case["expected"],
                "{case}"
            );
        }
    }
}

#[cfg(test)]
mod project_tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    #[test]
    fn angular_mounts_constants_tables_and_references_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/angular/projects.json"
        ))
        .unwrap();
        for project in corpus["projects"].as_array().unwrap() {
            let files: BTreeMap<String, String> =
                serde_json::from_value(project["files"].clone()).unwrap();
            let sources = files
                .iter()
                .filter(|(path, _)| path.ends_with(".ts"))
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect::<BTreeMap<_, _>>();
            let manifests = files
                .iter()
                .filter(|(path, _)| !path.ends_with(".ts"))
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect::<BTreeMap<_, _>>();
            let mut raw = project["nodes"].clone();
            for node in raw.as_array_mut().unwrap() {
                node["updatedAt"] = 0.into();
            }
            let nodes: Vec<Node> = serde_json::from_value(raw).unwrap();
            let resolve = |specifier: &str, from: &str, language: &str| -> Result<Option<String>> {
                let found = project["moduleAnswers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|answer| {
                        answer["specifier"] == specifier
                            && answer["from"] == from
                            && answer["language"] == language
                    });
                assert!(
                    found.is_some(),
                    "{} missing module answer {specifier} from {from}",
                    project["id"]
                );
                Ok(found.and_then(|answer| answer["result"].as_str().map(str::to_owned)))
            };
            let mut original = nodes.clone();
            for node in original.iter_mut().filter(|node| is_route(node)) {
                node.name = in_file_path(node).into();
            }
            let context = AngularContext::new(&original, &sources, &resolve, &manifests);
            let updates = context.post_extract().unwrap();
            for expected in nodes.iter().filter(|node| is_route(node)) {
                let actual = updates
                    .iter()
                    .find(|node| node.id == expected.id)
                    .or_else(|| original.iter().find(|node| node.id == expected.id))
                    .unwrap();
                assert_eq!(
                    actual.name, expected.name,
                    "{} {}",
                    project["id"], expected.id
                );
            }
            let context = AngularContext::new(&nodes, &sources, &resolve, &manifests);
            for case in project["constants"].as_array().unwrap() {
                let actual = context
                    .constant_text(
                        case["expression"].as_str().unwrap(),
                        case["file"].as_str().unwrap(),
                    )
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(actual).unwrap(),
                    case["expected"],
                    "{} {case}",
                    project["id"]
                );
            }
            let table = AngularTable::new(context).unwrap();
            let actual = table
                .routes
                .iter()
                .map(|(root, table)| {
                    (
                        root.clone(),
                        table
                            .exact
                            .iter()
                            .map(|(path, node)| (path.clone(), node.id.clone()))
                            .collect::<BTreeMap<_, _>>(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                project["table"],
                "{}",
                project["id"]
            );
            for case in project["resolutions"].as_array().unwrap() {
                let reference: UnresolvedReference =
                    serde_json::from_value(case["reference"].clone()).unwrap();
                let name = &reference.reference_name;
                let file = reference.file_path.as_deref().unwrap();
                let target = if reference.from_node_id.ends_with(":angular")
                    && (name.starts_with("layout:") || name.starts_with("import:"))
                {
                    table.context.component(name.strip_prefix("layout:").unwrap_or(name),file).unwrap().map(|node|json!({"target":node.id,"kind":null,"metadata":if name.starts_with("layout:"){json!({"layout":true})}else{Value::Null},"confidence":0.95,"alsoTargets":[]}))
                } else {
                    let lines = href::lines(&files[file]);
                    let hits = table.resolve(&reference, &lines).unwrap();
                    hits.first().map(|first|json!({"target":first.node.id,"kind":"navigates","metadata":first.metadata,"confidence":0.95,"alsoTargets":hits.iter().skip(1).map(|hit|json!({"targetNodeId":hit.node.id,"metadata":hit.metadata})).collect::<Vec<_>>()}))
                };
                assert_eq!(
                    serde_json::to_value(target).unwrap(),
                    case["expected"],
                    "{} {}",
                    project["id"],
                    case["reference"]
                );
            }
        }
    }
}
