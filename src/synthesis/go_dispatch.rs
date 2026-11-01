//! Reflective Go middleware and request-type routing.
use super::*;

pub(super) fn gin(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["go"]) {
        return Ok(Vec::new());
    }
    let dispatch = regex(r"\.handlers\s*\[[^\]]*\]\s*\(")?;
    let registration = regex(r"\.(?:Use|GET|POST|PUT|PATCH|DELETE|OPTIONS|HEAD|Any|Handle)\s*\(")?;
    let gate = regex(r"\.(?:GET|POST|PUT|PATCH|DELETE|OPTIONS|HEAD|Any|Handle)\(")?;
    let trailing_call = regex(r"\(\s*\)$")?;
    let identifier = regex(r"(?:\.|^)([A-Za-z_]\w*)$")?;
    let mut dispatchers = Vec::new();
    for node in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "method" && n.language == "go")
    {
        ctx.step()?;
        if dispatch.is_match(ctx.slice(node).unwrap_or(""))? {
            dispatchers.push(node);
        }
    }
    if dispatchers.is_empty() {
        return Ok(Vec::new());
    }
    let mut registered = Vec::new();
    let mut names = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".go") || !source.contains(".Use(") && !gate.is_match(source)? {
            continue;
        }
        let safe = comments::strip_for_regex(source, "go");
        for found in registration.find_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let Some(arguments) = balanced(&safe, found.end() - 1) else {
                continue;
            };
            let line = ctx.line_at(file, found.start());
            for argument in split_args(arguments) {
                ctx.step()?;
                let argument = argument.trim();
                let cleaned = trailing_call.replace(argument, "");
                if cleaned.is_empty()
                    || cleaned.starts_with(['"', '`'])
                    || cleaned.starts_with("func")
                {
                    continue;
                }
                if let Some(found) = identifier.captures(&cleaned)? {
                    let name = found.get(1).unwrap().as_str();
                    if names.insert(name.to_owned()) {
                        registered.push((name.to_owned(), format!("{file}:{line}")));
                    }
                }
            }
        }
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for from in dispatchers {
        let mut added = 0;
        for (name, registered_at) in &registered {
            ctx.step()?;
            if added >= CALLBACK_CAP {
                break;
            }
            let target = ctx
                .named(name)
                .find(|n| matches!(n.kind.as_str(), "function" | "method") && n.language == "go");
            if let Some(target) = target
                && from.id != target.id
                && seen.insert((&from.id, &target.id))
            {
                let mut edge = call(from, target, "gin-middleware-chain", name);
                edge.metadata = Some(
                    json!({"synthesizedBy":"gin-middleware-chain","via":name,"registeredAt":registered_at}),
                );
                ctx.emit(&mut output, edge)?;
                added += 1;
            }
        }
    }
    Ok(output)
}

pub(super) fn goframe(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["go"]) {
        return Ok(Vec::new());
    }
    let mut routes: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut order = Vec::new();
    let mut wanted = HashSet::new();
    for route in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "route" && n.language == "go")
    {
        ctx.step()?;
        let Some((_, key)) = route.qualified_name.split_once("::goframe-route:") else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        if !routes.contains_key(key) {
            order.push(key);
        }
        routes.entry(key).or_default().push(route);
        wanted.insert(key);
        wanted.insert(key.rsplit('.').next().unwrap_or(key));
    }
    if routes.is_empty() {
        return Ok(Vec::new());
    }
    let parameter = regex(r"\*\s*(?:(\w+)\.)?([A-Z]\w*)\b")?;
    let mut handlers: HashMap<String, Vec<&Node>> = HashMap::new();
    for method in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "method" && n.language == "go")
    {
        ctx.step()?;
        let Some(signature) = &method.signature else {
            continue;
        };
        for found in parameter.captures_iter(signature) {
            ctx.step()?;
            let found = found?;
            let name = found.get(2).unwrap().as_str();
            if let Some(package) = found.get(1) {
                let qualified = format!("{}.{name}", package.as_str());
                if wanted.contains(qualified.as_str()) {
                    handlers.entry(qualified).or_default().push(method);
                }
            }
            if wanted.contains(name) {
                handlers.entry(name.to_owned()).or_default().push(method);
            }
        }
    }
    let controller = regex(r"/controller(s)?/")?;
    let addon = regex(r"(?:^|/)addons/([^/]+)/")?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for key in order {
        let bare = key.rsplit('.').next().unwrap_or(key);
        let Some(candidates) = handlers.get(key).or_else(|| handlers.get(bare)) else {
            continue;
        };
        for &route in &routes[key] {
            ctx.step()?;
            let mut selected = candidates.clone();
            if selected.len() > 1 {
                let mut controllers = Vec::new();
                for &method in &selected {
                    ctx.step()?;
                    if controller.is_match(&method.file_path)? {
                        controllers.push(method);
                    }
                }
                if !controllers.is_empty() {
                    selected = controllers;
                }
                if selected.len() > 1 {
                    let scope = addon_name(&addon, &route.file_path)?;
                    let mut local = Vec::new();
                    for &method in &selected {
                        ctx.step()?;
                        if addon_name(&addon, &method.file_path)? == scope {
                            local.push(method);
                        }
                    }
                    selected = local;
                }
            }
            if selected.len() != 1 {
                continue;
            }
            let target = selected[0];
            if output.len() >= 2000
                || route.id == target.id
                || !seen.insert((&route.id, &target.id))
            {
                continue;
            }
            let mut edge = call(route, target, "goframe-route", "");
            edge.metadata = Some(
                json!({"synthesizedBy":"goframe-route","route":route.name,"requestType":bare,"registeredAt":format!("{}:{}",target.file_path,target.start_line)}),
            );
            ctx.emit(&mut output, edge)?;
        }
    }
    Ok(output)
}

fn addon_name<'a>(pattern: &fancy_regex::Regex, path: &'a str) -> Result<&'a str> {
    Ok(pattern
        .captures(path)?
        .and_then(|m| m.get(1))
        .map_or("", |m| m.as_str()))
}
fn balanced(source: &str, start: usize) -> Option<&str> {
    let mut depth = 0usize;
    for (offset, byte) in source.as_bytes()[start..].iter().enumerate() {
        if *byte == b'(' {
            depth += 1;
        } else if *byte == b')' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return source.get(start + 1..start + offset);
            }
        }
    }
    None
}
fn split_args(source: &str) -> Vec<&str> {
    let mut output = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, byte) in source.bytes().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                output.push(&source[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !source[start..].trim().is_empty() {
        output.push(&source[start..]);
    }
    output
}
