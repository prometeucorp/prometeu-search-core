use super::*;
struct Patterns {
    gate: fancy_regex::Regex,
    constant: fancy_regex::Regex,
    handler: fancy_regex::Regex,
    plugin: fancy_regex::Regex,
    plain: fancy_regex::Regex,
    attribute: fancy_regex::Regex,
    entry_attribute: fancy_regex::Regex,
    turbofish: fancy_regex::Regex,
    path: fancy_regex::Regex,
    derive: fancy_regex::Regex,
    event_word: fancy_regex::Regex,
    item: fancy_regex::Regex,
    rename: fancy_regex::Regex,
    emit: fancy_regex::Regex,
    listen: fancy_regex::Regex,
    typed_listen: fancy_regex::Regex,
    literal: fancy_regex::Regex,
    constant_name: fancy_regex::Regex,
    type_at: fancy_regex::Regex,
    word: fancy_regex::Regex,
    wrapped_type: fancy_regex::Regex,
}
fn patterns() -> &'static Patterns {
    static P: std::sync::LazyLock<Patterns> = std::sync::LazyLock::new(|| {
        let r = |p| regex(p).expect("valid reference pattern");
        Patterns {
            gate: r(r"\btauri|generate_handler|collect_commands|\.\s*(?:emit|listen|once)"),
            constant: r(
                r#"\b(?:const|static)\s+([A-Z][A-Z0-9_]*)\s*:\s*&\s*(?:'static\s+)?str\s*=\s*"([^"\n]*)""#,
            ),
            handler: r(r"\b(?:generate_handler|collect_commands)\s*!\s*[[({]"),
            plugin: r(
                r#"\bBuilder\s*(?:::\s*<[^;()]*>\s*)?::\s*(?:new\s*\(\s*(?:"([^"]+)"|([A-Z][A-Z0-9_]*))|default\s*\()|\.\s*plugin_name\s*\(\s*(?:"([^"]+)"|([A-Z][A-Z0-9_]*))"#,
            ),
            plain: r(
                r"^(?:cfg|cfg_attr|doc|allow|expect|warn|deny|forbid|inline|cold|must_use|deprecated|track_caller|test|ignore|no_mangle|rustfmt|clippy|instrument|tracing|specta|async_trait)\b",
            ),
            attribute: r(r"^#\[\s*([A-Za-z_][\w:]*)"),
            entry_attribute: r(r"#\s*\[[^\]]*\]"),
            turbofish: r(r"::\s*<[\s\S]*>$"),
            path: r(r"^[A-Za-z_]\w*(?:::[A-Za-z_]\w*)*$"),
            derive: r(r"#\s*\[\s*derive\s*\(([^)]*)\)\s*\]"),
            event_word: r(r"\bEvent\b"),
            item: r(
                r"^(?:\s*#\s*\[(?:[^\]]|\][^\s])*\])*\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum)\s+([A-Za-z_]\w*)",
            ),
            rename: r(r#"#\s*\[\s*tauri_specta\s*\(\s*event_name\s*=\s*"([^"]+)"\s*\)\s*\]"#),
            emit: r(
                r"\.\s*(emit(?:_str|_all|_to|_str_to|_filter|_str_filter|_and_trigger)?)\s*(?:::\s*<[^()]*>\s*)?\(",
            ),
            listen: r(r"\.\s*(?:listen|once)(?:_any|_global)?\s*(?:::\s*<[^()]*>\s*)?\("),
            typed_listen: r(r"\b([A-Z]\w*)\s*::\s*(?:listen|once)(?:_any)?\s*\("),
            literal: r(r#"^&?"([^"\\]*)"$"#),
            constant_name: r(r"^(?:[a-z_]\w*::)*([A-Z][A-Z0-9_]*)$"),
            type_at: r(r"(?:[A-Za-z_]\w*\s*::\s*)*[A-Za-z_]\w*\s*$"),
            word: r(r"[A-Za-z_]\w*$"),
            wrapped_type: r(r"^\s*&?\s*(?:[a-z_]\w*::)*([A-Z]\w*)"),
        }
    });
    &P
}
fn conventional(file: &str) -> Option<String> {
    if file.starts_with("src-tauri/") {
        Some(String::new())
    } else {
        file.find("/src-tauri/").map(|at| file[..at].to_owned())
    }
}
fn crate_root(ctx: &Context<'_>, file: &str, memo: &mut HashMap<String, String>) -> String {
    let mut dir = file.rsplit_once('/').map_or("", |v| v.0);
    let mut seen = Vec::new();
    let root = loop {
        if let Some(root) = memo.get(dir) {
            break root.clone();
        }
        seen.push(dir.to_owned());
        let manifest = if dir.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{dir}/Cargo.toml")
        };
        if ctx.manifests.contains_key(manifest.as_str())
            || ctx.sources.contains_key(manifest.as_str())
        {
            break dir.to_owned();
        }
        if dir.is_empty() {
            break String::new();
        }
        dir = dir.rsplit_once('/').map_or("", |v| v.0);
    };
    for dir in seen {
        memo.insert(dir, root.clone());
    }
    root
}
fn attributes(ctx: &Context<'_>, node: &Node) -> Result<Vec<String>> {
    if node
        .decorators
        .as_ref()
        .is_some_and(|attrs| attrs.iter().any(|attr| attr == "tauri::command"))
    {
        return Ok(vec!["tauri::command".into()]);
    }
    let Some(source) = ctx.sources.get(node.file_path.as_str()) else {
        return Ok(Vec::new());
    };
    ctx.steps(source.len() / 32 + 1)?;
    let lines: Vec<_> = source.split('\n').collect();
    let start = node.start_line.saturating_sub(1) as usize;
    let mut out = vec![trim(lines.get(start).copied().unwrap_or(""))];
    for &line in lines[..start.min(lines.len())].iter().rev() {
        let line = trim(line);
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with("#[") || line.ends_with([']', ',', '(']) {
            out.push(line);
            if out.len() > 16 {
                break;
            }
        } else {
            break;
        }
    }
    let mut names = Vec::new();
    for line in out {
        if let Some(m) = patterns().attribute.captures(line)? {
            names.push(m.get(1).unwrap().as_str().to_owned());
        }
    }
    Ok(names)
}
fn path_score(segments: &[String], node: &Node) -> usize {
    let file = node
        .file_path
        .strip_suffix(".rs")
        .unwrap_or(&node.file_path);
    let parts: Vec<_> = file.split('/').collect();
    let names: HashSet<_> = if let Some(src) = parts.iter().rposition(|p| *p == "src") {
        parts[src.saturating_sub(1)..]
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 1 || src == 0)
            .map(|(_, part)| part.replace('-', "_"))
            .collect()
    } else {
        parts
            .last()
            .into_iter()
            .map(|part| part.replace('-', "_"))
            .collect()
    };
    segments[..segments.len().saturating_sub(1)]
        .iter()
        .filter(|name| names.contains(*name))
        .count()
}
struct Entry {
    segments: Vec<String>,
    line: u32,
}
struct Handler {
    line: u32,
    entries: Vec<Entry>,
    opaque: bool,
    plugin: Option<String>,
}
fn handler(
    file: &File<'_>,
    constants: &HashMap<String, Option<String>>,
    at: usize,
    open: usize,
) -> Result<Option<Handler>> {
    if close_bracket(&file.code, open).is_none() {
        return Ok(None);
    }
    let mut entries = Vec::new();
    let mut opaque = false;
    for (start, end) in arguments(&file.code, open) {
        let replaced = patterns()
            .entry_attribute
            .replace_all(&file.code[start..end], " ");
        let item = trim(&replaced);
        if item.contains('$') {
            opaque = true;
            continue;
        }
        let stripped = patterns().turbofish.replace(item, "");
        let path: String = stripped
            .chars()
            .filter(|&ch| !crate::search::query_whitespace(ch))
            .collect();
        if !patterns().path.is_match(&path)? {
            continue;
        }
        let segments: Vec<_> = path.split("::").map(str::to_owned).collect();
        let units = item.encode_utf16().count();
        let search = file.before(end, units);
        let named = file.code[search..]
            .find(segments.last().unwrap())
            .map(|offset| search + offset)
            .filter(|&n| n >= start && n < end)
            .unwrap_or(start);
        entries.push(Entry {
            segments,
            line: file.line(named),
        });
    }
    let mut plugin = None;
    for m in patterns().plugin.captures_iter(&file.text) {
        let m = m?;
        if m.get(0).unwrap().start() >= at {
            break;
        }
        plugin = m
            .get(1)
            .or(m.get(3))
            .map(|m| m.as_str().to_owned())
            .or_else(|| {
                m.get(2)
                    .or(m.get(4))
                    .and_then(|m| constants.get(m.as_str()).cloned().flatten())
            });
    }
    Ok(Some(Handler {
        line: file.line(at),
        entries,
        opaque,
        plugin,
    }))
}
fn candidates<'a>(
    ctx: &Context<'a>,
    entry: &Entry,
    file: &str,
    attributes: &HashMap<String, Vec<String>>,
    crates: &mut HashMap<String, String>,
) -> Result<Vec<&'a Node>> {
    let mut nodes = Vec::new();
    for node in ctx.named(entry.segments.last().unwrap()).filter(|node| {
        node.language == "rust"
            && node.kind == "function"
            && !crate::classify::is_test_path(&node.file_path)
    }) {
        ctx.step()?;
        let mut command = false;
        for attr in attributes.get(&node.id).into_iter().flatten() {
            if !patterns().plain.is_match(attr)? {
                command = true;
                break;
            }
        }
        if command {
            nodes.push(node)
        }
    }
    nodes.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.start_line.cmp(&b.start_line))
            .then(a.id.cmp(&b.id))
    });
    if nodes.len() > 1 {
        let own = conventional(file).unwrap_or_else(|| crate_root(ctx, file, crates));
        let same: Vec<_> = nodes
            .iter()
            .copied()
            .filter(|node| {
                conventional(&node.file_path)
                    .unwrap_or_else(|| crate_root(ctx, &node.file_path, crates))
                    == own
            })
            .collect();
        if !same.is_empty() {
            nodes = same
        }
    }
    if nodes.len() > 1 {
        let best = nodes
            .iter()
            .map(|node| path_score(&entry.segments, node))
            .max()
            .unwrap();
        nodes.retain(|node| path_score(&entry.segments, node) == best)
    }
    if nodes.len() > 6 {
        nodes.clear()
    }
    Ok(nodes)
}
fn event_structs<'a>(file: &File<'a>, side: &mut Side<'a>) -> Result<()> {
    if !file.source.contains("tauri_specta") {
        return Ok(());
    }
    for m in patterns().derive.captures_iter(&file.code) {
        let m = m?;
        if !patterns().event_word.is_match(m.get(1).unwrap().as_str())? {
            continue;
        }
        let whole = m.get(0).unwrap();
        let end = file.limit(whole.end(), 4000);
        let Some(item) = patterns().item.captures(&file.code[whole.end()..end])? else {
            continue;
        };
        let name = item.get(1).unwrap().as_str();
        let Some(&node) = file
            .nodes
            .iter()
            .find(|node| node.name == name && matches!(node.kind.as_str(), "struct" | "enum"))
        else {
            continue;
        };
        let span =
            &file.text[file.before(whole.start(), 400)..whole.end() + item.get(0).unwrap().end()];
        let span = &span[span.rfind('}').map_or(0, |n| n + 1)..];
        let event = patterns()
            .rename
            .captures(span)?
            .and_then(|m| m.get(1).map(|m| m.as_str().to_owned()))
            .unwrap_or(kebab(name)?);
        let event = EventStruct {
            node,
            event,
            scope: side.scope(file.path),
        };
        side.by_event
            .entry(event.event.clone())
            .or_default()
            .push(event.clone());
        side.by_type
            .entry(name.to_owned())
            .or_default()
            .push(event.clone());
        side.by_camel.entry(camel(name)?).or_default().push(event);
    }
    Ok(())
}
fn string_at(
    file: &File<'_>,
    span: Option<&(usize, usize)>,
    constants: &HashMap<String, Option<String>>,
) -> Result<Option<String>> {
    let Some(&(start, end)) = span else {
        return Ok(None);
    };
    let raw = trim(&file.text[start..end]);
    if let Some(m) = patterns().literal.captures(raw)? {
        return Ok(Some(m.get(1).unwrap().as_str().to_owned()));
    }
    Ok(patterns()
        .constant_name
        .captures(raw)?
        .and_then(|m| constants.get(m.get(1).unwrap().as_str()).cloned().flatten()))
}
fn type_at(file: &File<'_>, end: usize) -> Result<Option<String>> {
    let start = file.before(end, 200);
    let Some(m) = patterns().type_at.find(&file.code[start..end + 1])? else {
        return Ok(None);
    };
    Ok(m.as_str()
        .rsplit("::")
        .map(trim)
        .find(|part| part.as_bytes().first().is_some_and(u8::is_ascii_uppercase))
        .map(str::to_owned))
}
fn receiver_type(file: &File<'_>, dot: usize, function_start: usize) -> Result<Option<String>> {
    let head = file.code[..dot].trim_end_matches(crate::search::query_whitespace);
    let Some(i) = head.len().checked_sub(1) else {
        return Ok(None);
    };
    let c = file.code.as_bytes()[i];
    if matches!(c, b')' | b'}') {
        let Some(open) = open_bracket(&file.code, i) else {
            return Ok(None);
        };
        let before = file.code[..open]
            .trim_end_matches(crate::search::query_whitespace)
            .len();
        if before > 0
            && (file.code.as_bytes()[before - 1].is_ascii_alphanumeric()
                || file.code.as_bytes()[before - 1] == b'_')
        {
            return type_at(file, before - 1);
        }
        return Ok(patterns()
            .wrapped_type
            .captures(&file.code[open + 1..i])?
            .and_then(|m| m.get(1).map(|m| m.as_str().to_owned())));
    }
    if !(c.is_ascii_alphanumeric() || c == b'_') {
        return Ok(None);
    }
    let Some(word) = patterns()
        .word
        .find(&file.code[file.before(i, 120)..i + 1])?
    else {
        return Ok(None);
    };
    let name = word.as_str();
    if name.as_bytes()[0].is_ascii_uppercase() {
        return Ok(Some(name.to_owned()));
    }
    let n = regex::escape(name);
    let pattern = regex(&format!(
        r"\blet\s+(?:mut\s+)?{n}\s*(?::\s*&?\s*(?:[a-z_]\w*::)*([A-Z]\w*)[^=;]*)?=\s*&?\s*(?:[a-z_]\w*::)*([A-Z]\w*)|\b{n}\s*:\s*&?\s*(?:[a-z_]\w*::)*([A-Z]\w*)"
    ))?;
    let mut found = None;
    for m in pattern.captures_iter(&file.code[function_start.min(i)..i]) {
        let m = m?;
        found = m
            .get(1)
            .or(m.get(2))
            .or(m.get(3))
            .map(|m| m.as_str().to_owned())
            .or(found);
    }
    Ok(found)
}
fn endpoints<'a>(
    file: &File<'a>,
    constants: &HashMap<String, Option<String>>,
    side: &mut Side<'a>,
) -> Result<()> {
    let scope = side.scope(file.path);
    for m in patterns().emit.captures_iter(&file.code) {
        let m = m?;
        let at = m.get(0).unwrap().start();
        let spans = arguments(&file.code, m.get(0).unwrap().end() - 1);
        let line = file.line(at);
        let Some(owner) = file.enclosing(line) else {
            continue;
        };
        let targeted = m.get(1).unwrap().as_str().ends_with("_to");
        let mut event = if targeted && spans.len() < 3 {
            None
        } else {
            string_at(file, spans.get(usize::from(targeted)), constants)?
        };
        if event.is_none()
            && let Some(ty) = receiver_type(file, at, file.offset(owner.start_line))?
        {
            event = side
                .event(&side.by_type, &ty, scope.as_deref())
                .map(|s| s.event.clone());
        }
        if let Some(event) = event {
            side.emitters.push(Endpoint {
                node: owner,
                event,
                file: file.path.to_owned(),
                line,
                column: file.column(at),
                rust: true,
                scope: scope.clone(),
            });
        }
    }
    for m in patterns().listen.captures_iter(&file.code) {
        let m = m?;
        let at = m.get(0).unwrap().start();
        let spans = arguments(&file.code, m.get(0).unwrap().end() - 1);
        let Some(event) = string_at(file, spans.first(), constants)? else {
            continue;
        };
        let line = file.line(at);
        if let Some(owner) = file.enclosing(line) {
            side.listeners.push(Endpoint {
                node: owner,
                event,
                file: file.path.to_owned(),
                line,
                column: file.column(at),
                rust: true,
                scope: scope.clone(),
            });
        }
    }
    for m in patterns().typed_listen.captures_iter(&file.code) {
        let m = m?;
        let at = m.get(0).unwrap().start();
        let Some(event) = side
            .event(&side.by_type, m.get(1).unwrap().as_str(), scope.as_deref())
            .filter(|s| !s.event.is_empty())
            .map(|s| s.event.clone())
        else {
            continue;
        };
        let line = file.line(at);
        if let Some(owner) = file.enclosing(line) {
            side.listeners.push(Endpoint {
                node: owner,
                event,
                file: file.path.to_owned(),
                line,
                column: file.column(at),
                rust: true,
                scope: scope.clone(),
            });
        }
    }
    Ok(())
}
pub(super) fn read<'a>(ctx: &Context<'a>) -> Result<Side<'a>> {
    let mut files = Vec::new();
    let mut roots = Vec::new();
    let mut constants: HashMap<String, Option<String>> = HashMap::new();
    let mut attrs = HashMap::new();
    for (&path, &source) in ctx.sources {
        if !path.ends_with(".rs") {
            continue;
        }
        if let Some(root) = conventional(path) {
            push_unique(&mut roots, &root)
        }
        if crate::classify::is_test_path(path)
            || source.is_empty()
            || !patterns().gate.is_match(source)?
        {
            continue;
        }
        let file = File::new(ctx, path, source, "rust")?;
        for m in patterns().constant.captures_iter(&file.text) {
            let m = m?;
            let name = m.get(1).unwrap().as_str().to_owned();
            let value = m.get(2).unwrap().as_str().to_owned();
            match constants.get(&name) {
                Some(Some(known)) if *known == value => {}
                None => {
                    constants.insert(name, Some(value));
                }
                _ => {
                    constants.insert(name, None);
                }
            }
        }
        files.push(file);
    }
    for node in ctx
        .nodes
        .iter()
        .filter(|node| node.language == "rust" && node.kind == "function")
    {
        attrs.insert(node.id.clone(), attributes(ctx, node)?);
    }
    let apps = roots
        .iter()
        .map(|root| App {
            root: root.clone(),
            commands: Table::default(),
            open_list: None,
        })
        .collect();
    roots.sort_by_key(|root| std::cmp::Reverse(root.len()));
    let mut side = Side {
        roots,
        apps,
        plugins: Table::default(),
        any_app: Table::default(),
        by_event: HashMap::new(),
        by_type: HashMap::new(),
        by_camel: HashMap::new(),
        emitters: Vec::new(),
        listeners: Vec::new(),
    };
    let mut crates = HashMap::new();
    for file in &files {
        event_structs(file, &mut side)?;
        for m in patterns().handler.find_iter(&file.code) {
            ctx.step()?;
            let m = m?;
            let Some(list) = handler(file, &constants, m.start(), m.end() - 1)? else {
                continue;
            };
            let root =
                conventional(file.path).unwrap_or_else(|| crate_root(ctx, file.path, &mut crates));
            let app = side
                .apps
                .iter()
                .position(|app| app.root == root)
                .unwrap_or_else(|| {
                    side.apps.push(App {
                        root,
                        commands: Table::default(),
                        open_list: None,
                    });
                    side.apps.len() - 1
                });
            if list.opaque {
                side.apps[app].open_list = Some(format!("{}:{}", file.path, list.line));
            }
            for entry in list.entries {
                let nodes = candidates(ctx, &entry, file.path, &attrs, &mut crates)?;
                if nodes.is_empty() {
                    continue;
                }
                let command = Command {
                    nodes,
                    registered: format!("{}:{}", file.path, entry.line),
                };
                let name = entry.segments.last().unwrap();
                if let Some(plugin) = &list.plugin {
                    side.plugins
                        .add(&format!("plugin:{plugin}|{name}"), command)
                } else {
                    side.apps[app].commands.add(name, command)
                }
            }
        }
    }
    for app in &mut side.apps {
        let Some(registered) = &app.open_list else {
            continue;
        };
        let mut declared: Vec<(String, Vec<&Node>)> = Vec::new();
        for file in &files {
            if !within(file.path, &app.root) {
                continue;
            }
            for &node in &file.nodes {
                if node.kind != "function"
                    || !attrs
                        .get(&node.id)
                        .into_iter()
                        .flatten()
                        .any(|attr| matches!(attr.as_str(), "command" | "tauri::command"))
                {
                    continue;
                }
                if let Some((_, nodes)) = declared.iter_mut().find(|(name, _)| *name == node.name) {
                    nodes.push(node)
                } else {
                    declared.push((node.name.clone(), vec![node]));
                }
            }
        }
        for (name, nodes) in declared {
            if nodes.len() <= 6 {
                app.commands.add(
                    &name,
                    Command {
                        nodes,
                        registered: registered.clone(),
                    },
                );
            }
        }
    }
    for file in &files {
        endpoints(file, &constants, &mut side)?;
    }
    side.apps
        .sort_by_key(|app| std::cmp::Reverse(app.root.len()));
    for app in &side.apps {
        for key in &app.commands.keys {
            match &app.commands.values[key] {
                None => {
                    if !side.any_app.values.contains_key(key) {
                        side.any_app.keys.push(key.clone());
                    }
                    side.any_app.values.insert(key.clone(), None);
                }
                Some(command) => side.any_app.add(key, command.clone()),
            }
        }
    }
    Ok(side)
}
