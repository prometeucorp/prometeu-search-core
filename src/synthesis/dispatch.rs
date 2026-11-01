//! Framework dispatch keyed by a declared request or event type.
use super::*;

pub(super) fn spring(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["java"]) {
        return Ok(Vec::new());
    }
    let listener = regex(r"@(?:EventListener|TransactionalEventListener)\b")?;
    let annotation_type =
        regex(r"@(?:EventListener|TransactionalEventListener)\s*\(\s*([A-Z][A-Za-z0-9_]*)\.class")?;
    let app_listener = regex(r"\bApplicationListener\s*<")?;
    let publish = regex(r"\.publishEvent\s*\(\s*new\s+([A-Z][A-Za-z0-9_]*)")?;
    let mut listeners: HashMap<String, Vec<&Node>> = HashMap::new();
    let mut publishers = Vec::new();
    for (&file, &content) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".java") || content.is_empty() {
            continue;
        }
        if content.contains(".publishEvent(") {
            publishers.push(file);
        }
        let has_annotation =
            content.contains("@EventListener") || content.contains("@TransactionalEventListener");
        let has_app = app_listener.is_match(content)?;
        if !has_annotation && !has_app {
            continue;
        }
        for method in ctx.in_file(file).filter(|n| n.kind == "method") {
            ctx.step()?;
            let mut annotations = Vec::new();
            for line in method.start_line..method.start_line.saturating_add(8) {
                let Some(text) = ctx.line(file, line) else {
                    break;
                };
                let text = text.trim();
                if !text.starts_with('@') {
                    break;
                }
                annotations.push(text);
            }
            let head = annotations.join("\n");
            let annotated = has_annotation && listener.is_match(&head)?;
            if !annotated && !(has_app && method.name == "onApplicationEvent") {
                continue;
            }
            let mut event = spring_parameter(method.signature.as_deref())?;
            if event.is_none() && annotated {
                event = annotation_type
                    .captures(&head)?
                    .map(|found| found.get(1).unwrap().as_str().to_owned());
            }
            if let Some(event) = event {
                listeners.entry(event).or_default().push(method);
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for file in publishers {
        let source = ctx.sources[file];
        let safe = strip_cstyle(source, false);
        let newlines = newlines(&safe);
        let mut added = 0;
        for found in publish.captures_iter(&safe) {
            ctx.step()?;
            if added >= 80 {
                break;
            }
            let found = found?;
            let event = found.get(1).unwrap().as_str();
            let Some(targets) = listeners.get(event) else {
                continue;
            };
            let line =
                1 + newlines.partition_point(|&at| at < found.get(0).unwrap().start()) as u32;
            let Some(from) = ctx.enclosing(file, line)? else {
                continue;
            };
            for &target in targets {
                ctx.step()?;
                if from.id == target.id || !seen.insert((&from.id, &target.id)) {
                    continue;
                }
                let mut edge = call(from, target, "spring-event", event);
                edge.line = Some(line);
                edge.metadata = Some(
                    json!({"synthesizedBy":"spring-event","via":event,"registeredAt":format!("{file}:{line}")}),
                );
                ctx.emit(&mut out, edge)?;
                added += 1;
            }
        }
    }
    Ok(out)
}
fn spring_parameter(signature: Option<&str>) -> Result<Option<String>> {
    let Some(signature) = signature else {
        return Ok(None);
    };
    let Some(open) = signature.find('(') else {
        return Ok(None);
    };
    let tail = &signature[open + 1..];
    let inner = tail.split(')').next().unwrap_or(tail).trim();
    let first = inner.split(',').next().unwrap_or("").trim();
    let words: Vec<_> = first
        .split_whitespace()
        .filter(|word| *word != "final" && !word.starts_with('@'))
        .collect();
    if words.len() < 2 {
        return Ok(None);
    }
    let kind = words[words.len() - 2].split('<').next().unwrap_or("");
    Ok(matches(r"^[A-Z][A-Za-z0-9_]*$", kind)?.then(|| kind.to_owned()))
}

pub(super) fn mediatr<'a>(ctx: &Context<'a>) -> Result<Vec<Edge>> {
    if !ctx.has(&["csharp"]) {
        return Ok(Vec::new());
    }
    let dispatch =
        regex(r"([A-Za-z_][\w.]*)\s*\.\s*(Send|Publish)\s*\(\s*(new\s+[A-Z]\w*|[A-Za-z_]\w*)")?;
    let receiver = regex(r"(?i)(?:mediator|sender|publisher)")?;
    let mut sites = Vec::new();
    for (&file, &content) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".cs") || !content.contains(".Send(") && !content.contains(".Publish(") {
            continue;
        }
        let safe = strip_cstyle(content, false);
        let lines: Vec<_> = safe.split('\n').collect();
        let newlines = newlines(&safe);
        for found in dispatch.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            if !receiver.is_match(found.get(1).unwrap().as_str())? {
                continue;
            }
            let line =
                1 + newlines.partition_point(|&at| at < found.get(0).unwrap().start()) as u32;
            let Some(from) = ctx.enclosing(file, line)? else {
                continue;
            };
            if let Some(kind) = argument_type(
                found.get(3).unwrap().as_str(),
                &lines,
                from.start_line,
                line,
            )? {
                sites.push((
                    file,
                    line,
                    found.get(2).unwrap().as_str().to_owned(),
                    kind,
                    from,
                ));
            }
        }
    }
    if sites.is_empty() {
        return Ok(Vec::new());
    }
    let mut handlers: HashMap<String, Vec<&Node>> = HashMap::new();
    let base = regex(r"(?:IRequestHandler|INotificationHandler)\s*<\s*([A-Za-z_]\w*)")?;
    let first = regex(r"\bHandle\s*\(\s*(?:\[[^\]]*\]\s*)?(?:[A-Za-z_]\w*\.)*([A-Za-z_]\w*)")?;
    for (&file, &content) in ctx.sources {
        ctx.step()?;
        if !file.ends_with(".cs") || !content.contains("Handle(") {
            continue;
        }
        let methods: Vec<_> = ctx
            .in_file(file)
            .filter(|n| n.kind == "method" && n.name == "Handle")
            .collect();
        if methods.is_empty() {
            continue;
        }
        for class in ctx.in_file(file).filter(|n| n.kind == "class") {
            ctx.step()?;
            ctx.steps(methods.len())?;
            let handles: Vec<_> = methods
                .iter()
                .copied()
                .filter(|n| n.start_line >= class.start_line && n.start_line <= class.end_line)
                .collect();
            if handles.is_empty() {
                continue;
            }
            let mut add = |kind: String, method: &'a Node| {
                let known = handlers.entry(kind).or_default();
                if !known.iter().any(|n| n.id == method.id) {
                    known.push(method);
                }
            };
            let declaration_head = head(ctx, file, class.start_line, 4, "\n");
            let declaration = declaration_head
                .split('{')
                .next()
                .unwrap_or(&declaration_head);
            if let Some(found) = base.captures(declaration)? {
                add(found.get(1).unwrap().as_str().to_owned(), handles[0]);
            }
            let base_types = base_types(&head(ctx, file, class.start_line, 16, "\n"))?;
            for method in handles {
                let head = head(ctx, file, method.start_line, 3, " ");
                if let Some(found) = first.captures(&head)? {
                    let kind = found.get(1).unwrap().as_str();
                    if base_types.contains(kind) {
                        add(kind.to_owned(), method);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut added_per_file: HashMap<&str, usize> = HashMap::new();
    for (file, line, verb, kind, from) in sites {
        let Some(targets) = handlers.get(&kind) else {
            continue;
        };
        let added = added_per_file.entry(file).or_default();
        if *added >= 80 {
            continue;
        }
        let shared: Vec<_> = targets
            .iter()
            .map(|target| shared_folders(file, &target.file_path))
            .collect();
        let best = shared.iter().copied().max().unwrap_or(0);
        for (i, &target) in targets.iter().enumerate() {
            ctx.step()?;
            if targets.len() >= 2
                && (verb == "Send" && shared[i] != best
                    || verb != "Send" && best > 0 && shared[i] == 0)
            {
                continue;
            }
            if target.id == from.id || !seen.insert((&from.id, &target.id)) {
                continue;
            }
            let mut edge = call(from, target, "mediatr-dispatch", &kind);
            edge.line = Some(line);
            edge.metadata = Some(
                json!({"synthesizedBy":"mediatr-dispatch","via":kind,"registeredAt":format!("{file}:{line}")}),
            );
            ctx.emit(&mut out, edge)?;
            *added += 1;
        }
    }
    Ok(out)
}
fn newlines(source: &str) -> Vec<usize> {
    source
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| (b == b'\n').then_some(i))
        .collect()
}
fn head(ctx: &Context<'_>, file: &str, start: u32, count: u32, separator: &str) -> String {
    (start..start.saturating_add(count))
        .filter_map(|line| ctx.line(file, line))
        .collect::<Vec<_>>()
        .join(separator)
}
fn base_types(head: &str) -> Result<HashSet<String>> {
    let text = head.split('{').next().unwrap_or(head);
    let text = regex(r"\([^()]*(?:\([^()]*\)[^()]*)*\)")?
        .replace_all(text, " ")
        .into_owned();
    let mut types = HashSet::new();
    let Some(colon) = text.find(':') else {
        return Ok(types);
    };
    let own = regex(r"<([^<>]*)>")?
        .captures(&text[..colon])?
        .map(|found| {
            found
                .get(1)
                .unwrap()
                .as_str()
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    for found in
        regex(r"[<,]\s*(?:[A-Za-z_]\w*\.)*([A-Za-z_]\w*)")?.captures_iter(&text[colon + 1..])
    {
        let found = found?;
        let name = found.get(1).unwrap().as_str();
        if !own.contains(name) {
            types.insert(name.to_owned());
        }
    }
    Ok(types)
}
fn argument_type(argument: &str, lines: &[&str], start: u32, end: u32) -> Result<Option<String>> {
    if let Some(found) = regex(r"^new\s+([A-Z]\w*)")?.captures(argument)? {
        return Ok(Some(found.get(1).unwrap().as_str().to_owned()));
    }
    if !matches(r"^[A-Za-z_]\w*$", argument)? {
        return Ok(None);
    }
    let argument = ::regex::escape(argument);
    let assigned = regex(&format!(r"\b{argument}\b\s*=\s*new\s+([A-Z]\w*)"))?;
    let mapped = regex(&format!(
        r"\b{argument}\b\s*=[^;]*?\.\s*\w+<(?:[^<>;]*,\s*)?(?:\w+\.)*([A-Z]\w*)>\s*\("
    ))?;
    let declaration = regex(&format!(r"\b([A-Z]\w*)\b\s+{argument}\b"))?;
    let mut declared = None;
    for line in lines
        .iter()
        .take(end as usize)
        .skip(start.saturating_sub(1) as usize)
    {
        if let Some(found) = assigned.captures(line)?.or(mapped.captures(line)?) {
            return Ok(Some(found.get(1).unwrap().as_str().to_owned()));
        }
        if declared.is_none() {
            declared = declaration
                .captures(line)?
                .map(|found| found.get(1).unwrap().as_str().to_owned());
        }
    }
    Ok(declared)
}
fn shared_folders(left: &str, right: &str) -> usize {
    let left: Vec<_> = left.split('/').collect();
    let right: Vec<_> = right.split('/').collect();
    left.iter()
        .take(left.len().saturating_sub(1))
        .zip(right.iter().take(right.len().saturating_sub(1)))
        .take_while(|(left, right)| left == right)
        .count()
}
