use super::*;
use std::sync::LazyLock;
const GLOBAL: &str = r"(?:window\s*\.\s*)?__TAURI(?:_INTERNALS)?__";
struct Patterns {
    file: fancy_regex::Regex,
    gate: fancy_regex::Regex,
    global: fancy_regex::Regex,
    global_binding: fancy_regex::Regex,
    dynamic: fancy_regex::Regex,
    event_binding: fancy_regex::Regex,
    project: fancy_regex::Regex,
    name: fancy_regex::Regex,
    handler: fancy_regex::Regex,
    parameter: fancy_regex::Regex,
    member: fancy_regex::Regex,
}
fn patterns() -> &'static Patterns {
    static P: LazyLock<Patterns> = LazyLock::new(|| {
        let r = |s: &str| regex(s).expect("valid reference pattern");
        Patterns {
            file: r(r"\.(?:[cm]?[jt]sx?|vue|svelte)$"),
            gate: r(r"invoke|listen|once|\bemit|\bcommands\b|\bevents\b|__TAURI"),
            global: r(&format!(
                r"(?<![\w$.]){GLOBAL}\s*\.\s*(?:(?:core|tauri)\s*\.\s*)?invoke\b"
            )),
            global_binding: r(&format!(
                r"\b(?:const|let|var)\s*(?:\{{[^}}]*?\binvoke\b(?:\s*:\s*([\w$]+))?[^}}]*\}}\s*=\s*{GLOBAL}(?:\s*\.\s*(?:core|tauri))?\b|([\w$]+)\s*=\s*{GLOBAL}(?:\s*\.\s*(?:core|tauri))?\s*\.\s*invoke\b)"
            )),
            dynamic: r(
                r#"\{([^{}]*)\}\s*=\s*await\s+import\(\s*['"](@tauri-apps/api[^'"]*)['"]\s*\)"#,
            ),
            event_binding: r(&format!(
                r"\b(?:const|let|var)\s*\{{([^}}]*)\}}\s*=\s*{GLOBAL}\s*\.\s*event\b"
            )),
            project: r(r"^(?:\.{1,2}/|[~@]/|#/|[#$]\w)"),
            name: r(r"^[A-Za-z_$][\w$]*$"),
            handler: r(r"^(?:this\s*\.\s*)?([A-Za-z_$][\w$]*)$"),
            parameter: r(r"^[^(]*\(\s*([A-Za-z_$][\w$]*)"),
            member: r(r"^\.\s*(listen|once|emit)(?![\w$])"),
        }
    });
    &P
}
#[derive(Default)]
struct Bindings {
    invoke: Vec<String>,
    invoke_holders: Vec<String>,
    listen: Vec<String>,
    emit: Vec<String>,
    emit_to: Vec<String>,
    dynamic: Vec<String>,
    event_holders: Vec<String>,
    handles: bool,
    typed_commands: Vec<String>,
    typed_events: Vec<String>,
}
struct Scan<'a> {
    file: File<'a>,
    imports: Vec<&'a ImportBinding>,
    language: &'static str,
    bindings: Bindings,
}
#[derive(Clone)]
struct Forwarder {
    name: String,
    file: String,
}
#[derive(Clone, Copy)]
struct Call {
    at: usize,
    open: usize,
}
fn bare(name: &str) -> Result<fancy_regex::Regex> {
    regex(&format!(r"(?<![\w$.]){}(?![\w$])", regex::escape(name)))
}
fn holder(name: &str, member: &str) -> Result<fancy_regex::Regex> {
    regex(&format!(
        r"(?<![\w$.]){}\s*\.\s*{member}(?![\w$])",
        regex::escape(name)
    ))
}
fn skip_types(file: &File<'_>, at: usize) -> usize {
    if file.code.as_bytes().get(at) != Some(&b'<') {
        return at;
    }
    let mut depth = 0;
    for i in at..file.limit(at, 40_000) {
        let c = file.code.as_bytes()[i];
        if c == b'<' {
            depth += 1
        } else if c == b'>' && file.code.as_bytes().get(i.wrapping_sub(1)) != Some(&b'=') {
            depth -= 1;
            if depth == 0 {
                return skip_space(&file.code, i + 1);
            }
        }
    }
    at
}
fn calls(ctx: &Context<'_>, file: &File<'_>, pattern: &fancy_regex::Regex) -> Result<Vec<Call>> {
    ctx.steps(file.code.len() / 32 + 1)?;
    let mut out = Vec::new();
    for m in pattern.find_iter(&file.code) {
        let m = m?;
        ctx.step()?;
        let open = skip_types(file, skip_space(&file.code, m.end()));
        if file.code.as_bytes().get(open) == Some(&b'(') {
            out.push(Call {
                at: m.start(),
                open,
            })
        }
    }
    Ok(out)
}
fn js_string(file: &File<'_>, span: Option<&(usize, usize)>) -> Result<Option<String>> {
    let Some(&(start, end)) = span else {
        return Ok(None);
    };
    let at = skip_space(&file.text, start);
    if let Some(&quote @ (b'\'' | b'"' | b'`')) = file.text.as_bytes().get(at) {
        let Some(value) = href::read_string(&file.text, at)? else {
            return Ok(None);
        };
        if value.contains(href::HOLE) {
            return Ok(None);
        }
        let mut i = at + 1;
        while i < file.text.len() {
            let c = file.text.as_bytes()[i];
            i += 1;
            if c == b'\\' {
                i = (i + 1).min(file.text.len())
            } else if c == quote {
                break;
            }
        }
        return Ok((trim(&file.text[at..end]).len() == i - at).then_some(value));
    }
    let name = trim(&file.text[start..end]);
    if !patterns().name.is_match(name)? {
        return Ok(None);
    }
    let pattern = regex(&format!(
        r#"\bconst\s+{}\s*(?::[^=\n]*)?=\s*(['"`])([^'"`\n$]*)\1"#,
        regex::escape(name)
    ))?;
    let Some(m) = pattern.captures(&file.text)? else {
        return Ok(None);
    };
    let declared = file.owner(m.get(0).unwrap().start());
    if declared.is_some_and(|node| matches!(node.kind.as_str(), "function" | "method"))
        && declared.map(|n| &n.id) != file.owner(start).map(|n| &n.id)
    {
        return Ok(None);
    }
    Ok(Some(m.get(2).unwrap().as_str().to_owned()))
}
fn bindings(
    ctx: &Context<'_>,
    file: &File<'_>,
    imports: &[&ImportBinding],
    language: &str,
) -> Result<Bindings> {
    let mut out = Bindings::default();
    let mut dynamic = Vec::new();
    for m in patterns().dynamic.captures_iter(&file.text) {
        let m = m?;
        for part in m.get(1).unwrap().as_str().split(',') {
            let mut parts = part.split(':').map(trim);
            let name = parts.next().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let local = parts.next().filter(|s| !s.is_empty()).unwrap_or(name);
            push_unique(&mut out.dynamic, local);
            dynamic.push((
                local.to_owned(),
                name.to_owned(),
                m.get(2).unwrap().as_str().to_owned(),
                false,
            ));
        }
    }
    for (local, name, source, namespace) in imports
        .iter()
        .map(|m| {
            (
                m.local_name.as_str(),
                m.imported_name.as_str(),
                m.source.as_str(),
                m.is_namespace,
            )
        })
        .chain(
            dynamic
                .iter()
                .map(|(l, n, s, ns)| (l.as_str(), n.as_str(), s.as_str(), *ns)),
        )
    {
        match source {
            "@tauri-apps/api" if name == "tauri" => push_unique(&mut out.invoke_holders, local),
            "@tauri-apps/api" if name == "event" => push_unique(&mut out.event_holders, local),
            "@tauri-apps/api" | "@tauri-apps/api/core" | "@tauri-apps/api/tauri" => {
                if namespace {
                    push_unique(&mut out.invoke_holders, local)
                } else if name == "invoke" {
                    push_unique(&mut out.invoke, local)
                }
            }
            "@tauri-apps/api/event" => {
                if namespace {
                    push_unique(&mut out.event_holders, local)
                } else if matches!(name, "listen" | "once") {
                    push_unique(&mut out.listen, local)
                } else if matches!(name, "emit" | "emitTo") {
                    push_unique(&mut out.emit, local);
                    if name == "emitTo" {
                        push_unique(&mut out.emit_to, local)
                    }
                }
            }
            "@tauri-apps/api/window"
            | "@tauri-apps/api/webviewWindow"
            | "@tauri-apps/api/webview" => out.handles = true,
            _ if name == "commands" => {
                if patterns().project.is_match(source)?
                    && (ctx.resolve_module)(source, file.path, language).is_none()
                {
                    push_unique(&mut out.typed_commands, local)
                }
            }
            _ if name == "events"
                && (patterns().project.is_match(source)?
                    || (ctx.resolve_module)(source, file.path, language).is_some()) =>
            {
                push_unique(&mut out.typed_events, local)
            }
            _ => {}
        }
    }
    for m in patterns().global_binding.captures_iter(&file.code) {
        let m = m?;
        push_unique(
            &mut out.invoke,
            m.get(1).or(m.get(2)).map_or("invoke", |m| m.as_str()),
        );
    }
    for m in patterns().event_binding.captures_iter(&file.code) {
        let m = m?;
        for part in m.get(1).unwrap().as_str().split(',') {
            let mut parts = part.split(':').map(trim);
            let name = parts.next().unwrap_or("");
            let local = parts.next().filter(|s| !s.is_empty()).unwrap_or(name);
            if matches!(name, "listen" | "once") {
                push_unique(&mut out.listen, local)
            } else if matches!(name, "emit" | "emitTo") {
                push_unique(&mut out.emit, local);
                if name == "emitTo" {
                    push_unique(&mut out.emit_to, local)
                }
            }
        }
    }
    Ok(out)
}
fn shadowed(
    ctx: &Context<'_>,
    file: &File<'_>,
    name: &str,
    owner: &Node,
    at: usize,
) -> Result<bool> {
    if !matches!(owner.kind.as_str(), "function" | "method") {
        return Ok(false);
    }
    if super::super::jsx::code_binds(ctx, owner, name, file.line(at))? {
        return Ok(true);
    }
    let start = file.position(owner.start_line, owner.start_column);
    let pattern = regex(&format!(
        r"(?:\b(?:const|let|var)\s*|\(\s*)[{{[][^=;(){{}}]*?(?<![\w$:.]){}(?![\w$])",
        regex::escape(name)
    ))?;
    Ok(pattern.is_match(&file.code[start.min(at)..at])?)
}
fn hidden(
    ctx: &Context<'_>,
    scan: &Scan<'_>,
    binding: Option<&str>,
    owner: &Node,
    at: usize,
) -> Result<bool> {
    match binding {
        Some(name) if !scan.bindings.dynamic.iter().any(|n| n == name) => {
            shadowed(ctx, &scan.file, name, owner, at)
        }
        _ => Ok(false),
    }
}
fn handler<'a>(
    ctx: &Context<'a>,
    scan: &Scan<'a>,
    span: Option<&(usize, usize)>,
    owner: &'a Node,
) -> Result<&'a Node> {
    let Some(&(start, end)) = span else {
        return Ok(owner);
    };
    let Some(m) = patterns()
        .handler
        .captures(trim(&scan.file.text[start..end]))?
    else {
        return Ok(owner);
    };
    let name = m.get(1).unwrap().as_str();
    if let Some(&node) = scan
        .file
        .nodes
        .iter()
        .find(|node| node.name == name && callable(node))
    {
        return Ok(node);
    }
    let Some(import) = scan.imports.iter().find(|m| m.local_name == name) else {
        return Ok(owner);
    };
    let Some(path) = (ctx.resolve_module)(&import.source, scan.file.path, scan.language) else {
        return Ok(owner);
    };
    Ok(ctx
        .in_file(&path)
        .filter(|node| node.name == import.imported_name && callable(node))
        .min_by(|a, b| a.start_line.cmp(&b.start_line).then(a.id.cmp(&b.id)))
        .unwrap_or(owner))
}
fn local_forwarder<'a>(
    ctx: &Context<'_>,
    scan: &'a Scan<'_>,
    forwarder: &'a Forwarder,
) -> Option<&'a str> {
    if forwarder.file == scan.file.path {
        return Some(&forwarder.name);
    }
    let import = scan
        .imports
        .iter()
        .find(|m| m.imported_name == forwarder.name)?;
    (ctx.resolve_module)(&import.source, scan.file.path, scan.language)
        .map(|_| import.local_name.as_str())
}
fn forwarders(ctx: &Context<'_>, scans: &[Scan<'_>]) -> Result<Vec<Forwarder>> {
    let mut found: Vec<Forwarder> = Vec::new();
    for _ in 0..3 {
        let before = found.len();
        for scan in scans {
            let mut patterns = Vec::new();
            for name in &scan.bindings.invoke {
                patterns.push(bare(name)?)
            }
            for name in &scan.bindings.invoke_holders {
                patterns.push(holder(name, "invoke")?)
            }
            patterns.push(self::patterns().global.clone());
            for item in &found {
                if let Some(local) = local_forwarder(ctx, scan, item) {
                    patterns.push(bare(local)?)
                }
                patterns.push(regex(&format!(
                    r"\.\s*{}(?![\w$])",
                    regex::escape(&item.name)
                ))?);
            }
            let mut here: Vec<Forwarder> = Vec::new();
            for pattern in patterns {
                for call in calls(ctx, &scan.file, &pattern)? {
                    ctx.step()?;
                    let spans = arguments(&scan.file.code, call.open);
                    let Some(&(start, end)) = spans.first() else {
                        continue;
                    };
                    let arg = trim(&scan.file.code[start..end]);
                    if !self::patterns().name.is_match(arg)? {
                        continue;
                    }
                    let Some(owner) = scan.file.owner(call.at).filter(|node| {
                        matches!(node.kind.as_str(), "function" | "method")
                            && node.name != "<anonymous>"
                    }) else {
                        continue;
                    };
                    let start = scan.file.position(owner.start_line, owner.start_column);
                    if self::patterns()
                        .parameter
                        .captures(&scan.file.code[start..scan.file.limit(start, 600)])?
                        .and_then(|m| m.get(1))
                        .is_some_and(|m| m.as_str() == arg)
                    {
                        if let Some(item) = here.iter_mut().find(|f| f.name == owner.name) {
                            item.file = scan.file.path.to_owned()
                        } else {
                            here.push(Forwarder {
                                name: owner.name.clone(),
                                file: scan.file.path.to_owned(),
                            })
                        }
                    }
                }
            }
            for item in here {
                if !found.iter().any(|f| f.name == item.name) {
                    found.push(item)
                }
            }
            for item in found.clone().iter().filter(|f| f.file == scan.file.path) {
                let alias = regex(&format!(
                    r"(?:^|[\s;{{])(?:(?:public|private|protected|readonly|static)\s+)*([A-Za-z_$][\w$]*)\s*(?::[^=;\n]*)?=\s*{}\s*[;\n]",
                    regex::escape(&item.name)
                ))?;
                for m in alias.captures_iter(&scan.file.code) {
                    let m = m?;
                    let name = m.get(1).unwrap().as_str();
                    if !found.iter().any(|f| f.name == name) {
                        found.push(Forwarder {
                            name: name.to_owned(),
                            file: item.file.clone(),
                        })
                    }
                }
            }
        }
        if found.len() == before {
            break;
        }
    }
    Ok(found)
}
fn invoke<'a>(
    ctx: &Context<'a>,
    scan: &Scan<'a>,
    side: &Side<'a>,
    out: &mut Collected<'a>,
    call: Call,
    binding: Option<&str>,
    callee: &str,
) -> Result<()> {
    ctx.steps(scan.file.code.len() / 32 + scan.file.nodes.len() + 1)?;
    let Some(key) = js_string(&scan.file, arguments(&scan.file.code, call.open).first())? else {
        return Ok(());
    };
    let table = if key.starts_with("plugin:") {
        &side.plugins
    } else {
        side.table(scan.file.path)
    };
    let Some(target) = table.get(&key) else {
        return Ok(());
    };
    let Some(owner) = scan.file.owner(call.at) else {
        return Ok(());
    };
    if hidden(ctx, scan, binding, owner, call.at)? {
        return Ok(());
    }
    out.command(
        ctx,
        owner,
        &key,
        target,
        callee,
        (scan.file.line(call.at), scan.file.column(call.at)),
    )
}
struct EventCall<'a> {
    call: Call,
    binding: Option<&'a str>,
    event: Option<&'a str>,
    index: usize,
    listen: bool,
}
fn event<'a>(
    ctx: &Context<'a>,
    scan: &Scan<'a>,
    side: &Side<'a>,
    out: &mut Collected<'a>,
    action: EventCall<'_>,
) -> Result<()> {
    let EventCall {
        call,
        binding,
        event,
        index,
        listen,
    } = action;
    ctx.steps(scan.file.code.len() / 32 + scan.file.nodes.len() + 1)?;
    let spans = arguments(&scan.file.code, call.open);
    let name = match event {
        Some(name) => Some(name.to_owned()),
        None => js_string(&scan.file, spans.get(if listen { 0 } else { index }))?,
    };
    let Some(name) = name else { return Ok(()) };
    if listen && name.starts_with("tauri://") {
        return Ok(());
    }
    let Some(owner) = scan.file.owner(call.at) else {
        return Ok(());
    };
    if hidden(ctx, scan, binding, owner, call.at)? {
        return Ok(());
    }
    let line = scan.file.line(call.at);
    let column = scan.file.column(call.at);
    let scope = side.scope(scan.file.path);
    let node = if listen {
        handler(ctx, scan, spans.get(index), owner)?
    } else {
        owner
    };
    let endpoint = Endpoint {
        node,
        event: name.clone(),
        file: scan.file.path.to_owned(),
        line,
        column,
        rust: false,
        scope: scope.clone(),
    };
    if listen {
        out.listeners.push(endpoint);
        if let Some(event) = side.event(&side.by_event, &name, scope.as_deref())
            && out
                .subscriptions
                .insert((owner.id.clone(), event.node.id.clone()))
        {
            out.subscribed.push((owner, event.clone(), line, column))
        }
    } else {
        out.emitters.push(endpoint)
    }
    Ok(())
}
fn read_frontend<'a>(
    ctx: &Context<'a>,
    scan: &Scan<'a>,
    forwarders: &[Forwarder],
    side: &Side<'a>,
    out: &mut Collected<'a>,
) -> Result<()> {
    let b = &scan.bindings;
    let file = &scan.file;
    for name in &b.invoke {
        for call in calls(ctx, file, &bare(name)?)? {
            invoke(ctx, scan, side, out, call, Some(name), name)?
        }
    }
    for name in &b.invoke_holders {
        for call in calls(ctx, file, &holder(name, "invoke")?)? {
            invoke(
                ctx,
                scan,
                side,
                out,
                call,
                Some(name),
                &format!("{name}.invoke"),
            )?
        }
    }
    for call in calls(ctx, file, &patterns().global)? {
        invoke(ctx, scan, side, out, call, None, "invoke")?
    }
    for item in forwarders {
        if let Some(local) = local_forwarder(ctx, scan, item) {
            for call in calls(ctx, file, &bare(local)?)? {
                invoke(ctx, scan, side, out, call, Some(local), local)?
            }
        }
        for call in calls(
            ctx,
            file,
            &regex(&format!(r"\.\s*{}(?![\w$])", regex::escape(&item.name)))?,
        )? {
            invoke(ctx, scan, side, out, call, None, &item.name)?
        }
    }
    for name in &b.typed_commands {
        let table = side.table(file.path);
        let mut names = HashMap::new();
        for key in &table.keys {
            names.insert(camel(key)?, key);
        }
        for m in holder(name, r"([A-Za-z_$][\w$]*)")?.captures_iter(&file.code) {
            let m = m?;
            let member = m.get(1).unwrap().as_str();
            let Some(key) = names.get(member) else {
                continue;
            };
            let whole = m.get(0).unwrap();
            let open = skip_types(file, skip_space(&file.code, whole.end()));
            if file.code.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let Some(target) = table.get(key) else {
                continue;
            };
            let Some(owner) = file.owner(whole.start()) else {
                continue;
            };
            if shadowed(ctx, file, name, owner, whole.start())? {
                continue;
            }
            out.command(
                ctx,
                owner,
                key,
                target,
                &format!("{name}.{member}"),
                (file.line(whole.start()), file.column(whole.start())),
            )?;
        }
    }
    for name in &b.listen {
        for call in calls(ctx, file, &bare(name)?)? {
            event(
                ctx,
                scan,
                side,
                out,
                EventCall {
                    call,
                    binding: Some(name),
                    event: None,
                    index: 1,
                    listen: true,
                },
            )?
        }
    }
    for name in &b.emit {
        for call in calls(ctx, file, &bare(name)?)? {
            event(
                ctx,
                scan,
                side,
                out,
                EventCall {
                    call,
                    binding: Some(name),
                    event: None,
                    index: usize::from(b.emit_to.contains(name)),
                    listen: false,
                },
            )?
        }
    }
    for name in &b.event_holders {
        for (member, listen) in [("(?:listen|once)", true), ("emit", false)] {
            for call in calls(ctx, file, &holder(name, member)?)? {
                event(
                    ctx,
                    scan,
                    side,
                    out,
                    EventCall {
                        call,
                        binding: Some(name),
                        event: None,
                        index: usize::from(listen),
                        listen,
                    },
                )?
            }
        }
    }
    if b.handles {
        for (member, listen) in [("(?:listen|once)", true), ("emit", false)] {
            for call in calls(ctx, file, &regex(&format!(r"\.\s*{member}(?![\w$])"))?)? {
                event(
                    ctx,
                    scan,
                    side,
                    out,
                    EventCall {
                        call,
                        binding: None,
                        event: None,
                        index: usize::from(listen),
                        listen,
                    },
                )?
            }
        }
    }
    for name in &b.typed_events {
        for m in holder(name, r"([A-Za-z_$][\w$]*)")?.captures_iter(&file.code) {
            let m = m?;
            let whole = m.get(0).unwrap();
            let mut end = skip_space(&file.code, whole.end());
            if file.code.as_bytes().get(end) == Some(&b'(') {
                let Some(close) = close_bracket(&file.code, end) else {
                    continue;
                };
                end = skip_space(&file.code, close + 1);
            }
            let Some(member) = patterns()
                .member
                .captures(&file.code[end..file.limit(end, 32)])?
            else {
                continue;
            };
            let open = skip_space(&file.code, end + member.get(0).unwrap().end());
            if file.code.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let Some(structure) = side.event(
                &side.by_camel,
                m.get(1).unwrap().as_str(),
                side.scope(file.path).as_deref(),
            ) else {
                continue;
            };
            event(
                ctx,
                scan,
                side,
                out,
                EventCall {
                    call: Call {
                        at: whole.start(),
                        open,
                    },
                    binding: Some(name),
                    event: Some(&structure.event),
                    index: 0,
                    listen: member.get(1).unwrap().as_str() != "emit",
                },
            )?;
        }
    }
    Ok(())
}
fn scan<'a>(ctx: &Context<'a>, path: &'a str, source: &'a str) -> Result<Scan<'a>> {
    let language = if path.ends_with(".svelte") {
        "svelte"
    } else if path.ends_with(".vue") {
        "vue"
    } else {
        "typescript"
    };
    let file = File::new(ctx, path, source, "typescript")?;
    let imports = ctx.file_imports.get(path).cloned().unwrap_or_default();
    let bindings = bindings(ctx, &file, &imports, language)?;
    Ok(Scan {
        file,
        imports,
        language,
        bindings,
    })
}
pub(super) fn read<'a>(ctx: &Context<'a>, side: &Side<'a>, out: &mut Collected<'a>) -> Result<()> {
    let mut files = Vec::new();
    for (&path, &source) in ctx.sources {
        if patterns().file.is_match(path)? && !crate::classify::is_test_path(path) {
            files.push((path, source));
        }
    }
    let mut scans = Vec::new();
    let mut scanned = HashSet::new();
    for &(path, source) in &files {
        if !source.is_empty() && patterns().gate.is_match(source)? {
            scans.push(scan(ctx, path, source)?);
            scanned.insert(path);
        }
    }
    let forwarders = forwarders(ctx, &scans)?;
    if !forwarders.is_empty() {
        for (path, source) in files {
            if !scanned.contains(path)
                && !source.is_empty()
                && forwarders.iter().any(|f| source.contains(&f.name))
            {
                scans.push(scan(ctx, path, source)?);
            }
        }
    }
    for scan in &scans {
        ctx.step()?;
        read_frontend(ctx, scan, &forwarders, side, out)?;
    }
    Ok(())
}
