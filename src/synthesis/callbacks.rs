//! Observer stores, closure collections, and named emitter callbacks.
use super::*;

pub(super) fn fields(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let registrar = regex(
        r"^(on[A-Z]\w*|subscribe|addListener|addEventListener|register|watch|listen|addCallback)$",
    )?;
    let dispatcher = regex(r"(?i)(emit|trigger|notify|dispatch|fire|publish|flush)")?;
    let field = regex(r"this\.(\w+)\.(?:add|push|set)\(")?;
    let for_of = regex(r"\bof\s+(?:Array\.from\(\s*)?this\.(\w+)")?;
    let call_like = regex(r"\b\w+\s*\(")?;
    let for_each = regex(r"this\.(\w+)\.forEach\(")?;
    let mut registrars = Vec::new();
    let mut dispatchers = Vec::new();
    for kind in ["method", "function"] {
        for node in ctx.nodes.iter().filter(|n| n.kind == kind) {
            ctx.step()?;
            let is_registrar = registrar.is_match(&node.name)?;
            let is_dispatcher = dispatcher.is_match(&node.name)?;
            if !is_registrar && !is_dispatcher {
                continue;
            }
            let Some(source) = ctx.slice(node).filter(|s| !s.is_empty()) else {
                continue;
            };
            if is_registrar && let Some(found) = field.captures(source)? {
                registrars.push((node, found.get(1).unwrap().as_str().to_owned()));
            }
            if is_dispatcher {
                let for_of = for_of.captures(source)?;
                let found = if for_of.is_some() && call_like.is_match(source)? {
                    for_of
                } else {
                    for_each.captures(source)?
                };
                if let Some(found) = found {
                    dispatchers.push((node, found.get(1).unwrap().as_str().to_owned()));
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (registrar, field) in registrars {
        ctx.steps(dispatchers.len())?;
        let channel: Vec<_> = dispatchers
            .iter()
            .filter(|(d, f)| d.file_path == registrar.file_path && *f == field)
            .map(|(d, _)| *d)
            .collect();
        if channel.is_empty() {
            continue;
        }
        let argument = regex(&format!(
            r"{}\s*\(\s*(this\.\w+|\w+)\s*(?=[,)])",
            ::regex::escape(&registrar.name)
        ))?;
        let mut added = 0;
        for edge in ctx.incoming(&registrar.id).filter(|e| e.kind == "calls") {
            ctx.step()?;
            if added >= CALLBACK_CAP {
                break;
            }
            let Some(line) = edge.line.filter(|line| *line > 0) else {
                continue;
            };
            let Some(caller) = ctx.node(&edge.source) else {
                continue;
            };
            let Some(source_line) = ctx.line(&caller.file_path, line) else {
                continue;
            };
            let Some(found) = argument.captures(source_line)? else {
                continue;
            };
            let name = found.get(1).unwrap().as_str();
            let references: Vec<_> = ctx
                .outgoing(&caller.id)
                .filter(|r| {
                    r.kind == "references"
                        && r.line == edge.line
                        && r.metadata
                            .as_ref()
                            .is_some_and(|m| m["fnRef"] == true && m["refName"] == name)
                })
                .collect();
            if references.len() != 1 {
                continue;
            }
            let reference = references[0];
            let Some(callback) = ctx
                .node(&reference.target)
                .filter(|n| matches!(n.kind.as_str(), "method" | "function"))
            else {
                continue;
            };
            if !name.starts_with("this.")
                && callback.file_path != caller.file_path
                && reference
                    .metadata
                    .as_ref()
                    .is_none_or(|m| m["resolvedBy"] != "import")
            {
                continue;
            }
            for dispatcher in &channel {
                ctx.step()?;
                if dispatcher.id == callback.id || !seen.insert((&dispatcher.id, &callback.id)) {
                    continue;
                }
                let mut derived = call(dispatcher, callback, "callback", &registrar.name);
                derived.metadata = Some(
                    json!({"synthesizedBy":"callback","via":registrar.name,"field":field,"registeredAt":format!("{}:{line}",caller.file_path)}),
                );
                ctx.emit(&mut out, derived)?;
                added += 1;
            }
        }
    }
    Ok(out)
}

pub(super) fn collections(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    type Sites<'a> = HashMap<String, Vec<(&'a Node, u32)>>;
    let dispatch = regex(r"(\w+)\.forEach\s*\{\s*(?:\$0|it)\s*\(")?;
    let append_write =
        regex(r"(\w+)\.write\s*\{\s*\$0(?:\.(\w+))?\.(?:append|add|push|insert)\s*\(")?;
    let append_direct = regex(r"(\w+)\.(?:append|add|push|insert)\s*\(")?;
    let mut dispatchers: Sites<'_> = HashMap::new();
    let mut registrars: Sites<'_> = HashMap::new();
    let mut order = Vec::new();
    for kind in ["method", "function"] {
        for node in ctx
            .nodes
            .iter()
            .filter(|n| n.kind == kind && matches!(n.language.as_str(), "swift" | "kotlin"))
        {
            ctx.step()?;
            let Some(source) = ctx.slice(node).filter(|s| !s.is_empty()) else {
                continue;
            };
            let has_each = source.contains(".forEach");
            let has_append = [".append(", ".add(", ".push(", ".insert("]
                .iter()
                .any(|needle| source.contains(needle));
            if !has_each && !has_append {
                continue;
            }
            let newlines: Vec<_> = source
                .bytes()
                .enumerate()
                .filter_map(|(i, b)| (b == b'\n').then_some(i))
                .collect();
            let line_at = |at| node.start_line + newlines.partition_point(|&line| line < at) as u32;
            if has_each {
                for found in dispatch.captures_iter(source) {
                    ctx.step()?;
                    let found = found?;
                    let field = found.get(1).unwrap().as_str();
                    if !dispatchers.contains_key(field) {
                        order.push(field.to_owned());
                    }
                    let sites = dispatchers.entry(field.into()).or_default();
                    if !sites.iter().any(|(n, _)| n.id == node.id) {
                        sites.push((node, line_at(found.get(0).unwrap().start())));
                    }
                }
            }
            if has_append {
                for (pattern, nested) in [(&append_write, true), (&append_direct, false)] {
                    for found in pattern.captures_iter(source) {
                        ctx.step()?;
                        let found = found?;
                        let field = if nested {
                            found.get(2).or_else(|| found.get(1))
                        } else {
                            found.get(1)
                        }
                        .unwrap()
                        .as_str();
                        if field.is_empty() || field.bytes().all(|b| b.is_ascii_digit()) {
                            continue;
                        }
                        let sites = registrars.entry(field.into()).or_default();
                        if !sites.iter().any(|(n, _)| n.id == node.id) {
                            sites.push((node, line_at(found.get(0).unwrap().start())));
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for field in order {
        let disps = &dispatchers[&field];
        let Some(regs) = registrars.get(&field) else {
            continue;
        };
        if disps.len() > 8 || regs.len() > 8 {
            continue;
        }
        for (dispatcher, line) in disps {
            for (registrar, registered_line) in regs {
                ctx.step()?;
                if dispatcher.id == registrar.id || !seen.insert((&dispatcher.id, &registrar.id)) {
                    continue;
                }
                let mut edge = call(dispatcher, registrar, "closure-collection", "");
                edge.line = Some(*line);
                edge.metadata = Some(
                    json!({"synthesizedBy":"closure-collection","field":field,"registeredAt":format!("{}:{registered_line}",registrar.file_path)}),
                );
                ctx.emit(&mut out, edge)?;
            }
        }
    }
    Ok(out)
}

pub(super) fn emitter(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let emit = regex(r#"\.(?:emit|fire|dispatchEvent)\(\s*['"]([^'"]+)['"]"#)?;
    let on = regex(
        r#"\.(?:on|once|addListener)\(\s*['"]([^'"]+)['"]\s*,\s*(?:function\s+(\w+)|(?:this\.)?(\w+))"#,
    )?;
    let mut order = Vec::new();
    let mut dispatchers: HashMap<String, Vec<&Node>> = HashMap::new();
    let mut handlers: HashMap<String, Vec<(&Node, String)>> = HashMap::new();
    for (&path, &source) in ctx.sources {
        ctx.step()?;
        let has_emit = [".emit(", ".fire(", ".dispatchEvent("]
            .iter()
            .any(|needle| source.contains(needle));
        let has_on = [".on(", ".once(", ".addListener("]
            .iter()
            .any(|needle| source.contains(needle));
        if has_emit {
            for found in emit.captures_iter(source) {
                ctx.step()?;
                let found = found?;
                let line = ctx.line_at(path, found.get(0).unwrap().start());
                let mut enclosing: Option<&Node> = None;
                for node in ctx.in_file(path) {
                    ctx.step()?;
                    if matches!(node.kind.as_str(), "method" | "function" | "component")
                        && node.start_line <= line
                        && node.end_line >= line
                        && enclosing.is_none_or(|best| node.start_line >= best.start_line)
                    {
                        enclosing = Some(node);
                    }
                }
                let Some(enclosing) = enclosing else {
                    continue;
                };
                let event = found.get(1).unwrap().as_str();
                if !dispatchers.contains_key(event) {
                    order.push(event.to_owned());
                }
                let nodes = dispatchers.entry(event.into()).or_default();
                if !nodes.iter().any(|n| n.id == enclosing.id) {
                    nodes.push(enclosing);
                }
            }
        }
        if has_on {
            for found in on.captures_iter(source) {
                ctx.step()?;
                let found = found?;
                let name = found.get(2).or_else(|| found.get(3)).unwrap().as_str();
                let Some(handler) = ctx.named(name).find(|n| {
                    matches!(n.kind.as_str(), "function" | "method")
                        && (!manual_mock(&n.file_path) || crate::classify::is_test_path(path))
                }) else {
                    continue;
                };
                let event = found.get(1).unwrap().as_str();
                let site = format!(
                    "{path}:{}",
                    ctx.line_at(path, found.get(0).unwrap().start())
                );
                let sites = handlers.entry(event.into()).or_default();
                if let Some(existing) = sites.iter_mut().find(|(n, _)| n.id == handler.id) {
                    existing.1 = site;
                } else {
                    sites.push((handler, site));
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for event in order {
        let dispatchers = &dispatchers[&event];
        let Some(handlers) = handlers.get(&event) else {
            continue;
        };
        if dispatchers.len() > 6 || handlers.len() > 6 {
            continue;
        }
        for dispatcher in dispatchers {
            for (handler, site) in handlers {
                ctx.step()?;
                if dispatcher.id == handler.id || !seen.insert((&dispatcher.id, &handler.id)) {
                    continue;
                }
                let mut edge = call(dispatcher, handler, "event-emitter", "");
                edge.line = None;
                edge.metadata = Some(
                    json!({"synthesizedBy":"event-emitter","event":event,"registeredAt":site}),
                );
                ctx.emit(&mut out, edge)?;
            }
        }
    }
    Ok(out)
}
