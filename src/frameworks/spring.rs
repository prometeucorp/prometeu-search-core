//! Spring HTTP mappings and configuration-key declarations.
use super::{
    extractors::regex,
    routes::{reference, route},
};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, supplemental::Context};
use std::collections::{HashMap, HashSet};

fn space(source: &str, at: usize) -> usize {
    source[at..]
        .char_indices()
        .find(|(_, ch)| !crate::search::query_whitespace(*ch))
        .map_or(source.len(), |(offset, _)| at + offset)
}
fn identifier(source: &str, at: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    let first = *bytes.get(at)?;
    if !first.is_ascii_alphabetic() && !matches!(first, b'_' | b'$') {
        return None;
    }
    let mut end = at + 1;
    while bytes
        .get(end)
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$'))
    {
        end += 1;
    }
    Some(&source[at..end])
}
fn quoted(source: &str, at: usize) -> Option<usize> {
    if source[at..].starts_with("\"\"\"") {
        return source[at + 3..]
            .find("\"\"\"")
            .map(|offset| at + offset + 6);
    }
    let quote = source.as_bytes()[at];
    let mut next = at + 1;
    while let Some(&ch) = source.as_bytes().get(next) {
        if ch == b'\\' {
            next += 2;
            continue;
        }
        if ch == quote {
            return Some(next + 1);
        }
        if ch == b'\n' {
            return None;
        }
        next += 1;
    }
    None
}
fn group(source: &str, at: usize) -> Option<usize> {
    let open = source.as_bytes()[at];
    let close = match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        _ => b'>',
    };
    let mut depth = 0;
    let mut cursor = at;
    while let Some(&ch) = source.as_bytes().get(cursor) {
        if ch == b'"' || ch == b'\'' && open != b'<' {
            cursor = quoted(source, cursor)?;
            continue;
        }
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(cursor + 1);
            }
        }
        cursor += 1;
    }
    None
}
fn annotation(source: &str, at: usize, kotlin: bool) -> Option<(&str, usize)> {
    if source.as_bytes().get(at) != Some(&b'@') {
        return None;
    }
    let mut cursor = at + 1;
    if kotlin && source.as_bytes().get(cursor) == Some(&b'[') {
        return Some(("", group(source, cursor)?));
    }
    loop {
        cursor += identifier(source, cursor)?.len();
        if source.as_bytes().get(cursor) != Some(&b'.') {
            break;
        }
        cursor += 1;
    }
    if kotlin && source.as_bytes().get(cursor) == Some(&b':') {
        cursor += 1;
        loop {
            cursor += identifier(source, cursor)?.len();
            if source.as_bytes().get(cursor) != Some(&b'.') {
                break;
            }
            cursor += 1;
        }
    }
    cursor = space(source, cursor);
    if source.as_bytes().get(cursor) != Some(&b'(') {
        return Some(("", cursor));
    }
    let end = group(source, cursor)?;
    Some((&source[cursor + 1..end - 1], end))
}
fn modifiers(source: &str, at: usize, kotlin: bool) -> Option<usize> {
    const JAVA: &[&str] = &[
        "public",
        "protected",
        "private",
        "static",
        "final",
        "abstract",
        "synchronized",
        "native",
        "strictfp",
        "default",
    ];
    const KOTLIN: &[&str] = &[
        "public",
        "protected",
        "private",
        "internal",
        "open",
        "override",
        "final",
        "abstract",
        "suspend",
        "inline",
        "operator",
        "infix",
        "tailrec",
        "external",
        "actual",
        "expect",
    ];
    let mut cursor = at;
    loop {
        cursor = space(source, cursor);
        if source.as_bytes().get(cursor) == Some(&b'@') {
            cursor = annotation(source, cursor, kotlin)?.1;
            continue;
        }
        let word = identifier(source, cursor);
        if word.is_none_or(|word| !if kotlin { KOTLIN } else { JAVA }.contains(&word)) {
            return Some(cursor);
        }
        cursor += word?.len();
    }
}
fn method(source: &str, at: usize, kotlin: bool) -> Option<&str> {
    let mut cursor = modifiers(source, at, kotlin)?;
    if kotlin {
        if identifier(source, cursor) != Some("fun") {
            return None;
        }
        cursor = space(source, cursor + 3);
        if source.as_bytes().get(cursor) == Some(&b'<') {
            cursor = space(source, group(source, cursor)?);
        }
    } else {
        if source.as_bytes().get(cursor) == Some(&b'<') {
            cursor = group(source, cursor)?;
        }
        cursor = modifiers(source, cursor, kotlin)?;
        let ty = identifier(source, cursor)?;
        if ["class", "interface", "enum", "record"].contains(&ty) {
            return None;
        }
        cursor += ty.len();
        loop {
            let next = space(source, cursor);
            match source.as_bytes().get(next) {
                Some(b'<') => cursor = group(source, next)?,
                Some(b'.') => {
                    let next = space(source, next + 1);
                    cursor = next + identifier(source, next)?.len();
                }
                Some(b'[') if source.as_bytes().get(space(source, next + 1)) == Some(&b']') => {
                    cursor = space(source, next + 1) + 1
                }
                _ => break,
            }
        }
        cursor = space(source, cursor);
    }
    let name = identifier(source, cursor)?;
    (source.as_bytes().get(space(source, cursor + name.len())) == Some(&b'(')).then_some(name)
}
fn paths(args: &str, constants: &HashMap<String, String>) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut has_paths = false;
    let value = regex(r#"(?:^|,)\s*(?:"([^"]*)"|'([^']*)'|([\w.]+))\s*(?=,|$)"#)?;
    for arg in regex(r#"(?:^|,)\s*(?:(\w+)\s*=\s*)?(\{(?:[^"'}]|"[^"]*"|'[^']*')*\}|\[(?:[^"'\]]|"[^"]*"|'[^']*')*\]|"[^"]*"|'[^']*'|[^,]+)"#)?.captures_iter(args) {
        let arg=arg?;if arg.get(1).is_some_and(|key|!matches!(key.as_str(),"value"|"path")){continue;}has_paths=true;
        let scope=arg[2].trim_matches(crate::search::query_whitespace);let scope=scope.strip_prefix(['{','[']).unwrap_or(scope);let scope=scope.strip_suffix(['}',']']).unwrap_or(scope).trim_matches(crate::search::query_whitespace);
        if scope.is_empty()&&seen.insert(String::new()){out.push(String::new());}
        for matched in value.captures_iter(scope){let matched=matched?;let path=matched.get(1).or_else(||matched.get(2)).map(|path|path.as_str()).or_else(||constants.get(matched.get(3)?.as_str().rsplit('.').next()?).map(String::as_str));if let Some(path)=path&&seen.insert(path.to_owned()){out.push(path.into());}}
    }
    if !has_paths {
        out.push(String::new());
    }
    Ok(out)
}
fn join(prefix: &str, path: &str) -> String {
    format!(
        "/{}",
        [prefix, path]
            .iter()
            .map(|path| path.trim_matches('/'))
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    )
}

