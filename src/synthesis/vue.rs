//! Vue template components and named event handlers.
use super::*;

pub(super) fn templates(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["vue"]) {
        return Ok(Vec::new());
    }
    let template = regex(r"(?i)<template[^>]*>([\s\S]*)</template>")?;
    let script = regex(r"(?i)<script[^>]*>([\s\S]*?)</script>")?;
    let destructure = regex(r"(?:const|let|var)\s*\{([^}]+)\}\s*=\s*(\w+)\s*\(")?;
    let destructured_part = regex(r"^(\w+)\s*(?::\s*(\w+))?$")?;
    let composable_name = regex(r"^use[A-Z]")?;
    let kebab = regex(r"<([a-z][a-z0-9]*(?:-[a-z0-9]+)+)[\s/>]")?;
    let pascal = regex(r"<([A-Z][A-Za-z0-9]*)[\s/>]")?;
    let handler = regex(r#"(?:@|v-on:)([a-zA-Z][\w-]*)(?:\.[\w]+)*\s*=\s*"([^"]+)""#)?;
    let handler_name = regex(r"^([A-Za-z_$][\w$]*)\s*(?:\(|$)")?;
    let mut nuxt = HashMap::new();
    for node in ctx.nodes.iter().filter(|n| n.kind == "component") {
        ctx.step()?;
        if let Some(name) = nuxt_name(&node.file_path) {
            nuxt.entry(name).or_insert(node);
        }
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &content) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".vue") {
            continue;
        }
        let Some(template) = template.captures(content)? else {
            continue;
        };
        let template = template.get(1).unwrap().as_str();
        if template.is_empty() {
            continue;
        }
        let Some(component) = ctx.in_file(file).find(|n| n.kind == "component") else {
            continue;
        };
        let script_capture = script.captures(content)?;
        let script = script_capture
            .as_ref()
            .and_then(|m| m.get(1))
            .map_or("", |m| m.as_str());
        let mut destructured = HashMap::new();
        for found in destructure.captures_iter(script) {
            ctx.step()?;
            let found = found?;
            let factory = found.get(2).unwrap().as_str();
            if !composable_name.is_match(factory)? {
                continue;
            }
            for part in found.get(1).unwrap().as_str().split(',') {
                if let Some(found) = destructured_part.captures(part.trim())? {
                    let key = found.get(1).unwrap().as_str();
                    let alias = found.get(2).map_or(key, |m| m.as_str());
                    destructured.insert(alias, (factory, key));
                }
            }
        }
        let mut added = 0;
        let mut add = |target: Option<&Node>, metadata: Value| -> Result<()> {
            let Some(target) = target else {
                return Ok(());
            };
            if added >= 30
                || target.id == component.id
                || crosses_web_boundary(&target.language)
                || !seen.insert((
                    component.id.clone(),
                    target.id.clone(),
                    metadata["synthesizedBy"].as_str().unwrap().to_owned(),
                ))
            {
                return Ok(());
            }
            let mut edge = call(component, target, "", "");
            edge.metadata = Some(metadata);
            ctx.emit(&mut output, edge)?;
            added += 1;
            Ok(())
        };
        for (pattern, convert) in [(&kebab, true), (&pascal, false)] {
            for found in pattern.captures_iter(template) {
                ctx.step()?;
                let found = found?;
                let written = found.get(1).unwrap().as_str();
                let name = if convert {
                    kebab_to_pascal(written)
                } else {
                    written.to_owned()
                };
                let target = resolve(
                    ctx,
                    &name,
                    file,
                    &["component", "function", "class"],
                    nuxt.get(&name).copied(),
                )?;
                add(target, json!({"synthesizedBy":"jsx-render","via":written}))?;
            }
        }
        for found in handler.captures_iter(template) {
            ctx.step()?;
            let found = found?;
            let event = found.get(1).unwrap().as_str();
            let expression = found.get(2).unwrap().as_str().trim();
            if expression.contains("=>") || expression.starts_with('$') {
                continue;
            }
            let Some(name) = handler_name.captures(expression)? else {
                continue;
            };
            let name = name.get(1).unwrap().as_str();
            let direct = if name == expression {
                resolve(ctx, name, file, &["method", "function"], None)?
            } else {
                None
            };
            if direct.is_some() {
                add(direct, json!({"synthesizedBy":"vue-handler","event":event}))?;
                continue;
            }
            let Some(&(factory, key)) = destructured.get(name) else {
                continue;
            };
            if let Some(composable) = resolve(ctx, factory, file, &["method", "function"], None)? {
                ctx.steps(ctx.names.get(key).map_or(0, Vec::len))?;
                let target = ctx.named(key).find(|n| {
                    matches!(
                        n.kind.as_str(),
                        "method" | "function" | "variable" | "constant"
                    ) && n.file_path == composable.file_path
                });
                add(
                    target,
                    json!({"synthesizedBy":"vue-handler","event":event,"via":factory}),
                )?;
            }
        }
    }
    Ok(output)
}

fn resolve<'a>(
    ctx: &Context<'a>,
    name: &str,
    file: &str,
    kinds: &[&str],
    fallback: Option<&'a Node>,
) -> Result<Option<&'a Node>> {
    let mut first = None;
    for node in ctx.named(name) {
        ctx.step()?;
        if !kinds.contains(&node.kind.as_str())
            || manual_mock(&node.file_path) && !crate::classify::is_test_path(file)
        {
            continue;
        }
        if node.file_path == file {
            return Ok(Some(node));
        }
        if first.is_none() {
            first = Some(node);
        }
    }
    if ctx
        .imports
        .get(&(file, name))
        .is_some_and(|binding| binding.external)
    {
        return Ok(None);
    }
    Ok(first.or(fallback))
}
fn kebab_to_pascal(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or(String::new(), |first| {
                first.to_uppercase().collect::<String>() + characters.as_str()
            })
        })
        .collect()
}
fn nuxt_name(path: &str) -> Option<String> {
    let (_, rest) = path.rsplit_once("components/")?;
    let rest = rest
        .rsplit_once('.')
        .filter(|(_, extension)| {
            ["vue", "ts", "tsx", "js", "jsx"].contains(&extension.to_ascii_lowercase().as_str())
        })
        .map_or(rest, |(name, _)| name);
    let parts: Vec<_> = rest
        .split('/')
        .filter(|part| !part.is_empty())
        .map(kebab_to_pascal)
        .collect();
    if parts.len() < 2 {
        return None;
    }
    let mut output: Vec<String> = Vec::new();
    for part in parts {
        if let Some(previous) = output.last_mut()
            && part.starts_with(previous.as_str())
        {
            *previous = part;
        } else {
            output.push(part);
        }
    }
    Some(output.concat())
}
pub(super) fn crosses_web_boundary(language: &str) -> bool {
    matches!(
        language,
        "c" | "cpp"
            | "objc"
            | "swift"
            | "java"
            | "kotlin"
            | "scala"
            | "csharp"
            | "razor"
            | "vbnet"
            | "cfml"
            | "cfscript"
            | "python"
            | "go"
            | "rust"
            | "php"
            | "ruby"
            | "dart"
            | "lua"
            | "luau"
            | "r"
            | "erlang"
            | "pascal"
            | "solidity"
            | "nix"
            | "cobol"
    )
}
