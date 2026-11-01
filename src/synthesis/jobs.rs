//! Task queues and PHP event dispatch, with the reference's framework gates.
use super::comments::{Language, strip};
use super::*;

pub(super) fn celery(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["python"]) {
        return Ok(Vec::new());
    }
    let dispatch = regex(r"\b([A-Za-z_]\w*)\s*\.\s*(?:delay|apply_async)\s*\(")?;
    let decorator = regex(r"@\s*(?:[A-Za-z_][\w.]*\.)?(?:shared_task|task)\b")?;
    let declaration = regex(r"^(?:async\s+def|def|class)\b")?;
    let mut tasks: HashMap<&str, bool> = HashMap::new();
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".py")
            || !source.contains(".delay(") && !source.contains(".apply_async(")
        {
            continue;
        }
        let safe = strip(source, Language::Python);
        let mut added = 0;
        for found in dispatch.captures_iter(&safe) {
            ctx.step()?;
            if added >= 80 {
                break;
            }
            let found = found?;
            let name = found.get(1).unwrap().as_str();
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let Some(from) = ctx.enclosing(file, line)? else {
                continue;
            };
            let mut candidates = Vec::new();
            for candidate in ctx.named(name).filter(|n| n.kind == "function") {
                ctx.step()?;
                let is_task = if let Some(&task) = tasks.get(candidate.id.as_str()) {
                    task
                } else {
                    let mut task = false;
                    if candidate.file_path.ends_with(".py") {
                        for line in (candidate.start_line.saturating_sub(12).max(1)
                            ..candidate.start_line)
                            .rev()
                        {
                            let text = ctx.line(&candidate.file_path, line).unwrap_or("").trim();
                            if declaration.is_match(text)? {
                                break;
                            }
                            if decorator.is_match(text)? {
                                task = true;
                                break;
                            }
                        }
                    }
                    tasks.insert(&candidate.id, task);
                    task
                };
                if is_task {
                    candidates.push(candidate);
                }
            }
            let target = if candidates.len() == 1 {
                candidates.first().copied()
            } else {
                candidates.into_iter().find(|n| n.file_path == file)
            };
            if let Some(target) = target
                && target.id != from.id
                && seen.insert((&from.id, &target.id))
            {
                ctx.emit(
                    &mut output,
                    dispatched(from, target, "celery-dispatch", name, file, line),
                )?;
                added += 1;
            }
        }
    }
    Ok(output)
}

pub(super) fn sidekiq(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["ruby"]) {
        return Ok(Vec::new());
    }
    let dispatch =
        regex(r"([A-Z][A-Za-z0-9_]*(?:::[A-Z][A-Za-z0-9_]*)*)\s*\.\s*perform_(?:async|in|at)\b")?;
    let gate = regex(r"\.perform_(?:async|in|at)\b")?;
    let mixin = regex(r"\binclude\s+Sidekiq::(?:Job|Worker)\b")?;
    let mut methods = HashMap::new();
    let mut perform = |class: &Node| -> Result<Option<&Node>> {
        if let Some(method) = methods.get(&class.id) {
            return Ok(*method);
        }
        let method = if mixin.is_match(ctx.slice(class).unwrap_or(""))? {
            first_method(ctx, class, "perform")?
        } else {
            None
        };
        methods.insert(class.id.clone(), method);
        Ok(method)
    };
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".rb") || !gate.is_match(source)? {
            continue;
        }
        let safe = strip(source, Language::Ruby);
        let mut added = 0;
        for found in dispatch.captures_iter(&safe) {
            ctx.step()?;
            if added >= 80 {
                break;
            }
            let found = found?;
            let name = found.get(1).unwrap().as_str();
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let Some(from) = ctx.enclosing(file, line)? else {
                continue;
            };
            let mut target = None;
            if name.contains("::") {
                for class in ctx.qualified(name).filter(|n| n.kind == "class") {
                    ctx.step()?;
                    if let Some(method) = perform(class)? {
                        target = Some(method);
                        break;
                    }
                }
            }
            if target.is_none() {
                let mut candidates = Vec::new();
                for class in ctx
                    .named(name.rsplit("::").next().unwrap())
                    .filter(|n| n.kind == "class")
                {
                    ctx.step()?;
                    if let Some(method) = perform(class)? {
                        candidates.push(method);
                    }
                }
                if candidates.len() == 1 {
                    target = candidates.first().copied();
                }
            }
            if let Some(target) = target
                && target.id != from.id
                && seen.insert((&from.id, &target.id))
            {
                ctx.emit(
                    &mut output,
                    dispatched(from, target, "sidekiq-dispatch", name, file, line),
                )?;
                added += 1;
            }
        }
    }
    Ok(output)
}

