//! Markup navigation onto canonical file-based routes.
use super::*;
use crate::frameworks::frontend::{href, tables};

pub(super) fn vue_links(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    use crate::frameworks::frontend::vue_router;
    ctx.steps(ctx.nodes.len())?;
    let table = vue_router::table(ctx.nodes, ctx.sources)?;
    if table.is_empty() {
        return Ok(Vec::new());
    }
    let extension = regex(r"\.(?:vue|[cm]?[jt]sx?)$")?;
    let tag = regex(r"<(?:router-link|RouterLink|NuxtLink|nuxt-link)\b")?;
    let link = regex(
        r#"<(router-link|RouterLink|NuxtLink|nuxt-link)\b([^>]*?)\s:?to\s*=\s*(?:"([^"]*)"|'([^']*)')"#,
    )?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let mut counts = HashMap::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !extension.is_match(file)? || crate::classify::is_test_path(file) {
            continue;
        }
        let Some(app) =
            vue_router::for_file(&table, file).filter(|app| !app.routes.exact.is_empty())
        else {
            continue;
        };
        if !tag.is_match(source)? {
            continue;
        }
        for found in link.captures_iter(source) {
            ctx.step()?;
            let found = found?;
            let whole = found.get(0).unwrap();
            let value = found
                .get(3)
                .or_else(|| found.get(4))
                .map_or("", |m| m.as_str())
                .trim();
            if value.is_empty() {
                continue;
            }
            let line = ctx.line_at(file, whole.start());
            let Some(component) = ctx.enclosing(file, line)? else {
                continue;
            };
            let bound = whole.as_str().contains(":to");
            let named = if bound {
                vue_router::name(value)?
            } else {
                None
            };
            let by_name = named.as_deref().and_then(|name| app.names.get(name));
            let destinations = if let Some(&target) = by_name {
                vec![(target, named.as_ref().unwrap().clone())]
            } else {
                let destination = if bound {
                    match vue_router::path_object(value)? {
                        Some(href) => Some(href),
                        None => {
                            href::read_string(value, 0)?.and_then(|literal| href::to_href(&literal))
                        }
                    }
                } else {
                    href::to_href(value)
                };
                let Some(destination) = destination.filter(|href| href.path.starts_with('/'))
                else {
                    continue;
                };
                tables::destinations(&destination, &app.routes)?
                    .into_iter()
                    .map(|(node, href)| (node, href.display.clone()))
                    .collect()
            };
            for (target, display) in destinations {
                ctx.step()?;
                if seen.contains(&(&component.id, &target.id)) {
                    continue;
                }
                let count = counts.entry(component.id.as_str()).or_insert(0);
                *count += 1;
                if *count > 24 {
                    continue;
                }
                seen.insert((&component.id, &target.id));
                let mut edge = call(component, target, "vue-router-link", "");
                edge.kind = "navigates".into();
                edge.line = Some(line);
                let mut metadata =
                    json!({"synthesizedBy":"vue-router-link","href":display,"navMethod":"link"});
                if by_name.is_some() {
                    metadata["by"] = json!("name");
                }
                metadata["registeredAt"] = json!(format!("{file}:{line}"));
                edge.metadata = Some(metadata);
                ctx.emit(&mut output, edge)?;
            }
        }
    }
    Ok(output)
}

