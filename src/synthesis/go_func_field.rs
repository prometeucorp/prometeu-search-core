//! Go callbacks carried by typed struct fields, using committed imports and source scopes.
use super::*;
use std::cmp::Ordering;
pub(crate) mod scope;
const IDENT: &str = r"[A-Za-z_]\w*";
pub(crate) const PREDECLARED: &[&str] = &[
    "any",
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
];
fn matching(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, byte) in text.bytes().enumerate().skip(open) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}
fn items(text: &str, open: usize, close: usize) -> Vec<(&str, usize)> {
    let mut output = Vec::new();
    let mut start = open + 1;
    let mut depth = 0i32;
    let mut push = |start: usize, end: usize| {
        let raw = &text[start..end];
        let value = raw.trim();
        if !value.is_empty() {
            output.push((value, start + raw.len() - raw.trim_start().len()));
        }
    };
    for (i, byte) in text.bytes().enumerate().take(close).skip(open + 1) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                push(start, i);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(start, close);
    output
}
fn statement_end(text: &str, from: usize) -> usize {
    let mut depth = 0i32;
    for (i, byte) in text.bytes().enumerate().skip(from) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1
            }
            b'\n' | b';' if depth == 0 => return i,
            _ => {}
        }
    }
    text.len()
}
fn byte_at(text: &str, column: u32) -> usize {
    let mut units = 0;
    for (at, ch) in text.char_indices() {
        if units >= column {
            return at;
        }
        units += ch.len_utf16() as u32;
    }
    text.len()
}
struct Masked {
    text: String,
    original: String,
    starts: Vec<usize>,
    scope: scope::Scopes,
}
impl Masked {
    fn new(source: &str) -> Result<Self> {
        let text = scope::blank(source);
        let mut starts = vec![0];
        starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        Ok(Self {
            text,
            original: source.to_owned(),
            starts,
            scope: scope::Scopes::new(source)?,
        })
    }
    fn offset(&self, line: u32, column: u32) -> usize {
        let start = self
            .starts
            .get(line.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(self.text.len());
        let end = self
            .starts
            .get(line as usize)
            .copied()
            .unwrap_or(self.text.len());
        start + byte_at(&self.original[start..end], column)
    }
    fn line(&self, at: usize) -> u32 {
        self.starts.partition_point(|&s| s <= at) as u32
    }
}
struct Patterns {
    named: fancy_regex::Regex,
    func: fancy_regex::Regex,
    spec: fancy_regex::Regex,
    package: fancy_regex::Regex,
    struct_open: fancy_regex::Regex,
    fields: fancy_regex::Regex,
    selector_call: fancy_regex::Regex,
    selector_assign: fancy_regex::Regex,
    names_from: fancy_regex::Regex,
    names_before: fancy_regex::Regex,
    plain: fancy_regex::Regex,
    conversion: fancy_regex::Regex,
    key: fancy_regex::Regex,
    typed_result: fancy_regex::Regex,
    literal: fancy_regex::Regex,
    created: fancy_regex::Regex,
    asserted: fancy_regex::Regex,
    call: fancy_regex::Regex,
}
fn patterns() -> &'static Patterns {
    static PATTERNS: std::sync::LazyLock<Patterns> = std::sync::LazyLock::new(|| {
        let r = |p: &str| regex(p).expect("valid reference pattern");
        Patterns {
            named: r(&format!(
                r"^[*&\s]*({IDENT})(?:\s*\.\s*({IDENT}))?\s*(?:\[[\s\S]*\])?\s*$"
            )),
            func: r(r"^func\s*\("),
            spec: r(&format!(r"^{IDENT}\s*(?:\[[^\]]*\])?\s*=?\s*([^\n;]+)")),
            package: r(r"(?m)^\s*package\s+([A-Za-z_]\w*)"),
            struct_open: r(r"\bstruct\s*\{"),
            fields: r(r"^([A-Za-z_]\w*(?:\s*,\s*[A-Za-z_]\w*)*)\s+(\S[\s\S]*)$"),
            selector_call: r(&format!(
                r"(?<![\w.])({IDENT}(?:\s*\.\s*{IDENT})?)\s*\.\s*({IDENT})\s*\("
            )),
            selector_assign: r(&format!(
                r"(?<![\w.])({IDENT}(?:\s*\.\s*{IDENT})?)\s*\.\s*({IDENT})\s*=(?!=)"
            )),
            names_from: r(&format!(r"^{IDENT}((?:\s*,\s*{IDENT})*)\s*(?::=|=(?!=))")),
            names_before: r(&format!(r"(?:{IDENT}\s*,\s*)*$")),
            plain: r(&format!(r"^{IDENT}(?:\s*\.\s*{IDENT})?$")),
            conversion: r(&format!(r"^({IDENT}(?:\s*\.\s*{IDENT})?)\s*\(")),
            key: r(r"^([A-Za-z_]\w*)\s*:(?!=)\s*"),
            typed_result: r(r"^([A-Za-z_]\w*)\s+(\S[\s\S]*)$"),
            literal: r(&format!(
                r"^&?\s*({IDENT}(?:\s*\.\s*{IDENT})?)\s*(?:\[[^\]]*\])?\s*\{{"
            )),
            created: r(&format!(
                r"^new\s*\(\s*({IDENT}(?:\s*\.\s*{IDENT})?)\s*(?:\[[^\]]*\])?\s*\)$"
            )),
            asserted: r(&format!(
                r"\.\s*\(\s*(?!type\s*\))(\*?\s*{IDENT}(?:\s*\.\s*{IDENT})?)\s*\)$"
            )),
            call: r(&format!(r"^({IDENT}(?:\s*\.\s*{IDENT})*)\s*\(")),
        }
    });
    &PATTERNS
}
fn compact(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}
fn type_kind(node: &Node) -> bool {
    node.language == "go" && matches!(node.kind.as_str(), "struct" | "interface" | "type_alias")
}
fn function(node: &Node) -> bool {
    matches!(node.kind.as_str(), "function" | "method")
}
pub(crate) fn result_types(signature: Option<&str>) -> Result<Option<Vec<String>>> {
    let masked = scope::blank(signature.unwrap_or(""));
    let text = masked.trim();
    if !text.starts_with('(') {
        return Ok(None);
    }
    let Some(close) = matching(text, 0) else {
        return Ok(None);
    };
    let rest = text[close + 1..].trim();
    if !rest.starts_with('(') {
        return Ok(Some(if rest.is_empty() {
            vec![]
        } else {
            vec![rest.to_owned()]
        }));
    }
    let Some(end) = matching(rest, 0) else {
        return Ok(None);
    };
    let values = items(rest, 0, end);
    let split = |text: &str| -> Result<Option<String>> {
        Ok(patterns()
            .typed_result
            .captures(text.trim())?
            .filter(|m| !scope_keywords(m.get(1).unwrap().as_str()))
            .map(|m| m.get(2).unwrap().as_str().trim().to_owned()))
    };
    if values
        .last()
        .map(|v| split(v.0))
        .transpose()?
        .flatten()
        .is_none()
    {
        return Ok(Some(
            values.into_iter().map(|v| v.0.trim().to_owned()).collect(),
        ));
    }
    let mut ty = String::new();
    let mut out = Vec::new();
    for (value, _) in values.into_iter().rev() {
        if let Some(new) = split(value)? {
            ty = new
        }
        out.push(ty.clone());
    }
    out.reverse();
    Ok(Some(out))
}
fn scope_keywords(name: &str) -> bool {
    matches!(name, "chan" | "func" | "map" | "struct" | "interface")
}
#[derive(Clone)]
struct Field {
    name: String,
    ty: String,
    embedded: bool,
}
struct Struct<'a> {
    name: String,
    dir: String,
    pkg: String,
    node: &'a Node,
    fields: Vec<Field>,
    func_fields: HashSet<String>,
}
#[derive(Clone)]
struct TypeText {
    ty: String,
    file: String,
}
#[derive(Clone, Copy)]
enum Resolved<'a> {
    Struct(usize),
    Interface(&'a Node),
}
#[derive(Clone)]
enum Outcome<'a> {
    Function(&'a Node),
    Copy(usize, String),
}
#[derive(Clone)]
struct ValueAt<'a> {
    target: &'a Node,
    site: String,
    test: bool,
    via: Option<String>,
}
struct Pass<'a, 'c> {
    ctx: &'c Context<'a>,
    files: HashMap<String, Masked>,
    structs: Vec<Struct<'a>>,
    struct_keys: HashMap<(String, String), usize>,
    specs: HashMap<String, String>,
    field_names: HashSet<String>,
}
impl<'a, 'c> Pass<'a, 'c> {
    fn import_dir(&self, qualifier: &str, file: &str) -> Option<String> {
        let binding = self.ctx.imports.get(&(file, qualifier))?;
        (self.ctx.go_package_dir)(file, &binding.source)
    }
    fn type_nodes(&self, name: &str, dir: &str) -> Vec<&'a Node> {
        if PREDECLARED.contains(&name) {
            return Vec::new();
        }
        self.ctx
            .named(name)
            .filter(|n| type_kind(n) && directory(&n.file_path) == dir)
            .collect()
    }
    fn named(&self, text: &str, file: &str) -> Result<Option<(String, String, Vec<&'a Node>)>> {
        let Some(m) = patterns().named.captures(text.trim())? else {
            return Ok(None);
        };
        let first = m.get(1).unwrap().as_str();
        let second = m.get(2).map(|v| v.as_str());
        if second.is_none() && PREDECLARED.contains(&first) {
            return Ok(None);
        }
        let dir = if second.is_some() {
            let Some(dir) = self.import_dir(first, file) else {
                return Ok(None);
            };
            dir
        } else {
            directory(file)
        };
        let name = second.unwrap_or(first).to_owned();
        let nodes = self.type_nodes(&name, &dir);
        Ok(Some((name, dir, nodes)))
    }
    fn func_type(&self, node: &Node) -> Result<bool> {
        Ok(node.kind == "type_alias"
            && patterns()
                .func
                .is_match(self.specs.get(&node.id).map_or("", String::as_str))?)
    }
    fn func_field(&self, ty: &str, file: &str) -> Result<bool> {
        if patterns().func.is_match(ty)? {
            return Ok(true);
        }
        if ty.starts_with('*') {
            return Ok(false);
        }
        let Some((_, _, nodes)) = self.named(ty, file)? else {
            return Ok(false);
        };
        if nodes.is_empty() {
            return Ok(false);
        }
        for node in nodes {
            if !self.func_type(node)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn resolve(&self, text: &str, file: &str, depth: usize) -> Result<Option<Resolved<'a>>> {
        self.ctx.step()?;
        if depth > 3 {
            return Ok(None);
        }
        let Some((name, dir, nodes)) = self.named(text, file)? else {
            return Ok(None);
        };
        if let Some(&i) = self.struct_keys.get(&(dir, name)) {
            return Ok(Some(Resolved::Struct(i)));
        }
        if let Some(&node) = nodes.iter().find(|n| n.kind == "interface") {
            return Ok(Some(Resolved::Interface(node)));
        }
        if let Some(node) = nodes.first()
            && let Some(ty) = self.specs.get(&node.id)
        {
            return self.resolve(ty, &node.file_path, depth + 1);
        }
        Ok(None)
    }
    fn has_method(&self, i: usize, name: &str) -> bool {
        let s = &self.structs[i];
        self.ctx.named(name).any(|node| {
            node.language == "go"
                && node.kind == "method"
                && node.qualified_name == format!("{}::{name}", s.name)
                && directory(&node.file_path) == s.dir
        })
    }
    fn field_owner(&self, i: usize, field: &str) -> Result<Option<usize>> {
        let s = &self.structs[i];
        if s.func_fields.contains(field) {
            return Ok(Some(i));
        }
        if s.fields.iter().any(|f| f.name == field) || self.has_method(i, field) {
            return Ok(None);
        }
        let mut holders = Vec::new();
        for embedded in s.fields.iter().filter(|f| f.embedded) {
            if let Some(Resolved::Struct(j)) = self.resolve(&embedded.ty, &s.node.file_path, 0)?
                && self.structs[j].fields.iter().any(|f| f.name == field)
            {
                holders.push(j)
            }
        }
        Ok(
            (holders.len() == 1 && self.structs[holders[0]].func_fields.contains(field))
                .then(|| holders[0]),
        )
    }
    fn rhs_type(
        &self,
        f: &Masked,
        function: &Node,
        start: usize,
        index: usize,
        names: usize,
    ) -> Result<Option<TypeText>> {
        let rhs = &f.text[start..statement_end(&f.text, start)];
        if rhs.trim_start().starts_with("range ") {
            return Ok(None);
        }
        let lead = rhs.len() - rhs.trim_start().len();
        let wrapped = format!("({rhs})");
        let values = items(&wrapped, 0, wrapped.len() - 1);
        let (value, result) = if names > 1 && values.len() == names {
            (values[index].0, 0)
        } else if values.len() == 1 {
            (values[0].0, index)
        } else {
            return Ok(None);
        };
        if result == 0 {
            for pattern in [
                &patterns().literal,
                &patterns().created,
                &patterns().asserted,
            ] {
                if let Some(m) = pattern.captures(value)? {
                    return Ok(Some(TypeText {
                        ty: m.get(1).unwrap().as_str().to_owned(),
                        file: function.file_path.clone(),
                    }));
                }
            }
        }
        let Some(call) = patterns().call.captures(value)? else {
            return Ok(None);
        };
        if matching(value, call.get(0).unwrap().end() - 1) != Some(value.len() - 1) {
            return Ok(None);
        }
        let name = compact(call.get(1).unwrap().as_str());
        let line = f.line(start + lead);
        let callees: HashSet<_> = self
            .ctx
            .outgoing(&function.id)
            .filter(|e| {
                e.kind == "calls"
                    && e.provenance.as_deref() != Some("heuristic")
                    && e.line == Some(line)
                    && e.metadata.as_ref().is_some_and(|m| m["refName"] == name)
            })
            .map(|e| e.target.as_str())
            .collect();
        if callees.len() != 1 {
            return Ok(None);
        }
        let Some(callee) = self
            .ctx
            .node(callees.into_iter().next().unwrap())
            .filter(|n| function_node(n))
        else {
            return Ok(None);
        };
        Ok(result_types(callee.signature.as_deref())?
            .and_then(|types| types.get(result).cloned())
            .map(|ty| TypeText {
                ty,
                file: callee.file_path.clone(),
            }))
    }
    fn lookup(
        &self,
        function: &Node,
        f: &Masked,
        name: &str,
        at: usize,
    ) -> Result<Option<Option<TypeText>>> {
        let Some(decl) = f.scope.lookup(name, at)? else {
            return Ok(None);
        };
        if let Some(ty) = decl.ty {
            return Ok(Some((!ty.starts_with("...")).then(|| TypeText {
                ty,
                file: function.file_path.clone(),
            })));
        }
        let p = decl.at;
        let mut end = (p + 400).min(f.text.len());
        while !f.text.is_char_boundary(end) {
            end -= 1;
        }
        let Some(from) = patterns().names_from.captures(&f.text[p..end])? else {
            return Ok(Some(None));
        };
        let line = f.line(p);
        let before = patterns()
            .names_before
            .find(&f.text[f.starts[line as usize - 1]..p])?
            .unwrap();
        let index = if before.as_str().is_empty() {
            0
        } else {
            before.as_str().split(',').count() - 1
        };
        let names = index
            + from
                .get(1)
                .filter(|v| !v.as_str().is_empty())
                .map_or(1, |v| v.as_str().split(',').count());
        Ok(Some(self.rhs_type(
            f,
            function,
            p + from.get(0).unwrap().end(),
            index,
            names,
        )?))
    }
    fn expr_type(
        &self,
        function: &Node,
        f: &Masked,
        expr: &str,
        at: usize,
    ) -> Result<Option<Resolved<'a>>> {
        let mut parts = expr.split('.');
        let Some(base) = self
            .lookup(function, f, parts.next().unwrap_or(""), at)?
            .flatten()
        else {
            return Ok(None);
        };
        let mut ty = self.resolve(&base.ty, &base.file, 0)?;
        for part in parts {
            let Some(Resolved::Struct(i)) = ty else {
                return Ok(None);
            };
            let mut holder = i;
            let mut field = self.structs[i].fields.iter().find(|f| f.name == part);
            if field.is_none() {
                let mut owners = Vec::new();
                for emb in self.structs[i].fields.iter().filter(|f| f.embedded) {
                    if let Some(Resolved::Struct(j)) =
                        self.resolve(&emb.ty, &self.structs[i].node.file_path, 0)?
                        && self.structs[j].fields.iter().any(|f| f.name == part)
                    {
                        owners.push(j)
                    }
                }
                if owners.len() != 1 {
                    return Ok(None);
                }
                holder = owners[0];
                field = self.structs[holder].fields.iter().find(|f| f.name == part);
            }
            ty = match field {
                Some(field) => self.resolve(&field.ty, &self.structs[holder].node.file_path, 0)?,
                None => None,
            };
        }
        Ok(ty)
    }
    fn method(&self, node: &Node, member: &str) -> Option<&'a Node> {
        let methods: Vec<_> = self
            .ctx
            .children(&node.id)
            .into_iter()
            .filter(|n| n.kind == "method" && n.name == member)
            .collect();
        (methods.len() == 1).then(|| methods[0])
    }
    fn value(
        &self,
        owner: &Node,
        function: Option<&Node>,
        f: &Masked,
        value: &str,
        at: usize,
        line: u32,
    ) -> Result<Option<Outcome<'a>>> {
        let mut value = value.trim();
        if let Some(conv) = patterns().conversion.captures(value)?
            && matching(value, conv.get(0).unwrap().end() - 1) == Some(value.len() - 1)
        {
            let Some((_, _, nodes)) =
                self.named(conv.get(1).unwrap().as_str(), &owner.file_path)?
            else {
                return Ok(None);
            };
            if nodes.is_empty() {
                return Ok(None);
            }
            for node in nodes {
                if !self.func_type(node)? {
                    return Ok(None);
                }
            }
            value = value[conv.get(0).unwrap().end()..value.len() - 1].trim();
        }
        if !patterns().plain.is_match(value)? {
            return Ok(None);
        }
        let name = compact(value);
        let (base, member) = name.split_once('.').unwrap_or((&name, ""));
        let local = match function {
            Some(function) => self.lookup(function, f, base, at)?,
            None => None,
        };
        let refs: HashSet<_> = self
            .ctx
            .outgoing(&owner.id)
            .filter(|e| {
                e.kind == "references"
                    && e.line == Some(line)
                    && e.metadata
                        .as_ref()
                        .is_some_and(|m| m["fnRef"] == true && m["refName"] == name)
            })
            .map(|e| e.target.as_str())
            .collect();
        let ref_fn = if refs.len() == 1 {
            self.ctx
                .node(refs.iter().next().unwrap())
                .filter(|n| function_node(n))
        } else {
            None
        };
        if member.is_empty() {
            if local.is_some() {
                return Ok(None);
            }
            if let Some(node) = ref_fn {
                return Ok((node.kind == "function").then_some(Outcome::Function(node)));
            }
            if !refs.is_empty() {
                return Ok(None);
            }
            let functions: Vec<_> = self
                .ctx
                .named(&name)
                .filter(|n| {
                    n.language == "go"
                        && n.kind == "function"
                        && directory(&n.file_path) == directory(&owner.file_path)
                })
                .collect();
            return Ok((functions.len() == 1).then(|| Outcome::Function(functions[0])));
        }
        if let Some(Some(ty)) = &local {
            match self.resolve(&ty.ty, &ty.file, 0)? {
                Some(Resolved::Interface(node)) => {
                    if let Some(method) = self.method(node, member) {
                        return Ok(Some(Outcome::Function(method)));
                    }
                }
                Some(Resolved::Struct(i)) => {
                    if let Some(holder) = self.field_owner(i, member)? {
                        return Ok(Some(Outcome::Copy(holder, member.to_owned())));
                    }
                    if ref_fn.is_none()
                        && let Some(method) = self.method(self.structs[i].node, member)
                    {
                        return Ok(Some(Outcome::Function(method)));
                    }
                }
                None => {}
            }
        }
        if let Some(node) = ref_fn {
            return Ok(Some(Outcome::Function(node)));
        }
        if local.is_some() || !refs.is_empty() {
            return Ok(None);
        }
        let Some(dir) = self.import_dir(base, &owner.file_path) else {
            return Ok(None);
        };
        let functions: Vec<_> = self
            .ctx
            .named(member)
            .filter(|n| {
                n.language == "go" && n.kind == "function" && directory(&n.file_path) == dir
            })
            .collect();
        Ok((functions.len() == 1).then(|| Outcome::Function(functions[0])))
    }
}
fn function_node(node: &Node) -> bool {
    function(node)
}

