use super::*;

pub(super) struct Attribute {
    pub mentioned: bool,
    pub evidence: Vec<Evidence>,
}
fn indent(line: &str) -> usize {
    line.chars()
        .take_while(|ch| crate::search::query_whitespace(*ch))
        .map(char::len_utf16)
        .sum()
}
pub(super) fn joined(
    lines: &[String],
    ln: usize,
    first: &str,
    last: usize,
) -> Result<(String, usize)> {
    if value::expression(first)?.open != value::Open::Continues {
        return Ok((first.to_owned(), ln));
    }
    let mut text = first.to_owned();
    let mut end = ln;
    while end < last && end - ln < 30 {
        text = regex(r"\\\s*$")?.try_replacen(&text, 1, "")?.into_owned();
        text.push(' ');
        text.push_str(trim(lines.get(end).map(String::as_str).unwrap_or_default()));
        end += 1;
        match value::expression(&text)?.open {
            value::Open::None => return Ok((text, end)),
            value::Open::Broken => break,
            _ => {}
        }
    }
    Ok((first.to_owned(), ln))
}
pub(super) fn callees(raw: &str) -> Result<Vec<String>> {
    fn walk(raw: &str, depth: usize) -> Result<Vec<String>> {
        ensure!(depth <= 128, "Python conditional evidence depth exceeded");
        if let Some([a, b]) = value::conditional(raw) {
            let mut names = walk(a, depth + 1)?;
            names.extend(walk(b, depth + 1)?);
            return Ok(names);
        }
        let stripped = regex(r"^await\s+")?.try_replacen(trim(raw), 1, "")?;
        let Some(shape) = value::expression(&stripped)?.shape else {
            return Ok(Vec::new());
        };
        Ok(regex(r"^([A-Za-z_][\w.]*) ?\(\)$")?
            .captures(&shape)?
            .map(|capture| vec![capture[1].to_owned()])
            .unwrap_or_default())
    }
    walk(raw, 0)
}
struct Statement {
    statements: Vec<String>,
    nested: Option<String>,
}
fn def_statements(
    lines: &[String],
    definition: usize,
    upto: usize,
) -> Result<(String, Vec<Statement>)> {
    let mut header = String::new();
    let mut ln = definition;
    while ln <= upto && ln <= lines.len() {
        header.push(' ');
        header.push_str(trim(&lines[ln - 1]));
        if regex(r"\)\s*(?:->[^:]*)?:\s*$")?.is_match(&header)? {
            break;
        }
        ln += 1;
    }
    let mut body = Vec::new();
    let mut depth = 0i32;
    let mut continued = false;
    let mut nested_indent = None;
    for k in ln + 1..upto {
        let raw = lines.get(k - 1).map(String::as_str).unwrap_or_default();
        if trim(raw).is_empty() {
            continue;
        }
        let blanked = statements::blank_strings(raw);
        let inside = depth > 0 || continued;
        for ch in blanked.chars() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = (depth - 1).max(0),
                _ => {}
            }
        }
        continued = trim(&blanked).ends_with('\\');
        if inside {
            continue;
        }
        if let Some(nested) = nested_indent {
            if indent(raw) > nested {
                continue;
            }
            nested_indent = None
        }
        let text = trim(&blanked);
        if let Some(capture) = regex(r"^(?:async\s+)?(?:def|class)\s+(\w+)")?.captures(text)? {
            nested_indent = Some(indent(raw));
            body.push(Statement {
                statements: Vec::new(),
                nested: Some(capture[1].to_owned()),
            });
            continue;
        }
        body.push(Statement {
            statements: text
                .split(';')
                .map(trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
                .collect(),
            nested: None,
        });
    }
    Ok((header, body))
}
pub(super) fn bound(
    lines: &[String],
    definition: usize,
    upto: usize,
    name: &str,
    params: bool,
) -> Result<bool> {
    let (header, body) = def_statements(lines, definition, upto)?;
    if params && let Some(list) = regex(r"\((.*)\)")?.captures(&header)? {
        for param in list[1].split(',').map(trim) {
            let param = param
                .strip_prefix("**")
                .or_else(|| param.strip_prefix('*'))
                .unwrap_or(param);
            if trim(param.split([':', '=']).next().unwrap_or_default()) == name {
                return Ok(true);
            }
        }
    }
    let escaped = regex::escape(name);
    let loop_pattern = regex(&format!(
        r"^(?:async\s+)?for\s+[\w\s,()\[\]]*(?<![.\w]){escaped}\b[\w\s,()\[\]]*\bin\b"
    ))?;
    let as_pattern = regex(&format!(r"\bas\s+\(?\s*{escaped}\b"))?;
    let walrus = regex(&format!(r"(?<![.\w]){escaped}\s*:="))?;
    for row in body {
        if row.nested.as_deref() == Some(name) {
            return Ok(true);
        }
        for statement in row.statements {
            for target in statements::assignment_targets(&statement)? {
                if statements::target_binds(&target, name)? {
                    return Ok(true);
                }
            }
            if loop_pattern.is_match(&statement)?
                || walrus.is_match(&statement)?
                || !regex(r"^(?:from|import)\b")?.is_match(&statement)?
                    && as_pattern.is_match(&statement)?
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
pub(super) fn constructor(text: &str) -> Result<Option<String>> {
    let mut text = text;
    while trim(text).starts_with('(') {
        let wrapped = trim(text);
        let mut depth = 0i32;
        let mut close = None;
        for (at, ch) in wrapped.char_indices() {
            if ch == '(' {
                depth += 1
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    close = Some(at);
                    break;
                }
            }
        }
        if close != Some(wrapped.len() - 1) {
            break;
        }
        text = trim(&wrapped[1..wrapped.len() - 1]);
    }
    let Some(callee) = regex(r"^((?:[A-Za-z_]\w*\.)*[A-Z]\w*)\s*\(")?.captures(text)? else {
        return Ok(None);
    };
    let start = callee.get(0).unwrap().end() - 1;
    let mut depth = 0i32;
    let mut quote = None;
    let mut escaped = false;
    for (at, ch) in text.char_indices().filter(|(at, _)| *at >= start) {
        if escaped {
            escaped = false;
            continue;
        }
        if let Some(q) = quote {
            if ch == '\\' {
                escaped = true
            } else if ch == q {
                quote = None
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(regex(r"^[\s\\]*$")?
                        .is_match(&text[at + 1..])?
                        .then(|| callee[1].to_owned()));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}
impl<'a> Typed<'a, '_> {
    pub(super) fn attr_statements(&self, owner: &'a Node, attr: &str) -> Result<Rc<Attribute>> {
        let key = (owner.id.clone(), attr.to_owned());
        if let Some(cached) = self.statements.borrow().get(&key) {
            return Ok(cached.clone());
        }
        let lines = self.line_text(&owner.file_path);
        let nested = self
            .in_file(&owner.file_path)
            .iter()
            .copied()
            .filter(|node| {
                node.kind == "class"
                    && node.id != owner.id
                    && node.start_line > owner.start_line
                    && node.end_line <= owner.end_line
            })
            .collect::<Vec<_>>();
        let attr = regex::escape(attr);
        let class_level = regex(&format!(
            r"^{attr}\s*(?::\s*([^=]+?))?\s*(?:=\s*(.+?))?\s*$"
        ))?;
        let via_self = regex(&format!(
            r"\b(?:self|cls)\.{attr}\s*(?::\s*([^=]+?))?\s*(?:=(?!=)\s*(.+?))?\s*$"
        ))?;
        let mut body_indent = None;
        let mut signature = None;
        let mut def_start = None;
        let mut nested_indent = None;
        let mut mentioned = false;
        let mut evidence = Vec::new();
        let mut ln = owner.start_line as usize + 1;
        while ln <= owner.end_line as usize {
            self.charge(1)?;
            let line = lines.get(ln - 1).map(String::as_str).unwrap_or_default();
            if trim(line).is_empty()
                || nested
                    .iter()
                    .any(|node| ln >= node.start_line as usize && ln <= node.end_line as usize)
            {
                ln += 1;
                continue;
            }
            let body = *body_indent.get_or_insert_with(|| indent(line));
            let text = trim(line);
            if regex(r"^(?:async\s+)?def\s")?.is_match(text)? && indent(line) == body {
                def_start = Some(ln);
                nested_indent = None;
                let mut head = text.to_owned();
                let mut k = ln;
                while k < owner.end_line as usize
                    && !regex(r"\)\s*(?:->[^:]*)?:\s*$")?.is_match(&head)?
                {
                    self.charge(1)?;
                    head.push(' ');
                    head.push_str(trim(lines.get(k).map(String::as_str).unwrap_or_default()));
                    k += 1
                }
                signature = Some(head);
                ln += 1;
                continue;
            }
            if nested_indent.is_some_and(|nested| indent(line) <= nested) {
                nested_indent = None
            }
            if nested_indent.is_none()
                && regex(r"^(?:async\s+)?def\s")?.is_match(text)?
                && indent(line) > body
            {
                nested_indent = Some(indent(line));
                ln += 1;
                continue;
            }
            let capture = if indent(line) == body {
                class_level.captures(text)?
            } else {
                via_self.captures(text)?
            };
            let Some(capture) = capture else {
                ln += 1;
                continue;
            };
            let annotation = capture.get(1).map(|value| value.as_str());
            let mut assigned = capture.get(2).map(|value| value.as_str().to_owned());
            if annotation.is_none() && assigned.is_none() {
                ln += 1;
                continue;
            }
            mentioned = true;
            if nested_indent.is_some_and(|nested| indent(line) > nested) {
                ln += 1;
                continue;
            }
            let in_def = indent(line) != body && def_start.is_some();
            let at = ln;
            if annotation.is_none()
                && let Some(value) = &assigned
            {
                let (text, end) = joined(lines, ln, value, owner.end_line as usize)?;
                assigned = Some(text);
                ln = end
            }
            ln += 1;
            if annotation.is_none() && in_def {
                let value = trim(assigned.as_deref().unwrap_or_default());
                let mut rebound = false;
                for name in callees(value)? {
                    if self.bound(
                        lines,
                        def_start.unwrap(),
                        at,
                        name.split('.').next().unwrap_or_default(),
                        true,
                    )? {
                        rebound = true;
                        break;
                    }
                }
                if rebound
                    || regex(r"^[A-Za-z_]\w*$")?.is_match(value)?
                        && self.bound(lines, def_start.unwrap(), at, value, false)?
                {
                    continue;
                }
            }
            let ev = if let Some(annotation) = annotation {
                value::annotation(annotation)?
            } else {
                value::value(
                    assigned.as_deref().unwrap_or_default(),
                    if in_def { signature.as_deref() } else { None },
                )?
            };
            if let Some(mut ev) = ev {
                if annotation.is_none()
                    && !in_def
                    && let Evidence::Type { class_level, .. } = &mut ev
                {
                    *class_level = Some(true)
                }
                evidence.push(ev)
            }
        }
        let answer = Rc::new(Attribute {
            mentioned,
            evidence,
        });
        self.statements.borrow_mut().insert(key, answer.clone());
        Ok(answer)
    }
}
