//! Rails route DSL and convention bindings from the pinned reference implementation.
use super::{
    extractors::regex,
    routes::{reference, route},
};
use anyhow::{Result, ensure};
use prometeu_extract::{ExtractionResult, Node, supplemental::Context};
use std::collections::HashSet;

const ACTIONS: &[&str] = &[
    "index", "create", "new", "show", "edit", "update", "destroy",
];
fn method(action: &str) -> &'static str {
    match action {
        "create" => "POST",
        "update" => "PATCH",
        "destroy" => "DELETE",
        _ => "GET",
    }
}
fn sym(text: &str) -> String {
    let text = text.strip_prefix(':').unwrap_or(text);
    let text = text.strip_prefix(['\'', '"']).unwrap_or(text);
    text.strip_suffix(['\'', '"']).unwrap_or(text).into()
}
fn option(tail: &str, key: &str) -> Result<Option<String>> {
    Ok(regex(&format!(r#"\b{key}:\s*(:\w+|["'][^"']*["'])"#))?
        .captures(tail)?
        .map(|m| sym(&m[1])))
}
fn action_option(tail: &str, key: &str) -> Result<Option<HashSet<String>>> {
    let pattern = regex(&format!(
        r#"(?:\b{key}:|:{key}\s*=>)\s*(?:\[([^\]]*)\]|%[iIwW]\s*[\[(]([^\])]*)[\])]|:(\w+)|["'](\w+)["'])"#
    ))?;
    let Some(m) = pattern.captures(tail)? else {
        return Ok(None);
    };
    Ok(Some(if let Some(array) = m.get(1) {
        array
            .as_str()
            .split(',')
            .map(|value| sym(value.trim_matches(crate::search::query_whitespace)))
            .filter(|s| !s.is_empty())
            .collect()
    } else if let Some(words) = m.get(2) {
        words
            .as_str()
            .split(crate::search::query_whitespace)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        [m.get(3).or(m.get(4)).unwrap().as_str().to_owned()]
            .into_iter()
            .collect()
    }))
}
fn actions(tail: &str, plural: bool) -> Result<Vec<&'static str>> {
    let only = action_option(tail, "only")?;
    let except = action_option(tail, "except")?;
    Ok(ACTIONS
        .iter()
        .copied()
        .filter(|action| {
            (plural || *action != "index")
                && only.as_ref().map_or_else(
                    || !except.as_ref().is_some_and(|set| set.contains(*action)),
                    |set| set.contains(*action),
                )
        })
        .collect())
}
fn pluralize(name: &str) -> Result<String> {
    Ok(if regex(r"[^aeiou]y$")?.is_match(name)? {
        format!("{}ies", &name[..name.len() - 1])
    } else if regex(r"(s|x|z|ch|sh)$")?.is_match(name)? {
        format!("{name}es")
    } else {
        format!("{name}s")
    })
}
fn singularize(name: &str) -> Result<String> {
    Ok(if let Some(base) = name.strip_suffix("ies") {
        format!("{base}y")
    } else if regex(r"(ss|us)$")?.is_match(name)? {
        name.into()
    } else if regex(r"(x|ch|sh|ses)$")?.is_match(name)? && name.ends_with("es") {
        name[..name.len() - 2].into()
    } else {
        name.strip_suffix('s').unwrap_or(name).into()
    })
}
fn join(parts: &[&str]) -> String {
    let joined = format!("/{}", parts.join("/"));
    let mut out = String::new();
    for ch in joined.chars() {
        if ch != '/' || !out.ends_with('/') {
            out.push(ch)
        }
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}
fn emit(
    ctx: &Context<'_>,
    out: &mut ExtractionResult,
    seen: Option<&mut HashSet<String>>,
    method: &str,
    path: &str,
    target: &str,
    position: (u32, u32),
) -> Result<()> {
    let (line, width) = position;
    let scoped = seen.is_some();
    ensure!(
        out.nodes.len() < 100_000,
        "Rails route expansion limit exceeded"
    );
    let mut node = route(ctx, line, method, path, width);
    node.qualified_name = format!("{}::route:{target}", ctx.path);
    if scoped {
        node.id = format!("route:{}:{line}:{method}:{path}:{target}", ctx.path)
    }
    if seen.is_some_and(|seen| !seen.insert(node.id.clone())) {
        return Ok(());
    }
    out.unresolved_references
        .push(reference(&node, target, "references"));
    out.nodes.push(node);
    Ok(())
}

pub(crate) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let ctx = Context::new(path, source, "ruby");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".rb") {
        return Ok(out);
    }
    let safe = crate::synthesis::comments::strip_for_regex(source, "ruby");
    if regex(r"\broutes\.draw\b")?.is_match(&safe)?
        || regex(r"(?:^|/)config/routes/")?.is_match(path)?
    {
        return scoped(&ctx, &safe, out);
    }
    for m in regex(r#"\b(get|post|put|patch|delete|match)\s+['"]([^'"]+)['"]\s*(?:,\s*to:\s*|=>\s*)['"]([^#'"]+)#([^'"]+)['"]"#)?.captures_iter(&safe){let m=m?;let whole=m.get(0).unwrap();let line=ctx.position(whole.start()).0;let verb=m[1].to_uppercase();let path=&m[2];let target=format!("{}#{}",&m[3],&m[4]);emit(&ctx,&mut out,None,&verb,path,&target,(line,source[whole.start()..whole.end()].encode_utf16().count() as u32))?;out.nodes.last_mut().unwrap().qualified_name=format!("{}::route:{path}",ctx.path);}
    for m in regex(r"\b(resources?)\s+:(\w+)([^\n]*)")?.captures_iter(&safe) {
        let m = m?;
        let whole = m.get(0).unwrap();
        let line = ctx.position(whole.start()).0;
        let plural = &m[1] == "resources";
        let name = &m[2];
        let controller = if plural {
            name.into()
        } else {
            pluralize(name)?
        };
        for action in actions(&m[3], plural)? {
            let path = match action {
                "index" | "create" => format!("/{name}"),
                "new" => format!("/{name}/new"),
                "edit" => format!("/{name}/:id/edit"),
                _ => format!("/{name}/:id"),
            };
            let target = format!("{controller}#{action}");
            emit(
                &ctx,
                &mut out,
                None,
                method(action),
                &path,
                &target,
                (
                    line,
                    source[whole.start()..whole.end()].encode_utf16().count() as u32,
                ),
            )?;
            out.nodes.last_mut().unwrap().id =
                format!("route:{}:{line}:{}:{target}", ctx.path, method(action));
            out.unresolved_references.last_mut().unwrap().from_node_id =
                out.nodes.last().unwrap().id.clone();
        }
    }
    Ok(out)
}
#[derive(Clone, Default)]
struct Frame {
    kind: &'static str,
    path: String,
    module: String,
    controller: Option<String>,
    collection: String,
    member: String,
}
fn scoped(ctx: &Context<'_>, safe: &str, mut out: ExtractionResult) -> Result<ExtractionResult> {
    let mut stack = vec![Frame {
        kind: "block",
        ..Default::default()
    }];
    let mut seen = HashSet::new();
    let lines = safe.split('\n').collect::<Vec<_>>();
    let mut at = 0;
    let mut work = 0usize;
    let opens = regex(r"\bdo\s*(?:\|[^|]*\|)?\s*$")?;
    let namespace = regex(r#"^namespace\s+(:\w+|["'][\w/-]+["'])(.*)$"#)?;
    let scope = regex(r"^scope\b\s*\(?\s*(.*?)\)?\s*(?:do\b.*)?$")?;
    let resources = regex(r"^(resources?)\s+:(\w+)(.*)$")?;
    let explicit =
        regex(r#"^(get|post|put|patch|delete|match)\s*\(?\s*(:\w+|["'][^"']*["'])(.*)$"#)?;
    while at < lines.len() {
        let line = at as u32 + 1;
        let mut text = lines[at]
            .trim_matches(crate::search::query_whitespace)
            .to_owned();
        at += 1;
        if text.is_empty() {
            continue;
        }
        loop {
            work = work.saturating_add(text.len());
            ensure!(work <= 10_000_000, "Rails statement scan limit exceeded");
            let balance = text.bytes().filter(|c| matches!(c, b'[' | b'(')).count() as isize
                - text.bytes().filter(|c| matches!(c, b']' | b')')).count() as isize;
            if balance <= 0 || at >= lines.len() || opens.is_match(&text)? {
                break;
            }
            text.push(' ');
            text.push_str(lines[at].trim_matches(crate::search::query_whitespace));
            at += 1;
        }
        if regex(r"^end\b")?.is_match(&text)? {
            if stack.len() > 1 {
                stack.pop();
            }
            continue;
        }
        let opens = opens.is_match(&text)?;
        let frame = stack.last().unwrap().clone();
        let mut pushed = None;
        if let Some(m) = namespace.captures(&text)? {
            let name = sym(&m[1]);
            let custom = option(&m[2], "path")?;
            pushed = Some(Frame {
                kind: "namespace",
                path: join(&[&frame.path, custom.as_deref().unwrap_or(&name)]),
                module: format!("{}{name}/", frame.module),
                ..Default::default()
            });
        } else if let Some(m) = scope.captures(&text)?.filter(|_| opens) {
            let args = &m[1];
            let lead = regex(r#"^(["'])([^"']*)\1"#)?
                .captures(args)?
                .map(|m| m[2].to_owned());
            let path = lead.or(option(args, "path")?).unwrap_or_default();
            let module = option(args, "module")?.map_or(frame.module.clone(), |module| {
                format!("{}{module}/", frame.module)
            });
            pushed = Some(Frame {
                kind: "scope",
                path: join(&[&frame.path, &path]),
                module,
                ..Default::default()
            });
        } else if let Some(m) = resources.captures(&text)? {
            let plural = &m[1] == "resources";
            let name = &m[2];
            let tail = &m[3];
            let segment = option(tail, "path")?.unwrap_or_else(|| name.into());
            let controller = format!(
                "{}{}",
                frame.module,
                option(tail, "controller")?.unwrap_or(if plural {
                    name.into()
                } else {
                    pluralize(name)?
                })
            );
            let collection = join(&[&frame.path, &segment]);
            let member = if plural {
                join(&[&collection, ":id"])
            } else {
                collection.clone()
            };
            for action in actions(tail, plural)? {
                let path = match action {
                    "index" | "create" => collection.clone(),
                    "new" => join(&[&collection, "new"]),
                    "edit" => join(&[&member, "edit"]),
                    _ => member.clone(),
                };
                emit(
                    ctx,
                    &mut out,
                    Some(&mut seen),
                    method(action),
                    &path,
                    &format!("{controller}#{action}"),
                    (line, 0),
                )?;
            }
            if opens {
                let nested = if plural {
                    join(&[&collection, &format!(":{}_id", singularize(name)?)])
                } else {
                    collection.clone()
                };
                pushed = Some(Frame {
                    kind: "resource",
                    path: nested,
                    module: frame.module.clone(),
                    controller: Some(controller),
                    collection,
                    member,
                });
            }
        } else if regex(r"^member\b")?.is_match(&text)? && opens {
            pushed = Some(Frame {
                kind: "member",
                ..frame.clone()
            });
        } else if regex(r"^collection\b")?.is_match(&text)? && opens {
            pushed = Some(Frame {
                kind: "collection",
                ..frame.clone()
            });
        } else if let Some(m) =
            regex(r#"^root\s*\(?\s*(?:to:\s*)?["']([\w/]+)#(\w+)["']"#)?.captures(&text)?
        {
            emit(
                ctx,
                &mut out,
                Some(&mut seen),
                "GET",
                &join(&[&frame.path]),
                &format!("{}{}#{}", frame.module, &m[1], &m[2]),
                (line, 0),
            )?;
        } else if let Some(m) = explicit.captures(&text)? {
            let method = if &m[1] == "match" {
                "ANY".into()
            } else {
                m[1].to_uppercase()
            };
            let raw = sym(&m[2]);
            let tail = &m[3];
            let to = regex(r#"(?:\bto:\s*|=>\s*)["']([\w/]+)#(\w+)["']"#)?.captures(tail)?;
            let resource = stack
                .iter()
                .rev()
                .find(|frame| matches!(frame.kind, "resource" | "member" | "collection"));
            if let Some(to) = to {
                let base = match frame.kind {
                    "member" => &frame.member,
                    "collection" => &frame.collection,
                    _ => &frame.path,
                };
                emit(
                    ctx,
                    &mut out,
                    Some(&mut seen),
                    &method,
                    &join(&[base, &raw]),
                    &format!("{}{}#{}", frame.module, &to[1], &to[2]),
                    (line, 0),
                )?;
            } else if let Some(resource) = resource.filter(|r| r.controller.is_some())
                && regex(r"^[\w-]+$")?.is_match(&raw)?
            {
                let action = option(tail, "action")?
                    .unwrap_or_else(|| raw.clone())
                    .replace('-', "_");
                let base = if frame.kind == "collection" {
                    &resource.collection
                } else {
                    &resource.member
                };
                emit(
                    ctx,
                    &mut out,
                    Some(&mut seen),
                    &method,
                    &join(&[base, &raw]),
                    &format!("{}#{action}", resource.controller.as_deref().unwrap()),
                    (line, 0),
                )?;
            }
        }
        if let Some(frame) = pushed {
            if opens {
                stack.push(frame)
            }
        } else if opens || regex(r"^(?:if|unless|case|begin|while|until)\b")?.is_match(&text)? {
            stack.push(Frame {
                kind: "block",
                ..frame
            });
        }
    }
    Ok(out)
}

pub(crate) fn claims(name: &str) -> Result<bool> {
    regex(r"^[\w/]+#\w+$")?.is_match(name).map_err(Into::into)
}
fn snake(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase())
        } else {
            out.push(ch)
        }
    }
    out.chars().skip(1).collect()
}
pub(crate) fn resolve<'a>(
    name: &str,
    file_exists: impl Fn(&str) -> bool,
    named: impl Fn(&str) -> Vec<&'a Node>,
    in_file: impl Fn(&str) -> Vec<&'a Node>,
) -> Result<Option<(&'a Node, f64)>> {
    if let Some(m) = regex(r"^([\w/]+)#(\w+)$")?.captures(name)? {
        let ctrl = &m[1];
        let action = &m[2];
        let direct = format!("app/controllers/{ctrl}_controller.rb");
        let action_node = |file: &str| {
            in_file(file)
                .into_iter()
                .find(|n| matches!(n.kind.as_str(), "method" | "function") && n.name == action)
        };
        if file_exists(&direct)
            && let Some(node) = action_node(&direct)
        {
            return Ok(Some((node, 0.85)));
        }
        let last = ctrl.rsplit('/').next().unwrap();
        let class = format!(
            "{}Controller",
            last.split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    chars.next().map_or(String::new(), |first| {
                        format!("{}{}", first.to_uppercase(), chars.as_str())
                    })
                })
                .collect::<String>()
        );
        let all = named(&class)
            .into_iter()
            .filter(|n| n.kind == "class")
            .collect::<Vec<_>>();
        let own = all
            .iter()
            .copied()
            .filter(|n| {
                ctrl.contains('/')
                    && format!("/{}", n.file_path).ends_with(&format!("/{ctrl}_controller.rb"))
            })
            .collect::<Vec<_>>();
        for node in if own.is_empty() { all } else { own } {
            if let Some(action) = action_node(&node.file_path) {
                return Ok(Some((action, 0.85)));
            }
        }
        return Ok(None);
    }
    let direct = |directories: &[&str], kind: &str| {
        let snake = snake(name);
        directories.iter().find_map(|dir| {
            let file = format!("{dir}/{snake}.rb");
            file_exists(&file)
                .then(|| {
                    in_file(&file)
                        .into_iter()
                        .find(|node| node.kind == kind && node.name == name)
                })
                .flatten()
        })
    };
    if regex(r"^[A-Z][a-zA-Z]+$")?.is_match(name)?
        && let Some(node) = direct(&["app/models", "app/models/concerns"], "class").or_else(|| {
            named(name)
                .into_iter()
                .find(|n| n.kind == "class" && n.file_path.contains("app/models/"))
        })
    {
        return Ok(Some((node, 0.8)));
    }
    if name.ends_with("Controller")
        && let Some(node) = direct(
            &[
                "app/controllers",
                "app/controllers/api",
                "app/controllers/api/v1",
            ],
            "class",
        )
        .or_else(|| {
            named(name)
                .into_iter()
                .find(|n| n.kind == "class" && n.file_path.contains("controllers/"))
        })
    {
        return Ok(Some((node, 0.85)));
    }
    if name.ends_with("Helper")
        && let Some(node) = direct(&["app/helpers"], "module")
    {
        return Ok(Some((node, 0.8)));
    }
    if (name.ends_with("Service") || name.ends_with("Job"))
        && let Some(node) = direct(&["app/services", "app/jobs", "app/workers"], "class")
    {
        return Ok(Some((node, 0.8)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn rails_routes_and_conventions_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/rails/cases.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let result = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut actual =
                json!({"nodes":result.nodes,"references":result.unresolved_references});
            for node in actual["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(
                actual, case["expected"],
                "{} {} {}",
                case["origin"], case["path"], case["source"]
            );
        }
        for project in corpus["projects"].as_array().unwrap() {
            let mut nodes = project["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0)
            }
            let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            for query in project["resolutions"].as_array().unwrap() {
                let name = query["name"].as_str().unwrap();
                assert_eq!(claims(name).unwrap(), query["claimed"].as_bool().unwrap());
                let result = resolve(
                    name,
                    |path| project["files"].get(path).is_some(),
                    |name| {
                        let mut nodes = nodes.iter().filter(|n| n.name == name).collect::<Vec<_>>();
                        nodes.sort_by(|a, b| {
                            (&a.file_path, a.start_line, &a.id).cmp(&(
                                &b.file_path,
                                b.start_line,
                                &b.id,
                            ))
                        });
                        nodes
                    },
                    |path| {
                        let mut nodes = nodes
                            .iter()
                            .filter(|n| n.file_path == path)
                            .collect::<Vec<_>>();
                        nodes.sort_by(|a, b| (a.start_line, &a.id).cmp(&(b.start_line, &b.id)));
                        nodes
                    },
                )
                .unwrap();
                assert_eq!(
                    result
                        .map(|(node, confidence)| json!({"id":node.id,"confidence":confidence}))
                        .unwrap_or(Value::Null),
                    query["expected"],
                    "{} {name}",
                    project["name"]
                );
            }
        }
    }
}
