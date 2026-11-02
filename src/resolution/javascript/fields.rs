use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Field {
    pub value_type: bool,
    #[serde(rename = "type")]
    pub type_name: String,
    pub array: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
}
#[derive(Clone)]
struct Wrapper {
    holder: Option<String>,
    method: String,
}
struct OnLine {
    field: Field,
    via: Option<Wrapper>,
    injector: Option<&'static str>,
    open: Option<usize>,
}
fn joined(tail: &str) -> Result<bool> {
    let rest = regex(r"^(?:\s*\|\s*(?:null|undefined)\b)+")?.replace(tail, "");
    test(r"^\s*[|&]", &rest)
}
fn element(kind: &str, rest: &str) -> Result<Option<String>> {
    if let Some(array) = regex(r"^\s*\[\s*\]")?.find(rest)? {
        let tail = &rest[array.end()..];
        return Ok((!test(r"^\s*\[", tail)? && !joined(tail)?).then(|| kind.to_string()));
    }
    let (args, index) = match kind {
        "Array" | "ReadonlyArray" => (1, 0),
        "Record" => (2, 1),
        _ => return Ok(None),
    };
    let Some(open) = regex(r"^\s*<")?.find(rest)? else {
        return Ok(None);
    };
    let mut depth = 0i32;
    let mut from = open.end();
    let mut parts = Vec::new();
    let start = from;
    for (i, ch) in rest.char_indices().filter(|(i, _)| *i >= start) {
        match ch {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                parts.push(trim(&rest[from..i]));
                from = i + 1;
            }
            '>' if depth == 0 => {
                parts.push(trim(&rest[from..i]));
                if test(r"^\s*\[", &rest[i + 1..])? || joined(&rest[i + 1..])? {
                    return Ok(None);
                }
                return Ok(
                    (parts.len() == args && test(r"^[A-Za-z_$][\w.$]*$", parts[index])?)
                        .then(|| parts[index].to_string()),
                );
            }
            _ => {}
        }
    }
    Ok(None)
}
fn on_line(line: &str, field: &str) -> Result<Option<OnLine>> {
    let prefix = format!(r"(?<![\w$#]){}\b", regex::escape(field));
    let patterns = [
        r"\s*[?!]?\s*:\s*(?:readonly\s+)?typeof\s+([A-Za-z_$][\w.$]*)",
        r"\s*[?!]?\s*:\s*(?:readonly\s+)?([A-Za-z_$][\w.$]*)",
        r"\s*=\s*new\s+([A-Za-z_$][\w.$]*)",
        r"\s*=\s*this\.(?:([\w$#]+)\.)?([\w$#]+)\(\s*(?:new\s+|((?:this\.)?(?:[\w$#]+\.)*createInstance)\s*\(\s*)([A-Za-z_$][\w.$]*)",
        r"\s*=\s*(inject|(?:this\.)?(?:[\w$#]+\.)*createInstance)\s*(?:<\s*([A-Z][\w.$]*)[^;=(){}]*>)?\s*\(\s*([A-Z][\w.$]*)",
    ];
    for (index, pattern) in patterns.iter().enumerate() {
        let expression = regex(&(prefix.clone() + pattern))?;
        let Some(captures) = expression.captures(line)? else {
            continue;
        };
        let matched = captures.get(0).unwrap();
        let kind = match index {
            3 => captures.get(4),
            4 => captures.get(2).or_else(|| captures.get(3)),
            _ => captures.get(1),
        };
        let Some(kind) = kind
            .map(|matched| matched.as_str())
            .filter(|kind| !kind.is_empty())
        else {
            continue;
        };
        let rest = &line[matched.end()..];
        let declaration = Field {
            value_type: index == 0,
            type_name: kind.to_string(),
            array: test(
                r"^\s*(?:<[^;=(){}<>]*(?:<[^;=(){}<>]*>[^;=(){}<>]*)*>)?\s*\[\s*\]",
                rest,
            )?,
            element: if index == 1 || index == 2 {
                element(kind, rest)?
            } else {
                None
            },
        };
        let via = (index == 3).then(|| Wrapper {
            holder: captures.get(1).map(|m| m.as_str().to_string()),
            method: captures.get(2).unwrap().as_str().to_string(),
        });
        let injector = if index == 3 && captures.get(3).is_some() {
            Some("createInstance")
        } else if index == 4 {
            Some(if captures.get(1).unwrap().as_str() == "inject" {
                "inject"
            } else {
                "createInstance"
            })
        } else {
            None
        };
        let open = if index >= 3 {
            matched.as_str().find('(').map(|at| matched.start() + at)
        } else {
            None
        };
        return Ok(Some(OnLine {
            field: declaration,
            via,
            injector,
            open,
        }));
    }
    Ok(None)
}

