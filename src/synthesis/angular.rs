//! Angular component rendering, template events, bindings, and navigation.
use super::*;
use crate::frameworks::frontend::{
    angular::{AngularContext, AngularTable, components, template},
    href, object, tables,
};
use serde_json::json;
mod members;
pub(super) fn edges(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !crate::frameworks::declared_dependencies(
        ctx.manifests,
        &ctx.sources.keys().copied().collect::<Vec<_>>(),
    )
    .contains("@angular/core")
    {
        return Ok(Vec::new());
    }
    let resolve = |specifier: &str, from: &str, language: &str| {
        Ok((ctx.resolve_module)(specifier, from, language))
    };
    let mut angular = AngularContext::new(ctx.nodes, ctx.sources, &resolve, ctx.manifests);
    angular.auxiliary_sources = Some(ctx.auxiliary_sources);
    let components = components::components(&angular)?;
    if components.is_empty() {
        return Ok(Vec::new());
    }
    let mut selectors: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, component) in components.iter().enumerate() {
        for selector in &component.selectors {
            selectors.entry(selector).or_default().push(index);
        }
    }
    let table = AngularTable::new(angular)?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for component in &components {
        ctx.step()?;
        let Some(template) = &component.template else {
            continue;
        };
        let text = template.text;
        ctx.steps(text.len() / 64 + 1)?;
        let lines = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect::<Vec<_>>();
        let line_at = |at| lines.partition_point(|&start| start <= at) as u32;
        let site_line = |at| {
            if template.inline {
                template.first_line + line_at(at) - 1
            } else {
                component.node.start_line
            }
        };
        let registered = |at| {
            format!(
                "{}:{}",
                template.file,
                template.first_line + line_at(at) - 1
            )
        };
        let mut children = 0;
        for capture in regex(r"<([a-zA-Z][\w-]*)(?=[\s/>])")?.captures_iter(text) {
            if children >= 40 {
                break;
            }
            ctx.step()?;
            let capture = capture?;
            let tag = &capture[1];
            let Some(all) = selectors.get(tag) else {
                continue;
            };
            let child = if all.len() == 1 {
                Some(&components[all[0]])
            } else {
                let root = tables::app_root(component.file)?;
                let near = all
                    .iter()
                    .map(|&index| &components[index])
                    .filter(|child| child.file.starts_with(&root))
                    .collect::<Vec<_>>();
                (near.len() == 1).then(|| near[0])
            };
            let Some(child) = child.filter(|child| child.node.id != component.node.id) else {
                continue;
            };
            let key = format!("{}>{}", component.node.id, child.node.id);
            if !seen.insert(key) {
                continue;
            }
            children += 1;
            let at = capture.get(0).unwrap().start();
            ctx.emit(&mut output,edge(component.node,child.node,"calls",site_line(at),json!({"synthesizedBy":"angular-template","via":tag,"registeredAt":registered(at)})))?;
        }
        let own = table
            .context
            .nodes_in(component.file)
            .iter()
            .copied()
            .filter(|node| {
                matches!(node.kind.as_str(), "method" | "property")
                    && node
                        .qualified_name
                        .starts_with(&format!("{}::", component.node.qualified_name))
            })
            .collect::<Vec<_>>();
        let methods = own
            .iter()
            .copied()
            .filter(|node| node.kind == "method")
            .map(|node| (node.name.as_str(), node))
            .collect::<HashMap<_, _>>();
        let mut linked = HashSet::new();
        let mut handlers = 0;
        for capture in regex(r#"(?<!\[)\(([A-Za-z][\w.-]*)\)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)?
            .captures_iter(text)
        {
            if handlers >= 40 {
                break;
            }
            let capture = capture?;
            let statement = capture.get(2).or_else(|| capture.get(3)).unwrap().as_str();
            let event = format!("({})", &capture[1]);
            let at = capture.get(0).unwrap().start();
            let element = element_at(text, at)?;
            for call in regex(r"(?<![\w$.])(?:this\s*\.\s*)?([A-Za-z_$][\w$]*)\s*\(")?
                .captures_iter(statement)
            {
                ctx.step()?;
                let call = call?;
                let Some(&method) = methods.get(&call[1]) else {
                    continue;
                };
                if !seen.insert(format!("{}>{}>{event}", component.node.id, method.id)) {
                    continue;
                }
                linked.insert(method.id.as_str());
                handlers += 1;
                ctx.emit(&mut output,edge(component.node,method,"calls",site_line(at),json!({"synthesizedBy":"angular-event","via":event,"registeredAt":registered(at),"trigger":{"kind":"prop","name":event,"of":element}})))?;
            }
        }
        let mut named: HashMap<&str, Vec<&Node>> = HashMap::new();
        for node in own {
            named.entry(&node.name).or_default().push(node);
        }
        let source = table.context.read(component.file).unwrap_or("");
        let mut shapes = HashMap::new();
        for use_ in template::member_uses(text)? {
            ctx.step()?;
            let Some(candidates) = named.get(use_.name.as_str()) else {
                continue;
            };
            let Some((target, kind, fn_ref)) =
                members::target(candidates, use_.call, source, &mut shapes)?
            else {
                continue;
            };
            if !linked.insert(target.id.as_str()) {
                continue;
            }
            let mut metadata = json!({"synthesizedBy":"angular-binding","via":use_.via,"registeredAt":registered(use_.at)});
            if fn_ref {
                metadata["fnRef"] = true.into();
            }
            ctx.emit(
                &mut output,
                edge(component.node, target, kind, site_line(use_.at), metadata),
            )?;
        }
        let Some(routes) = table
            .for_file(component.file)
            .filter(|table| !table.exact.is_empty())
        else {
            continue;
        };
        let mut links = 0;
        if !source.is_empty() {
            let body = source
                .split('\n')
                .skip(component.node.start_line.saturating_sub(1) as usize)
                .take((component.node.end_line - component.node.start_line + 1) as usize)
                .collect::<Vec<_>>()
                .join("\n");
            let body_lines = std::iter::once(0)
                .chain(body.match_indices('\n').map(|(at, _)| at + 1))
                .collect::<Vec<_>>();
            for found in regex(r"(?<![\w$.])routerLink\s*:\s*")?.find_iter(&body) {
                if links >= 24 {
                    break;
                }
                let found = found?;
                let Some(value) =
                    field_value(&body, found.end())?.filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let Some(href) = table
                    .context
                    .destination(value, component.file, Some(component.node), 0, false)?
                    .filter(|href| href.path.starts_with('/'))
                else {
                    continue;
                };
                let line = component.node.start_line
                    + body_lines.partition_point(|&start| start <= found.start()) as u32
                    - 1;
                for (dest, href) in tables::destinations(&href, routes)? {
                    if !seen.insert(format!("{}>{}", component.node.id, dest.id)) {
                        continue;
                    }
                    links += 1;
                    ctx.emit(&mut output,edge(component.node,dest,"navigates",line,json!({"synthesizedBy":"angular-router-link","href":href.display,"navMethod":"routerLink","registeredAt":format!("{}:{line}",component.file)})))?;
                }
            }
        }
        for capture in regex(r#"(\[routerLink\]|\brouterLink)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)?
            .captures_iter(text)
        {
            if links >= 24 {
                break;
            }
            let capture = capture?;
            let value = capture.get(2).or_else(|| capture.get(3)).unwrap().as_str();
            let href = if &capture[1] == "[routerLink]" {
                table
                    .context
                    .destination(value, component.file, Some(component.node), 0, false)?
            } else {
                plain_href(value)?
            };
            let Some(href) = href.filter(|href| href.path.starts_with('/')) else {
                continue;
            };
            let at = capture.get(0).unwrap().start();
            for (dest, href) in tables::destinations(&href, routes)? {
                if !seen.insert(format!("{}>{}", component.node.id, dest.id)) {
                    continue;
                }
                links += 1;
                ctx.emit(&mut output,edge(component.node,dest,"navigates",site_line(at),json!({"synthesizedBy":"angular-router-link","href":href.display,"navMethod":"routerLink","registeredAt":registered(at),"template":true})))?;
            }
        }
    }
    Ok(output)
}
fn edge(from: &Node, to: &Node, kind: &str, line: u32, metadata: serde_json::Value) -> Edge {
    Edge {
        source: from.id.clone(),
        target: to.id.clone(),
        kind: kind.into(),
        line: Some(line),
        column: None,
        provenance: Some("heuristic".into()),
        metadata: Some(metadata),
    }
}
fn element_at(text: &str, at: usize) -> Result<Option<String>> {
    let Some(open) = text[..at + 1].rfind('<') else {
        return Ok(None);
    };
    Ok(regex(r"^<([a-zA-Z][\w-]*)")?
        .captures(object::prefix(&text[open..], 64))?
        .map(|capture| capture[1].into()))
}
fn field_value(text: &str, at: usize) -> Result<Option<&str>> {
    let mut i = at;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            let Some(end) = object::skip_string(text, i)? else {
                return Ok(None);
            };
            i = end + 1;
            continue;
        }
        if matches!(ch, b'(' | b'[' | b'{') {
            let Some(end) = object::matching(text, i)? else {
                return Ok(None);
            };
            i = end + 1;
            continue;
        }
        if matches!(ch, b',' | b'}') {
            break;
        }
        i += 1;
    }
    Ok(Some(
        text[at..i].trim_matches(crate::search::query_whitespace),
    ))
}
fn plain_href(value: &str) -> Result<Option<href::Href>> {
    let value = value.trim_matches(crate::search::query_whitespace);
    let holes = regex(r"\{\{[\s\S]*?\}\}")?.replace_all(value, href::HOLE.to_string());
    Ok(if holes.starts_with('/') {
        crate::frameworks::frontend::angular::names_somewhere(href::to_href(&holes))
    } else {
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn angular_template_edges_match_independent_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/angular/projects.json"
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
                .filter(|(path, _)| path.ends_with(".json"))
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect::<BTreeMap<_, _>>();
            let auxiliary = files
                .iter()
                .filter(|(path, _)| path.ends_with(".html"))
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect::<BTreeMap<_, _>>();
            let mut raw = project["nodes"].clone();
            for node in raw.as_array_mut().unwrap() {
                node["updatedAt"] = 0.into();
            }
            let nodes: Vec<Node> = serde_json::from_value(raw).unwrap();
            let resolve = |specifier: &str, from: &str, language: &str| {
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
                found.and_then(|answer| answer["result"].as_str().map(str::to_owned))
            };
            let mut ctx = Context::new(&nodes, &[], &sources, &[]);
            ctx.manifests = &manifests;
            ctx.auxiliary_sources = &auxiliary;
            ctx.resolve_module = &resolve;
            assert_eq!(
                serde_json::to_value(edges(&ctx).unwrap()).unwrap(),
                project["templateEdges"],
                "{}",
                project["id"]
            );
        }
    }
}