pub(crate) fn imports(file: &str, source: &str) -> Result<Vec<ImportBinding>> {
    static DECL: std::sync::LazyLock<fancy_regex::Regex> =
        std::sync::LazyLock::new(|| regex(r"(?m)^(?:func|type|var|const)\b").unwrap());
    static SINGLE: std::sync::LazyLock<fancy_regex::Regex> = std::sync::LazyLock::new(|| {
        regex(r#"\bimport\s+(?:([A-Za-z_]\w*)[ \t]*)?["']([^"']+)["']"#).unwrap()
    });
    static BLOCK: std::sync::LazyLock<fancy_regex::Regex> =
        std::sync::LazyLock::new(|| regex(r"\bimport\s*\(\s*([^)]+)\s*\)").unwrap());
    static LINE: std::sync::LazyLock<fancy_regex::Regex> = std::sync::LazyLock::new(|| {
        regex(r#"(?:([A-Za-z_]\w*|\.)[ \t]*)?["']([^"']+)["']"#).unwrap()
    });
    static VERSION: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^v[0-9]+$").unwrap());
    static ASSUMED: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^[\p{L}\p{Nd}_]*").unwrap());
    let mut end = source.len();
    for m in DECL.find_iter(source) {
        let at = m?.start();
        if source[..at].rfind("/*") <= source[..at].rfind("*/") {
            end = at;
            break;
        }
    }
    let code = comments::strip_for_regex(&source[..end], "go");
    let mut specs = Vec::new();
    let mut add = |captures: fancy_regex::Captures<'_>| {
        specs.push((
            captures.get(1).map(|m| m.as_str().to_owned()),
            captures.get(2).unwrap().as_str().to_owned(),
        ));
    };
    for m in SINGLE.captures_iter(&code) {
        add(m?)
    }
    for m in BLOCK.captures_iter(&code) {
        for line in LINE.captures_iter(m?.get(1).unwrap().as_str()) {
            add(line?)
        }
    }
    let explicit: HashSet<_> = specs
        .iter()
        .filter_map(|(name, _)| name.as_deref().filter(|n| !matches!(*n, "." | "_")))
        .collect();
    let binding = |local_name: String, source: String| ImportBinding {
        file: file.to_owned(),
        local_name,
        source,
        imported_name: "*".into(),
        is_namespace: true,
        is_default: false,
        resolved_path: None,
        mapped_path: None,
        external: false,
    };
    let mut output = Vec::new();
    for (name, source) in &specs {
        let explicitly = name.as_deref().is_some_and(|n| n != ".");
        let local = if explicitly {
            name.as_deref().unwrap()
        } else {
            source.rsplit('/').next().unwrap_or("")
        };
        if explicitly || !explicit.contains(local) {
            output.push(binding(local.to_owned(), source.clone()))
        }
    }
    let bound: HashSet<_> = output.iter().map(|m| m.local_name.clone()).collect();
    let mut assumed: Vec<(String, Vec<String>)> = Vec::new();
    for (name, source) in &specs {
        if name.is_some() {
            continue;
        }
        let elements: Vec<_> = source.split('/').collect();
        let mut base = *elements.last().unwrap();
        if elements.len() > 1 && VERSION.is_match(base) {
            base = elements[elements.len() - 2]
        }
        base = base.strip_prefix("go-").unwrap_or(base);
        let name = ASSUMED.find(base).unwrap().as_str();
        if !name.is_empty() && !bound.contains(name) {
            if let Some((_, sources)) = assumed.iter_mut().find(|(n, _)| n == name) {
                sources.push(source.clone())
            } else {
                assumed.push((name.to_owned(), vec![source.clone()]))
            }
        }
    }
    for (name, sources) in assumed {
        if sources.len() == 1 {
            output.push(binding(name, sources.into_iter().next().unwrap()))
        }
    }
    Ok(output)
}
fn read_fields(f: &Masked, node: &Node) -> Result<Option<Vec<Field>>> {
    let from = f.offset(node.start_line, node.start_column);
    let to = f
        .starts
        .get(node.end_line as usize)
        .copied()
        .unwrap_or(f.text.len());
    if from > to {
        return Ok(None);
    }
    let Some(kw) = patterns().struct_open.find(&f.text[from..to])? else {
        return Ok(None);
    };
    let open = from + kw.end() - 1;
    let Some(close) = matching(&f.text, open) else {
        return Ok(None);
    };
    let mut fields = Vec::new();
    let mut take = |start: usize, end: usize| -> Result<()> {
        let mut decl = f.text[start..end].trim();
        if let Some(quote) = decl.chars().last().filter(|&ch| ch == '`' || ch == '"') {
            decl = decl[..decl.len() - 1]
                .rfind(quote)
                .map_or("", |at| &decl[..at])
                .trim();
        }
        if decl.is_empty() {
            return Ok(());
        }
        if let Some(m) = patterns().named.captures(decl)? {
            fields.push(Field {
                name: m.get(2).or(m.get(1)).unwrap().as_str().to_owned(),
                ty: decl.to_owned(),
                embedded: true,
            });
            return Ok(());
        }
        if let Some(m) = patterns().fields.captures(decl)? {
            let ty = m
                .get(2)
                .unwrap()
                .as_str()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            for name in m.get(1).unwrap().as_str().split(',') {
                fields.push(Field {
                    name: name.trim().to_owned(),
                    ty: ty.clone(),
                    embedded: false,
                });
            }
        }
        Ok(())
    };
    let mut start = open + 1;
    let mut depth = 0i32;
    for (i, byte) in f.text.bytes().enumerate().take(close).skip(open + 1) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'\n' | b';' if depth == 0 => {
                take(start, i)?;
                start = i + 1;
            }
            _ => {}
        }
    }
    take(start, close)?;
    Ok(Some(fields))
}
// English numeric collation is part of which registrations survive a channel cap.
fn natural(a: &str, b: &str) -> Ordering {
    static COLLATOR: std::sync::LazyLock<icu_collator::CollatorBorrowed<'static>> =
        std::sync::LazyLock::new(|| {
            let locale: icu_locale_core::Locale = "en".parse().expect("valid English locale");
            let mut preferences: icu_collator::CollatorPreferences = locale.into();
            preferences.numeric_ordering =
                Some(icu_collator::preferences::CollationNumericOrdering::True);
            icu_collator::Collator::try_new(preferences, Default::default())
                .expect("compiled English collation data")
        });
    COLLATOR.compare(a, b)
}
fn wiring(a: &ValueAt<'_>, b: &ValueAt<'_>) -> Ordering {
    a.test
        .cmp(&b.test)
        .then_with(|| natural(&a.site, &b.site))
        .then_with(|| a.target.id.cmp(&b.target.id))
}
type Channel = (usize, String);
struct Wiring<'a> {
    direct: HashMap<Channel, Vec<ValueAt<'a>>>,
    copies: HashMap<Channel, Vec<(Channel, String)>>,
}
impl<'a> Wiring<'a> {
    fn record(
        &mut self,
        pass: &Pass<'a, '_>,
        into: usize,
        field: &str,
        outcome: Option<Outcome<'a>>,
        file: &str,
        line: u32,
    ) {
        let Some(outcome) = outcome else { return };
        let channel = (into, field.to_owned());
        match outcome {
            Outcome::Copy(holder, field) => {
                let from = (holder, field.clone());
                if from == channel {
                    return;
                }
                let s = &pass.structs[holder];
                let label = format!(
                    "{}{}.{}",
                    if s.dir == pass.structs[into].dir {
                        String::new()
                    } else {
                        format!("{}.", s.pkg)
                    },
                    s.name,
                    field
                );
                let copies = self.copies.entry(channel).or_default();
                if !copies.iter().any(|(key, _)| *key == from) {
                    copies.push((from, label));
                }
            }
            Outcome::Function(target) => {
                let list = self.direct.entry(channel).or_default();
                let value = ValueAt {
                    target,
                    site: format!("{file}:{line}"),
                    test: file.ends_with("_test.go"),
                    via: None,
                };
                if let Some(old) = list.iter_mut().find(|v| v.target.id == target.id) {
                    if wiring(&value, old).is_lt() {
                        *old = value
                    }
                } else {
                    list.push(value)
                }
            }
        }
    }
    fn values(&self, channel: &Channel, cap: usize) -> Vec<ValueAt<'a>> {
        let mut values: HashMap<&str, ValueAt<'a>> = HashMap::new();
        let mut add = |value: ValueAt<'a>| {
            if values
                .get(value.target.id.as_str())
                .is_none_or(|old| wiring(&value, old).is_lt())
            {
                values.insert(&value.target.id, value);
            }
        };
        for value in self.direct.get(channel).into_iter().flatten() {
            add(value.clone())
        }
        for (from, label) in self.copies.get(channel).into_iter().flatten() {
            for value in self.direct.get(from).into_iter().flatten() {
                let mut value = value.clone();
                value.via = Some(label.clone());
                add(value)
            }
        }
        let mut values: Vec<_> = values.into_values().collect();
        values.sort_by(wiring);
        values.truncate(cap);
        values
    }
}
impl<'a, 'c> Pass<'a, 'c> {
    fn read_literal(
        &self,
        wiring: &mut Wiring<'a>,
        i: usize,
        owner: &Node,
        line: u32,
        column: u32,
    ) -> Result<()> {
        let Some(f) = self.files.get(&owner.file_path) else {
            return Ok(());
        };
        let s = &self.structs[i];
        let start = f
            .starts
            .get(line.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(0);
        let end = f.starts.get(line as usize).copied().unwrap_or(f.text.len());
        let pattern = regex(&format!(
            r"(?<![\w.])(?:({IDENT})\s*\.\s*)?{}\s*(?:\[[^\]{{}}]*\])?\s*\{{",
            regex::escape(&s.name)
        ))?;
        let mut open = None;
        let mut nearest = usize::MAX;
        for m in pattern.captures_iter(&f.text[start..end]) {
            self.ctx.step()?;
            let m = m?;
            let dir = match m.get(1) {
                Some(qualifier) => self.import_dir(qualifier.as_str(), &owner.file_path),
                None => Some(directory(&owner.file_path)),
            };
            if dir.as_deref() != Some(s.dir.as_str()) {
                continue;
            }
            let at = m.get(0).unwrap().start();
            let col = f.original[start..start + at].encode_utf16().count();
            let distance = col.abs_diff(column as usize);
            if distance < nearest {
                nearest = distance;
                open = Some(start + m.get(0).unwrap().end() - 1);
            }
        }
        let Some(open) = open else { return Ok(()) };
        let Some(close) = matching(&f.text, open) else {
            return Ok(());
        };
        let items = items(&f.text, open, close);
        let keys = items
            .iter()
            .map(|(value, _)| Ok(patterns().key.captures(value)?))
            .collect::<Result<Vec<_>>>()?;
        let mut fields = Vec::new();
        if keys.iter().all(Option::is_some) {
            for ((value, at), key) in items.iter().zip(&keys) {
                let key = key.as_ref().unwrap();
                let end = key.get(0).unwrap().end();
                fields.push((key.get(1).unwrap().as_str(), &value[end..], at + end));
            }
        } else if keys.iter().all(Option::is_none) && items.len() == s.fields.len() {
            for ((value, at), field) in items.iter().zip(&s.fields) {
                fields.push((field.name.as_str(), *value, *at));
            }
        }
        for (field, value, at) in fields {
            self.ctx.step()?;
            if s.func_fields.contains(field) {
                let line = f.line(at);
                let outcome =
                    self.value(owner, function(owner).then_some(owner), f, value, at, line)?;
                wiring.record(self, i, field, outcome, &owner.file_path, line);
            }
        }
        Ok(())
    }
}
pub(super) fn edges(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    edges_capped(ctx, CALLBACK_CAP)
}
fn edges_capped(ctx: &Context<'_>, cap: usize) -> Result<Vec<Edge>> {
    if !ctx.has(&["go"]) {
        return Ok(Vec::new());
    }
    let mut pass = Pass {
        ctx,
        files: HashMap::new(),
        structs: Vec::new(),
        struct_keys: HashMap::new(),
        specs: HashMap::new(),
        field_names: HashSet::new(),
    };
    for (&path, &source) in ctx.sources {
        if path.ends_with(".go") && !crate::classify::generated_file(path, source)? {
            ctx.steps(source.len() / 64 + 1)?;
            pass.files.insert(path.to_owned(), Masked::new(source)?);
        }
    }
    let mut declarations = Vec::new();
    for kind in ["struct", "type_alias"] {
        let mut nodes: Vec<_> = ctx
            .nodes
            .iter()
            .filter(|n| {
                n.language == "go" && n.kind == kind && pass.files.contains_key(&n.file_path)
            })
            .collect();
        nodes.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then(a.start_line.cmp(&b.start_line))
                .then(a.id.cmp(&b.id))
        });
        declarations.extend(nodes);
    }
    // Group by first encountered file, preserving the declaration query's insertion order.
    let mut files: Vec<(&str, Vec<&Node>)> = Vec::new();
    let mut positions = HashMap::new();
    for node in declarations {
        let index = *positions.entry(node.file_path.as_str()).or_insert_with(|| {
            files.push((&node.file_path, Vec::new()));
            files.len() - 1
        });
        files[index].1.push(node);
    }
    for (path, nodes) in files {
        let f = &pass.files[path];
        let dir = directory(path);
        let pkg = patterns()
            .package
            .captures(&f.text)?
            .and_then(|m| m.get(1).map(|m| m.as_str().to_owned()))
            .unwrap_or_else(|| dir.rsplit('/').next().unwrap_or("").to_owned());
        for node in nodes {
            ctx.step()?;
            if node.kind == "type_alias" {
                let at = f.offset(node.start_line, node.start_column);
                let end = at + byte_at(&f.original[at..], 400);
                if let Some(m) = patterns().spec.captures(&f.text[at..end])? {
                    pass.specs.insert(
                        node.id.clone(),
                        m.get(1).unwrap().as_str().trim().to_owned(),
                    );
                }
                continue;
            }
            let key = (dir.clone(), node.name.clone());
            if pass.struct_keys.contains_key(&key) {
                continue;
            }
            if let Some(fields) = read_fields(f, node)? {
                let index = pass.structs.len();
                pass.struct_keys.insert(key, index);
                pass.structs.push(Struct {
                    name: node.name.clone(),
                    dir: dir.clone(),
                    pkg: pkg.clone(),
                    node,
                    fields,
                    func_fields: HashSet::new(),
                });
            }
        }
    }
    for i in 0..pass.structs.len() {
        let mut names = HashSet::new();
        for field in &pass.structs[i].fields {
            ctx.step()?;
            if !field.embedded && pass.func_field(&field.ty, &pass.structs[i].node.file_path)? {
                names.insert(field.name.clone());
            }
        }
        pass.field_names.extend(names.iter().cloned());
        pass.structs[i].func_fields = names;
    }
    if pass.field_names.is_empty() {
        return Ok(Vec::new());
    }
    let mut literals: HashMap<&str, Vec<(usize, &Node, u32, u32)>> = HashMap::new();
    for (i, s) in pass.structs.iter().enumerate() {
        if s.func_fields.is_empty() {
            continue;
        }
        for decl in pass
            .type_nodes(&s.name, &s.dir)
            .into_iter()
            .filter(|n| n.kind == "struct")
        {
            for edge in ctx.incoming(&decl.id).filter(|e| e.kind == "instantiates") {
                ctx.step()?;
                if let Some(line) = edge.line.filter(|&l| l > 0)
                    && let Some(owner) = ctx.node(&edge.source).filter(|n| n.language == "go")
                {
                    literals.entry(&owner.file_path).or_default().push((
                        i,
                        owner,
                        line,
                        edge.column.unwrap_or(0),
                    ));
                }
            }
        }
    }
    let mut wiring = Wiring {
        direct: HashMap::new(),
        copies: HashMap::new(),
    };
    let mut dispatches = Vec::new();
    for (&path, _) in ctx.sources.iter().filter(|(path, _)| path.ends_with(".go")) {
        let Some(f) = pass.files.get(path) else {
            continue;
        };
        for &(i, owner, line, column) in literals.get(path).into_iter().flatten() {
            pass.read_literal(&mut wiring, i, owner, line, column)?;
        }
        for m in patterns().selector_call.captures_iter(&f.text) {
            ctx.step()?;
            let m = m?;
            let field = m.get(2).unwrap().as_str();
            if !pass.field_names.contains(field) {
                continue;
            }
            let at = m.get(0).unwrap().start();
            let line = f.line(at);
            let Some(function) = ctx.enclosing(path, line)?.filter(|n| n.language == "go") else {
                continue;
            };
            if let Some(Resolved::Struct(i)) =
                pass.expr_type(function, f, &compact(m.get(1).unwrap().as_str()), at)?
                && let Some(holder) = pass.field_owner(i, field)?
            {
                dispatches.push((
                    function,
                    line,
                    (holder, field.to_owned()),
                    format!("{}.{}", pass.structs[holder].name, field),
                ));
            }
        }
        for m in patterns().selector_assign.captures_iter(&f.text) {
            ctx.step()?;
            let m = m?;
            let field = m.get(2).unwrap().as_str();
            if !pass.field_names.contains(field) {
                continue;
            }
            let at = m.get(0).unwrap().start();
            let Some(function) = ctx
                .enclosing(path, f.line(at))?
                .filter(|n| n.language == "go")
            else {
                continue;
            };
            let Some(Resolved::Struct(i)) =
                pass.expr_type(function, f, &compact(m.get(1).unwrap().as_str()), at)?
            else {
                continue;
            };
            let Some(holder) = pass.field_owner(i, field)? else {
                continue;
            };
            let start = m.get(0).unwrap().end();
            let value = &f.text[start..statement_end(&f.text, start)];
            let at = start + value.len() - value.trim_start().len();
            let line = f.line(at);
            let outcome = pass.value(function, Some(function), f, value, at, line)?;
            wiring.record(&pass, holder, field, outcome, path, line);
        }
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let mut values: HashMap<Channel, Vec<ValueAt<'_>>> = HashMap::new();
    for (function, line, channel, field) in dispatches {
        let test = function.file_path.ends_with("_test.go");
        for value in values
            .entry(channel.clone())
            .or_insert_with(|| wiring.values(&channel, cap))
            .iter()
        {
            ctx.step()?;
            if value.target.id == function.id
                || (test
                    && value.test
                    && directory(value.site.rsplit_once(':').map_or("", |v| v.0))
                        != directory(&function.file_path))
                || !seen.insert((&function.id, &value.target.id))
            {
                continue;
            }
            let mut metadata =
                json!({"synthesizedBy":"go-func-field","field":field,"registeredAt":value.site});
            if let Some(via) = &value.via {
                metadata["via"] = via.clone().into();
            }
            ctx.emit(
                &mut output,
                Edge {
                    source: function.id.clone(),
                    target: value.target.id.clone(),
                    kind: "calls".into(),
                    line: Some(line),
                    column: None,
                    provenance: Some("heuristic".into()),
                    metadata: Some(metadata),
                },
            )?;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes_imports_and_all_callback_records_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/go-func-field.json"
        ))
        .unwrap();
        for case in corpus["ordering"].as_array().unwrap() {
            let actual = natural(
                case["left"].as_str().unwrap(),
                case["right"].as_str().unwrap(),
            );
            let sign = match actual {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            };
            assert_eq!(json!(sign), case["expected"], "collation {case}");
        }
        for case in corpus["imports"].as_array().unwrap() {
            let actual: Vec<_> = imports("main.go", case["source"].as_str().unwrap()).unwrap().into_iter().map(|binding|json!({"localName":binding.local_name,"source":binding.source,"exportedName":binding.imported_name,"isDefault":binding.is_default,"isNamespace":binding.is_namespace})).collect();
            assert_eq!(json!(actual), case["expected"], "imports {case}");
        }
        for case in corpus["cases"].as_array().unwrap() {
            let mut nodes = case["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = 0.into();
            }
            let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            let base_edges: Vec<Edge> = serde_json::from_value(case["baseEdges"].clone()).unwrap();
            let sources: BTreeMap<_, _> = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let imports: Vec<ImportBinding> = sources
                .iter()
                .filter(|(path, _)| path.ends_with(".go"))
                .flat_map(|(path, source)| imports(path, source).unwrap())
                .collect();
            let package = |file: &str, source: &str| {
                case["packages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| p["file"] == file && p["source"] == source)
                    .and_then(|p| p["dir"].as_str().map(str::to_owned))
            };
            let mut ctx = Context::new(&nodes, &base_edges, &sources, &[]);
            ctx.go_package_dir = &package;
            for binding in &imports {
                ctx.imports
                    .entry((&binding.file, &binding.local_name))
                    .or_insert(binding);
            }
            let masks: HashMap<_, _> = sources
                .iter()
                .filter(|(path, _)| path.ends_with(".go"))
                .map(|(&path, &source)| (path, Masked::new(source).unwrap()))
                .collect();
            for lookup in case["scopes"].as_array().unwrap() {
                let file = lookup["file"].as_str().unwrap();
                let mask = &masks[file];
                let got = mask
                    .scope
                    .lookup(
                        lookup["name"].as_str().unwrap(),
                        lookup["offset"].as_u64().unwrap() as usize,
                    )
                    .unwrap();
                let actual = got
                    .map(|decl| {
                        let line = mask.line(decl.at);
                        let start = mask.starts[line as usize - 1];
                        let column = mask.original[start..decl.at].encode_utf16().count();
                        let mut result = json!({"line":line,"column":column});
                        if let Some(ty) = decl.ty {
                            result["type"] = ty.into()
                        }
                        result
                    })
                    .unwrap_or(Value::Null);
                assert_eq!(actual, lookup["expected"], "{} scope {lookup}", case["id"]);
            }
            let actual =
                edges_capped(&ctx, case["maxPerChannel"].as_u64().unwrap_or(40) as usize).unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "{}",
                case["id"]
            );
        }
    }
}
