//! HarmonyOS event channels and page routes scoped by declared workspace modules.
use super::*;

pub(super) fn emitter(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["arkts"]) {
        return Ok(Vec::new());
    }
    let calls = regex(
        r"\bemitter\s*\.\s*(emit|on|once)\s*\(\s*([A-Za-z_$][\w$.]*|\{[^)]{0,120}?\beventId\s*:\s*[^,}]+[^)]*?\})",
    )?;
    let id = regex(r"\beventId\s*:\s*([\w$.]+)")?;
    let numeric = regex(r"^\d+$")?;
    let named = regex(r"^[\w$.]+$")?;
    type Sites<'a> = HashMap<String, Vec<(&'a Node, &'a str, u32)>>;
    let mut emits: Sites<'_> = HashMap::new();
    let mut handlers: Sites<'_> = HashMap::new();
    let mut order = Vec::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".ets") || !source.contains("emitter.") {
            continue;
        }
        let safe = strip_comments(source);
        for found in calls.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let Some(node) = enclosing(ctx, file, line)? else {
                continue;
            };
            let argument = found.get(2).unwrap().as_str().trim();
            let literal = if argument.starts_with('{') {
                id.captures(argument)?
            } else {
                None
            };
            let token = literal
                .as_ref()
                .and_then(|m| m.get(1))
                .map_or(argument, |m| m.as_str());
            let key = if numeric.is_match(token)? {
                Some(format!("num:{file}:{token}"))
            } else if token.contains('.') {
                Some(format!("name:{}:{token}", scope(ctx, file)))
            } else {
                let declaration = regex(&format!(
                    r"\b{}\b\s*(?::[^=\n]+)?=\s*(?:new\s+[\w$.]+\(\s*([^)\n]+?)\s*\)|([\w$.]+))",
                    regex::escape(token)
                ))?;
                let found = declaration.captures(&safe)?;
                let inner = found
                    .as_ref()
                    .and_then(|m| m.get(1).or_else(|| m.get(2)))
                    .map(|m| m.as_str().trim());
                if let Some(inner) = inner {
                    if numeric.is_match(inner)? {
                        Some(format!("num:{file}:{inner}"))
                    } else if named.is_match(inner)? {
                        Some(format!("name:{}:{inner}", scope(ctx, file)))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            let Some(key) = key else {
                continue;
            };
            if found.get(1).unwrap().as_str() == "emit" {
                if !emits.contains_key(&key) {
                    order.push(key.clone());
                }
                emits.entry(key).or_default().push((node, file, line));
            } else {
                handlers.entry(key).or_default().push((node, file, line));
            }
        }
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for key in order {
        let emitted = &emits[&key];
        let Some(registered) = handlers.get(&key) else {
            continue;
        };
        if emitted.len() > 8 || registered.len() > 8 {
            continue;
        }
        for &(from, _, line) in emitted {
            for &(target, file, registered_line) in registered {
                ctx.step()?;
                if from.id == target.id || !seen.insert((&from.id, &target.id)) {
                    continue;
                }
                let mut edge = call(from, target, "arkui-emitter", "");
                edge.line = Some(line);
                edge.metadata = Some(
                    json!({"synthesizedBy":"arkui-emitter","event":key.rsplit(':').next().unwrap_or(""),"registeredAt":format!("{file}:{registered_line}")}),
                );
                ctx.emit(&mut output, edge)?;
            }
        }
    }
    Ok(output)
}

pub(super) fn router(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["arkts"]) {
        return Ok(Vec::new());
    }
    let calls = regex(
        r#"\brouter\s*\.\s*(?:pushUrl|replaceUrl)\s*\(\s*\{[^)]{0,200}?\burl\s*:\s*['"]([\w\-./]+)['"]"#,
    )?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".ets") || !source.contains("router.") {
            continue;
        }
        let safe = strip_comments(source);
        for found in calls.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let url = found.get(1).unwrap().as_str();
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let Some(node) = enclosing(ctx, file, line)? else {
                continue;
            };
            let suffix = format!("/src/main/ets/{url}.ets");
            ctx.steps(ctx.sources.len())?;
            let mut candidates: Vec<_> = ctx
                .sources
                .keys()
                .copied()
                .filter(|path| path.ends_with(&suffix))
                .collect();
            if candidates.len() > 1 {
                let module = scope(ctx, file);
                let local: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|path| scope(ctx, path) == module)
                    .collect();
                if !local.is_empty() {
                    candidates = local;
                }
            }
            if candidates.len() != 1 {
                continue;
            }
            for target in ctx.in_file(candidates[0]) {
                ctx.step()?;
                if target.kind == "struct" && decorated(target, "Entry") {
                    if seen.insert((&node.id, &target.id)) {
                        let mut edge = call(node, target, "arkui-route", "");
                        edge.line = Some(line);
                        edge.metadata = Some(
                            json!({"synthesizedBy":"arkui-route","event":url,"registeredAt":format!("{}:{}",candidates[0],target.start_line)}),
                        );
                        ctx.emit(&mut output, edge)?;
                    }
                    break;
                }
            }
        }
    }
    Ok(output)
}

fn scope<'a>(ctx: &'a Context<'_>, file: &str) -> &'a str {
    ctx.module_dirs
        .iter()
        .find(|dir| {
            file == dir.as_str()
                || file
                    .strip_prefix(dir.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .map_or("", String::as_str)
}
fn enclosing<'a>(ctx: &Context<'a>, file: &str, line: u32) -> Result<Option<&'a Node>> {
    let mut best: Option<&Node> = None;
    for node in ctx.in_file(file) {
        ctx.step()?;
        if matches!(node.kind.as_str(), "method" | "function")
            && node.start_line <= line
            && node.end_line >= line
            && best.is_none_or(|previous| {
                node.end_line - node.start_line < previous.end_line - previous.start_line
            })
        {
            best = Some(node);
        }
    }
    Ok(best)
}