pub(super) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    if regex(r"(?i)^(application|bootstrap)(-[\w.-]+)?\.(yml|yaml|properties)$")?
        .is_match(path.rsplit('/').next().unwrap())?
    {
        return config(path, source);
    }
    let language = if path.ends_with(".kt") {
        "kotlin"
    } else {
        "java"
    };
    let ctx = Context::new(path, source, language);
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !path.ends_with(".kt") && !path.ends_with(".java") {
        return Ok(out);
    }
    let safe = crate::synthesis::comments::strip_for_regex(source, "java");
    let kotlin = language == "kotlin";
    let mut constants = HashMap::new();
    for matched in
        regex(r#"\bstatic\s+final\s+String\s+(\w+)\s*=\s*"([^"]*)"\s*;"#)?.captures_iter(&safe)
    {
        let matched = matched?;
        constants.insert(matched[1].to_owned(), matched[2].to_owned());
    }
    let class = regex(
        r"@RequestMapping\s*\(([^)]*)\)\s*(?:@[\w.]+(?:\([^)]*\))?\s*)*(?:public\s+|final\s+|abstract\s+|open\s+|data\s+|sealed\s+)*class\b",
    )?;
    let prefixes = if let Some(class) = class.captures(&safe)? {
        paths(&class[1], &constants)?
    } else {
        vec![String::new()]
    };
    let class_tail = regex(
        r"^\s*(?:@[\w.]+(?:\([^)]*\))?\s*)*(?:public\s+|final\s+|abstract\s+|open\s+|data\s+|sealed\s+)*class\b",
    )?;
    let verbs = regex(r"method\s*=\s*[{\[]?\s*(?:RequestMethod\.)?(\w+)")?;
    for (request, pattern) in [
        (
            false,
            r"@(GetMapping|PostMapping|PutMapping|PatchMapping|DeleteMapping)\b",
        ),
        (true, r"@RequestMapping\b"),
    ] {
        for matched in regex(pattern)?.captures_iter(&safe) {
            let matched = matched?;
            let start = matched.get(0).unwrap().start();
            let Some((args, end)) = annotation(&safe, start, kotlin) else {
                continue;
            };
            if request {
                let tail_end = super::rust::unit_end(source, end, 600);
                if class_tail.is_match(&safe[end..tail_end])? {
                    continue;
                }
            }
            let handler = method(&safe, start, kotlin);
            if request && handler.is_none() {
                continue;
            }
            let verb = if request {
                verbs
                    .captures(args)?
                    .map_or("ANY".into(), |verb| verb[1].to_uppercase())
            } else {
                matched[1].strip_suffix("Mapping").unwrap().to_uppercase()
            };
            let paths = paths(args, &constants)?;
            let line = ctx.position(start).0;
            for prefix in &prefixes {
                for path in &paths {
                    let path = join(prefix, path);
                    let mut node = route(
                        &ctx,
                        line,
                        &verb,
                        &path,
                        source[start..end].encode_utf16().count() as u32,
                    );
                    node.qualified_name = format!("{}::route:{path}", ctx.path);
                    if let Some(handler) = handler {
                        out.unresolved_references
                            .push(reference(&node, handler, "references"));
                    }
                    out.nodes.push(node);
                }
            }
        }
    }
    for (prefix, pattern) in [
        (
            false,
            r#"@Value\s*\(\s*["']\$\{([^}:]+)(?::[^}]*)?\}["']\s*\)"#,
        ),
        (
            true,
            r#"@ConfigurationProperties\s*\(\s*(?:prefix\s*=\s*)?["']([^"']+)["']"#,
        ),
    ] {
        for matched in regex(pattern)?.captures_iter(&safe) {
            let matched = matched?;
            let key = matched[1].trim_matches(crate::search::query_whitespace);
            if key.is_empty() {
                continue;
            }
            let text = matched.get(0).unwrap();
            let line = ctx.position(text.start()).0;
            let mut node = route(
                &ctx,
                line,
                "",
                key,
                source[text.start()..text.end()].encode_utf16().count() as u32,
            );
            node.kind = "constant".into();
            let (id, name) = if prefix {
                ("spring-cp", "ConfigurationProperties")
            } else {
                ("spring-value", "Value")
            };
            node.id = format!("{id}:{path}:{line}:{key}");
            node.qualified_name = format!("{path}::@{name}:{key}");
            node.signature = Some(format!("@{name}(\"{key}\")"));
            out.unresolved_references.push(reference(
                &node,
                &if prefix {
                    format!("{key}:prefix")
                } else {
                    key.into()
                },
                "references",
            ));
            out.nodes.push(node);
        }
    }
    Ok(out)
}

