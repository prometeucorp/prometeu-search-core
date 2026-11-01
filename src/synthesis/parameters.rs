//! Parameter-sensitive dispatch, following the reference's override filter.
use super::*;

const LANGUAGES: &[&str] = &["java", "kotlin", "csharp", "scala"];

pub(super) struct Filter<'a, 'b> {
    ctx: &'a Context<'b>,
    types: HashMap<String, Option<Vec<String>>>,
    scopes: HashMap<String, HashSet<String>>,
}
impl<'a, 'b> Filter<'a, 'b> {
    pub(super) fn new(ctx: &'a Context<'b>) -> Self {
        Self {
            ctx,
            types: HashMap::new(),
            scopes: HashMap::new(),
        }
    }
    fn types(&mut self, node: &Node) -> Result<Option<Vec<String>>> {
        if !self.types.contains_key(&node.id) {
            self.types
                .insert(node.id.clone(), parameter_types(node, self.ctx)?);
        }
        Ok(self.types[&node.id].clone())
    }
    pub(super) fn matching(&mut self, base: &Node, methods: &[&'b Node]) -> Result<Vec<&'b Node>> {
        let want = if LANGUAGES.contains(&base.language.as_str()) {
            self.types(base)?
        } else {
            None
        };
        let Some(want) = want else {
            return Ok(methods.to_vec());
        };
        let mut same = Vec::new();
        let mut maybe = Vec::new();
        for &method in methods {
            self.ctx.step()?;
            let have = if LANGUAGES.contains(&method.language.as_str()) {
                self.types(method)?
            } else {
                None
            };
            let Some(have) = have else {
                maybe.push(method);
                continue;
            };
            if have.len() != want.len() {
                continue;
            }
            if method.language != base.language {
                maybe.push(method);
                continue;
            }
            let mut fit = 0;
            for (a, b) in want.iter().zip(&have) {
                if a == b {
                    continue;
                }
                if loose_type(a, &base.language)? == loose_type(b, &base.language)? {
                    fit = 1;
                    continue;
                }
                if !self.scopes.contains_key(&base.id) {
                    self.scopes
                        .insert(base.id.clone(), type_parameters(base, self.ctx)?);
                }
                if fits_parameters(a, &self.scopes[&base.id], b)? {
                    fit = 1;
                } else {
                    fit = 2;
                    break;
                }
            }
            if fit == 0 {
                same.push(method);
            } else if fit == 1 {
                maybe.push(method);
            }
        }
        Ok(if same.is_empty() { maybe } else { same })
    }
}

fn replace(pattern: &str, text: &str, replacement: &str) -> Result<String> {
    Ok(regex(pattern)?.replace_all(text, replacement).into_owned())
}
fn loose_type(value: &str, language: &str) -> Result<String> {
    let pattern = regex(if language == "scala" {
        r"\[[^\[\]]*\]"
    } else {
        r"<[^<>]*>"
    })?;
    let mut value = value.to_owned();
    loop {
        let next = pattern.replace_all(&value, "").into_owned();
        if next == value {
            break;
        }
        value = next;
    }
    Ok(value.replace('?', ""))
}
fn fits_parameters(pattern: &str, names: &HashSet<String>, value: &str) -> Result<bool> {
    if names.is_empty() {
        return Ok(false);
    }
    if pattern.encode_utf16().count() > 200 || value.encode_utf16().count() > 200 {
        for name in names {
            if matches(
                &format!(r"(?<![\w$]){}(?![\w$])", ::regex::escape(name)),
                pattern,
            )? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    let mut expression = String::from("^");
    let mut at = 0;
    let mut named = false;
    for found in regex(r"[A-Za-z_$][\w$]*")?.find_iter(pattern) {
        let found = found?;
        expression.push_str(&::regex::escape(&pattern[at..found.start()]));
        if names.contains(found.as_str()) {
            named = true;
            expression.push_str(".+");
        } else {
            expression.push_str(&::regex::escape(found.as_str()));
        }
        at = found.end();
    }
    expression.push_str(&::regex::escape(&pattern[at..]));
    expression.push('$');
    Ok(named && matches(&expression, value)?)
}

fn parameter_types(method: &Node, ctx: &Context<'_>) -> Result<Option<Vec<String>>> {
    let Some(list) = parameter_list(method, ctx)? else {
        return Ok(None);
    };
    let mut types = Vec::new();
    for parameter in split_parameters(&list) {
        let Some(value) = parameter_type(parameter, &method.language)? else {
            return Ok(None);
        };
        if value == "\0receiver" {
            continue;
        }
        types.push(normalize_type(&value, &method.language)?);
    }
    Ok(Some(types))
}
fn parameter_list(method: &Node, ctx: &Context<'_>) -> Result<Option<String>> {
    if method.language == "csharp" {
        return csharp_list(method, ctx);
    }
    let Some(signature) = method
        .signature
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let groups = paren_groups(signature);
    let span = if method.language == "java" {
        groups
            .last()
            .copied()
            .filter(|(_, end)| *end == signature.len() - 1)
    } else if method.language == "scala" && signature.starts_with(':') {
        return Ok(Some(String::new()));
    } else {
        groups.first().copied().filter(|(start, _)| *start == 0)
    };
    Ok(span.map(|(start, end)| signature[start + 1..end].to_owned()))
}
fn csharp_list(method: &Node, ctx: &Context<'_>) -> Result<Option<String>> {
    let Some(head) = head(method, ctx, 40, true) else {
        return Ok(None);
    };
    let head = blank_csharp(&head);
    let mut at = 0;
    loop {
        while at < head.len() && head.as_bytes()[at].is_ascii_whitespace() {
            at += 1;
        }
        if head.as_bytes().get(at) != Some(&b'[') {
            break;
        }
        let Some(end) = closing_bracket(&head, at) else {
            return Ok(None);
        };
        at = end + 1;
    }
    let expression = regex(&format!(
        r"(?<![\w@])@?{}\s*(?:<[^<>()]*>)?\s*\(",
        ::regex::escape(&method.name)
    ))?;
    let Some(found) = expression.find_from_pos(&head, at)? else {
        return Ok(None);
    };
    let open = found.end() - 1;
    Ok(closing_bracket(&head, open).map(|end| head[open + 1..end].to_owned()))
}
fn blank_csharp(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && matches!(bytes.get(i + 1), Some(b'/' | b'*')) {
            let end = if bytes[i + 1] == b'/' {
                text[i..].find('\n').map_or(bytes.len(), |end| i + end)
            } else {
                text[i + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| i + end + 4)
            };
            for byte in &mut out[i..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            i = end;
        } else if matches!(bytes[i], b'"' | b'\'') {
            let quote = bytes[i];
            let verbatim = quote == b'"'
                && (i > 0 && bytes[i - 1] == b'@'
                    || i > 1 && bytes[i - 2] == b'@' && bytes[i - 1] == b'$');
            let mut end = i + 1;
            while end < bytes.len() {
                if if verbatim {
                    bytes[end] == b'"' && bytes.get(end + 1) == Some(&b'"')
                } else {
                    bytes[end] == b'\\'
                } {
                    end = (end + 2).min(bytes.len());
                    continue;
                }
                if bytes[end] == quote || !verbatim && bytes[end] == b'\n' {
                    break;
                }
                end += 1;
            }
            for byte in &mut out[i + 1..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            i = (end + 1).min(bytes.len());
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).expect("masked UTF-8 trivia")
}
fn parameter_type(parameter: &str, language: &str) -> Result<Option<String>> {
    if language == "csharp" {
        return csharp_type(parameter);
    }
    let bare = strip_annotations(parameter)?;
    let bare = bare.trim();
    if language == "java" {
        let bare = replace(r"\bfinal\b", bare, " ")?;
        let expression = regex(r"^([\s\S]*?)([A-Za-z_$][\w$]*)((?:\s*\[\s*\])*)$")?;
        let Some(found) = expression.captures(bare.trim())? else {
            return Ok(None);
        };
        let prefix = found.get(1).unwrap().as_str();
        if prefix.trim().is_empty() {
            return Ok(None);
        }
        if found.get(2).unwrap().as_str() == "this" {
            return Ok(Some("\0receiver".into()));
        }
        return Ok(Some(format!(
            "{}{}",
            prefix,
            replace(r"\s+", found.get(3).unwrap().as_str(), "")?
        )));
    }
    let Some(colon) = top_level_index(bare, |i, s| s[i] == b':') else {
        return Ok(None);
    };
    let mut value = &bare[colon + 1..];
    if let Some(assign) = top_level_index(value, |i, s| s[i] == b'=' && s.get(i + 1) != Some(&b'>'))
    {
        value = &value[..assign];
    }
    if value.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(if matches(r"\bvararg\b", &bare[..colon])? {
        format!("vararg {value}")
    } else {
        value.to_owned()
    }))
}
fn csharp_type(parameter: &str) -> Result<Option<String>> {
    let mut value = parameter.trim();
    while value.starts_with('[') {
        let Some(end) = closing_bracket(value, 0) else {
            return Ok(None);
        };
        value = value[end + 1..].trim();
    }
    if let Some(at) = top_level_index(value, |i, s| {
        s[i] == b'='
            && !i
                .checked_sub(1)
                .is_some_and(|j| matches!(s[j], b'=' | b'!' | b'<' | b'>'))
            && !matches!(s.get(i + 1), Some(b'=' | b'>'))
    }) {
        value = value[..at].trim();
    }
    let expression = regex(r"^([\s\S]*?\S)\s+@?[A-Za-z_]\w*$")?;
    let Some(found) = expression.captures(value)? else {
        return Ok(None);
    };
    let mut value = found.get(1).unwrap().as_str().trim();
    let mut kept = Vec::new();
    let modifier = regex(r"^(this|params|scoped|ref|out|in|readonly)\s+")?;
    while let Some(found) = modifier.captures(value)? {
        let name = found.get(1).unwrap().as_str();
        if !matches!(name, "this" | "params" | "scoped") {
            kept.push(name);
        }
        value = &value[found.get(0).unwrap().end()..];
    }
    if value.is_empty() {
        return Ok(None);
    }
    kept.push(value);
    Ok(Some(kept.join(" ")))
}
fn normalize_type(value: &str, language: &str) -> Result<String> {
    let mut value = replace(r"\s+", value, " ")?.trim().to_owned();
    if language == "java" {
        value = value.replace("...", "[]");
    }
    value = replace(r"\b[A-Za-z_]\w*\s*::\s*", &value, "")?;
    value = replace(r"\b(?:[A-Za-z_$][\w$]*\s*\.\s*)+(?=[A-Za-z_$])", &value, "")?;
    if language == "csharp" {
        let mut result = String::new();
        let mut at = 0;
        for found in regex(r"\b[A-Z][A-Za-z0-9]*\b")?.find_iter(&value) {
            let found = found?;
            result.push_str(&value[at..found.start()]);
            result.push_str(match found.as_str() {
                "String" => "string",
                "Object" => "object",
                "Boolean" => "bool",
                "Byte" => "byte",
                "SByte" => "sbyte",
                "Char" => "char",
                "Decimal" => "decimal",
                "Double" => "double",
                "Single" => "float",
                "Int16" => "short",
                "UInt16" => "ushort",
                "Int32" => "int",
                "UInt32" => "uint",
                "Int64" => "long",
                "UInt64" => "ulong",
                "IntPtr" => "nint",
                "UIntPtr" => "nuint",
                other => other,
            });
            at = found.end();
        }
        result.push_str(&value[at..]);
        value = result;
    }
    replace(r" (?![\w$])|(?<![\w$]) ", &value, "")
}
fn strip_annotations(text: &str) -> Result<String> {
    let annotation =
        regex(r"^@\s*[A-Za-z_$][\w$]*(?:\s*:\s*[A-Za-z_$][\w$]*)?(?:\s*\.\s*[A-Za-z_$][\w$]*)*")?;
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let found = if text.as_bytes()[i] == b'@' {
            annotation.find(&text[i..])?
        } else {
            None
        };
        if let Some(found) = found {
            let next = i + found.end();
            let open = next + text[next..].len() - text[next..].trim_start().len();
            i = if text.as_bytes().get(open) == Some(&b'(') {
                closing_bracket(text, open).map_or(text.len(), |end| end + 1)
            } else {
                next
            };
            out.push(' ');
        } else {
            let c = text[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    Ok(out)
}
fn split_parameters(list: &str) -> Vec<&str> {
    let mut parameters = Vec::new();
    let mut start = 0;
    loop {
        let comma = top_level_index(&list[start..], |i, s| s[i] == b',');
        let end = comma.map_or(list.len(), |at| start + at);
        let parameter = list[start..end].trim();
        if !parameter.is_empty() {
            parameters.push(parameter);
        }
        if comma.is_none() {
            return parameters;
        }
        start = end + 1;
    }
}
fn top_level_index(text: &str, at: impl Fn(usize, &[u8]) -> bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut angles = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if matches!(c, b'"' | b'\'') {
            i = closing_quote(text, i) + 1;
            continue;
        }
        if depth == 0 && angles == 0 && at(i, bytes) {
            return Some(i);
        }
        if matches!(c, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(c, b')' | b']' | b'}') {
            depth = depth.saturating_sub(1);
        } else if c == b'<'
            && i > 0
            && (word(bytes[i - 1]) || matches!(bytes[i - 1], b'?' | b']' | b'>'))
            && !matches!(bytes.get(i + 1), Some(b'<' | b'='))
        {
            angles += 1;
        } else if c == b'>' && angles > 0 && i > 0 && !matches!(bytes[i - 1], b'-' | b'=') {
            angles -= 1;
        }
        i += 1;
    }
    None
}
fn paren_groups(text: &str) -> Vec<(usize, usize)> {
    let mut groups = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'"' | b'\'') {
            i = closing_quote(text, i);
        } else if bytes[i] == b'(' {
            let Some(end) = closing_bracket(text, i) else {
                break;
            };
            groups.push((i, end));
            i = end;
        }
        i += 1;
    }
    groups
}
pub(super) fn closing_bracket(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        let c = bytes[i];
        if matches!(c, b'"' | b'\'') {
            i = closing_quote(text, i);
        } else if matches!(c, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(c, b')' | b']' | b'}') {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}
fn closing_quote(text: &str, open: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = open + 1;
    while end < bytes.len() && bytes[end] != bytes[open] {
        end += if bytes[end] == b'\\' { 2 } else { 1 };
    }
    end.min(bytes.len() - 1)
}
fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}
fn utf16_byte(text: &str, column: u32) -> usize {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        if units >= column {
            return at;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}
pub(super) fn head(node: &Node, ctx: &Context<'_>, limit: usize, column: bool) -> Option<String> {
    let source = ctx.slice(node)?;
    let mut lines = source
        .split('\n')
        .take(limit)
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    let first = lines.next()?;
    let first = if column {
        &first[utf16_byte(first, node.start_column)..]
    } else {
        first
    };
    Some(
        std::iter::once(first)
            .chain(lines)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}
fn type_parameters(node: &Node, ctx: &Context<'_>) -> Result<HashSet<String>> {
    let mut names = HashSet::new();
    ctx.steps(ctx.nodes.len())?;
    for declaration in ctx.nodes.iter().filter(|declaration| {
        declaration.file_path == node.file_path
            && declaration.start_line <= node.start_line
            && declaration.end_line >= node.start_line
            && matches!(
                declaration.kind.as_str(),
                "class"
                    | "struct"
                    | "interface"
                    | "trait"
                    | "protocol"
                    | "enum"
                    | "type_alias"
                    | "function"
                    | "method"
                    | "union"
                    | "module"
            )
    }) {
        ctx.step()?;
        if let Some(head) = head(declaration, ctx, 40, false) {
            names.extend(
                crate::graph::type_parameters::declared(declaration, &head, "")?.into_keys(),
            );
        }
    }
    Ok(names)
}
