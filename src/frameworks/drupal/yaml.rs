use super::*;

struct Pending {
    name: String,
    line: u32,
    path: Option<String>,
    handlers: Vec<String>,
    methods: Vec<String>,
}
fn flush(ctx: &Context<'_>, pending: &Option<Pending>, out: &mut ExtractionResult) {
    let Some(pending) = pending else {
        return;
    };
    let Some(path) = pending.path.as_deref().filter(|path| !path.is_empty()) else {
        return;
    };
    let tag = if pending.methods.is_empty() {
        String::new()
    } else {
        format!(" [{}]", pending.methods.join(","))
    };
    let route = node(
        ctx,
        "route",
        &format!("{path}{tag}"),
        &pending.name,
        format!("route:{}:{}:{path}", ctx.path, pending.line),
        pending.line,
    );
    out.unresolved_references.extend(
        pending
            .handlers
            .iter()
            .map(|handler| reference(&route, handler, "references")),
    );
    out.nodes.push(route);
}
pub(super) fn routes(ctx: &Context<'_>, out: &mut ExtractionResult) -> Result<()> {
    let mut pending = None;
    for (index, line) in ctx.source.split('\n').enumerate() {
        let trimmed = trim(line);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if regex(r"^\S.*:\s*$")?.is_match(line)? {
            flush(ctx, &pending, out);
            pending = Some(Pending {
                name: trim(&trimmed[..trimmed.len() - 1]).into(),
                line: index as u32 + 1,
                path: None,
                handlers: Vec::new(),
                methods: Vec::new(),
            });
            continue;
        }
        let Some(pending) = pending.as_mut() else {
            continue;
        };
        if let Some(capture) =
            regex(r##"^path:\s*['"]?([^'"#\n]+?)['"]?\s*(?:#.*)?$"##)?.captures(trimmed)?
        {
            pending.path = Some(trim(&capture[1]).into());
        } else if let Some(capture) =
            regex(r##"^_(?:controller|form):\s*['"]?([^'"#\n]+?)['"]?\s*(?:#.*)?$"##)?
                .captures(trimmed)?
        {
            pending.handlers.push(trim(&capture[1]).into());
        } else if let Some(capture) = regex(
            r##"^_(?:entity_form|entity_list|entity_view):\s*['"]?([^'"#\n]+?)['"]?\s*(?:#.*)?$"##,
        )?
        .captures(trimmed)?
        {
            pending.handlers.push(trim(&capture[1]).into());
        } else if let Some(capture) = regex(r"^methods:\s*\[([^\]]+)\]")?.captures(trimmed)? {
            pending.methods = capture[1]
                .split(',')
                .map(trim)
                .filter(|part| !part.is_empty())
                .map(str::to_uppercase)
                .collect();
        }
    }
    flush(ctx, &pending, out);
    Ok(())
}
fn uncomment(line: &str) -> &str {
    let mut quote = None;
    let mut previous = None;
    for (at, ch) in line.char_indices() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch == '#' && previous.is_none_or(crate::search::query_whitespace) {
            return &line[..at];
        }
        previous = Some(ch);
    }
    line
}
fn give(out: &mut ExtractionResult, current: usize, given: &mut HashSet<String>, id: &str) {
    let node = &out.nodes[current];
    if node.name != format!("service:{id}") && given.insert(id.into()) {
        out.unresolved_references.push(reference(
            node,
            &format!("drupal:service:{id}"),
            "references",
        ));
    }
}
fn give_all(
    out: &mut ExtractionResult,
    current: usize,
    given: &mut HashSet<String>,
    value: &str,
) -> Result<()> {
    for capture in regex(r#"(?:^|[\s\[,'"])@\??([A-Za-z_\\][\w.\\-]*)"#)?.captures_iter(value) {
        give(out, current, given, &capture?[1]);
    }
    Ok(())
}
fn class(out: &mut ExtractionResult, current: usize, fqcn: &str) {
    let node = &mut out.nodes[current];
    if node
        .signature
        .as_deref()
        .is_some_and(|signature| !signature.is_empty())
    {
        return;
    }
    node.signature = Some(fqcn.into());
    let split = fqcn.rfind('\\').map_or(0, |at| at + 1);
    out.unresolved_references.push(reference(
        node,
        &format!("drupal:fqcn:{}:{}", &fqcn[..split], &fqcn[split..]),
        "references",
    ));
}
pub(super) fn services(ctx: &Context<'_>, out: &mut ExtractionResult) -> Result<()> {
    let mut in_services = false;
    let mut id_indent = None;
    let mut current = None;
    let mut given = HashSet::new();
    for (index, line) in ctx.source.split('\n').enumerate() {
        let text = uncomment(line).trim_end_matches(crate::search::query_whitespace);
        let trimmed = trim(text);
        if trimmed.is_empty() {
            continue;
        }
        let indent = text.len()
            - text
                .trim_start_matches(crate::search::query_whitespace)
                .len();
        let indent = text[..indent].encode_utf16().count();
        if indent == 0 {
            in_services = regex(r"^services\s*:")?.is_match(trimmed)?;
            id_indent = None;
            current = None;
            continue;
        }
        if !in_services {
            continue;
        }
        if id_indent.is_none() {
            id_indent = Some(indent);
        }
        if id_indent == Some(indent) {
            current = None;
            let Some(id) = regex(r#"^(?:'([^']+)'|"([^"]+)"|([A-Za-z_\\][\w.\\-]*))\s*:\s*(.*)$"#)?
                .captures(trimmed)?
            else {
                continue;
            };
            let name = id
                .get(1)
                .or_else(|| id.get(2))
                .or_else(|| id.get(3))
                .unwrap()
                .as_str();
            if name.starts_with('_') {
                continue;
            }
            let line = index as u32 + 1;
            let service_name = format!("service:{name}");
            let service = node(
                ctx,
                "component",
                &service_name,
                &service_name,
                format!("component:{}:{line}:{service_name}", ctx.path),
                line,
            );
            let at = out.nodes.len();
            out.nodes.push(service);
            current = Some(at);
            given.clear();
            let value = &id[4];
            let fqcn = name.trim_start_matches('\\');
            if let Some(capture) = regex(r#"\bclass\s*:\s*['"]?([\w\\]+)"#)?.captures(value)? {
                class(out, at, capture[1].trim_start_matches('\\'));
            } else if fqcn.contains('\\') && !regex(r#"^['"]?@"#)?.is_match(value)? {
                class(out, at, fqcn);
            }
            give_all(out, at, &mut given, value)?;
            continue;
        }
        let Some(current) = current else {
            continue;
        };
        if let Some(field) =
            regex(r#"^(class|parent|alias|decorates)\s*:\s*['"]?([^'"\s]+)"#)?.captures(trimmed)?
        {
            if &field[1] == "class" {
                class(out, current, field[2].trim_start_matches('\\'));
            } else {
                give(
                    out,
                    current,
                    &mut given,
                    field[2].strip_prefix('@').unwrap_or(&field[2]),
                );
            }
        } else {
            give_all(out, current, &mut given, trimmed)?;
        }
    }
    Ok(())
}
