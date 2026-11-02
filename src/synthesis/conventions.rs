use super::*;

pub(super) fn pascal_forms(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut out = Vec::new();
    for &file in ctx.sources.keys() {
        ctx.step()?;
        let Some((stem, extension)) = file.rsplit_once('.') else {
            continue;
        };
        if !extension.eq_ignore_ascii_case("dfm") && !extension.eq_ignore_ascii_case("fmx") {
            continue;
        }
        let unit_path = format!("{stem}.pas");
        if !ctx.sources.contains_key(unit_path.as_str()) {
            continue;
        }
        let Some(form) = ctx.in_file(file).find(|n| n.kind == "file") else {
            continue;
        };
        let Some(unit) = ctx.in_file(&unit_path).find(|n| n.kind == "file") else {
            continue;
        };
        let mut edge = call(unit, form, "pascal-form", "");
        edge.kind = "references".into();
        edge.metadata = Some(json!({"synthesizedBy":"pascal-form","registeredAt":unit_path}));
        ctx.emit(&mut out, edge)?;
    }
    Ok(out)
}

pub(super) fn svelte_load(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["svelte"]) {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let pattern = regex(r"(.*/)(\+(?:page|layout))\.svelte$")?;
    for &file in ctx.sources.keys() {
        ctx.step()?;
        let Some(found) = pattern.captures(file)? else {
            continue;
        };
        let Some(page) = ctx.in_file(file).find(|n| n.kind == "component") else {
            continue;
        };
        let directory = found.get(1).unwrap().as_str();
        let prefix = found.get(2).unwrap().as_str();
        for extension in [".server.ts", ".server.js", ".ts", ".js"] {
            let loader = format!("{directory}{prefix}{extension}");
            if !ctx.sources.contains_key(loader.as_str()) {
                continue;
            }
            for hook in ctx.in_file(&loader) {
                ctx.step()?;
                let named = matches!(
                    hook.kind.as_str(),
                    "function" | "method" | "constant" | "variable"
                ) && matches!(hook.name.as_str(), "load" | "actions");
                if !named && !matches!(hook.kind.as_str(), "function" | "method") {
                    continue;
                }
                ctx.emit(&mut out, call(page, hook, "sveltekit-load", &hook.name))?;
            }
        }
    }
    Ok(out)
}

pub(super) fn redux_thunks(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(JS) {
        return Ok(Vec::new());
    }
    let declaration = regex(r"create(?:Async)?Thunk")?;
    let dispatch = regex(r"\bdispatch\s*\(\s*([A-Za-z_]\w*)\s*[(),]")?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for node in ctx.nodes.iter().filter(|n| n.kind == "constant") {
        ctx.step()?;
        let Some(signature) = node.signature.as_deref() else {
            continue;
        };
        if !declaration.is_match(signature)? {
            continue;
        }
        let Some(source) = ctx.slice(node).filter(|s| !s.is_empty()) else {
            continue;
        };
        let safe = strip_comments(source);
        let newlines: Vec<_> = safe
            .bytes()
            .enumerate()
            .filter_map(|(i, b)| (b == b'\n').then_some(i))
            .collect();
        let mut added = 0;
        for found in dispatch.captures_iter(&safe) {
            ctx.step()?;
            if added >= 24 {
                break;
            }
            let found = found?;
            let name = found.get(1).unwrap().as_str();
            if name == node.name {
                continue;
            }
            let candidates: Vec<_> = ctx
                .named(name)
                .filter(|n| matches!(n.kind.as_str(), "constant" | "function" | "method"))
                .collect();
            let mut target = None;
            for &candidate in &candidates {
                if let Some(signature) = &candidate.signature
                    && declaration.is_match(signature)?
                {
                    target = Some(candidate);
                    break;
                }
            }
            let target = target
                .or_else(|| candidates.iter().copied().find(|n| n.kind == "constant"))
                .or_else(|| {
                    candidates
                        .iter()
                        .copied()
                        .find(|n| n.file_path == node.file_path)
                })
                .or_else(|| candidates.first().copied());
            let Some(target) = target.filter(|target| target.id != node.id) else {
                continue;
            };
            if !seen.insert((&node.id, &target.id)) {
                continue;
            }
            let line = node.start_line
                + newlines.partition_point(|&at| at < found.get(0).unwrap().start()) as u32;
            let mut edge = call(node, target, "redux-thunk", name);
            edge.line = Some(line);
            edge.metadata = Some(
                json!({"synthesizedBy":"redux-thunk","via":name,"registeredAt":format!("{}:{line}",node.file_path)}),
            );
            ctx.emit(&mut out, edge)?;
            added += 1;
        }
    }
    Ok(out)
}