pub(super) fn tanstack_links(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    use crate::frameworks::frontend::tanstack;
    ctx.steps(ctx.nodes.len())?;
    let table = tanstack::table(ctx.nodes)?;
    if table.is_empty() {
        return Ok(Vec::new());
    }
    let extension = regex(r"\.(?:[cm]?[jt]sx?)$")?;
    let tag = regex(r"<(?:Link|Navigate)\b")?;
    let link = regex(r#"<(Link|Navigate)\b([^>]*?)\bto\s*=\s*(?:"([^"]*)"|'([^']*)'|\{\s*)"#)?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let mut counts = HashMap::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !extension.is_match(file)? || crate::classify::is_test_path(file) {
            continue;
        }
        let Some(routes) = tables::for_file(&table, file).filter(|routes| !routes.exact.is_empty())
        else {
            continue;
        };
        if !tag.is_match(source)? {
            continue;
        }
        let safe = strip_comments(source);
        for found in link.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let whole = found.get(0).unwrap();
            let mut literal = found
                .get(3)
                .or_else(|| found.get(4))
                .map(|m| m.as_str().to_owned());
            if literal.is_none()
                && matches!(safe.as_bytes().get(whole.end()), Some(b'\'' | b'"' | b'`'))
            {
                literal = href::read_string(&safe, whole.end())?;
            }
            let Some(literal) = literal else { continue };
            let Some(destination) = tanstack::destination(&serde_json::to_string(&literal)?)?
            else {
                continue;
            };
            let line = ctx.line_at(file, whole.start());
            let Some(component) = ctx.enclosing(file, line)? else {
                continue;
            };
            for (route, arm) in tables::destinations(&destination, routes)? {
                ctx.step()?;
                if seen.contains(&(&component.id, &route.id)) {
                    continue;
                }
                let count = counts.entry(component.id.as_str()).or_insert(0);
                *count += 1;
                if *count > 24 {
                    continue;
                }
                seen.insert((&component.id, &route.id));
                let mut edge = call(component, route, "tanstack-link", "");
                edge.kind = "navigates".into();
                edge.line = Some(line);
                edge.metadata = Some(
                    json!({"synthesizedBy":"tanstack-link","href":arm.display,"navMethod":if &found[1]=="Navigate" {"navigate"} else {"link"},"registeredAt":format!("{file}:{line}")}),
                );
                ctx.emit(&mut output, edge)?;
            }
        }
    }
    Ok(output)
}

pub(super) fn expo_returns(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(JS) {
        return Ok(Vec::new());
    }
    ctx.steps(ctx.nodes.len())?;
    let table = tables::expo(ctx.nodes)?;
    if table.exact.is_empty() {
        return Ok(Vec::new());
    }
    let extension = regex(r"\.(?:[cm]?[jt]sx?)$")?;
    let gate = regex(r"\.(?:push|replace|navigate|dismissTo)\(")?;
    let calls =
        regex(r"\.(push|replace|navigate|dismissTo)\(\s*(?:await\s+)?([A-Za-z_$][\w$.]*)\s*\(")?;
    let mut sites = Vec::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !extension.is_match(file)? || !gate.is_match(source)? {
            continue;
        }
        let safe = strip_comments(source);
        for found in calls.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            sites.push((
                file,
                ctx.line_at(file, found.get(0).unwrap().start()),
                found[1].to_owned(),
                found[2].to_owned(),
            ));
        }
    }
    type Screens<'a> = Vec<(&'a Node, String, u32, u32)>;
    let mut cached: HashMap<&str, Option<Screens<'_>>> = HashMap::new();
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (file, line, method, callee) in sites {
        ctx.step()?;
        let mut parts = callee.split('.');
        let head = parts.next().unwrap_or("");
        let bare = callee.rsplit('.').next().unwrap_or("");
        ctx.steps(ctx.names.get(bare).map_or(0, Vec::len))?;
        let candidates: Vec<_> = ctx
            .named(bare)
            .filter(|n| {
                matches!(n.kind.as_str(), "function" | "method")
                    && JS.contains(&n.language.as_str())
            })
            .collect();
        let local: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|n| n.file_path == file)
            .collect();
        let helper = if local.len() == 1 {
            local.first().copied()
        } else if local.len() > 1 {
            None
        } else {
            let imported = ctx
                .file_imports
                .get(file)
                .into_iter()
                .flatten()
                .find(|binding| binding.local_name == head || binding.local_name == bare);
            let via = imported
                .and_then(|binding| binding.mapped_path.as_deref())
                .map(|path| {
                    candidates
                        .iter()
                        .copied()
                        .filter(|n| n.file_path == path)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if via.len() == 1 {
                via.first().copied()
            } else if candidates.len() == 1 {
                candidates.first().copied()
            } else {
                None
            }
        };
        let Some(helper) = helper else {
            continue;
        };
        if !cached.contains_key(helper.id.as_str()) {
            let screens = helper_screens(ctx, helper, &table)?;
            cached.insert(&helper.id, screens);
        }
        let Some(screens) = &cached[helper.id.as_str()] else {
            continue;
        };
        for (target, display, literal_line, column) in screens {
            ctx.step()?;
            if !seen.insert((&helper.id, &target.id)) {
                continue;
            }
            let mut edge = call(helper, target, "expo-router-return", "");
            edge.kind = "navigates".into();
            edge.line = Some(*literal_line);
            edge.column = Some(*column);
            edge.metadata = Some(
                json!({"synthesizedBy":"expo-router-return","href":display,"navMethod":method,"registeredAt":format!("{file}:{line}")}),
            );
            ctx.emit(&mut output, edge)?;
        }
    }
    Ok(output)
}
type Screen<'a> = (&'a Node, String, u32, u32);
fn helper_screens<'a>(
    ctx: &Context<'a>,
    helper: &Node,
    table: &tables::Table<'a>,
) -> Result<Option<Vec<Screen<'a>>>> {
    let Some(source) = ctx.slice(helper) else {
        return Ok(None);
    };
    let normalized = source.replace("\r\n", "\n");
    let source = if ctx
        .lines(&helper.file_path)
        .is_some_and(|lines| lines.len() > helper.end_line as usize)
    {
        normalized.strip_suffix('\r').unwrap_or(&normalized)
    } else {
        &normalized
    };
    let body = strip_comments(source);
    let start = match (body.find("=>"), body.find('{')) {
        (Some(arrow), brace) if brace.is_none_or(|brace| arrow < brace) => arrow + 2,
        (_, Some(brace)) => brace + 1,
        _ => 0,
    };
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut i = start;
    while i < body.len() {
        if !matches!(body.as_bytes()[i], b'\'' | b'"' | b'`') {
            i += 1;
            continue;
        }
        ctx.step()?;
        let start = i;
        let literal = href::read_string(&body, i)?;
        i = href::string_end(&body, i)?.saturating_add(1);
        let Some(literal) = literal.filter(|value| value.starts_with('/')) else {
            continue;
        };
        let Some(href) = href::to_href(&literal) else {
            continue;
        };
        let Some(parts) = tables::normalize(&href.path, &helper.file_path)? else {
            continue;
        };
        let Some(route) = tables::matching(&parts, table) else {
            continue;
        };
        if !seen.insert(&route.id) {
            continue;
        }
        let line_start = body[..start].rfind('\n').map_or(0, |at| at + 1);
        let line = helper.start_line + body[..start].bytes().filter(|&b| b == b'\n').count() as u32;
        // Blanking preserves bytes; canonical columns are original UTF-16 units.
        let column = source[line_start..start].encode_utf16().count() as u32;
        found.push((route, href.display, line, column));
        if found.len() > 8 {
            return Ok(None);
        }
    }
    Ok((!found.is_empty()).then_some(found))
}

