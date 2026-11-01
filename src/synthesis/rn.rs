//! React Native event channels keyed by literal or scoped constant values.
use super::*;

const CONSTANTS: &[&str] = &["constant", "variable", "field", "property", "enum_member"];
struct Handler<'a> {
    node: &'a Node,
    file: &'a str,
    line: u32,
}

pub(super) fn events(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(JS) {
        return Ok(Vec::new());
    }
    let objc = regex(r#"\bsendEventWithName\s*:\s*@"([^"]+)""#)?;
    let swift = regex(r#"\bsendEvent\s*\(\s*withName\s*:\s*"([^"]+)""#)?;
    let jvm = regex(r#"\.emit\s*\(\s*"([^"]+)"\s*,"#)?;
    let native = regex(r#"\bsendEvent\s*\([^;{}]*?"([^"]+)""#)?;
    let objc_constant = regex(r"\bsendEventWithName\s*:\s*([A-Za-z_]\w*)\b")?;
    let swift_constant = regex(r"\bsendEvent\s*\(\s*withName\s*:\s*([A-Za-z_][\w.]*)")?;
    let jvm_constant = regex(r"\.emit\s*\(\s*([A-Za-z_][\w.]*)\s*,")?;
    let native_constant = regex(r"\bsendEvent\s*\(\s*[A-Za-z_][\w.]*\s*,\s*([A-Za-z_][\w.]*)\s*,")?;
    let named = regex(r#"\.(?:on|once|addListener)\(\s*['"]([^'"]+)['"]\s*,\s*([A-Za-z_][\w.]*)"#)?;
    let inline = regex(
        r#"\.(?:on|once|addListener)\(\s*['"]([^'"]+)['"]\s*,\s*(?:async\s*)?(?:\([^)]*\)\s*=>|[A-Za-z_$][\w$]*\s*=>|function\s*\()"#,
    )?;
    let constant = regex(
        r"\.(?:on|once|addListener)\(\s*((?:[A-Za-z_$][\w$]*\.)*[A-Za-z_$][\w$]*)\s*,\s*([A-Za-z_$][\w$.]*|(?:async\s*)?(?:\([^)]*\)\s*=>|[A-Za-z_$][\w$]*\s*=>|function\s*\())",
    )?;
    let bare_name = regex(r"^[A-Za-z_$][\w$.]*$")?;
    let mut constants = HashMap::new();
    let mut dispatchers: HashMap<String, Vec<&Node>> = HashMap::new();
    let mut order = Vec::new();
    let mut handlers: HashMap<String, Vec<Handler<'_>>> = HashMap::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if source.is_empty() {
            continue;
        }
        let patterns = if file.ends_with(".m") || file.ends_with(".mm") {
            vec![(&objc, false), (&objc_constant, true)]
        } else if file.ends_with(".swift") {
            vec![(&swift, false), (&native, false), (&swift_constant, true)]
        } else if file.ends_with(".java") || file.ends_with(".kt") {
            vec![
                (&jvm, false),
                (&native, false),
                (&jvm_constant, true),
                (&native_constant, true),
            ]
        } else {
            Vec::new()
        };
        for (pattern, is_constant) in patterns {
            for found in pattern.captures_iter(source) {
                ctx.step()?;
                let found = found?;
                let value = found.get(1).unwrap().as_str();
                let event = if is_constant {
                    constant_name(ctx, value, file, &mut constants)?
                } else {
                    Some(value.to_owned())
                };
                let Some(event) = event else {
                    continue;
                };
                let line = ctx.line_at(file, found.get(0).unwrap().start());
                if let Some(node) = ctx.enclosing(file, line)? {
                    if !dispatchers.contains_key(&event) {
                        order.push(event.clone());
                    }
                    let nodes = dispatchers.entry(event).or_default();
                    if !nodes.iter().any(|known| known.id == node.id) {
                        nodes.push(node);
                    }
                }
            }
        }
        if !file
            .rsplit('.')
            .next()
            .is_some_and(|ext| matches!(ext, "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs"))
        {
            continue;
        }
        for found in named.captures_iter(source) {
            ctx.step()?;
            let found = found?;
            let event = found.get(1).unwrap().as_str();
            let name = found.get(2).unwrap().as_str().rsplit('.').next().unwrap();
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            ctx.steps(ctx.names.get(name).map_or(0, Vec::len))?;
            let mut target = ctx
                .named(name)
                .find(|n| matches!(n.kind.as_str(), "method" | "function"));
            if target.is_none() {
                target = ctx.enclosing(file, line)?;
            }
            if target.is_none() {
                for node in ctx.in_file(file) {
                    ctx.step()?;
                    if matches!(node.kind.as_str(), "constant" | "variable")
                        && node.start_line <= line
                        && node.end_line >= line
                        && target.is_none_or(|previous| node.start_line >= previous.start_line)
                    {
                        target = Some(node);
                    }
                }
            }
            if let Some(node) = target {
                record(&mut handlers, event, Handler { node, file, line }, true);
            }
        }
        for found in inline.captures_iter(source) {
            ctx.step()?;
            let found = found?;
            let event = found.get(1).unwrap().as_str();
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            if let Some(node) = ctx.enclosing(file, line)? {
                record(&mut handlers, event, Handler { node, file, line }, false);
            }
        }
        for found in constant.captures_iter(source) {
            ctx.step()?;
            let found = found?;
            let Some(event) =
                constant_name(ctx, found.get(1).unwrap().as_str(), file, &mut constants)?
            else {
                continue;
            };
            let argument = found.get(2).unwrap().as_str();
            let bare = if bare_name.is_match(argument)? {
                argument.rsplit('.').next()
            } else {
                None
            };
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let mut named = None;
            if let Some(bare) = bare {
                for node in ctx.in_file(file) {
                    ctx.step()?;
                    if matches!(node.kind.as_str(), "method" | "function") && node.name == bare {
                        named = Some(node);
                        break;
                    }
                }
            }
            if let Some(node) = named.or(ctx.enclosing(file, line)?) {
                record(&mut handlers, &event, Handler { node, file, line }, false);
            }
        }
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for event in order {
        let dispatchers = &dispatchers[&event];
        let Some(handlers) = handlers.get(&event) else {
            continue;
        };
        if dispatchers.len() > 6 || handlers.len() > 6 {
            continue;
        }
        for &from in dispatchers {
            for handler in handlers {
                ctx.step()?;
                if from.id == handler.node.id || !seen.insert((&from.id, &handler.node.id)) {
                    continue;
                }
                let mut edge = call(from, handler.node, "rn-event-channel", "");
                edge.line = None;
                edge.metadata = Some(
                    json!({"synthesizedBy":"rn-event-channel","event":event,"registeredAt":format!("{}:{}",handler.file,handler.line)}),
                );
                ctx.emit(&mut output, edge)?;
            }
        }
    }
    Ok(output)
}

fn record<'a>(
    events: &mut HashMap<String, Vec<Handler<'a>>>,
    event: &str,
    handler: Handler<'a>,
    replace: bool,
) {
    let handlers = events.entry(event.to_owned()).or_default();
    if let Some(known) = handlers
        .iter_mut()
        .find(|known| known.node.id == handler.node.id)
    {
        if replace {
            *known = handler;
        }
    } else {
        handlers.push(handler);
    }
}
fn constant_name(
    ctx: &Context<'_>,
    expression: &str,
    file: &str,
    memo: &mut HashMap<(String, String), Option<String>>,
) -> Result<Option<String>> {
    let key = (file.to_owned(), expression.to_owned());
    if let Some(value) = memo.get(&key) {
        return Ok(value.clone());
    }
    let parts: Vec<_> = expression
        .strip_suffix(".rawValue")
        .unwrap_or(expression)
        .split('.')
        .collect();
    let name = parts.last().copied().unwrap_or("");
    let owner = if parts.len() > 1 {
        Some(parts[parts.len() - 2])
    } else {
        None
    };
    let mut value = None;
    if matches(r"^[A-Za-z_$][\w$]*$", name)?
        && name != "this"
        && (parts.len() <= 2 || parts[0] != "this")
    {
        let js = matches(r"\.(?:[cm]?[jt]sx?)$", file)?;
        let imported = |local: &str| {
            if js {
                ctx.imports
                    .get(&(file, local))
                    .and_then(|binding| binding.resolved_path.as_deref())
            } else {
                None
            }
        };
        let declared = |file: &str| {
            ctx.in_file(file)
                .filter(|n| n.name == name && CONSTANTS.contains(&n.kind.as_str()))
                .collect::<Vec<_>>()
        };
        let nodes = if let Some(owner) = owner {
            if let Some(from) = imported(owner) {
                declared(from)
            } else {
                let owner = regex(&format!(r"(?:^|[.:]){}(?:[.:]|$)", ::regex::escape(owner)))?;
                let mut nodes = Vec::new();
                for node in ctx
                    .named(name)
                    .filter(|n| CONSTANTS.contains(&n.kind.as_str()))
                {
                    ctx.step()?;
                    let prefix = node
                        .qualified_name
                        .rfind(name)
                        .map_or("", |at| &node.qualified_name[..at]);
                    if owner.is_match(prefix)? {
                        nodes.push(node);
                    }
                }
                nodes
            }
        } else {
            let mut nodes = declared(file);
            if nodes.is_empty()
                && let Some(from) = imported(name)
            {
                nodes = declared(from);
            }
            nodes
        };
        let literal = regex(r#"^\s*(?::[^=;]*)?[=:]\s*@?(["'`])([^"'`]+)\1"#)?;
        let define = regex(r#"^\s+@?(")([^"]+)""#)?;
        let mut values = HashSet::new();
        for node in nodes {
            ctx.step()?;
            let line = ctx.line(&node.file_path, node.start_line).unwrap_or("");
            if let Some(at) = line.find(name) {
                let rest = &line[at + name.len()..];
                if let Some(found) = literal.captures(rest)?.or(define.captures(rest)?) {
                    values.insert(found.get(2).unwrap().as_str().to_owned());
                }
            }
        }
        if values.len() == 1 {
            value = values.into_iter().next();
        }
    }
    memo.insert(key, value.clone());
    Ok(value)
}