pub(super) fn laravel<'a>(ctx: &Context<'a>) -> Result<Vec<Edge>> {
    if !ctx.has(&["php"]) {
        return Ok(Vec::new());
    }
    let parameter = regex(
        r"function\s+handle\s*\(\s*(?:\.\.\.\s*)?(\??[A-Za-z_\\][\w\\|]*)\s+&?\s*(?:\.\.\.\s*)?\$",
    )?;
    let event_type = regex(r"^[A-Z]\w*$")?;
    let map = regex(r"\$listen\s*=\s*\[")?;
    let entry =
        regex(r#"(?:([A-Za-z_\\][\w\\]*)::class|'([^']+)'|"([^"]+)")\s*=>\s*\[([^\]]*)\]"#)?;
    let class_name = regex(r#"(?:([A-Za-z_\\][\w\\]*)::class|'([^']+)'|"([^"]+)")"#)?;
    let mut listeners: HashMap<String, Vec<&Node>> = HashMap::new();
    let mut add = |event: &str, node: &'a Node| {
        let targets = listeners.entry(event.to_owned()).or_default();
        if !targets.iter().any(|target| target.id == node.id) {
            targets.push(node);
        }
    };
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".php") || source.is_empty() {
            continue;
        }
        if source.contains("function handle") {
            for node in ctx
                .in_file(file)
                .filter(|n| n.kind == "method" && n.name == "handle")
            {
                ctx.step()?;
                let declaration = (node.start_line..node.start_line.saturating_add(3))
                    .filter_map(|line| ctx.line(file, line))
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(found) = parameter.captures(&declaration)? {
                    for kind in found
                        .get(1)
                        .unwrap()
                        .as_str()
                        .trim_start_matches('?')
                        .split('|')
                    {
                        let event = php_name(kind);
                        if event_type.is_match(event)? {
                            add(event, node);
                        }
                    }
                }
            }
        }
        if source.contains("$listen") {
            let safe = strip(source, Language::Php);
            if let Some(found) = map.find(&safe)?
                && let Some(body) = array_body(&safe, found.end() - 1)
            {
                for found in entry.captures_iter(body) {
                    ctx.step()?;
                    let found = found?;
                    let event = php_name(capture_name(&found));
                    for class in class_name.captures_iter(found.get(4).unwrap().as_str()) {
                        ctx.step()?;
                        let class = class?;
                        let name = php_name(capture_name(&class));
                        for node in ctx.named(name).filter(|n| n.kind == "class") {
                            ctx.step()?;
                            if let Some(method) = first_method(ctx, node, "handle")? {
                                add(event, method);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    let dispatch = regex(r"\bevent\s*\(\s*new\s+\\?([A-Za-z_][\w\\]*)")?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".php") || !source.contains("event(") {
            continue;
        }
        let safe = strip(source, Language::Php);
        let mut added = 0;
        for found in dispatch.captures_iter(&safe) {
            ctx.step()?;
            if added >= 200 {
                break;
            }
            let found = found?;
            let event = php_name(found.get(1).unwrap().as_str());
            let Some(targets) = listeners.get(event) else {
                continue;
            };
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let Some(from) = ctx.enclosing(file, line)? else {
                continue;
            };
            for &target in targets {
                ctx.step()?;
                if from.id != target.id && seen.insert((&from.id, &target.id)) {
                    ctx.emit(
                        &mut output,
                        dispatched(from, target, "laravel-event", event, file, line),
                    )?;
                    added += 1;
                }
            }
        }
    }
    Ok(output)
}

fn first_method<'a>(ctx: &Context<'a>, class: &Node, name: &str) -> Result<Option<&'a Node>> {
    for node in ctx.in_file(&class.file_path) {
        ctx.step()?;
        if node.kind == "method"
            && node.name == name
            && node.start_line >= class.start_line
            && node.start_line <= class.end_line
        {
            return Ok(Some(node));
        }
    }
    Ok(None)
}
fn php_name(name: &str) -> &str {
    name.rsplit('\\')
        .next()
        .unwrap_or(name)
        .rsplit("::")
        .next()
        .unwrap_or(name)
        .trim()
}
fn capture_name<'a>(found: &fancy_regex::Captures<'a>) -> &'a str {
    (1..=3)
        .find_map(|i| found.get(i))
        .map_or("", |m| m.as_str())
}
fn array_body(source: &str, start: usize) -> Option<&str> {
    let mut depth = 0usize;
    for (offset, byte) in source.as_bytes()[start..].iter().enumerate() {
        if *byte == b'[' {
            depth += 1;
        } else if *byte == b']' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return source.get(start + 1..start + offset);
            }
        }
    }
    None
}
pub(super) fn dispatched(
    from: &Node,
    to: &Node,
    pass: &str,
    via: &str,
    file: &str,
    line: u32,
) -> Edge {
    let mut edge = call(from, to, pass, via);
    edge.line = Some(line);
    edge.metadata =
        Some(json!({"synthesizedBy":pass,"via":via,"registeredAt":format!("{file}:{line}")}));
    edge
}