pub(super) fn next_links(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(JS) {
        return Ok(Vec::new());
    }
    ctx.steps(ctx.nodes.len())?;
    let table = tables::next(ctx.nodes)?;
    if table.exact.is_empty() {
        return Ok(Vec::new());
    }
    let extension = regex(r"\.(?:[cm]?[jt]sx?|mdx)$")?;
    let link = regex(r#"<(Link|NextLink|a)\b([^>]*?)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|\{\s*)"#)?;
    let pathname = regex(r"\bpathname\s*:\s*")?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let mut counts = HashMap::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !extension.is_match(file)?
            || crate::classify::is_test_path(file)
            || !table.roots.iter().any(|root| file.starts_with(root))
            || !source.contains("href")
        {
            continue;
        }
        let safe = strip_comments(source);
        for found in link.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let tag = &found[1];
            let whole = found.get(0).unwrap();
            let mut literal = found
                .get(3)
                .or_else(|| found.get(4))
                .map(|m| m.as_str().to_owned());
            if literal.is_none() {
                let at = whole.end();
                if matches!(safe.as_bytes().get(at), Some(b'\'' | b'"' | b'`')) {
                    literal = href::read_string(&safe, at)?;
                } else if safe.as_bytes().get(at) == Some(&b'{') {
                    // Preserve the reference's sticky regexp offset on its sliced head.
                    let end = safe[at..]
                        .find('}')
                        .map_or(safe.len(), |offset| at + offset);
                    let head = &safe[at..end];
                    let units = safe[..at].encode_utf16().count();
                    let start = utf16_byte(head, units);
                    if let Some(start) = start
                        && let Some(matched) = pathname.find_from_pos(head, start)?
                        && matched.start() == start
                    {
                        literal = href::read_string(&safe, at + matched.end())?;
                    }
                }
            }
            let Some(literal) = literal else {
                continue;
            };
            if tag == "a" && !literal.starts_with('/') {
                continue;
            }
            let Some(href) = href::to_href(&literal) else {
                continue;
            };
            let line = ctx.line_at(file, whole.start());
            let Some(component) = ctx.enclosing(file, line)? else {
                continue;
            };
            for (page, arm) in tables::destinations(&href, &table)? {
                ctx.step()?;
                if seen.contains(&(&component.id, &page.id)) {
                    continue;
                }
                let count = counts.entry(component.id.as_str()).or_insert(0);
                *count += 1;
                if *count > 24 {
                    continue;
                }
                seen.insert((&component.id, &page.id));
                let mut edge = call(component, page, "next-link", "");
                edge.kind = "navigates".into();
                edge.line = Some(line);
                edge.metadata = Some(
                    json!({"synthesizedBy":"next-link","href":arm.display,"navMethod":if tag=="a"{"a"}else{"link"},"registeredAt":format!("{file}:{line}")}),
                );
                ctx.emit(&mut output, edge)?;
            }
        }
    }
    Ok(output)
}
pub(super) fn svelte_pages(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["svelte"]) {
        return Ok(Vec::new());
    }
    ctx.steps(ctx.nodes.len())?;
    let table = tables::svelte(ctx.nodes)?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for routes in table.values() {
        for &page in routes.exact.values() {
            ctx.step()?;
            if !seen.insert(&page.id) {
                continue;
            }
            if let Some(component) = ctx.in_file(&page.file_path).find(|n| n.kind == "component") {
                let mut edge = call(page, component, "sveltekit-page", "");
                edge.line = Some(component.start_line);
                edge.metadata =
                    Some(json!({"synthesizedBy":"sveltekit-page","registeredAt":page.file_path}));
                ctx.emit(&mut output, edge)?;
            }
        }
    }
    Ok(output)
}
pub(super) fn svelte_links(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["svelte"]) {
        return Ok(Vec::new());
    }
    ctx.steps(ctx.nodes.len())?;
    let table = tables::svelte(ctx.nodes)?;
    if table.is_empty() {
        return Ok(Vec::new());
    }
    let link = regex(r#"<a\b([^>]*?)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|\{\s*)"#)?;
    let interpolation = regex(r"\{[^}]*\}")?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let mut counts = HashMap::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".svelte")
            || crate::classify::is_test_path(file)
            || !source.contains("href")
        {
            continue;
        }
        let Some(routes) = tables::for_file(&table, file) else {
            continue;
        };
        if routes.exact.is_empty() {
            continue;
        }
        for found in link.captures_iter(source) {
            ctx.step()?;
            let found = found?;
            let whole = found.get(0).unwrap();
            let mut literal = found
                .get(2)
                .or_else(|| found.get(3))
                .map(|m| m.as_str().to_owned());
            if literal.is_none() {
                literal = href::read_string(source, whole.end())?;
            }
            let Some(literal) = literal else {
                continue;
            };
            if !literal.starts_with('/') {
                continue;
            }
            let Some(href) = href::to_href(&interpolation.replace_all(&literal, "\0")) else {
                continue;
            };
            let line = ctx.line_at(file, whole.start());
            let Some(component) = ctx.enclosing(file, line)? else {
                continue;
            };
            for (page, arm) in tables::destinations(&href, routes)? {
                ctx.step()?;
                if seen.contains(&(&component.id, &page.id)) {
                    continue;
                }
                let count = counts.entry(component.id.as_str()).or_insert(0);
                *count += 1;
                if *count > 24 {
                    continue;
                }
                seen.insert((&component.id, &page.id));
                let mut edge = call(component, page, "sveltekit-link", "");
                edge.kind = "navigates".into();
                edge.line = Some(line);
                edge.metadata = Some(
                    json!({"synthesizedBy":"sveltekit-link","href":arm.display,"navMethod":"a","registeredAt":format!("{file}:{line}")}),
                );
                ctx.emit(&mut output, edge)?;
            }
        }
    }
    Ok(output)
}
fn utf16_byte(source: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (at, ch) in source.char_indices() {
        if units == offset {
            return Some(at);
        }
        units += ch.len_utf16();
    }
    (units == offset).then_some(source.len())
}

