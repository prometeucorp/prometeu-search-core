//! Object-literal registries dispatched by a computed key.
use super::*;

const ENTRY: &[&str] = &[
    "execute", "run", "handle", "perform", "process", "call", "apply", "dispatch",
];

pub(super) fn objects(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let gate = regex(r"[\w$]\s*\[\s*[A-Za-z_$]")?;
    // The reference's guarded scan is an optimization of these unguarded
    // patterns, including resuming in the middle of an identifier after a match.
    let dispatch = regex(
        r"(?:\bnew\s+)?((?:this\.)?[A-Za-z_$][\w$]*)\s*\[\s*([A-Za-z_$][\w$.]*)\s*\]\s*(?:\(|\.[A-Za-z_$])",
    )?;
    let assignment = regex(
        r"(?:(?:const|let|var)\s+([A-Za-z_$][\w$]*)|((?:this\.)?[A-Za-z_$][\w$]*))\s*=\s*\{",
    )?;
    let after_call = regex(r"\]\s*\([^)]*\)\s*\.\s*([A-Za-z_$][\w$]*)")?;
    let after_bracket = regex(r"\]\s*\.\s*([A-Za-z_$][\w$]*)")?;
    let entry = regex(r#"^\s*(?:\[[^\]]+\]|['"]?[\w$]+['"]?)\s*:\s*([A-Za-z_$][\w$]*)\s*$"#)?;
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !file
            .rsplit('.')
            .next()
            .is_some_and(|ext| matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs"))
            || !gate.is_match(source)?
            || source.encode_utf16().count()
                > 200 * (source.bytes().filter(|&b| b == b'\n').count() + 1)
        {
            continue;
        }
        let safe = strip_comments(source);
        let mut dispatches = Vec::new();
        let mut references = HashSet::new();
        for found in dispatch.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let start = found.get(0).unwrap().start();
            let window = prefix_utf16(&safe[start..], 160);
            let chained = after_call
                .captures(window)?
                .or(after_bracket.captures(window)?)
                .and_then(|m| m.get(1))
                .map(|m| m.as_str().to_owned());
            let reference = found.get(1).unwrap().as_str().trim_start_matches("this.");
            references.insert(reference);
            dispatches.push((reference, ctx.line_at(file, start), chained));
        }
        if dispatches.is_empty() {
            continue;
        }
        let mut registries = HashMap::new();
        for found in assignment.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let name = found
                .get(1)
                .or_else(|| found.get(2))
                .unwrap()
                .as_str()
                .trim_start_matches("this.");
            if !references.contains(name) || registries.contains_key(name) {
                continue;
            }
            let Some(body) = body(&safe, found.get(0).unwrap().end() - 1) else {
                continue;
            };
            let mut names = Vec::new();
            for segment in segments(body) {
                ctx.step()?;
                if let Some(found) = entry.captures(segment)? {
                    let name = found.get(1).unwrap().as_str();
                    if name.len() >= 3 && !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
            if names.len() >= 2 {
                registries.insert(
                    name,
                    (names, ctx.line_at(file, found.get(0).unwrap().start())),
                );
            }
        }
        for (reference, line, chained) in dispatches {
            let Some((names, registered_line)) = registries.get(reference) else {
                continue;
            };
            let Some(from) = ctx.enclosing(file, line)? else {
                continue;
            };
            let mut added = 0;
            for name in names {
                ctx.step()?;
                if added >= 40 {
                    break;
                }
                let Some(target) = handler(ctx, name, chained.as_deref())? else {
                    continue;
                };
                if from.id == target.id || !seen.insert((&from.id, &target.id)) {
                    continue;
                }
                let mut edge = call(from, target, "object-registry", name);
                edge.line = Some(line);
                edge.metadata = Some(
                    json!({"synthesizedBy":"object-registry","via":name,"registeredAt":format!("{file}:{registered_line}")}),
                );
                ctx.emit(&mut output, edge)?;
                added += 1;
            }
        }
    }
    Ok(output)
}

fn handler<'a>(ctx: &Context<'a>, name: &str, chained: Option<&str>) -> Result<Option<&'a Node>> {
    ctx.steps(ctx.names.get(name).map_or(0, Vec::len))?;
    if let Some(function) = ctx.named(name).find(|n| n.kind == "function") {
        return Ok(Some(function));
    }
    if let Some(class) = ctx
        .named(name)
        .find(|n| matches!(n.kind.as_str(), "class" | "struct"))
    {
        let mut methods = Vec::new();
        for node in ctx.in_file(&class.file_path) {
            ctx.step()?;
            if node.kind == "method"
                && node.start_line >= class.start_line
                && node.start_line <= class.end_line
            {
                methods.push(node);
            }
        }
        let wanted = chained.filter(|name| ENTRY.contains(name));
        let method = wanted
            .and_then(|name| methods.iter().find(|n| n.name == name).copied())
            .or_else(|| {
                methods
                    .iter()
                    .find(|n| ENTRY.contains(&n.name.as_str()))
                    .copied()
            })
            .or_else(|| methods.iter().find(|n| n.name == "constructor").copied());
        return Ok(Some(method.unwrap_or(class)));
    }
    Ok(ctx.named(name).find(|n| n.kind == "method"))
}
fn body(source: &str, start: usize) -> Option<&str> {
    let mut depth = 0usize;
    for (offset, byte) in source.as_bytes()[start..].iter().enumerate() {
        if *byte == b'{' {
            depth += 1;
        } else if *byte == b'}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return source.get(start + 1..start + offset);
            }
        }
    }
    None
}
fn segments(source: &str) -> Vec<&str> {
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
    output.push(&source[start..]);
    output
}
fn prefix_utf16(source: &str, length: usize) -> &str {
    let mut units = 0;
    let end = source
        .char_indices()
        .find_map(|(at, character)| {
            units += character.len_utf16();
            (units > length).then_some(at)
        })
        .unwrap_or(source.len());
    &source[..end]
}
