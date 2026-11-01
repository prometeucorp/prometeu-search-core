//! The reference React loader reader: a bounded interpreter for static module values.
// Ported from the pinned MIT-licensed reference; see THIRD_PARTY_NOTICES.md.
use super::{
    object::{matching, skip_string},
    regex,
};
use anyhow::{Result, ensure};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

type Value = Option<Rc<Loaded>>;
type Scope = BTreeMap<String, Value>;
#[derive(Clone, Debug)]
pub(crate) enum Loaded {
    Module { spec: String },
    Export { spec: String, name: String },
    Name { name: String },
    Object { text: String, scope: usize },
    Array { items: Vec<Value> },
}
#[derive(Debug, serde::Serialize)]
pub(crate) struct Function<'a> {
    pub params: &'a str,
    pub body: &'a str,
    pub block: bool,
}
#[derive(Default)]
pub(crate) struct Reader {
    depth: Cell<usize>,
    work: Cell<usize>,
    scopes: RefCell<Vec<Scope>>,
}
struct Guard<'a>(&'a Cell<usize>);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
#[derive(Debug)]
enum Render {
    Unknown,
    Empty,
    Page(String),
}
impl Reader {
    fn enter(&self) -> Result<Guard<'_>> {
        ensure!(
            self.depth.get() < 128,
            "React loader nesting limit exceeded"
        );
        ensure!(
            self.work.get() < 1_000_000,
            "React loader work limit exceeded"
        );
        self.depth.set(self.depth.get() + 1);
        self.work.set(self.work.get() + 1);
        Ok(Guard(&self.depth))
    }
    pub(crate) fn value(&self, text: &str) -> Result<Value> {
        let scope = self.new_scope(Scope::new());
        self.loader_value(text, scope, None)
    }
    pub(crate) fn lazy_reference(
        &self,
        loader: &str,
        element_tag: &impl Fn(&str) -> Result<Option<String>>,
    ) -> Result<Option<String>> {
        let _guard = self.enter()?;
        if loader.is_empty() {
            return Ok(None);
        }
        let code = crate::synthesis::comments::strip_for_regex(loader, "typescript");
        let code = code.trim_matches(crate::search::query_whitespace);
        let rendered = if code.starts_with('{') {
            self.lazy_properties(code)?
        } else {
            self.route_render(self.value(code)?, element_tag)?
        };
        Ok(match rendered {
            Render::Empty => None,
            Render::Page(name) => Some(name),
            Render::Unknown => first_import(code)?.map(|spec| format!("lazy-import:{spec}")),
        })
    }
    fn route_render(
        &self,
        value: Value,
        tag: &impl Fn(&str) -> Result<Option<String>>,
    ) -> Result<Render> {
        let _guard = self.enter()?;
        let Some(value) = value else {
            return Ok(Render::Unknown);
        };
        let (text, scope) = match value.as_ref() {
            Loaded::Module { spec } => return Ok(Render::Page(format!("lazy-import:{spec}"))),
            Loaded::Object { text, scope } => (text, *scope),
            _ => return Ok(Render::Unknown),
        };
        let (props, spreads) = entries(text)?;
        let component = props.get("Component");
        let element = props.get("element");
        if let Some(shown) = component.or(element) {
            let held = if let Some(component) = component {
                self.value_of(component, scope)?
            } else {
                None
            };
            if let Some(page) = page_reference(&held) {
                return Ok(Render::Page(page));
            }
            let loaded = self.loaded_exports(shown, scope)?;
            if loaded.len() == 1 {
                return Ok(page_reference(&loaded[0])
                    .map(Render::Page)
                    .unwrap_or(Render::Unknown));
            }
            let tag = if component.is_none() && loaded.is_empty() {
                tag(element.unwrap())?
            } else {
                None
            };
            if matches!(held.as_deref(),Some(Loaded::Name{name}) if name=="Outlet")
                || tag.as_deref() == Some("Outlet")
            {
                return Ok(Render::Page("Outlet".into()));
            }
            return Ok(tag
                .and_then(|name| page_reference(&self.name_value(&name, scope)))
                .map(Render::Page)
                .unwrap_or(Render::Unknown));
        }
        if spreads.is_empty() {
            return Ok(Render::Empty);
        }
        let spread = if spreads.len() == 1 {
            self.value_of(&spreads[0], scope)?
        } else {
            None
        };
        Ok(match spread.as_deref() {
            Some(Loaded::Module { spec }) => Render::Page(format!("lazy-import:{spec}")),
            _ => Render::Unknown,
        })
    }
    fn lazy_properties(&self, code: &str) -> Result<Render> {
        let _guard = self.enter()?;
        if matching(code, 0)? != code.len().checked_sub(1) {
            return Ok(Render::Unknown);
        }
        let (props, _) = entries(&code[1..code.len() - 1])?;
        let Some(component) = props.get("Component") else {
            return Ok(Render::Empty);
        };
        if let Some(page) = page_reference(&self.value(component)?) {
            return Ok(Render::Page(page));
        }
        Ok(first_import(component)?
            .map(|spec| Render::Page(format!("lazy-import:{spec}")))
            .unwrap_or(Render::Unknown))
    }
    fn loaded_exports(&self, text: &str, scope: usize) -> Result<Vec<Value>> {
        let mut found = BTreeMap::new();
        for capture in regex(r"(?<![\w$.])([A-Za-z_$][\w$]*)(?:\s*\??\.\s*([A-Za-z_$][\w$]*))?")?
            .captures_iter(text)
        {
            let capture = capture?;
            let bound = self.scopes.borrow()[scope]
                .get(&capture[1])
                .cloned()
                .flatten();
            let value = if matches!(bound.as_deref(), Some(Loaded::Module { .. }))
                && capture.get(2).is_some()
            {
                self.member(bound, &capture[2])?
            } else {
                bound
            };
            if let Some(Loaded::Export { spec, name }) = value.as_deref() {
                found.insert(format!("{spec}#{name}"), value);
            }
        }
        Ok(found.into_values().collect())
    }
    fn loader_value(&self, text: &str, outer: usize, param: Value) -> Result<Value> {
        let _guard = self.enter()?;
        let Some(function) = function(text)? else {
            return Ok(None);
        };
        let inherited = self.scopes.borrow()[outer].clone();
        let scope = self.new_scope(inherited);
        for (index, pattern) in items(function.params)?.into_iter().enumerate() {
            self.bind(
                scope,
                parameter(pattern)?,
                if index == 0 { param.clone() } else { None },
            )?;
        }
        if function.block {
            self.block(function.body, scope)
        } else {
            self.value_of(function.body, scope)
        }
    }
    fn block(&self, body: &str, scope: usize) -> Result<Value> {
        let _guard = self.enter()?;
        let mut i = 0;
        while i < body.len() {
            let ch = body.as_bytes()[i];
            if quote(ch) || open(ch) {
                let Some(end) = skip(body, i)? else {
                    return Ok(None);
                };
                i = end + 1;
                continue;
            }
            if !body.is_char_boundary(i) {
                i += 1;
                continue;
            }
            let Some(word) = regex(r"^[A-Za-z_$][\w$]*")?.find(&body[i..])? else {
                i += 1;
                continue;
            };
            let w = word.as_str();
            let after = i + w.len();
            let statement = i == 0 || !id(body.as_bytes()[i - 1]) && body.as_bytes()[i - 1] != b'.';
            i = after;
            if !statement {
                continue;
            }
            if w == "return" {
                return self.value_of(&body[after..statement_end(body, after, false)?], scope);
            }
            if matches!(w, "const" | "let" | "var") {
                i = self.declare(body, after, scope)?;
            } else if matches!(w, "function" | "class")
                && let Some(c) = regex(r"^\s*\*?\s*([A-Za-z_$][\w$]*)")?
                    .captures(super::object::prefix(&body[after..], 128))?
            {
                self.scopes.borrow_mut()[scope].insert(c[1].into(), None);
            }
        }
        Ok(None)
    }
    fn declare(&self, body: &str, at: usize, scope: usize) -> Result<usize> {
        let mut i = at;
        loop {
            i = space(body, i);
            let end = if matches!(body.as_bytes().get(i), Some(b'{' | b'[')) {
                matching(body, i)?.map(|n| n + 1).unwrap_or(0)
            } else {
                i + regex(r"^[A-Za-z_$][\w$]*")?
                    .find(super::object::prefix(&body[i..], 128))?
                    .map(|m| m.as_str().len())
                    .unwrap_or(0)
            };
            if end <= i {
                return Ok(end.max(i + 1));
            }
            let pattern = &body[i..end];
            let init = initializer_at(body, end)?;
            let mut stop = end;
            if let Some(init) = init {
                stop = statement_end(body, init, true)?;
                let value = self.value_of(&body[init..stop], scope)?;
                self.bind(scope, pattern, value)?;
            } else {
                self.bind(scope, pattern, None)?;
            }
            stop = space(body, stop);
            if body.as_bytes().get(stop) != Some(&b',') {
                return Ok(stop);
            }
            i = stop + 1;
        }
    }
    fn value_of(&self, text: &str, scope: usize) -> Result<Value> {
        let _guard = self.enter()?;
        let mut s = text.trim_matches(crate::search::query_whitespace);
        loop {
            let before = s;
            if let Some(m) = regex(r"^await\b\s*")?.find(s)? {
                s = &s[m.end()..];
            }
            if s.starts_with('(') && matching(s, 0)? == s.len().checked_sub(1) {
                s = s[1..s.len() - 1].trim_matches(crate::search::query_whitespace);
            }
            if s == before {
                break;
            }
        }
        if s.starts_with('{') || s.starts_with('[') {
            if matching(s, 0)? != s.len().checked_sub(1) {
                return Ok(None);
            }
            let inside = &s[1..s.len() - 1];
            let value = if s.starts_with('{') {
                Loaded::Object {
                    text: inside.into(),
                    scope,
                }
            } else {
                Loaded::Array {
                    items: items(inside)?
                        .into_iter()
                        .map(|item| {
                            if item.is_empty() {
                                Ok(None)
                            } else {
                                self.value_of(item, scope)
                            }
                        })
                        .collect::<Result<_>>()?,
                }
            };
            return Ok(Some(Rc::new(value)));
        }
        let Some(head) =
            regex(r"^(?:(import)\s*\(|(Promise)\s*\.\s*all\s*\(|([A-Za-z_$][\w$]*)|\()")?
                .captures(s)?
        else {
            return Ok(None);
        };
        let (mut value, mut i) = if let Some(name) = head.get(3) {
            (
                self.name_value(name.as_str(), scope),
                head.get(0).unwrap().end(),
            )
        } else {
            let open = head.get(0).unwrap().end() - 1;
            let Some(close) = matching(s, open)? else {
                return Ok(None);
            };
            let inside = &s[open + 1..close];
            let value = if head.get(1).is_some() {
                regex(r#"^\s*(["'])([^"'`]+)\1\s*$"#)?
                    .captures(inside)?
                    .map(|c| Rc::new(Loaded::Module { spec: c[2].into() }))
            } else if head.get(2).is_some() {
                let all = self.value_of(inside, scope)?;
                if matches!(all.as_deref(), Some(Loaded::Array { .. })) {
                    all
                } else {
                    None
                }
            } else {
                self.value_of(inside, scope)?
            };
            (value, close + 1)
        };
        while value.is_some() && i < s.len() {
            let rest = &s[i..];
            if let Some(then) = regex(r"^\s*\??\.\s*then\s*\(")?.find(rest)? {
                let open = i + then.end() - 1;
                let Some(close) = matching(s, open)? else {
                    return Ok(None);
                };
                if !matches!(value.as_deref(), Some(Loaded::Module { .. })) {
                    return Ok(None);
                }
                value = if let Some(callback) = items(&s[open + 1..close])?.first() {
                    self.loader_value(callback, scope, value)?
                } else {
                    None
                };
                i = close + 1;
                continue;
            }
            if let Some(dot) = regex(r"^\s*\??\.\s*([A-Za-z_$][\w$]*)")?.captures(rest)? {
                value = self.member(value, &dot[1])?;
                i += dot.get(0).unwrap().end();
            } else if let Some(bracket) =
                regex(r#"^\s*\[\s*(["'])([A-Za-z_$][\w$]*)\1\s*\]"#)?.captures(rest)?
            {
                value = self.member(value, &bracket[2])?;
                i += bracket.get(0).unwrap().end();
            } else {
                return Ok(None);
            }
        }
        Ok(value)
    }
    fn member(&self, value: Value, key: &str) -> Result<Value> {
        let _guard = self.enter()?;
        match value.as_deref() {
            Some(Loaded::Module { spec }) => Ok(Some(Rc::new(Loaded::Export {
                spec: spec.clone(),
                name: key.into(),
            }))),
            Some(Loaded::Object { text, scope }) => {
                let (props, _) = entries(text)?;
                if let Some(text) = props.get(key) {
                    self.value_of(text, *scope)
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }
    fn bind(&self, scope: usize, pattern: &str, value: Value) -> Result<()> {
        let _guard = self.enter()?;
        let p = pattern.trim_matches(crate::search::query_whitespace);
        if p.is_empty() {
            return Ok(());
        }
        if regex(r"^[A-Za-z_$][\w$]*$")?.is_match(p)? {
            self.scopes.borrow_mut()[scope].insert(p.into(), value);
            return Ok(());
        }
        let close = if p.starts_with('{') || p.starts_with('[') {
            matching(p, 0)?
        } else {
            None
        };
        if close != p.len().checked_sub(1) {
            for name in regex(r"[A-Za-z_$][\w$]*")?.find_iter(p) {
                self.scopes.borrow_mut()[scope].insert(name?.as_str().into(), None);
            }
            return Ok(());
        }
        for (index, item) in items(&p[1..p.len() - 1])?.into_iter().enumerate() {
            if item.is_empty() {
                continue;
            }
            if let Some(rest) = item.strip_prefix("...") {
                self.bind(
                    scope,
                    rest.trim_start_matches(crate::search::query_whitespace),
                    None,
                )?;
                continue;
            }
            if p.starts_with('[') {
                let name = regex(r"\s*=(?![=>])[\s\S]*$")?.replace(item, "");
                let part = if let Some(Loaded::Array { items }) = value.as_deref() {
                    items.get(index).cloned().flatten()
                } else {
                    None
                };
                self.bind(scope, &name, part)?;
                continue;
            }
            if let Some(prop)=regex(r#"^(?:([A-Za-z_$][\w$]*)|(["'])([^"']*)\2)\s*(?::\s*([\s\S]+?))?\s*(?:=(?![=>])[\s\S]*)?$"#)?.captures(item)?{
                let key=prop.get(1).or_else(||prop.get(3)).unwrap().as_str();self.bind(scope,prop.get(4).map(|m|m.as_str()).unwrap_or(key),self.member(value.clone(),key)?)?;
            }else{self.bind(scope,&regex(r"^[^:]*:")?.replace(item,""),None)?;}
        }
        Ok(())
    }
    pub(crate) fn loaded_page(&self, value: Value) -> Result<Option<(String, String)>> {
        let _guard = self.enter()?;
        match value.as_deref() {
            Some(Loaded::Module { spec }) => Ok(Some((spec.clone(), "default".into()))),
            Some(Loaded::Export { spec, name }) => Ok(Some((spec.clone(), name.clone()))),
            Some(Loaded::Object { text, scope }) => {
                let (props, _) = entries(text)?;
                if let Some(inner) = props.get("default") {
                    self.loaded_page(self.value_of(inner, *scope)?)
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }
}
impl Reader {
    fn new_scope(&self, scope: Scope) -> usize {
        let mut scopes = self.scopes.borrow_mut();
        let id = scopes.len();
        scopes.push(scope);
        id
    }
    fn name_value(&self, name: &str, scope: usize) -> Value {
        self.scopes.borrow()[scope]
            .get(name)
            .cloned()
            .unwrap_or_else(|| Some(Rc::new(Loaded::Name { name: name.into() })))
    }
}
fn page_reference(value: &Value) -> Option<String> {
    match value.as_deref() {
        Some(Loaded::Export { spec, name }) => Some(format!("import:{spec}#{name}")),
        Some(Loaded::Name { name })
            if name.as_bytes().first().is_some_and(u8::is_ascii_uppercase) && name != "Outlet" =>
        {
            Some(name.clone())
        }
        _ => None,
    }
}
fn first_import(text: &str) -> Result<Option<String>> {
    Ok(regex(r#"\bimport\s*\(\s*["']([^"']+)["']\s*\)"#)?
        .captures(text)?
        .map(|m| m[1].into()))
}
fn quote(ch: u8) -> bool {
    matches!(ch, b'\'' | b'"' | b'`')
}
fn open(ch: u8) -> bool {
    matches!(ch, b'{' | b'(' | b'[')
}
fn id(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$')
}
fn skip(text: &str, at: usize) -> Result<Option<usize>> {
    if quote(text.as_bytes()[at]) {
        skip_string(text, at)
    } else {
        matching(text, at)
    }
}
fn space(text: &str, at: usize) -> usize {
    at + text
        .get(at..)
        .unwrap_or("")
        .chars()
        .take_while(|ch| crate::search::query_whitespace(*ch))
        .map(char::len_utf8)
        .sum::<usize>()
}
pub(crate) fn items(text: &str) -> Result<Vec<&str>> {
    let mut out = Vec::new();
    let mut depth = 0i64;
    let mut from = 0;
    let mut i = 0;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if quote(ch) {
            let Some(end) = skip_string(text, i)? else {
                break;
            };
            i = end;
        } else if open(ch) {
            depth += 1;
        } else if matches!(ch, b'}' | b']' | b')') {
            depth -= 1;
        } else if ch == b',' && depth == 0 {
            out.push(text[from..i].trim_matches(crate::search::query_whitespace));
            from = i + 1;
        }
        i += 1;
    }
    let last = text[from..].trim_matches(crate::search::query_whitespace);
    if !last.is_empty() {
        out.push(last);
    }
    Ok(out)
}
pub(crate) fn function(text: &str) -> Result<Option<Function<'_>>> {
    let mut s = text.trim_matches(crate::search::query_whitespace);
    if let Some(m) = regex(r"^async\b\s*")?.find(s)? {
        s = &s[m.end()..];
    }
    let keyword = regex(r"^function\b\s*\*?\s*(?:[A-Za-z_$][\w$]*\s*)?")?.find(s)?;
    if let Some(m) = keyword {
        s = &s[m.end()..];
    }
    let (params, mut rest) = if s.starts_with('(') {
        let Some(close) = matching(s, 0)? else {
            return Ok(None);
        };
        (
            &s[1..close],
            s[close + 1..].trim_start_matches(crate::search::query_whitespace),
        )
    } else {
        if keyword.is_some() {
            return Ok(None);
        }
        let Some(id) = regex(r"^[A-Za-z_$][\w$]*")?.find(s)? else {
            return Ok(None);
        };
        (
            &s[..id.end()],
            s[id.end()..].trim_start_matches(crate::search::query_whitespace),
        )
    };
    if keyword.is_none() && rest.starts_with(':') {
        let Some(at) = arrow(rest)? else {
            return Ok(None);
        };
        rest = &rest[at..];
    }
    if keyword.is_some() {
        return Ok(
            if rest.starts_with('{') && matching(rest, 0)? == rest.len().checked_sub(1) {
                Some(Function {
                    params,
                    body: &rest[1..rest.len() - 1],
                    block: true,
                })
            } else {
                None
            },
        );
    }
    let Some(body) = rest.strip_prefix("=>") else {
        return Ok(None);
    };
    let body = body.trim_matches(crate::search::query_whitespace);
    Ok(if !body.starts_with('{') {
        Some(Function {
            params,
            body,
            block: false,
        })
    } else if matching(body, 0)? == body.len().checked_sub(1) {
        Some(Function {
            params,
            body: &body[1..body.len() - 1],
            block: true,
        })
    } else {
        None
    })
}
fn arrow(text: &str) -> Result<Option<usize>> {
    let mut i = 0;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if quote(ch) || open(ch) {
            let Some(end) = skip(text, i)? else {
                return Ok(None);
            };
            i = end;
        } else if ch == b'=' && text.as_bytes().get(i + 1) == Some(&b'>') {
            return Ok(Some(i));
        }
        i += 1;
    }
    Ok(None)
}
fn parameter(param: &str) -> Result<&str> {
    let param = param
        .strip_prefix("...")
        .map(|s| s.trim_start_matches(crate::search::query_whitespace))
        .unwrap_or(param);
    if param.starts_with('{') || param.starts_with('[') {
        return Ok(matching(param, 0)?
            .map(|end| &param[..end + 1])
            .unwrap_or(""));
    }
    Ok(regex(r"^[A-Za-z_$][\w$]*")?
        .find(param)?
        .map(|m| m.as_str())
        .unwrap_or(""))
}
pub(crate) fn initializer_at(body: &str, at: usize) -> Result<Option<usize>> {
    let mut i = at;
    while i < body.len() {
        let ch = body.as_bytes()[i];
        if quote(ch) || open(ch) {
            let Some(end) = skip(body, i)? else {
                return Ok(None);
            };
            i = end;
        } else if ch == b'=' {
            if matches!(body.as_bytes().get(i + 1), Some(b'>' | b'=')) {
                i += 1;
            } else {
                return Ok(Some(i + 1));
            }
        } else if matches!(ch, b';' | b',' | b'}' | b')') {
            return Ok(None);
        }
        i += 1;
    }
    Ok(None)
}
pub(crate) fn statement_end(text: &str, from: usize, comma: bool) -> Result<usize> {
    let mut last = 0;
    let mut i = from;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if quote(ch) || open(ch) {
            let Some(end) = skip(text, i)? else {
                return Ok(text.len());
            };
            i = end;
            last = text.as_bytes()[end];
            i += 1;
            continue;
        }
        if matches!(ch, b';' | b'}' | b')' | b']') || comma && ch == b',' {
            return Ok(i);
        }
        if ch == b'\n' {
            let j = space(text, i + 1);
            if last != 0
                && !b"=(,[{?:&|+-*/%<>!.".contains(&last)
                && !text
                    .as_bytes()
                    .get(j)
                    .is_some_and(|ch| b".?:&|+-*/%=<>,([".contains(ch))
            {
                return Ok(i);
            }
            i += 1;
            continue;
        }
        if text.is_char_boundary(i) {
            let c = text[i..].chars().next().unwrap();
            if !crate::search::query_whitespace(c) {
                last = ch;
            }
            i += c.len_utf8();
        } else {
            i += 1;
        }
    }
    Ok(text.len())
}
pub(crate) fn entries(text: &str) -> Result<(BTreeMap<String, String>, Vec<String>)> {
    let mut props = BTreeMap::new();
    let mut spreads = Vec::new();
    for entry in items(text)?.into_iter().filter(|entry| !entry.is_empty()) {
        if let Some(spread) = regex(r"^\.\.\.\s*([\s\S]+)$")?.captures(entry)? {
            spreads.push(spread[1].into());
        } else if let Some(pair) =
            regex(r#"^(?:([A-Za-z_$][\w$]*)|(["'])([A-Za-z_$][\w$]*)\2)\s*:\s*([\s\S]+)$"#)?
                .captures(entry)?
        {
            props.insert(
                pair.get(1).or_else(|| pair.get(3)).unwrap().as_str().into(),
                pair[4].into(),
            );
        } else if let Some(method) =
            regex(r"^(async\s+)?([A-Za-z_$][\w$]*)\s*\(")?.captures(entry)?
        {
            props.insert(
                method[2].into(),
                format!(
                    "{}function {}",
                    method.get(1).map(|m| m.as_str()).unwrap_or(""),
                    &entry[method.get(0).unwrap().end() - 1..]
                ),
            );
        } else if regex(r"^[A-Za-z_$][\w$]*$")?.is_match(entry)? {
            props.insert(entry.into(), entry.into());
        }
    }
    Ok((props, spreads))
}
impl Reader {
    pub(crate) fn loader_of<'a>(&self, text: &'a str) -> Result<Option<std::borrow::Cow<'a, str>>> {
        let _guard = self.enter()?;
        let s = text.trim_matches(crate::search::query_whitespace);
        let call = regex(r"^[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*\s*(?:<[^()]*>)?\s*\(")?
            .find(s)?;
        if let Some(call) = call {
            let open = call.end() - 1;
            let Some(close) = matching(s, open)? else {
                return Ok(None);
            };
            for item in items(&s[open + 1..close])? {
                if item.is_empty() {
                    continue;
                }
                if function(item)?.is_some() {
                    return Ok(Some(item.into()));
                }
                if item.starts_with('{') {
                    if matching(item, 0)? == item.len().checked_sub(1) {
                        let (props, _) = entries(&item[1..item.len() - 1])?;
                        if let Some(loader) = props.get("loader")
                            && function(loader)?.is_some()
                        {
                            return Ok(Some(loader.clone().into()));
                        }
                    }
                    continue;
                }
                if let Some(inner) = self.loader_of(item)? {
                    return Ok(Some(inner));
                }
            }
        }
        let Some((yes, no)) = conditional(s)? else {
            return Ok(None);
        };
        let a = self.loader_of(yes)?;
        let b = self.loader_of(no)?;
        Ok(match (a, b) {
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            _ => None,
        })
    }
    pub(crate) fn wrapped_name(&self, expression: &str) -> Result<Option<String>> {
        let _guard = self.enter()?;
        let s = expression.trim_matches(crate::search::query_whitespace);
        let Some(callee) =
            regex(r"^[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*\s*(?:<[^()]*>)?\s*(?=\()")?
                .find(s)?
        else {
            return Ok(None);
        };
        let mut end = callee.end();
        let mut open = end;
        let mut args = Vec::new();
        while s.as_bytes().get(open) == Some(&b'(') {
            let Some(close) = matching(s, open)? else {
                return Ok(None);
            };
            args = items(&s[open + 1..close])?;
            end = close + 1;
            open = space(s, end);
        }
        if !regex(r"^(?:\s+(?:as|satisfies)\s[\s\S]*)?$")?.is_match(&s[end..])? {
            return Ok(None);
        }
        let first = args.first().copied().unwrap_or("");
        if args.len() > 1 && !first.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
            return Ok(None);
        }
        if regex(r"^[A-Za-z_$][\w$]*$")?.is_match(first)? {
            Ok(Some(first.into()))
        } else if first.is_empty() {
            Ok(None)
        } else {
            self.wrapped_name(first)
        }
    }
}
fn conditional(text: &str) -> Result<Option<(&str, &str)>> {
    let mut question = 0;
    let mut opened = 0;
    let mut i = 0;
    while i < text.len() {
        let ch = text.as_bytes()[i];
        if quote(ch) || open(ch) {
            let Some(end) = skip(text, i)? else {
                return Ok(None);
            };
            i = end;
        } else if ch == b'?' {
            if text.as_bytes().get(i + 1) == Some(&b'?')
                || text.as_bytes().get(i + 1) == Some(&b'.')
                    && !text.as_bytes().get(i + 2).is_some_and(u8::is_ascii_digit)
            {
                i += 1;
            } else {
                if opened == 0 {
                    question = i;
                }
                opened += 1;
            }
        } else if ch == b':' && opened > 0 {
            opened -= 1;
            if opened == 0 {
                return Ok(Some((&text[question + 1..i], &text[i + 1..])));
            }
        }
        i += 1;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value as Json, json};
    fn snapshot(value: &Value) -> Json {
        match value.as_deref() {
            None => Json::Null,
            Some(Loaded::Module { spec }) => json!({"kind":"module","spec":spec}),
            Some(Loaded::Export { spec, name }) => json!({"kind":"export","spec":spec,"name":name}),
            Some(Loaded::Name { name }) => json!({"kind":"name","name":name}),
            Some(Loaded::Object { text, .. }) => json!({"kind":"object","text":text}),
            Some(Loaded::Array { items }) => {
                json!({"kind":"array","items":items.iter().map(snapshot).collect::<Vec<_>>()})
            }
        }
    }
    #[test]
    fn loaders_match_independent_reference() {
        let corpus: Json = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/frontend/react-loader.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let reader = Reader::default();
            let value = reader.value(source).unwrap();
            let page = reader
                .loaded_page(value.clone())
                .unwrap()
                .map(|(spec, member)| json!({"spec":spec,"member":member}));
            let (props, spreads) = entries(source).unwrap();
            assert_eq!(
                json!({"source":source,"function":function(source).unwrap(),"value":snapshot(&value),"page":page,"loader":reader.loader_of(source).unwrap(),"wrapped":reader.wrapped_name(source).unwrap(),"items":items(source).unwrap(),"entries":{"props":props,"spreads":spreads},"initializer":initializer_at(source,0).unwrap().map(|v|source[..v].encode_utf16().count() as i64).unwrap_or(-1),"end":source[..statement_end(source,0,false).unwrap()].encode_utf16().count(),"commaEnd":source[..statement_end(source,0,true).unwrap()].encode_utf16().count()}),
                *case,
                "{source}"
            );
        }
        for case in corpus["lazy"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let result = Reader::default()
                .lazy_reference(source, &|text| {
                    Ok(case["tags"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|tag| tag["source"] == text)
                        .and_then(|tag| tag["expected"].as_str().map(str::to_owned)))
                })
                .unwrap();
            assert_eq!(json!(result), case["expected"], "{source}");
        }
    }
}