fn ends_statement(lines: &[String], row: usize, open: usize) -> Result<bool> {
    let mut depth = 0i32;
    for (r, line) in lines.iter().enumerate().skip(row).take(12) {
        let mut at = if r == row { open } else { 0 };
        let bytes = line.as_bytes();
        while at < bytes.len() {
            match bytes[at] {
                b'"' | b'\'' | b'`' => {
                    let quote = bytes[at];
                    let mut close = at + 1;
                    while close < bytes.len() && bytes[close] != quote {
                        close += if bytes[close] == b'\\' { 2 } else { 1 };
                    }
                    if close >= bytes.len() {
                        return Ok(false);
                    }
                    at = close;
                }
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        let rest = trim(&line[at + 1..]);
                        if !rest.is_empty() {
                            return Ok(rest.starts_with(';'));
                        }
                        let next = lines
                            .iter()
                            .skip(r + 1)
                            .take(3)
                            .find(|line| !trim(line).is_empty());
                        return Ok(match next {
                            Some(next) => {
                                !test(r"^\s*(?:[.?\[(!,:|&+*/%<>=-]|as\b|satisfies\b)", next)?
                            }
                            None => true,
                        });
                    }
                }
                _ => {}
            }
            at += 1;
        }
    }
    Ok(false)
}
impl<'a> Context<'a> {
    pub(super) fn field(
        &self,
        class: &'a Node,
        name: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<Field>> {
        let key = (class.id.clone(), name.to_string());
        if let Some(cached) = self.fields.borrow().get(&key) {
            return Ok(cached.clone());
        }
        self.fields.borrow_mut().insert(key.clone(), None);
        self.steps(self.source(class).len() / 64 + 1)?;
        let mut lines = Vec::new();
        for line in self
            .source(class)
            .split('\n')
            .skip(class.start_line.saturating_sub(1) as usize)
            .take(
                class
                    .end_line
                    .saturating_sub(class.start_line.saturating_sub(1)) as usize,
            )
        {
            let line = regex(r"//.*$")?.replace(line, "").to_string();
            lines.push(regex(r"/\*.*?\*/")?.replace_all(&line, "").to_string());
        }
        let mut found = None;
        for (row, line) in lines.iter().enumerate() {
            self.step()?;
            if !line.contains(name) {
                continue;
            }
            let Some(mut declaration) = on_line(line, name)? else {
                continue;
            };
            if declaration.field.element.is_some() {
                let below = lines
                    .iter()
                    .skip(row + 1)
                    .take_while(|line| line.trim_start().starts_with(['|', '&']))
                    .map(|line| format!(" {line}"))
                    .collect::<String>();
                if joined(&below)? {
                    declaration.field.element = None;
                }
            }
            if (declaration.via.is_some() || declaration.injector.is_some())
                && (!ends_statement(&lines, row, declaration.open.unwrap())?
                    || if let Some(via) = &declaration.via {
                        !self.wrapper_returns(class, via, at)?
                    } else {
                        false
                    }
                    || if let Some(injector) = declaration.injector {
                        !self.injects(class, injector)?
                    } else {
                        false
                    })
            {
                continue;
            }
            found = Some(declaration.field);
            break;
        }
        self.fields.borrow_mut().insert(key, found.clone());
        Ok(found)
    }
    fn injects(&self, class: &Node, injector: &str) -> Result<bool> {
        if injector == "inject" {
            return Ok(self
                .imported(&class.file_path, "inject")
                .is_some_and(|mapping| mapping.source == "@angular/core"));
        }
        for node in self.named("createInstance").filter(|node| {
            matches!(node.kind.as_str(), "method" | "function")
                && family(&node.language, "typescript")
        }) {
            let head = self
                .lines(&node.file_path)?
                .skip(node.start_line.saturating_sub(1) as usize)
                .take(3)
                .collect::<Vec<_>>()
                .join(" ");
            if test(r"\bInstanceType\s*<", &head)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn wrapper_returns(
        &self,
        class: &'a Node,
        via: &Wrapper,
        at: &UnresolvedReference,
    ) -> Result<bool> {
        let owner = if let Some(holder) = &via.holder {
            match self.field_through_bases(class, holder, at, 0)? {
                Some((declaration, owner)) if !declaration.value_type && !declaration.array => {
                    self.named_class(&declaration.type_name, owner, at)?
                }
                _ => None,
            }
        } else {
            Some(class)
        };
        let Some(owner) = owner else {
            return Ok(false);
        };
        let methods = self.class_methods(owner, &via.method, at, 0)?;
        if methods.is_empty() {
            return Ok(false);
        }
        for method in methods {
            if !self.returns_argument(method)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(super) fn returns_argument(&self, method: &Node) -> Result<bool> {
        let Some(signature) =
            regex(r"^\(\s*[\w$]+\s*:\s*([A-Za-z_$][\w$]*)\s*\)\s*:\s*([A-Za-z_$][\w$]*)$")?
                .captures(trim(method.signature.as_deref().unwrap_or("")))?
        else {
            return Ok(false);
        };
        if signature[1] != signature[2] {
            return Ok(false);
        }
        let head = self
            .lines(&method.file_path)?
            .skip(method.start_line.saturating_sub(1) as usize)
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        test(
            &format!(
                r"(?<![\w$#]){}\s*<\s*(?:const\s+)?{}\b",
                regex::escape(&method.name),
                regex::escape(&signature[1])
            ),
            &head,
        )
    }
    fn class_methods(
        &self,
        class: &'a Node,
        name: &str,
        at: &UnresolvedReference,
        depth: usize,
    ) -> Result<Vec<&'a Node>> {
        self.step()?;
        let own: Vec<_> = self
            .in_file(&class.file_path)
            .filter(|node| {
                node.kind == "method"
                    && node.name == name
                    && node.start_line >= class.start_line
                    && node.end_line <= class.end_line
                    && node
                        .qualified_name
                        .rsplit_once("::")
                        .map(|(owner, _)| owner.rsplit([':', '.']).next().unwrap_or(owner))
                        == Some(class.name.as_str())
            })
            .collect();
        if !own.is_empty() || depth >= 10 {
            return Ok(own);
        }
        for super_name in self.declared_supers(class)? {
            if let Some(base) = self.named_class(&super_name, class, at)?
                && base.id != class.id
            {
                let methods = self.class_methods(base, name, at, depth + 1)?;
                if !methods.is_empty() {
                    return Ok(methods);
                }
            }
        }
        Ok(vec![])
    }
}