pub(super) fn react_links(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    use crate::frameworks::frontend::{object, react_router, react_tables};
    let routes = react_router::table(ctx.nodes)?;
    if routes.is_empty() {
        return Ok(Vec::new());
    }
    let module = |specifier: &str, from: &str, language: &str| {
        Ok((ctx.resolve_module)(specifier, from, language))
    };
    let context = react_tables::Context::new(ctx.nodes, ctx.sources, &module)?;
    let extension = regex(r"\.(?:[cm]?[jt]sx?|mdx)$")?;
    let wrapper = regex(r"\bstyled\s*\(\s*(?:Link|NavLink)\s*\)")?;
    let import = regex(r#"\bfrom\s*['"]react-router(?:-dom)?['"]"#)?;
    let mut named = HashMap::<&str, Vec<String>>::new();
    let mut defaults = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !extension.is_match(file)?
            || crate::classify::is_test_path(file)
            || !source.contains("styled")
            || !wrapper.is_match(source)?
            || !import.is_match(source)?
        {
            continue;
        }
        if regex(r"\bexport\s+default\s+styled\s*\(\s*(?:Link|NavLink)\s*\)")?.is_match(source)? {
            defaults.insert(file);
        }
        for found in regex(r"\b(?:export\s+)?(?:const|let)\s+([A-Z][\w$]*)\s*=\s*styled\s*\(\s*(?:Link|NavLink)\s*\)")?.captures_iter(source){let found=found?;let names=named.entry(file).or_default();if !names.iter().any(|name|name==&found[1]){names.push(found[1].into());}}
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let mut counts = HashMap::<&str, usize>::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !extension.is_match(file)?
            || crate::classify::is_test_path(file)
            || !regex(r"\bto\s*=")?.is_match(source)?
        {
            continue;
        }
        let Some(table) = tables::for_file(&routes, file).filter(|table| !table.exact.is_empty())
        else {
            continue;
        };
        let mut tags = [
            "Link",
            "NavLink",
            "Navigate",
            "Redirect",
            "LinkContainer",
            "IndexLinkContainer",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        tags.extend(named.get(file).into_iter().flatten().cloned());
        if !named.is_empty() || !defaults.is_empty() {
            for binding in ctx.file_imports.get(file).into_iter().flatten() {
                let Some(target) = binding.resolved_path.as_deref() else {
                    continue;
                };
                if binding.is_default && defaults.contains(target)
                    || named
                        .get(target)
                        .is_some_and(|names| names.contains(&binding.imported_name))
                {
                    tags.push(binding.local_name.clone());
                }
            }
        }
        let alternatives = tags
            .iter()
            .map(|name| regex::escape(name))
            .collect::<Vec<_>>()
            .join("|");
        if !regex(&format!(r"<(?:{alternatives})\b"))?.is_match(source)? {
            continue;
        }
        let safe = comments::strip_for_regex(source, "typescript");
        let pattern = regex(&format!(
            r#"<({alternatives})\b((?:[^>]|=>)*?)\bto\s*=\s*(?:"([^"]*)"|'([^']*)'|(?=\{{))"#
        ))?;
        for found in pattern.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let whole = found.get(0).unwrap();
            let tag = &found[1];
            let destination = if let Some(quoted) = found.get(3).or_else(|| found.get(4)) {
                href::to_href(quoted.as_str())
            } else {
                let at = whole.end();
                let Some(close) = object::matching(&safe, at)? else {
                    continue;
                };
                let expression = &safe[at + 1..close];
                let destination = href::parse(expression)?;
                if destination.is_none() {
                    if let Some(configured) = context.configured_href(expression, file)? {
                        href::parse(&configured)?
                    } else {
                        None
                    }
                } else {
                    destination
                }
            };
            let Some(destination) = destination.filter(|href| href.path.starts_with('/')) else {
                continue;
            };
            let line = ctx.line_at(file, whole.start());
            let Some(component) = ctx.enclosing(file, line)? else {
                continue;
            };
            for (route, arm) in tables::destinations(&destination, table)? {
                ctx.step()?;
                if seen.contains(&(&component.id, &route.id)) {
                    continue;
                }
                let count = counts.entry(&component.id).or_default();
                *count += 1;
                if *count > 24 {
                    continue;
                }
                seen.insert((&component.id, &route.id));
                ctx.emit(&mut output,Edge{source:component.id.clone(),target:route.id.clone(),kind:"navigates".into(),line:Some(line),column:None,provenance:Some("heuristic".into()),metadata:Some(json!({"synthesizedBy":"react-router-link","href":arm.display,"navMethod":match tag{"Navigate"=>"navigate","Redirect"=>"redirect",_=>"link"},"registeredAt":format!("{file}:{line}")}))})?;
            }
        }
    }
    Ok(output)
}
