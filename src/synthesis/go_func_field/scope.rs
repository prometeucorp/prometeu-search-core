//! Go declarations and block scopes, preserving the reference's conservative boundaries.
use super::*;

pub(crate) fn blank(source: &str) -> String {
    let mut out = source.as_bytes().to_vec();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let (start, end, next) = if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let end = source[i..].find('\n').map_or(bytes.len(), |n| i + n);
            (i, end, end)
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let end = source[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |n| i + n + 4);
            (i, end, end)
        } else if bytes[i] == b'`' {
            let end = source[i + 1..].find('`').map_or(bytes.len(), |n| i + n + 1);
            (i + 1, end, end + 1)
        } else if matches!(bytes[i], b'\'' | b'"') {
            let mut end = i + 1;
            while end < bytes.len() && bytes[end] != bytes[i] && bytes[end] != b'\n' {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            (i + 1, end.min(bytes.len()), end + 1)
        } else {
            i += 1;
            continue;
        };
        for byte in &mut out[start..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        i = next;
    }
    String::from_utf8(out).expect("masked complete UTF-8 spans")
}
const KEYWORDS: &[&str] = &["chan", "func", "interface", "map", "struct"];
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Local,
    Header,
    Case,
    Param,
}
#[derive(Clone)]
pub(crate) struct Declaration {
    pub at: usize,
    pub ty: Option<String>,
    kind: Kind,
    body: usize,
}
type Scope = (usize, usize, Declaration);
type Signature = (Vec<(usize, usize)>, usize);
pub(crate) struct Scopes {
    code: String,
    events: Vec<usize>,
    opens: Vec<Option<usize>>,
    closes: HashMap<usize, usize>,
    declarations: HashMap<String, Vec<Declaration>>,
    scopes: RefCell<HashMap<String, Vec<Scope>>>,
}
fn ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
fn window(code: &str, start: usize, len: usize) -> &str {
    let mut end = (start + len).min(code.len());
    while !code.is_char_boundary(end) {
        end -= 1;
    }
    &code[start.min(code.len())..end]
}
fn close(code: &str, open: usize) -> usize {
    matching(code, open).unwrap_or(code.len())
}
fn skip(code: &str, at: &mut usize) {
    while matches!(code.as_bytes().get(*at), Some(b' ' | b'\t')) {
        *at += 1;
    }
}
fn named_group(code: &str, at: &mut usize) -> bool {
    let bytes = code.as_bytes();
    let start = *at;
    if !bytes
        .get(start)
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
    {
        return false;
    }
    let mut q = start + 1;
    while bytes.get(q).is_some_and(|b| ident(*b)) {
        q += 1;
    }
    if KEYWORDS.contains(&&code[start..q]) {
        return false;
    }
    skip(code, &mut q);
    if bytes.get(q) == Some(&b'[') {
        q = close(code, q) + 1;
    }
    skip(code, &mut q);
    if bytes.get(q) != Some(&b'(') {
        return false;
    }
    *at = q;
    true
}
fn group(code: &str, at: &mut usize, groups: &mut Vec<(usize, usize)>) -> bool {
    if code.as_bytes().get(*at) != Some(&b'(') {
        return false;
    }
    let end = close(code, *at);
    groups.push((*at + 1, end));
    *at = end + 1;
    skip(code, at);
    true
}
fn signature(code: &str, mut at: usize) -> Result<Option<Signature>> {
    let mut groups = Vec::new();
    skip(code, &mut at);
    if code.as_bytes().get(at) != Some(&b'(') {
        if !named_group(code, &mut at) || !group(code, &mut at, &mut groups) {
            return Ok(None);
        }
    } else {
        group(code, &mut at, &mut groups);
        if named_group(code, &mut at) && !group(code, &mut at, &mut groups) {
            return Ok(None);
        }
    }
    if code.as_bytes().get(at) == Some(&b'(') {
        group(code, &mut at, &mut groups);
    }
    let mut depth = 0i32;
    while at < code.len() {
        match code.as_bytes()[at] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => {
                depth -= 1;
                if depth < 0 {
                    return Ok(None);
                }
            }
            b'{' if depth == 0 => {
                let start = code[..at]
                    .char_indices()
                    .rev()
                    .nth(11)
                    .map_or(0, |(i, _)| i);
                if patterns().result_brace.is_match(&code[start..at])? {
                    at = close(code, at)
                } else {
                    return Ok(Some((groups, at)));
                }
            }
            b'\n' | b',' | b';' | b'=' | b'}' if depth == 0 => return Ok(None),
            _ => {}
        }
        at += 1;
    }
    Ok(None)
}
fn parameter_names(code: &str, from: usize, to: usize) -> Result<Vec<(String, usize, String)>> {
    if from > to || to > code.len() {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = from;
    for (i, byte) in code.bytes().enumerate().take(to).skip(from) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push((start, to));
    let mut output = Vec::new();
    let mut grouped = Vec::new();
    for (start, end) in parts {
        let text = &code[start..end];
        let Some(m) = patterns().parameter.captures(text)? else {
            continue;
        };
        let name = m.get(1).unwrap().as_str();
        if KEYWORDS.contains(&name) {
            continue;
        }
        let rest = &text[m.get(0).unwrap().end()..];
        if patterns().qualified.is_match(rest)? {
            continue;
        }
        let at = start + m.get(1).unwrap().start();
        if rest.trim().is_empty() {
            grouped.push((name.to_owned(), at));
            continue;
        }
        let ty = rest.trim().to_owned();
        for (name, at) in grouped.drain(..) {
            output.push((name, at, ty.clone()));
        }
        output.push((name.to_owned(), at, ty));
    }
    Ok(output)
}
struct Patterns {
    header: fancy_regex::Regex,
    case: fancy_regex::Regex,
    literal: fancy_regex::Regex,
    new: fancy_regex::Regex,
    spec: fancy_regex::Regex,
    ty: fancy_regex::Regex,
    var: fancy_regex::Regex,
    func: fancy_regex::Regex,
    function_type: fancy_regex::Regex,
    result_brace: fancy_regex::Regex,
    parameter: fancy_regex::Regex,
    qualified: fancy_regex::Regex,
    header_open: fancy_regex::Regex,
    chained: fancy_regex::Regex,
    clause: fancy_regex::Regex,
}
fn patterns() -> &'static Patterns {
    static PATTERNS: std::sync::LazyLock<Patterns> = std::sync::LazyLock::new(|| {
        let r = |pattern| regex(pattern).expect("valid reference pattern");
        Patterns {
            header: r(r"^\s*(?:\}\s*else\s+)?(?:if|for|switch)\b"),
            case: r(r"^\s*case\b"),
            literal: r(r"^[ \t]*&?[ \t]*((?:[A-Za-z_]\w*\.)?[A-Za-z_]\w*)[ \t]*\{"),
            new: r(r"^[ \t]*new\([ \t]*((?:[A-Za-z_]\w*\.)?[A-Za-z_]\w*)[ \t]*\)"),
            spec: r(
                r"^[ \t]*([A-Za-z_]\w*(?:[ \t]*,[ \t]*[A-Za-z_]\w*)*)(?=[ \t]*(?:\n|$|=(?!=))|[ \t]+(?:[*[A-Za-z_(]|<-))",
            ),
            ty: r(r"^[ \t]+([^\s=;][^=;\n]*?)[ \t]*(?:[=;\n]|$)"),
            var: r(r"(?<![\w.])(?:var|const)\b[ \t]*"),
            func: r(r"(?<![\w.])func\b"),
            function_type: r(r"(?:[\]*)]|\bchan(?:[ \t]*<-)?)[ \t]*$"),
            result_brace: r(r"(?:struct|interface)\s*$"),
            parameter: r(r"^\s*([A-Za-z_]\w*)"),
            qualified: r(r"^\s*\.(?!\.\.)"),
            header_open: r(r"^[ \t]*(?:\n|$)"),
            chained: r(r"^[ \t]*else\b[ \t]*"),
            clause: r(r"\n[ \t]*(?:case\b|default[ \t]*:)"),
        }
    });
    &PATTERNS
}
impl Scopes {
    pub fn new(source: &str) -> Result<Self> {
        let code = blank(source);
        let mut events = Vec::new();
        let mut opens = Vec::new();
        let mut closes = HashMap::new();
        let mut stack = Vec::new();
        for (i, byte) in code.bytes().enumerate() {
            if byte == b'{' {
                stack.push(i)
            } else if byte == b'}' {
                if let Some(open) = stack.pop() {
                    closes.insert(open, i);
                }
            } else {
                continue;
            }
            events.push(i);
            opens.push(stack.last().copied());
        }
        let mut this = Self {
            code,
            events,
            opens,
            closes,
            declarations: HashMap::new(),
            scopes: RefCell::new(HashMap::new()),
        };
        // Read the names backwards from each short declaration.
        let declarations: Vec<_> = this.code.match_indices(":=").map(|(i, _)| i).collect();
        for i in declarations {
            let bytes = this.code.as_bytes();
            let mut names = Vec::new();
            let mut p = i;
            loop {
                while p > 0 && matches!(bytes[p - 1], b' ' | b'\t') {
                    p -= 1;
                }
                let mut s = p;
                while s > 0 && ident(bytes[s - 1]) {
                    s -= 1;
                }
                if s == p || bytes[s].is_ascii_digit() || s > 0 && bytes[s - 1] == b'.' {
                    break;
                }
                names.push((this.code[s..p].to_owned(), s));
                p = s;
                while p > 0 && matches!(bytes[p - 1], b' ' | b'\t') {
                    p -= 1;
                }
                if p == 0 || bytes[p - 1] != b',' {
                    break;
                }
                p -= 1;
            }
            let Some((_, first)) = names.last() else {
                continue;
            };
            let line = this.code[..*first].rfind('\n').map_or(0, |n| n + 1);
            let lead = &this.code[line..*first];
            let kind = if patterns().header.is_match(lead)? {
                Kind::Header
            } else if patterns().case.is_match(lead)? {
                Kind::Case
            } else {
                Kind::Local
            };
            let rhs = window(&this.code, i + 2, 158);
            let literal = patterns()
                .literal
                .captures(rhs)?
                .or(patterns().new.captures(rhs)?)
                .and_then(|m| m.get(1).map(|v| v.as_str().to_owned()));
            let ty = literal.filter(|name| names.len() == 1 && !KEYWORDS.contains(&name.as_str()));
            for (name, at) in names {
                this.add(
                    name,
                    Declaration {
                        at,
                        ty: ty.clone(),
                        kind,
                        body: 0,
                    },
                );
            }
        }
        let vars: Vec<_> = patterns()
            .var
            .find_iter(&this.code)
            .map(|m| Ok(m?.end()))
            .collect::<Result<_>>()?;
        for start in vars {
            if this.code.as_bytes().get(start) != Some(&b'(') {
                this.spec(start)?;
                continue;
            }
            let end = close(&this.code, start);
            this.spec(start + 1)?;
            let mut depth = 0i32;
            let mut starts = Vec::new();
            for i in start + 1..end {
                match this.code.as_bytes()[i] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth -= 1,
                    b'\n' if depth == 0 => starts.push(i + 1),
                    _ => {}
                }
            }
            for at in starts {
                this.spec(at)?;
            }
        }
        let functions: Vec<_> = patterns()
            .func
            .find_iter(&this.code)
            .map(|m| Ok(m?.start()))
            .collect::<Result<_>>()?;
        for at in functions {
            let start = this.code[..at]
                .char_indices()
                .rev()
                .nth(7)
                .map_or(0, |(i, _)| i);
            if patterns().function_type.is_match(&this.code[start..at])? {
                continue;
            }
            let Some((groups, body)) = signature(&this.code, at + 4)? else {
                continue;
            };
            for (from, to) in groups {
                for (name, at, ty) in parameter_names(&this.code, from, to)? {
                    this.add(
                        name,
                        Declaration {
                            at,
                            ty: Some(ty),
                            kind: Kind::Param,
                            body,
                        },
                    );
                }
            }
        }
        Ok(this)
    }
    fn add(&mut self, name: String, decl: Declaration) {
        if name != "_" {
            self.declarations.entry(name).or_default().push(decl);
        }
    }
    fn spec(&mut self, at: usize) -> Result<()> {
        let Some(m) = patterns()
            .spec
            .captures(&self.code[at.min(self.code.len())..])?
        else {
            return Ok(());
        };
        let end = at + m.get(0).unwrap().end();
        let ty = patterns()
            .ty
            .captures(window(&self.code, end, 160))?
            .and_then(|m| m.get(1).map(|v| v.as_str().to_owned()));
        let list = m.get(1).unwrap();
        let mut p = at + list.start();
        let mut names = Vec::new();
        for part in list.as_str().split(',') {
            let name = part.trim();
            names.push((name.to_owned(), p + part.find(name).unwrap_or(0)));
            p += part.len() + 1;
        }
        for (name, at) in names {
            self.add(
                name,
                Declaration {
                    at,
                    ty: ty.clone(),
                    kind: Kind::Local,
                    body: 0,
                },
            );
        }
        Ok(())
    }
    fn block(&self, at: usize) -> Option<(usize, usize)> {
        let index = self
            .events
            .partition_point(|&event| event < at)
            .checked_sub(1)?;
        let open = self.opens[index]?;
        Some((open, *self.closes.get(&open).unwrap_or(&self.code.len())))
    }
    fn header_blocks(&self, at: usize) -> Result<Vec<(usize, usize)>> {
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut i = at;
        while i < self.code.len() {
            match self.code.as_bytes()[i] {
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth -= 1,
                b'}' => break,
                b'{' => {
                    let end = *self.closes.get(&i).unwrap_or(&self.code.len());
                    if depth != 0
                        || !patterns()
                            .header_open
                            .is_match(window(&self.code, i + 1, 79))?
                    {
                        i = end;
                        i += 1;
                        continue;
                    }
                    out.push((i, end));
                    let Some(m) = patterns().chained.find(window(
                        &self.code,
                        (end + 1).min(self.code.len()),
                        39,
                    ))?
                    else {
                        break;
                    };
                    i = end + m.end();
                    depth = 0;
                }
                _ => {}
            }
            i += 1;
        }
        Ok(out)
    }
    pub fn lookup(&self, name: &str, at: usize) -> Result<Option<Declaration>> {
        if !self.scopes.borrow().contains_key(name) {
            let mut scopes = Vec::new();
            for decl in self.declarations.get(name).into_iter().flatten() {
                if decl.kind == Kind::Param {
                    scopes.push((
                        decl.body,
                        *self.closes.get(&decl.body).unwrap_or(&self.code.len()),
                        decl.clone(),
                    ));
                    continue;
                }
                let Some((open, mut to)) = self.block(decl.at) else {
                    continue;
                };
                if decl.kind == Kind::Header {
                    for (start, end) in self.header_blocks(decl.at)? {
                        scopes.push((start, end, decl.clone()));
                    }
                    continue;
                }
                let from = statement_end(&self.code, decl.at);
                for m in patterns().clause.find_iter(&self.code[from..]) {
                    let index = from + m?.start();
                    if index >= to {
                        break;
                    }
                    if self.block(index + 1).is_some_and(|b| b.0 == open) {
                        to = index;
                        break;
                    }
                }
                if from < to {
                    scopes.push((from, to, decl.clone()));
                }
            }
            self.scopes.borrow_mut().insert(name.to_owned(), scopes);
        }
        Ok(self.scopes.borrow()[name]
            .iter()
            .filter(|(start, end, _)| at > *start && at < *end)
            .max_by_key(|(start, _, _)| *start)
            .map(|(_, _, decl)| decl.clone()))
    }
}
