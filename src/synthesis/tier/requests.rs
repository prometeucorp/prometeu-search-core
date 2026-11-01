//! Spring and Laravel test requests, including literal interpolation and helper chains.
use super::*;
use crate::frameworks::frontend::href::HOLE;
const VERBS: &str = "get|post|put|patch|delete|head|options";
fn whitespace(source: &str, at: usize) -> usize {
    source.len()
        - source[at..]
            .trim_start_matches(crate::search::query_whitespace)
            .len()
}
fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
fn close_brace(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0;
    for (at, &byte) in bytes.iter().enumerate().skip(open) {
        if byte == b'{' {
            depth += 1
        } else if byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return at;
            }
        }
    }
    bytes.len().saturating_sub(1)
}
fn operand_end(bytes: &[u8], mut at: usize, concat: u8) -> usize {
    let mut depth = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if matches!(byte, b'\'' | b'"') {
            at += 1;
            while at < bytes.len() && bytes[at] != byte {
                if bytes[at] == b'\\' {
                    at += 1
                }
                at += 1;
            }
        } else if matches!(byte, b'(' | b'[') {
            depth += 1
        } else if matches!(byte, b')' | b']') {
            if depth == 0 {
                return at;
            }
            depth -= 1;
        } else if depth == 0
            && (matches!(byte, b',' | b';')
                || (byte == concat
                    && !(concat == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit))))
        {
            return at;
        }
        if byte == b'-' && bytes.get(at + 1) == Some(&b'>') {
            at += 1
        }
        at += 1;
    }
    at
}
pub(super) fn read_path(source: &str, mut at: usize, language: &str) -> Result<Option<String>> {
    let bytes = source.as_bytes();
    let concat = if language == "php" { b'.' } else { b'+' };
    let mut output = Vec::new();
    let mut literal = false;
    loop {
        at = whitespace(source, at.min(source.len()));
        let byte = bytes.get(at).copied();
        if matches!(byte, Some(b'\'' | b'"')) {
            let quote = byte.unwrap();
            let interpolates = quote == b'"' && language != "java";
            at += 1;
            while at < bytes.len() && bytes[at] != quote {
                let byte = bytes[at];
                if byte == b'\\' {
                    at += 1;
                    if let Some(&escaped) = bytes.get(at) {
                        output.push(escaped)
                    }
                    at += 1;
                    continue;
                }
                if interpolates
                    && byte == b'$'
                    && bytes
                        .get(at + 1)
                        .is_some_and(|byte| *byte == b'{' || word(*byte))
                {
                    if bytes[at + 1] == b'{' {
                        at = close_brace(bytes, at + 1);
                    } else {
                        while bytes.get(at + 1).is_some_and(|byte| word(*byte)) {
                            at += 1
                        }
                        if language == "php" {
                            while bytes.get(at + 1..at + 3) == Some(b"->")
                                && bytes.get(at + 3).is_some_and(|byte| word(*byte))
                            {
                                at += 2;
                                while bytes.get(at + 1).is_some_and(|byte| word(*byte)) {
                                    at += 1
                                }
                            }
                        }
                    }
                    output.push(0);
                    at += 1;
                    continue;
                }
                if interpolates
                    && language == "php"
                    && byte == b'{'
                    && bytes.get(at + 1) == Some(&b'$')
                {
                    at = close_brace(bytes, at);
                    output.push(0);
                    at += 1;
                    continue;
                }
                output.push(byte);
                at += 1;
            }
            if at >= bytes.len() {
                return Ok(None);
            }
            at += 1;
            literal = true;
        } else {
            if !literal {
                return Ok(None);
            }
            let end = operand_end(bytes, at, concat);
            if end == at {
                break;
            }
            output.push(0);
            at = end;
        }
        at = whitespace(source, at.min(source.len()));
        if bytes.get(at) == Some(&concat)
            && !(concat == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit))
        {
            at += 1;
            continue;
        }
        break;
    }
    if !literal {
        return Ok(None);
    }
    let output = String::from_utf8(output)?;
    Ok(Some(if language == "php" {
        output
    } else {
        regex(r"\{[^{}/]*\}")?
            .replace_all(&output, HOLE.to_string())
            .into_owned()
    }))
}
pub(super) fn path(raw: &str) -> Result<Option<http::Path>> {
    let mut value = raw.split(['?', '#']).next().unwrap_or("").to_owned();
    if let Some(found) = regex(r"(?i)^(?:[a-z][a-z0-9+.-]*:)?//[^/]*(/.*)?$")?.captures(&value)? {
        value = found.get(1).map_or("/", |part| part.as_str()).into();
    }
    http::path(&format!("/{}", value.trim_start_matches('/')), None)
}
fn helper(
    ctx: &Context<'_>,
    name: &str,
    memo: &mut HashMap<String, bool>,
    depth: u32,
) -> Result<bool> {
    if let Some(&known) = memo.get(name) {
        return Ok(known);
    }
    memo.insert(name.into(), false);
    let mut found = false;
    for node in ctx.named(name) {
        ctx.step()?;
        if node.kind != "method"
            || node.language != "php"
            || !crate::classify::is_test_path(&node.file_path)
        {
            continue;
        }
        let Some(source) = ctx.sources.get(node.file_path.as_str()) else {
            continue;
        };
        let body = crate::frameworks::frontend::href::lines(source)
            .into_iter()
            .skip(node.start_line.saturating_sub(1) as usize)
            .take((node.end_line - node.start_line + 1) as usize)
            .collect::<Vec<_>>()
            .join("\n");
        ctx.steps(body.len())?;
        if regex(&format!(r"->\s*(?:(?:{VERBS})(?:Json)?|json|call)\s*\("))?.is_match(&body)? {
            found = true;
        } else if depth == 0 {
            for call in regex(r"\$this\s*->\s*(\w+)\s*\(")?.captures_iter(&body) {
                let call = call?;
                if &call[1] != name && helper(ctx, &call[1], memo, depth + 1)? {
                    found = true;
                    break;
                }
            }
        }
        if found {
            break;
        }
    }
    if found || depth == 0 {
        memo.insert(name.into(), found);
    } else {
        memo.remove(name);
    }
    Ok(found)
}
fn this_rooted(ctx: &Context<'_>, source: &str, arrow: usize) -> Result<bool> {
    let mut from = arrow;
    for _ in 0..4 {
        let Some(root) = source[..from].rfind("$this") else {
            return Ok(false);
        };
        let mut at = root + 5;
        loop {
            at = whitespace(source, at);
            if at == arrow {
                return Ok(true);
            }
            if !source[at..].starts_with("->") {
                break;
            }
            at += 2;
            at = whitespace(source, at);
            let Some(ident) = regex(r"^\w+")?.find(scan::prefix(&source[at..], 64))? else {
                break;
            };
            at += ident.end();
            at = whitespace(source, at);
            if source.as_bytes().get(at) != Some(&b'(') {
                break;
            }
            let Some(close) = scan::close_paren(ctx, source, at)? else {
                break;
            };
            at = close + 1;
        }
        from = root;
    }
    Ok(false)
}
fn collect<'a>(
    ctx: &Context<'a>,
    file: &str,
    sites: &mut Vec<Site<'a>>,
    helpers: &mut HashMap<String, bool>,
) -> Result<()> {
    let Some(source) = ctx.sources.get(file).filter(|source| !source.is_empty()) else {
        return Ok(());
    };
    if !regex(r"[mM]ockMvc|[wW]ebTestClient\b|[rR]estTemplate\b|\bRestAssured\b|\.\s*when\s*\(\s*\)|\$this\s*->|Pest\\Laravel")?.is_match(source)?{return Ok(())}
    ctx.steps(source.len())?;
    let language = if file.ends_with(".php") {
        "php"
    } else if file.ends_with(".kt") || file.ends_with(".kts") {
        "kotlin"
    } else {
        "java"
    };
    let safe = super::super::comments::strip_for_regex(
        source,
        if language == "php" { "php" } else { "java" },
    );
    let mut add = |index: usize, arg: usize, method: &str, callee: String| -> Result<()> {
        let Some(raw) = read_path(&safe, arg, language)? else {
            return Ok(());
        };
        let Some(path) = path(&raw)? else {
            return Ok(());
        };
        let line = ctx.line_at(file, index);
        let from = ctx
            .enclosing(file, line)?
            .or_else(|| ctx.in_file(file).find(|node| node.kind == "file"));
        let Some(from) = from else { return Ok(()) };
        sites.push(Site {
            from,
            line,
            column: column(ctx, file, index),
            callee,
            method: method.to_uppercase(),
            path,
        });
        Ok(())
    };
    if language == "php" {
        for found in regex(&format!(r"->\s*({VERBS})(Json)?\s*\("))?.captures_iter(&safe) {
            let found = found?;
            let text = found.get(0).unwrap();
            if this_rooted(ctx, &safe, text.start())? {
                add(
                    text.start(),
                    text.end(),
                    &found[1],
                    format!(
                        "$this->{}{}",
                        &found[1],
                        found.get(2).map_or("", |part| part.as_str())
                    ),
                )?;
            }
        }
        for found in
            regex(r#"->\s*(?:json|call)\s*\(\s*(['"])([A-Za-z]+)\1\s*,"#)?.captures_iter(&safe)
        {
            let found = found?;
            let text = found.get(0).unwrap();
            if this_rooted(ctx, &safe, text.start())? {
                add(text.start(), text.end(), &found[2], "$this->json".into())?;
            }
        }
        for found in regex(&format!(r"->\s*(({VERBS})[A-Z]\w*)\s*\("))?.captures_iter(&safe) {
            let found = found?;
            let text = found.get(0).unwrap();
            let name = &found[1];
            if regex(&format!(r"^(?:{VERBS})Json$"))?.is_match(name)? {
                continue;
            }
            if this_rooted(ctx, &safe, text.start())? && helper(ctx, name, helpers, 0)? {
                add(
                    text.start(),
                    text.end(),
                    &found[2],
                    format!("$this->{name}"),
                )?;
            }
        }
        if regex(r"Pest\\Laravel")?.is_match(&safe)? {
            for found in
                regex(&format!(r"(?<![\w$>:\\])({VERBS})(Json)?\s*\("))?.captures_iter(&safe)
            {
                let found = found?;
                let text = found.get(0).unwrap();
                add(
                    text.start(),
                    text.end(),
                    &found[1],
                    format!(
                        "{}{}",
                        &found[1],
                        found.get(2).map_or("", |part| part.as_str())
                    ),
                )?;
            }
        }
        return Ok(());
    }
    if regex(r"\bMockMvcRequestBuilders\b")?.is_match(&safe)? {
        for found in regex(&format!(
            r"(?<![\w$.])(?:MockMvcRequestBuilders\s*\.\s*)?({VERBS}|multipart)\s*\("
        ))?
        .captures_iter(&safe)
        {
            let found = found?;
            let text = found.get(0).unwrap();
            add(
                text.start(),
                text.end(),
                if &found[1] == "multipart" {
                    "post"
                } else {
                    &found[1]
                },
                found[1].into(),
            )?;
        }
        for found in regex(r"(?<![\w$.])(?:MockMvcRequestBuilders\s*\.\s*)?request\s*\(\s*HttpMethod\s*\.\s*([A-Z]+)\s*,")?.captures_iter(&safe){let found=found?;let text=found.get(0).unwrap();add(text.start(),text.end(),&found[1],"request".into())?;}
    }
    for (pattern, kind) in [
        (format!(r"\b\w*[mM]ockMvc\s*\.\s*({VERBS})\s*\("), 0),
        (format!(r"\.\s*({VERBS})\s*\(\s*\)\s*\.\s*uri\s*\("), 1),
        (
            format!(r"(?:\.\s*when\s*\(\s*\)\s*|\bRestAssured\s*)\.\s*({VERBS})\s*\("),
            2,
        ),
    ] {
        for found in regex(&pattern)?.captures_iter(&safe) {
            let found = found?;
            let text = found.get(0).unwrap();
            let name = &found[1];
            let callee = match kind {
                0 => format!("mockMvc.{name}"),
                1 => format!("{name}().uri"),
                _ => name.into(),
            };
            add(text.start(), text.end(), name, callee)?;
        }
    }
    for found in regex(r"\b\w*[Rr]estTemplate\s*\.\s*(getForEntity|getForObject|postForEntity|postForObject|postForLocation|put|delete|patchForObject|exchange)\s*\(")?.captures_iter(&safe){let found=found?;let text=found.get(0).unwrap();let name=&found[1];
        let method=if name.starts_with("get"){Some("GET".into())}else if name.starts_with("post"){Some("POST".into())}else if name=="put"{Some("PUT".into())}else if name=="delete"{Some("DELETE".into())}else if name.starts_with("patch"){Some("PATCH".into())}else{
            let args=scan::arguments(ctx,&safe,text.end()-1)?;regex(r"HttpMethod\s*\.\s*([A-Z]+)")?.captures(args.as_ref().and_then(|args|args.get(1)).copied().unwrap_or(""))?.map(|found|found[1].to_owned())};
        if let Some(method)=method{add(text.start(),text.end(),&method,format!("restTemplate.{name}"))?;}
    }
    Ok(())
}
pub(super) fn edges(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let routes = http::routes(ctx);
    if routes.is_empty() {
        return Ok(Vec::new());
    }
    let mut sites = Vec::new();
    let mut helpers = HashMap::new();
    for &file in ctx.sources.keys() {
        if regex(r"\.(?:java|kt|kts|php)$")?.is_match(file)? && crate::classify::is_test_path(file)
        {
            collect(ctx, file, &mut sites, &mut helpers)?;
        }
    }
    let mut out = Vec::new();
    http::edges(ctx, &sites, &routes, &mut out, &mut HashSet::new(), true)?;
    Ok(out)
}
