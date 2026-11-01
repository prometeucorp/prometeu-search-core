//! React Router's structural route scanner, retaining original source byte offsets.
use super::{
    object::{matching, prefix, skip_string},
    react_loader::{Reader, items},
    regex,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
}
impl Part {
    fn literal(lit: String) -> Self {
        Self {
            lit: Some(lit),
            expr: None,
        }
    }
    fn shown(&self) -> String {
        self.lit
            .clone()
            .unwrap_or_else(|| format!("{{{}}}", self.expr.as_deref().unwrap_or("undefined")))
    }
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Route {
    pub path: String,
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lazy: Option<String>,
    pub layouts: Vec<String>,
    pub at: usize,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Name {
    pub name: String,
    pub call: bool,
    pub at: usize,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct MountedName {
    #[serde(flatten)]
    pub name: Name,
    pub prefix: Vec<Part>,
    pub layouts: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Use {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Name>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub literal: Option<usize>,
    pub prefix: Vec<Part>,
    pub layouts: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Scan {
    pub routes: Vec<Route>,
    pub names: Vec<MountedName>,
    pub uses: Vec<Use>,
}
#[derive(Default)]
pub(crate) struct Options {
    pub objects: bool,
    pub table: Option<(usize, Vec<Part>, Vec<String>)>,
    pub uses: bool,
}
#[derive(Clone)]
struct Scope {
    part: Part,
    at: usize,
    start: usize,
    end: usize,
    component: Option<String>,
    lazy: Option<String>,
    jsx: bool,
    outlet: bool,
    pathless: bool,
    index: bool,
    opaque: bool,
}
impl Scope {
    fn renders(&self) -> bool {
        self.component.is_some() || self.lazy.is_some()
    }
    fn layout(&self) -> Option<String> {
        self.component.clone().or_else(|| self.lazy.clone())
    }
    fn holds(&self, start: usize, end: usize) -> bool {
        self.start < start && self.end >= end
    }
}
struct Tag {
    at: usize,
    spreads: Vec<String>,
    path: Option<String>,
}
struct Site {
    name: Option<Name>,
    literal: Option<usize>,
    at: usize,
}
struct Scanner<'a> {
    source: &'a str,
    allow_jsx: bool,
    options: Options,
    scopes: Vec<Scope>,
    names: Vec<Name>,
    sites: Vec<Site>,
    tags: Vec<Tag>,
    routers: Vec<(usize, usize)>,
    depth: usize,
    work: usize,
}
pub(crate) fn declarations(source: &str, allow_jsx: bool) -> Result<Vec<Route>> {
    let objects=regex(r"\b(?:createBrowserRouter|createHashRouter|createMemoryRouter|createRoutesFromElements|useRoutes)\b")?.is_match(source)?;
    if !objects && !regex(r"<Route\b")?.is_match(source)? {
        return Ok(Vec::new());
    }
    Ok(scan(
        source,
        allow_jsx,
        Options {
            objects,
            ..Options::default()
        },
    )?
    .routes)
}
pub(crate) fn scan(source: &str, allow_jsx: bool, options: Options) -> Result<Scan> {
    let mut scanner = Scanner {
        source,
        allow_jsx,
        options,
        scopes: Vec::new(),
        names: Vec::new(),
        sites: Vec::new(),
        tags: Vec::new(),
        routers: Vec::new(),
        depth: 0,
        work: 0,
    };
    if let Some((at, _, _)) = scanner.options.table {
        scanner.unit(scanner.trivia(at), true)?;
    } else {
        let mut at = 0;
        loop {
            at = scanner.trivia(at);
            if at >= source.len() {
                break;
            }
            at = scanner.unit(at, false)?;
        }
    }
    scanner.finish()
}
fn space(c: char) -> bool {
    crate::search::query_whitespace(c)
}
fn trim(s: &str) -> &str {
    s.trim_matches(space)
}
fn literal(value: &str) -> Result<Option<String>> {
    Ok(regex(r#"^(?:"((?:\\.|[^"\\])*)"|'((?:\\.|[^'\\])*)')$"#)?
        .captures(trim(value))?
        .map(|m| m.get(1).or_else(|| m.get(2)).unwrap().as_str().into()))
}
pub(crate) fn compose(parts: impl IntoIterator<Item = String>) -> String {
    let mut path = String::new();
    for part in parts {
        if part.starts_with('/') {
            path = part;
        } else if !part.is_empty() {
            path = format!("{}/{part}", path.trim_end_matches('/'));
        }
    }
    let mut result = String::from("/");
    let mut slash = true;
    for c in path.trim_start_matches('/').chars() {
        if c != '/' || !slash {
            result.push(c);
        }
        slash = c == '/';
    }
    if result.len() > 1 && result.ends_with('/') {
        result.pop();
    }
    result
}
pub(crate) fn element_tag(value: &str) -> Result<Option<String>> {
    component(value, true)
}
fn component(value: &str, jsx: bool) -> Result<Option<String>> {
    if value.is_empty() {
        return Ok(None);
    }
    if !jsx {
        return Ok(regex(r"^\s*([A-Z][\w]*)\s*$")?
            .captures(value)?
            .map(|m| m[1].into()));
    }
    let inner = regex(r"^\s*(?:\(\s*)?")?.replace(value, "");
    if !regex(r"^<\s*[A-Z]")?.is_match(&inner)? {
        return Ok(None);
    }
    let (own, props) = element_tags(&inner)?;
    let wrapper = regex(
        r"^(?:Suspense|ErrorBoundary|\w*(?:Guard|Provider|Wrapper|Route)|(?:Protected|Private|Auth|Require\w*)\w*)$",
    )?;
    for name in own.iter().chain(props.iter()) {
        if name != "Outlet" && !wrapper.is_match(name)? {
            return Ok(Some(name.clone()));
        }
    }
    Ok(own
        .iter()
        .rev()
        .find(|name| name.as_str() != "Outlet")
        .or_else(|| own.first())
        .cloned())
}
fn element_tags(text: &str) -> Result<(Vec<String>, Vec<String>)> {
    let mut own = Vec::new();
    let mut props = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text.as_bytes()[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(open) = regex(r"^<\s*([A-Za-z][\w.:-]*)(?=[\s/>])")?.captures(&text[i..])? else {
            i += 1;
            continue;
        };
        if regex(r"^[A-Z]\w*$")?.is_match(&open[1])? {
            own.push(open[1].into());
        }
        let mut j = i + open.get(0).unwrap().end();
        let mut attr = String::new();
        while j < text.len() && text.as_bytes()[j] != b'>' {
            if !text.is_char_boundary(j) {
                j += 1;
                continue;
            }
            let ch = text.as_bytes()[j];
            if let Some(name) = regex(r"^[A-Za-z_$][\w$:-]*")?.find(prefix(&text[j..], 64))? {
                attr = name.as_str().into();
                j += name.end();
                continue;
            }
            let end = if ch == b'{' {
                matching(text, j)?
            } else if matches!(ch, b'\'' | b'"') {
                skip_string(text, j)?
            } else {
                Some(j)
            };
            let Some(end) = end else { break };
            if ch == b'{' && regex(r"(?i)^(?:component|element|page)$")?.is_match(&attr)? {
                for found in regex(r"<\s*([A-Z]\w*)(?=[\s/>])")?.captures_iter(&text[j..end]) {
                    props.push(found?[1].into());
                }
            }
            if matches!(ch, b'{' | b'\'' | b'"') {
                attr.clear();
            }
            j = end + 1;
        }
        i = j.saturating_add(1);
    }
    Ok((own, props))
}
impl Scanner<'_> {
    fn ch(&self, at: usize) -> u8 {
        self.source.as_bytes().get(at).copied().unwrap_or(0)
    }
    fn trivia(&self, mut at: usize) -> usize {
        while at < self.source.len() {
            let tail = &self.source[at..];
            let c = tail.chars().next().unwrap();
            if space(c) {
                at += c.len_utf8();
            } else if tail.starts_with("//") {
                at = tail.find('\n').map_or(self.source.len(), |end| at + end);
            } else if let Some(comment) = tail.strip_prefix("/*") {
                at = comment
                    .find("*/")
                    .map_or(self.source.len(), |end| at + end + 4);
            } else {
                break;
            }
        }
        at
    }
    fn before(&self, at: usize, units: usize) -> &str {
        let end = self.source[..at].trim_end_matches(space).len();
        let mut used = 0;
        let start = self.source[..end]
            .char_indices()
            .rev()
            .find_map(|(i, c)| {
                used += c.len_utf16();
                (used >= units).then_some(i)
            })
            .unwrap_or(0);
        &self.source[start..end]
    }
    fn tail(&self, at: usize, units: usize) -> &str {
        let mut count = 0;
        let from = self.source[..at]
            .char_indices()
            .rev()
            .find_map(|(i, ch)| {
                count += ch.len_utf16();
                (count >= units).then_some(i)
            })
            .unwrap_or(0);
        &self.source[from..at]
    }
    fn unit(&mut self, at: usize, route_list: bool) -> Result<usize> {
        ensure!(self.depth < 128, "React route nesting limit exceeded");
        self.work += 1;
        ensure!(self.work < 1_000_000, "React route work limit exceeded");
        self.depth += 1;
        let result = self.unit_inner(at, route_list);
        self.depth -= 1;
        result
    }
    fn unit_inner(&mut self, at: usize, route_list: bool) -> Result<usize> {
        let ch = self.ch(at);
        let len = self.source.len();
        if matches!(ch, b'\'' | b'"' | b'`') {
            let mut i = at + 1;
            while i < len {
                if self.ch(i) == b'\\' {
                    i = (i + 2).min(len);
                    while !self.source.is_char_boundary(i) {
                        i += 1;
                    }
                    continue;
                }
                if self.ch(i) == ch {
                    return Ok(i + 1);
                }
                if ch == b'`' && self.source[i..].starts_with("${") {
                    i = self.unit(i + 1, false)?;
                    continue;
                }
                i += self.source[i..].chars().next().unwrap().len_utf8();
            }
            return Ok(i);
        }
        if ch == b'/' {
            let before = self.before(at, 12);
            if before.is_empty()
                || regex(r"[=(:,[!&|?{};]$")?.is_match(before)?
                || regex(r"\b(?:return|throw|case|yield)\s*$")?.is_match(before)?
            {
                let mut class = false;
                let mut i = at + 1;
                while i < len && self.ch(i) != b'\n' {
                    if self.ch(i) == b'\\' {
                        i += 2;
                        continue;
                    }
                    if self.ch(i) == b'[' {
                        class = true;
                    } else if self.ch(i) == b']' {
                        class = false;
                    } else if self.ch(i) == b'/' && !class {
                        i += 1;
                        while self.ch(i).is_ascii_alphabetic() {
                            i += 1;
                        }
                        return Ok(i);
                    }
                    i += 1;
                }
            }
        }
        if self.allow_jsx
            && ch == b'<'
            && regex(r"^<(?:[A-Za-z][\w.:-]*|>)")?.is_match(&self.source[at..])?
        {
            let before = self.before(at, 12);
            if !regex(r#"[\w$)\]'"]$"#)?.is_match(before)?
                || regex(r"\breturn$")?.is_match(before)?
            {
                return self.jsx(at);
            }
        }
        let close = match ch {
            b'{' => b'}',
            b'[' => b']',
            b'(' => b')',
            _ => return Ok(at + self.source[at..].chars().next().map_or(0, char::len_utf8)),
        };
        let call = if ch == b'(' {
            self.router_call(at)?
        } else {
            None
        };
        let router = ch == b'('
            && regex(r"(?:^|[^\w$.])createRoutesFrom(?:Elements|Children)\s*$")?
                .is_match(self.tail(at, 40))?;
        let objects = self.options.objects || self.options.table.is_some();
        let mut i = at + 1;
        let mut fields = HashMap::<String, (String, usize)>::new();
        loop {
            i = self.trivia(i);
            if i >= len || self.ch(i) == close {
                break;
            }
            let key = if ch == b'{' {
                regex(r#"^(?:([A-Za-z_$][\w$]*)|["']([^"']+)["'])\s*:"#)?
                    .captures(&self.source[i..])?
            } else {
                None
            };
            if let Some(key) = key {
                let name = key
                    .get(1)
                    .or_else(|| key.get(2))
                    .unwrap()
                    .as_str()
                    .to_owned();
                let start = i;
                let value_at = self.trivia(i + key.get(0).unwrap().end());
                let children = objects && name == "children";
                i = value_at;
                let mut value_end = i;
                loop {
                    i = self.trivia(i);
                    if i >= len || matches!(self.ch(i), b',') || self.ch(i) == close {
                        break;
                    }
                    i = self.unit(i, children)?;
                    value_end = i;
                }
                let value = trim(&self.source[value_at..value_end]).to_owned();
                if children && regex(r"^[A-Za-z_$][\w$]*$")?.is_match(&value)? {
                    self.names.push(Name {
                        name: value.clone(),
                        call: false,
                        at: value_at,
                    });
                }
                fields.insert(name, (value, start));
            } else {
                let entry = i;
                loop {
                    i = self.trivia(i);
                    if i >= len || self.ch(i) == b',' || self.ch(i) == close {
                        break;
                    }
                    i = self.unit(i, false)?;
                }
                if objects
                    && ch == b'{'
                    && let Some(method) = regex(r"^(async\s+)?lazy\s*\(")?
                        .captures(prefix(&self.source[entry..], 32))?
                {
                    fields.insert(
                        "lazy".into(),
                        (
                            format!(
                                "{}function {}",
                                method.get(1).map_or("", |m| m.as_str()),
                                trim(&self.source[entry + method.get(0).unwrap().end() - 1..i])
                            ),
                            entry,
                        ),
                    );
                }
                if route_list && ch == b'[' {
                    let text =
                        regex(r"/\*[\s\S]*?\*/|//[^\n]*")?.replace_all(&self.source[entry..i], "");
                    let text = trim(&text);
                    if let Some(named) =
                        regex(r"^(?:\.\.\.\s*)?([A-Za-z_$][\w$]*)\s*(\([\s\S]*\))?$")?
                            .captures(text)?
                    {
                        let args = named.get(2).map(|m| text.len() - m.as_str().len());
                        if args.is_none()
                            || matching(text, args.unwrap())? == text.len().checked_sub(1)
                        {
                            self.names.push(Name {
                                name: named[1].into(),
                                call: args.is_some(),
                                at: entry,
                            });
                        }
                    }
                }
            }
            if self.ch(i) == b',' {
                i += 1;
            }
        }
        if objects && ch == b'{' && self.ch(i) == close {
            let path_field = fields.get("path");
            let path = path_field
                .map(|field| literal(&field.0))
                .transpose()?
                .flatten();
            let expr = if let Some(field) = path_field {
                if path.is_none()
                    && regex(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)+$")?.is_match(&field.0)?
                {
                    Some(field.0.clone())
                } else {
                    None
                }
            } else {
                None
            };
            let shown = element_tag(fields.get("element").map_or("", |field| field.0.as_str()))?
                .or(component(
                    fields.get("Component").map_or("", |field| field.0.as_str()),
                    false,
                )?);
            let loaded = Reader::default().lazy_reference(
                fields.get("lazy").map_or("", |field| field.0.as_str()),
                &element_tag,
            )?;
            let outlet = shown
                .as_ref()
                .or(loaded.as_ref())
                .is_some_and(|name| name == "Outlet")
                && path.as_deref() != Some("");
            let component = shown.filter(|name| name != "Outlet");
            let lazy = loaded.filter(|name| name != "Outlet");
            let mut scope = Scope {
                part: Part::default(),
                at,
                start: at,
                end: i,
                component,
                lazy,
                jsx: false,
                outlet,
                pathless: false,
                index: false,
                opaque: false,
            };
            if let Some(field) = path_field
                && (path.is_some() || expr.is_some())
            {
                scope.part = Part { lit: path, expr };
                scope.at = field.1;
                self.scopes.push(scope);
            } else if path_field.is_none()
                && fields.get("index").is_some_and(|field| field.0 == "true")
            {
                scope.part = Part::literal(String::new());
                scope.at = fields["index"].1;
                self.scopes.push(scope);
            } else if path_field.is_none() && scope.renders() && fields.contains_key("children") {
                scope.part = Part::literal(String::new());
                scope.pathless = true;
                self.scopes.push(scope);
            }
        }
        if let Some(call) = call
            && i < len
        {
            self.settle(call, at, i)?;
        }
        if router {
            self.routers.push((at, i));
        }
        Ok(if i < len { i + 1 } else { i })
    }
    fn router_call(&self, at: usize) -> Result<Option<Option<Name>>> {
        if !self.options.uses {
            return Ok(None);
        }
        let before = self.tail(at, 64);
        if regex(r"(?:^|[^\w$.])(?:useRoutes|create(?:Browser|Hash|Memory)Router)\s*$")?
            .is_match(before)?
        {
            return Ok(Some(None));
        }
        let Some(map) =
            regex(r"(?:^|[^\w$.])([A-Za-z_$][\w$]*)\s*\??\.\s*map\s*$")?.captures(before)?
        else {
            return Ok(None);
        };
        let at =
            before.as_ptr() as usize - self.source.as_ptr() as usize + map.get(1).unwrap().start();
        Ok(Some(Some(Name {
            name: map[1].into(),
            call: false,
            at,
        })))
    }
    fn settle(&mut self, map: Option<Name>, open: usize, close: usize) -> Result<()> {
        let at = self.trivia(open + 1);
        if at >= close {
            return Ok(());
        }
        if let Some(map) = map {
            let tags = self
                .tags
                .iter()
                .filter(|tag| tag.at > at && tag.at < close)
                .collect::<Vec<_>>();
            if !tags.is_empty() && maps_fields(&self.source[at..close], &tags)? {
                self.sites.push(Site {
                    name: Some(map),
                    literal: None,
                    at: open,
                });
            }
            return Ok(());
        }
        if matches!(self.ch(at), b'[' | b'{') {
            self.sites.push(Site {
                name: None,
                literal: Some(at),
                at: open,
            });
            return Ok(());
        }
        let Some(id) = regex(r"^[A-Za-z_$][\w$]*")?.find(&self.source[at..close])? else {
            return Ok(());
        };
        if regex(r"^createRoutesFrom(?:Elements|Children)$")?.is_match(id.as_str())? {
            return Ok(());
        }
        let next = self.trivia(at + id.end());
        if next == close || self.ch(next) == b',' || self.ch(next) == b'(' {
            self.sites.push(Site {
                name: Some(Name {
                    name: id.as_str().into(),
                    call: self.ch(next) == b'(',
                    at,
                }),
                literal: None,
                at: open,
            });
        }
        Ok(())
    }
    fn jsx(&mut self, at: usize) -> Result<usize> {
        ensure!(self.depth < 128, "React JSX route nesting limit exceeded");
        self.depth += 1;
        let result = self.jsx_inner(at);
        self.depth -= 1;
        result
    }
    fn jsx_inner(&mut self, at: usize) -> Result<usize> {
        let len = self.source.len();
        let tag = regex(r"^<([\w.:-]*)")?
            .captures(&self.source[at..])?
            .unwrap();
        let name = tag[1].to_owned();
        let mut i = at + tag.get(0).unwrap().end();
        let mut attrs = HashMap::<String, String>::new();
        let mut spreads = Vec::new();
        loop {
            i = self.trivia(i);
            if i >= len || self.ch(i) == b'>' || self.source[i..].starts_with("/>") {
                break;
            }
            let Some(attr) = regex(r"^[\w:-]+")?.find(&self.source[i..])? else {
                let from = i;
                i = self.unit(i, false)?;
                if let Some(spread) = regex(r"^\{\s*\.\.\.\s*([A-Za-z_$][\w$]*)\s*\}$")?
                    .captures(&self.source[from..i])?
                {
                    spreads.push(spread[1].into());
                }
                continue;
            };
            let attr = attr.as_str().to_owned();
            i = self.trivia(i + attr.len());
            if self.ch(i) != b'=' {
                attrs.insert(attr, "{true}".into());
                continue;
            }
            i = self.trivia(i + 1);
            let start = i;
            i = self.unit(i, false)?;
            attrs.insert(attr, self.source[start..i].into());
        }
        let router =
            if regex(r"^(?:Browser|Hash|Memory|Native|Static|History|unstable_History)?Router$")?
                .is_match(&name)?
            {
                self.routers.push((at, len));
                Some(self.routers.len() - 1)
            } else {
                None
            };
        let mut scope = None;
        let mut addressless = None;
        if name == "Route" && i < len {
            let expression = |name: &str| -> Option<String> {
                attrs.get(name).map(|value| {
                    value
                        .strip_prefix('{')
                        .and_then(|value| value.strip_suffix('}'))
                        .unwrap_or(value)
                        .to_owned()
                })
            };
            let path_text = attrs.get("path");
            let path = literal(path_text.map_or("", String::as_str))?
                .or(literal(expression("path").as_deref().unwrap_or(""))?);
            self.tags.push(Tag {
                at,
                spreads,
                path: expression("path").map(|value| trim(&value).into()),
            });
            let element = element_tag(expression("element").as_deref().unwrap_or(""))?
                .filter(|name| name != "Outlet");
            let component = component(expression("component").as_deref().unwrap_or(""), false)?
                .or_else(|| element.clone());
            let lazy = Reader::default()
                .lazy_reference(expression("lazy").as_deref().unwrap_or(""), &element_tag)?
                .filter(|name| name != "Outlet");
            let mut route = Scope {
                part: Part::literal(path.clone().unwrap_or_default()),
                at,
                start: at,
                end: len,
                component,
                lazy,
                jsx: true,
                outlet: false,
                pathless: false,
                index: false,
                opaque: false,
            };
            if path.as_ref().is_some_and(|path| !path.is_empty()) {
                self.scopes.push(route);
                scope = Some(self.scopes.len() - 1);
            } else if path_text.is_none() || path.as_deref() == Some("") {
                if regex(r"^\{\s*true\s*\}$")?
                    .is_match(attrs.get("index").map_or("", String::as_str))?
                {
                    if route.renders() {
                        route.index = true;
                        self.scopes.push(route);
                        scope = Some(self.scopes.len() - 1);
                    }
                } else {
                    let page =
                        path.as_deref() == Some("") && (element.is_some() || route.lazy.is_some());
                    addressless = Some((route, self.tags.len(), page));
                }
            } else {
                route.opaque = true;
                self.scopes.push(route);
                scope = Some(self.scopes.len() - 1);
            }
        }
        let end = if self.source[i..].starts_with("/>") {
            i + 2
        } else {
            i = (i + 1).min(len);
            loop {
                if i >= len {
                    break i;
                }
                if self.source[i..].starts_with("</") {
                    break self.source[i + 2..]
                        .find('>')
                        .map_or(len, |end| i + end + 3);
                }
                if self.ch(i) == b'<' && regex(r"^<(?:[A-Za-z]|>)")?.is_match(&self.source[i..])? {
                    i = self.jsx(i)?;
                } else if self.ch(i) == b'{' {
                    i = self.unit(i, false)?;
                } else {
                    i += self.source[i..].chars().next().unwrap().len_utf8();
                }
            }
        };
        if let Some(scope) = scope {
            self.scopes[scope].end = end;
        }
        if let Some(router) = router {
            self.routers[router].1 = end;
        }
        if let Some((mut route, tags, page)) = addressless {
            route.end = end;
            if self.tags.len() > tags {
                if route.renders() {
                    route.pathless = true;
                    self.scopes.push(route);
                }
            } else if page {
                route.index = true;
                self.scopes.push(route);
            }
        }
        Ok(end)
    }
    fn around(&self, start: usize, end: usize) -> Vec<&Scope> {
        let mut around = self
            .scopes
            .iter()
            .filter(|outer| outer.holds(start, end))
            .collect::<Vec<_>>();
        around.sort_by_key(|scope| scope.start);
        around
    }
    fn finish(self) -> Result<Scan> {
        ensure!(
            self.scopes.len().saturating_mul(self.scopes.len()) < 10_000_000,
            "React route relation limit exceeded"
        );
        let (prefix, inherited) = self
            .options
            .table
            .as_ref()
            .map(|(_, parts, layouts)| (parts.clone(), layouts.clone()))
            .unwrap_or_default();
        let mut parts = Vec::new();
        let mut paths = Vec::new();
        let mut pages = Vec::new();
        for (index, scope) in self.scopes.iter().enumerate() {
            let mut full = prefix.clone();
            full.extend(
                self.around(scope.start, scope.end)
                    .iter()
                    .map(|scope| scope.part.clone()),
            );
            full.push(scope.part.clone());
            paths.push(compose(full.iter().map(Part::shown)));
            parts.push(full);
            let addressed = if !scope.index {
                true
            } else {
                let outer = self.around(scope.start, scope.end);
                !outer.iter().any(|scope| scope.opaque)
                    && (outer.iter().any(|scope| {
                        scope.part.lit.as_ref().is_some_and(|path| !path.is_empty())
                            || scope
                                .part
                                .expr
                                .as_ref()
                                .is_some_and(|expr| !expr.is_empty())
                    }) || self
                        .routers
                        .iter()
                        .any(|&(start, end)| start < scope.start && end >= scope.end))
            };
            if !scope.pathless
                && !scope.opaque
                && (scope.jsx || scope.outlet || scope.renders())
                && addressed
            {
                pages.push(index);
            }
        }
        let mut routes = Vec::new();
        for &index in &pages {
            let scope = &self.scopes[index];
            if pages.iter().any(|&inner| {
                let nested = &self.scopes[inner];
                nested.jsx == scope.jsx
                    && nested.renders()
                    && scope.holds(nested.start, nested.end)
                    && paths[inner] == paths[index]
            }) {
                continue;
            }
            let mut layouts = inherited.clone();
            layouts.extend(
                self.around(scope.start, scope.end)
                    .into_iter()
                    .filter(|outer| outer.renders() && !outer.opaque && (scope.jsx || !outer.jsx))
                    .filter_map(Scope::layout),
            );
            routes.push(Route {
                path: paths[index].clone(),
                parts: parts[index].clone(),
                component: scope.component.clone(),
                lazy: scope.lazy.clone(),
                layouts,
                at: scope.at,
            });
        }
        routes.sort_by_key(|route| route.at);
        let mount = |at| {
            let outer = self.around(at, at);
            (
                outer
                    .iter()
                    .map(|scope| scope.part.clone())
                    .collect::<Vec<_>>(),
                outer
                    .iter()
                    .filter(|scope| scope.renders() && !scope.opaque)
                    .filter_map(|scope| scope.layout())
                    .collect::<Vec<_>>(),
            )
        };
        let names = self
            .names
            .iter()
            .map(|name| {
                let (parts, layouts) = mount(name.at);
                MountedName {
                    name: name.clone(),
                    prefix: prefix.iter().cloned().chain(parts).collect(),
                    layouts: inherited.iter().cloned().chain(layouts).collect(),
                }
            })
            .collect();
        let uses = self
            .sites
            .iter()
            .map(|site| {
                let (prefix, layouts) = mount(site.at);
                Use {
                    name: site.name.clone(),
                    literal: site.literal,
                    prefix,
                    layouts,
                }
            })
            .collect();
        Ok(Scan {
            routes,
            names,
            uses,
        })
    }
}
fn maps_fields(callback: &str, tags: &[&Tag]) -> Result<bool> {
    let mut param = None;
    let mut rest = HashSet::<String>::new();
    let mut paths = HashSet::<String>::new();
    let mut bind = |pattern: &str| -> Result<()> {
        for entry in items(&pattern[1..pattern.len() - 1])? {
            if let Some(spread) = regex(r"^\.\.\.\s*([A-Za-z_$][\w$]*)$")?.captures(entry)? {
                rest.insert(spread[1].into());
            }
            if let Some(path) =
                regex(r"^path\s*(?::\s*([A-Za-z_$][\w$]*))?\s*(?:=[\s\S]*)?$")?.captures(entry)?
            {
                paths.insert(path.get(1).map_or("path", |m| m.as_str()).into());
            }
        }
        Ok(())
    };
    if let Some(head) = regex(r"^(?:async\s+)?(?:function\b[^(]*)?\(\s*")?.find(callback)? {
        let at = head.end();
        if callback.as_bytes().get(at) == Some(&b'{') {
            let Some(close) = matching(callback, at)? else {
                return Ok(false);
            };
            bind(&callback[at..close + 1])?;
        } else {
            param = regex(r"^[A-Za-z_$][\w$]*")?
                .find(&callback[at..])?
                .map(|m| m.as_str().to_owned());
        }
    } else {
        param = regex(r"^(?:async\s+)?([A-Za-z_$][\w$]*)\s*=>")?
            .captures(callback)?
            .map(|m| m[1].into());
    }
    if let Some(param) = &param {
        for found in regex(&format!(
            r"\b(?:const|let|var)\s*(\{{[^{{}}]*\}})\s*=\s*{}\s*[;\n]",
            param.replace('$', "\\$")
        ))?
        .captures_iter(callback)
        {
            bind(&found?[1])?;
        }
    }
    let own = param
        .as_ref()
        .map(|param| regex(&format!(r"^{}\s*\??\.\s*path$", param.replace('$', "\\$"))))
        .transpose()?;
    for tag in tags {
        if tag
            .spreads
            .iter()
            .any(|name| Some(name) == param.as_ref() || rest.contains(name))
        {
            return Ok(true);
        }
        if let Some(path) = &tag.path
            && (paths.contains(path)
                || own
                    .as_ref()
                    .map(|own| own.is_match(path).map_err(anyhow::Error::from))
                    .transpose()?
                    .unwrap_or(false))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
