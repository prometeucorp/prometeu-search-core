//! Generated store hooks and named action dispatch.
use super::*;

pub(super) fn rtk_query(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(JS) {
        return Ok(Vec::new());
    }
    let derive = regex(r"^use([A-Z][A-Za-z0-9]*?)(?:Query|Mutation)$")?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for hook in ctx.nodes.iter().filter(|n| {
        n.kind == "function" && n.signature.as_deref() == Some("= RTK Query generated hook")
    }) {
        ctx.step()?;
        let Some(found) = derive.captures(&hook.name)? else {
            continue;
        };
        let name = found.get(1).unwrap().as_str();
        let name = name.strip_prefix("Lazy").unwrap_or(name);
        if name.is_empty() {
            continue;
        }
        let endpoint = format!("{}{}", name[..1].to_ascii_lowercase(), &name[1..]);
        ctx.steps(ctx.names.get(endpoint.as_str()).map_or(0, Vec::len))?;
        let target = ctx
            .named(&endpoint)
            .find(|n| n.kind == "function" && n.file_path == hook.file_path);
        if let Some(target) = target
            && hook.id != target.id
            && seen.insert((&hook.id, &target.id))
        {
            ctx.emit(
                &mut output,
                jobs::dispatched(
                    hook,
                    target,
                    "rtk-query",
                    &endpoint,
                    &hook.file_path,
                    hook.start_line,
                ),
            )?;
        }
    }
    Ok(output)
}

pub(super) fn pinia(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["vue", "typescript", "javascript", "tsx", "jsx"]) {
        return Ok(Vec::new());
    }
    let factories = regex(r"\b(?:export\s+)?const\s+(\w+)\s*=\s*defineStore\s*\(")?;
    let bindings = regex(r"\bconst\s+(\w+)\s*=\s*(?:await\s+)?(\w+)\s*\(")?;
    let calls = regex(r"(\w+)\s*\.\s*(\w+)\s*\(")?;
    let mut factory_file = HashMap::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !consumer(file) || !source.contains("defineStore") {
            continue;
        }
        for found in factories.captures_iter(source) {
            ctx.step()?;
            let found = found?;
            factory_file.insert(found.get(1).unwrap().as_str(), file);
        }
    }
    if factory_file.is_empty() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !consumer(file) || !source.contains("Store") {
            continue;
        }
        let safe = strip_comments(source);
        let mut var_store = HashMap::new();
        for found in bindings.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            if let Some(&store) = factory_file.get(found.get(2).unwrap().as_str()) {
                var_store.insert(found.get(1).unwrap().as_str(), store);
            }
        }
        if var_store.is_empty() {
            continue;
        }
        let fallback = ctx.in_file(file).find(|n| n.kind == "component");
        let mut added = 0;
        for found in calls.captures_iter(&safe) {
            ctx.step()?;
            if added >= 80 {
                break;
            }
            let found = found?;
            let Some(&store) = var_store.get(found.get(1).unwrap().as_str()) else {
                continue;
            };
            let method = found.get(2).unwrap().as_str();
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let Some(from) = ctx.enclosing(file, line)?.or(fallback) else {
                continue;
            };
            ctx.steps(ctx.names.get(method).map_or(0, Vec::len))?;
            let target = ctx
                .named(method)
                .find(|n| n.kind == "function" && n.file_path == store);
            if let Some(target) = target
                && target.id != from.id
                && seen.insert((&from.id, &target.id))
            {
                ctx.emit(
                    &mut output,
                    jobs::dispatched(from, target, "pinia-store", method, file, line),
                )?;
                added += 1;
            }
        }
    }
    Ok(output)
}

pub(super) fn vuex(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["vue", "typescript", "javascript", "tsx", "jsx"]) {
        return Ok(Vec::new());
    }
    let dispatch = regex(r#"\b(?:dispatch|commit)\s*\(\s*['"]([A-Za-z][\w/]*)['"]"#)?;
    let signal = regex(
        r"\bdefineStore\b|\bcreateStore\b|\bVuex\b|\bmutations\b|\bactions\b|\bgetters\b|\bnamespaced\b",
    )?;
    let mut store_files = HashMap::new();
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !consumer(file) || !source.contains("dispatch(") && !source.contains("commit(") {
            continue;
        }
        let safe = strip_comments(source);
        let fallback = ctx.in_file(file).find(|n| n.kind == "component");
        let mut added = 0;
        for found in dispatch.captures_iter(&safe) {
            ctx.step()?;
            if added >= 120 {
                break;
            }
            let found = found?;
            let key = found.get(1).unwrap().as_str();
            let mut parts = key.rsplit('/');
            let action = parts.next().unwrap_or(key);
            let namespace = parts.next();
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let Some(from) = ctx.enclosing(file, line)?.or(fallback) else {
                continue;
            };
            let mut candidates = Vec::new();
            for node in ctx.named(action).filter(|n| n.kind == "function") {
                ctx.step()?;
                let store = if let Some(&store) = store_files.get(node.file_path.as_str()) {
                    store
                } else {
                    let mut signals = HashSet::new();
                    if let Some(source) = ctx.sources.get(node.file_path.as_str()) {
                        for found in signal.find_iter(source) {
                            ctx.step()?;
                            signals.insert(found?.as_str());
                            if signals.len() >= 2 {
                                break;
                            }
                        }
                    }
                    let store = signals.len() >= 2;
                    store_files.insert(node.file_path.as_str(), store);
                    store
                };
                if store {
                    candidates.push(node);
                }
            }
            let preferred = if let Some(namespace) = namespace {
                let path = regex(&format!(r"[\\/]{}[\\/.]", ::regex::escape(namespace)))?;
                let mut matching = None;
                for &candidate in &candidates {
                    ctx.step()?;
                    if path.is_match(&candidate.file_path)? {
                        matching = Some(candidate);
                        break;
                    }
                }
                matching
            } else {
                candidates.iter().copied().find(|n| n.file_path == file)
            };
            let target = preferred.or_else(|| {
                if candidates.len() == 1 {
                    candidates.first().copied()
                } else {
                    None
                }
            });
            if let Some(target) = target
                && target.id != from.id
                && seen.insert((&from.id, &target.id))
            {
                ctx.emit(
                    &mut output,
                    jobs::dispatched(from, target, "vuex-dispatch", key, file, line),
                )?;
                added += 1;
            }
        }
    }
    Ok(output)
}

fn consumer(file: &str) -> bool {
    file.rsplit('.').next().is_some_and(|extension| {
        matches!(
            extension,
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "vue"
        )
    })
}
