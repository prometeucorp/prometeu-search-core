//! ASP.NET extraction and repository-wide route-name finalization.
//! SPDX-License-Identifier: MIT. Reference provenance: tests/fixtures/upstream/frameworks/README.md.
use super::{
    extractors::regex,
    routes::{reference, route},
};
use anyhow::{Result, ensure};
use prometeu_extract::{ExtractionResult, Node, supplemental::Context};
use std::collections::{BTreeMap, HashMap};

fn join(prefix: &str, sub: &str) -> String {
    let parts: Vec<_> = [prefix, sub]
        .into_iter()
        .map(|part| part.trim_matches('/'))
        .filter(|part| !part.is_empty())
        .collect();
    format!("/{}", parts.join("/"))
}
fn end_at_units(source: &str, start: usize, units: usize) -> usize {
    let mut count = 0;
    source[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            count += ch.len_utf16();
            (count > units).then_some(start + offset)
        })
        .unwrap_or(source.len())
}
fn declaration(ctx: &Context<'_>, start: usize, end: usize, method: &str, path: &str) -> Node {
    let mut node = route(
        ctx,
        ctx.position(start).0,
        method,
        path,
        ctx.source[start..end].encode_utf16().count() as u32,
    );
    node.qualified_name = format!("{}::route:{path}", node.file_path);
    node
}

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let ctx = Context::new(path, source, "csharp");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".cs") {
        return Ok(out);
    }
    let safe = crate::synthesis::comments::strip_for_regex(source, "csharp");
    let prefix = regex(
        r#"\[Route\s*\(\s*"([^"]+)"[^)]*\)\]\s*(?:\[[^\]]*\]\s*)*(?:public\s+|sealed\s+|abstract\s+|partial\s+)*class\b"#,
    )?;
    let prefix = prefix.captures(&safe)?;
    let prefix = prefix.as_ref().map_or("", |capture| &capture[1]);
    let method_decl =
        regex(r"(?:public|private|protected|internal)\s+[\w<>,\s\[\]?.]+?\s+(\w+)\s*\(")?;
    for matched in regex(
        r#"\[(HttpGet|HttpPost|HttpPut|HttpPatch|HttpDelete)(?:\s*\(\s*"([^"]+)"[^)]*\))?\s*\]"#,
    )?
    .captures_iter(&safe)
    {
        let matched = matched?;
        let text = matched.get(0).unwrap();
        let method = matched[1].strip_prefix("Http").unwrap().to_uppercase();
        let target = join(prefix, matched.get(2).map_or("", |m| m.as_str()));
        let node = declaration(&ctx, text.start(), text.end(), &method, &target);
        let end = end_at_units(source, text.end(), 600);
        if let Some(handler) = method_decl.captures(&safe[text.end()..end])? {
            out.unresolved_references
                .push(reference(&node, &handler[1], "references"));
        }
        out.nodes.push(node);
    }
    let whitespace = regex(r"\s+")?;
    let tail = regex(r"(?:\.|^)([A-Za-z_][A-Za-z0-9_]*)$")?;
    for matched in regex(r#"\.Map(Get|Post|Put|Patch|Delete)\s*\(\s*"([^"]+)"\s*,\s*([^,)]+)"#)?
        .captures_iter(&safe)
    {
        let matched = matched?;
        let text = matched.get(0).unwrap();
        let node = declaration(
            &ctx,
            text.start(),
            text.end(),
            &matched[1].to_uppercase(),
            &join("", &matched[2]),
        );
        let cleaned =
            whitespace.replace_all(matched[3].trim_matches(crate::search::query_whitespace), "");
        if let Some(handler) = tail.captures(&cleaned)? {
            out.unresolved_references
                .push(reference(&node, &handler[1], "references"));
        }
        out.nodes.push(node);
    }
    let classes = regex(r"\bclass\s+([A-Za-z_]\w*)")?;
    let classes: Vec<_> = classes
        .captures_iter(&safe)
        .collect::<std::result::Result<_, _>>()?;
    let literal = regex(r#"\bRoutePrefix\s*(?:=>|=)\s*"([^"]+)""#)?;
    let literal = literal.captures(&safe)?;
    for matched in regex(
        r#"\.Map(Get|Post|Put|Patch|Delete)\s*\(\s*([A-Za-z_]\w*)\s*(?:,\s*"([^"]*)")?\s*\)"#,
    )?
    .captures_iter(&safe)
    {
        let matched = matched?;
        let text = matched.get(0).unwrap();
        let before = classes.partition_point(|class| class.get(0).unwrap().end() <= text.start());
        let Some(class) = before.checked_sub(1).map(|index| &classes[index]) else {
            continue;
        };
        let group = &class[1];
        let method = matched[1].to_uppercase();
        let sub = matched.get(3).map_or("", |m| m.as_str());
        let prefix = literal
            .as_ref()
            .map_or_else(|| format!("/{group}"), |literal| literal[1].to_owned());
        let mut node = declaration(&ctx, text.start(), text.end(), &method, &join(&prefix, sub));
        node.qualified_name = format!("{path}::group:{group}:{method}:{sub}");
        out.unresolved_references
            .push(reference(&node, &matched[2], "references"));
        out.nodes.push(node);
    }
    let configure = regex(r"\bvoid\s+Configure\s*\(\s*\)\s*\{")?;
    let configures: Vec<_> = configure
        .find_iter(&safe)
        .collect::<std::result::Result<_, _>>()?;
    let handlers = regex(r"\b(HandleAsync|ExecuteAsync)\s*\(")?;
    let handlers: Vec<_> = handlers
        .captures_iter(&safe)
        .collect::<std::result::Result<_, _>>()?;
    let verbs = regex(r"\b(Get|Post|Put|Patch|Delete)\s*\(\s*([^;]*?)\s*\)\s*;")?;
    let literals = regex(r#""([^"]+)""#)?;
    let nameof = regex(r"\{\s*nameof\s*\(\s*(\w+)\s*\)\s*\}")?;
    let constant = regex(r"^([A-Za-z_]\w*)\.([A-Za-z_]\w*)$")?;
    let close = regex(r"\n\s*\}\s*\n")?;
    let mut work = 0usize;
    for class in regex(r"\bclass\s+([A-Za-z_]\w*)[^{;]*?:\s*(?:FastEndpoints\.)?(?:Endpoint(?:WithoutRequest|WithoutResponse)?\b|Ep\.)")?.find_iter(&safe) {
        let class=class?;
        let Some(configure)=configures.get(configures.partition_point(|capture|capture.start()<class.start())) else {continue;};
        let handler=handlers.get(handlers.partition_point(|capture|capture.get(0).unwrap().start()<class.start()));
        let body=&safe[configure.start()..];
        let limit=end_at_units(source,configure.start(),3000);
        let close=close.find(body)?.map(|capture|capture.end());
        for verb in verbs.captures_iter(body) {
            let verb=verb?;let text=verb.get(0).unwrap();
            let start=configure.start()+text.start();if start>=limit{break;}
            work+=1;ensure!(work<=100_000,"ASP.NET route extraction exceeds work budget");
            let method=verb[1].to_uppercase();let args=&verb[2];
            let mut targets=Vec::new();
            for literal in literals.captures_iter(args){let literal=literal?;let value=nameof.replace_all(&literal[1],"$1");let target=join("",&value);targets.push((target.clone(),target));}
            if targets.is_empty() && let Some(constant)=constant.captures(args)? {let name=format!("{}.{}",&constant[1],&constant[2]);targets.push((name.clone(),format!("const:{name}")));}
            for (name,key) in targets {
                let mut node=declaration(&ctx,start,start,&method,&name);node.qualified_name=format!("{path}::fastendpoint:{method}:{key}");
                if let Some(handler)=handler{out.unresolved_references.push(reference(&node,&handler[1],"references"));}
                out.nodes.push(node);
            }
            // Preserve the reference's check after emission, including its boundary behavior.
            if close.is_some_and(|end|end<=text.start()){break;}
        }
    }
    Ok(out)
}

/// Return changed records; route identities and qualified expressions stay stable.
pub(super) fn post_extract(nodes: &[Node], files: &BTreeMap<String, String>) -> Result<Vec<Node>> {
    let constants = regex(r"::fastendpoint:([A-Z]+):const:([A-Za-z_]\w*)\.([A-Za-z_]\w*)$")?;
    let mut owners: HashMap<&str, Vec<&Node>> = HashMap::new();
    for node in nodes.iter().filter(|node| {
        node.language == "csharp" && matches!(node.kind.as_str(), "class" | "struct")
    }) {
        owners.entry(&node.name).or_default().push(node);
    }
    let mut resolved: HashMap<(String, String), Option<String>> = HashMap::new();
    let mut scanned = 0usize;
    let mut updates = Vec::new();
    for route in nodes
        .iter()
        .filter(|node| node.kind == "route" && node.language == "csharp")
    {
        let Some(key) = constants.captures(&route.qualified_name)? else {
            continue;
        };
        let pair = (key[2].to_owned(), key[3].to_owned());
        if !resolved.contains_key(&pair) {
            let pattern = regex(&format!(
                r#"\b(?:const\s+string|static\s+readonly\s+string|static\s+string)\s+{}\s*=\s*"([^"]+)""#,
                &key[3]
            ))?;
            let mut value = None;
            for owner in owners
                .get(key.get(2).unwrap().as_str())
                .into_iter()
                .flatten()
            {
                let text = files.get(&owner.file_path).map_or("", String::as_str);
                scanned = scanned.saturating_add(text.len());
                ensure!(
                    scanned <= 128 * 1024 * 1024,
                    "ASP.NET route finalization exceeds source budget"
                );
                if let Some(capture) = pattern.captures(text)? {
                    value = Some(capture[1].to_owned());
                    break;
                }
            }
            resolved.insert(pair.clone(), value);
        }
        if let Some(value) = resolved.get(&pair).and_then(Option::as_ref) {
            let name = format!("{} {}", &key[1], join("", value));
            if name != route.name {
                let mut update = route.clone();
                update.name = name;
                updates.push(update);
            }
        }
    }
    let prefix = regex(r#"\$"([^"{]*)\{\s*(?:groupName|type\.Name|name|prefix)\s*\}""#)?;
    let fallback = regex(r#"MapGroup\(\s*\$"([^"{]*)\{"#)?;
    let mut head = None;
    for (_, content) in files
        .iter()
        .filter(|(file, content)| file.ends_with(".cs") && content.contains("MapGroup"))
        .take(400)
    {
        scanned = scanned.saturating_add(content.len());
        ensure!(
            scanned <= 128 * 1024 * 1024,
            "ASP.NET route finalization exceeds source budget"
        );
        let captured = match prefix.captures(content)? {
            Some(capture) => Some(capture),
            None => fallback.captures(content)?,
        };
        if let Some(capture) = captured {
            head = Some(capture[1].to_owned());
            break;
        }
    }
    let head = head.unwrap_or_default();
    let groups = regex(r"::group:([A-Za-z_]\w*):([A-Z]+):(.*)$")?;
    let literal = regex(r#"\bRoutePrefix\s*(?:=>|=)\s*""#)?;
    let mut explicit_prefix = HashMap::new();
    for route in nodes
        .iter()
        .filter(|node| node.kind == "route" && node.language == "csharp")
    {
        let Some(group) = groups.captures(&route.qualified_name)? else {
            continue;
        };
        if !explicit_prefix.contains_key(&route.file_path) {
            let found = if let Some(content) = files.get(&route.file_path) {
                scanned = scanned.saturating_add(content.len());
                ensure!(
                    scanned <= 128 * 1024 * 1024,
                    "ASP.NET route finalization exceeds source budget"
                );
                literal.is_match(content)?
            } else {
                false
            };
            explicit_prefix.insert(route.file_path.clone(), found);
        }
        if explicit_prefix[&route.file_path] {
            continue;
        }
        let prefix = format!("{}/{}", head.trim_end_matches('/'), &group[1]);
        let name = format!("{} {}", &group[2], join(&prefix, &group[3]));
        if name != route.name {
            let mut update = route.clone();
            update.name = name;
            updates.push(update);
        }
    }
    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn extraction_and_finalization_match_independent_reference_records() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/frameworks/aspnet.json"
        ))
        .unwrap();
        assert_eq!(
            corpus["revision"],
            "6aa358ccbae30fbf27318f79ee183212f726f9ba"
        );
        for case in corpus["cases"].as_array().unwrap() {
            let actual = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut actual =
                json!({"nodes":actual.nodes,"references":actual.unresolved_references});
            for node in actual["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(actual, case["expected"], "{}", case["id"]);
        }
        for project in corpus["projects"].as_array().unwrap() {
            let mut input = project["nodes"].clone();
            for node in input.as_array_mut().unwrap() {
                node.as_object_mut()
                    .unwrap()
                    .insert("updatedAt".into(), 0.into());
            }
            let nodes: Vec<Node> = serde_json::from_value(input).unwrap();
            let files = serde_json::from_value(project["files"].clone()).unwrap();
            let actual = post_extract(&nodes, &files).unwrap();
            let mut output = serde_json::to_value(&actual).unwrap();
            for node in output.as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(output, project["expected"], "{}", project["id"]);
            let updates: HashMap<_, _> = actual
                .into_iter()
                .map(|node| (node.id.clone(), node))
                .collect();
            let finalized: Vec<_> = nodes
                .into_iter()
                .map(|node| updates.get(&node.id).cloned().unwrap_or(node))
                .collect();
            assert!(
                post_extract(&finalized, &files).unwrap().is_empty(),
                "finalization must be idempotent: {}",
                project["id"]
            );
        }
    }
}