fn config(path: &str, source: &str) -> Result<ExtractionResult> {
    let properties = path.to_lowercase().ends_with(".properties");
    let ctx = Context::new(path, source, if properties { "properties" } else { "yaml" });
    let mut out = ctx.result.clone();
    out.nodes.clear();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (line, raw) in source.split('\n').enumerate() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = raw.trim_matches(crate::search::query_whitespace);
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || if properties {
                trimmed.starts_with('!')
            } else {
                trimmed == "---" || trimmed.starts_with("- ")
            }
        {
            continue;
        }
        let mut quote = None;
        let mut escaped = false;
        let mut separator = None;
        for (at, ch) in raw.char_indices() {
            if properties {
                if escaped {
                    escaped = false;
                    continue;
                }
                if matches!(ch, '=' | ':') {
                    separator = Some(at);
                    break;
                }
                if ch == '\\' {
                    escaped = true;
                }
            } else {
                if let Some(q) = quote {
                    if ch == q && raw.as_bytes().get(at.wrapping_sub(1)) != Some(&b'\\') {
                        quote = None;
                    }
                    continue;
                }
                if matches!(ch, '\'' | '"') {
                    quote = Some(ch);
                } else if ch == ':' {
                    separator = Some(at);
                    break;
                }
            }
        }
        let Some(separator) = separator else {
            continue;
        };
        let indent = if properties {
            0
        } else {
            raw.bytes()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count()
        };
        let key = raw[indent..separator].trim_matches(crate::search::query_whitespace);
        if key.is_empty() {
            continue;
        }
        let value = raw[separator + 1..].trim_matches(crate::search::query_whitespace);
        let key = if properties {
            key.to_owned()
        } else {
            while stack
                .last()
                .is_some_and(|(previous, _)| *previous >= indent)
            {
                stack.pop();
            }
            let dotted = stack
                .iter()
                .map(|(_, key)| key.as_str())
                .chain(std::iter::once(key))
                .collect::<Vec<_>>()
                .join(".");
            if value.is_empty() || value.starts_with('#') {
                stack.push((indent, key.into()));
                continue;
            }
            dotted
        };
        let value = if properties {
            value
        } else {
            value.strip_prefix(['\'', '"']).unwrap_or(value)
        };
        let value = if properties {
            value
        } else {
            value.strip_suffix(['\'', '"']).unwrap_or(value)
        };
        let line = line as u32 + 1;
        let mut node = route(&ctx, line, "", &key, value.encode_utf16().count() as u32);
        node.id = format!("spring-config:{path}:{line}:{key}");
        node.kind = "constant".into();
        node.name = key.rsplit('.').next().unwrap().into();
        node.qualified_name = key.clone();
        node.signature = Some(key);
        out.nodes.push(node);
    }
    Ok(out)
}
