//! C/C++ callback tables, macro registrations, and receiver-typed indirect calls.
use super::*;
use crate::auxiliary_sources::{c_file, includable, include_paths, normalize};
use prometeu_extract::{CfnptrFacts, CfnptrField, CfnptrFileIn, CfnptrStructIn};
mod macros;
const TYPE_KEYWORDS: &[&str] = &[
    "void", "int", "char", "short", "long", "unsigned", "signed", "float", "double", "const",
    "struct", "union", "enum", "static", "volatile", "register", "inline",
];
fn trim(text: &str) -> &str {
    text.trim_matches(crate::search::query_whitespace)
}
fn matching(text: &str, open: usize, left: u8, right: u8) -> Option<usize> {
    let mut depth = 0i32;
    for (i, byte) in text.bytes().enumerate().skip(open) {
        if byte == left {
            depth += 1
        } else if byte == right {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}
fn split(text: &str, separator: u8) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    for (i, byte) in text.bytes().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ if byte == separator && depth == 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}
struct Patterns {
    init: fancy_regex::Regex,
    inline: fancy_regex::Regex,
    inline_after: fancy_regex::Regex,
    array: fancy_regex::Regex,
    dispatch: fancy_regex::Regex,
    array_dispatch: fancy_regex::Regex,
    assignment: fancy_regex::Regex,
    designated: fancy_regex::Regex,
    identifier: fancy_regex::Regex,
    array_designated: fancy_regex::Regex,
    cast: fancy_regex::Regex,
    address: fancy_regex::Regex,
    word: fancy_regex::Regex,
    if_guard: fancy_regex::Regex,
    subscript: fancy_regex::Regex,
    member: fancy_regex::Regex,
    member_end: fancy_regex::Regex,
}
fn patterns() -> &'static Patterns {
    static P: std::sync::LazyLock<Patterns> = std::sync::LazyLock::new(|| {
        let r = |p| regex(p).expect("valid reference pattern");
        Patterns {
            init: r(
                r"(?:^|[;{}])\s*(?:(?:static|const|extern|register|volatile)\s+)*(?:(?:struct|union)\s+)?(\w+)\s+(\w+)\s*(\[[^\]]*\])?\s*=\s*\{",
            ),
            inline: r(r"\b(?:struct|union)\s+(\w+)\s*\{"),
            inline_after: r(r"^\s*(\w+)\s*(\[[^\]]*\])?\s*(=\s*\{)?"),
            array: r(
                r"(?:^|[;{}])\s*(?:(?:static|const|extern|register|volatile)\s+)*(\w+)\s+(\*\s*)?(\w+)\s*\[[^\]]*\]\s*=\s*\{",
            ),
            dispatch: r(r"((?:\w+(?:\s*\[[^\][]*\])?\s*(?:->|\.)\s*)+)(\w+)\s*\)?\s*\("),
            array_dispatch: r(r"(?:\(\s*\*\s*)?\b(\w+)\s*\[[^\][]*\]\s*\)?\s*\("),
            assignment: r(r"(\w+)\s*(?:->|\.)\s*(\w+)\s*=\s*(\w+)\s*(?:->|\.)\s*(\w+)"),
            designated: r(r"^\.\s*(\w+)\s*=\s*(?:&\s*)?(\w+)\s*$"),
            identifier: r(r"^&?\s*(\w+)\s*$"),
            array_designated: r(r"^\[[^\]]*\]\s*=\s*([\s\S]*)$"),
            cast: r(r"^\((?:[\w\s*]+)\)\s*"),
            address: r(r"^&\s*"),
            word: r(r"^(\w+)$"),
            if_guard: r(r"#\s*if"),
            subscript: r(r"\s*\[[^\]]*\]"),
            member: r(r"\s*(?:->|\.)\s*"),
            member_end: r(r"\s*(?:->|\.)\s*$"),
        }
    });
    &P
}
#[derive(Clone)]
struct Field {
    name: String,
    index: u32,
    is_pointer: bool,
    ty: String,
}
#[derive(Default, Clone)]
struct Targets {
    ids: Vec<String>,
    seen: HashSet<String>,
}
impl Targets {
    fn add(&mut self, id: &str) -> bool {
        if self.seen.insert(id.to_owned()) {
            self.ids.push(id.to_owned());
            true
        } else {
            false
        }
    }
}
struct File {
    text: String,
    lines: Vec<usize>,
}
impl File {
    fn new(source: &str) -> Self {
        let text = prometeu_extract::cfnptr_strip_c(source.to_owned());
        let mut lines = vec![0];
        lines.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { text, lines }
    }
    fn slice(&self, node: &Node) -> &str {
        if node.start_line == 0 {
            return "";
        }
        let start = self
            .lines
            .get(node.start_line as usize - 1)
            .copied()
            .unwrap_or(self.text.len());
        let mut end = self
            .lines
            .get(node.end_line.max(node.start_line) as usize)
            .copied()
            .unwrap_or(self.text.len());
        if end > start && self.text.as_bytes()[end - 1] == b'\n' {
            end -= 1;
        }
        &self.text[start..end]
    }
}
struct State<'a, 'c> {
    ctx: &'c Context<'a>,
    files: Vec<String>,
    texts: HashMap<String, File>,
    facts: HashMap<String, CfnptrFacts>,
    includes: HashMap<String, Vec<String>>,
    fn_pointer_types: HashSet<String>,
    fn_types: HashSet<String>,
    layouts: HashMap<String, Vec<Field>>,
    all_fields: HashMap<String, Vec<Vec<Field>>>,
    owners: HashMap<String, HashSet<String>>,
    registrations: HashMap<String, Targets>,
    arrays: HashMap<String, Vec<(String, Targets)>>,
    globals: HashMap<String, String>,
    environments: HashMap<String, macros::Environment>,
    receiver_patterns: RefCell<HashMap<String, fancy_regex::Regex>>,
}
impl<'a, 'c> State<'a, 'c> {
    fn raw(&self, path: &str) -> Option<&'a str> {
        self.ctx
            .sources
            .get(path)
            .or_else(|| self.ctx.auxiliary_sources.get(path))
            .copied()
    }
    fn ensure_text(&mut self, path: &str) {
        if !self.texts.contains_key(path)
            && let Some(source) = self.raw(path)
        {
            self.texts.insert(path.to_owned(), File::new(source));
        }
    }
    fn resolved_include(&self, from: &str, include: &str) -> Option<String> {
        let base = from.rsplit_once('/').map_or("", |v| v.0);
        [normalize(base, include), normalize("", include)]
            .into_iter()
            .flatten()
            .find(|path| self.raw(path).is_some())
    }
    fn local_includes(&mut self, file: &str) -> Vec<String> {
        if let Some(includes) = self.includes.get(file) {
            return includes.clone();
        }
        let includes: Vec<String> = self
            .raw(file)
            .map(|source| {
                include_paths(source)
                    .filter_map(|path| self.resolved_include(file, path))
                    .collect()
            })
            .unwrap_or_default();
        self.includes.insert(file.to_owned(), includes.clone());
        includes
    }
    fn fields(&self, raw: Vec<CfnptrField>) -> Vec<Field> {
        raw.into_iter()
            .map(|f| Field {
                name: f.name.clone(),
                index: f.index,
                is_pointer: !f.name.is_empty()
                    && (f.ptr
                        || !f.ty.is_empty()
                            && (self.fn_pointer_types.contains(&f.ty)
                                || self.fn_types.contains(&f.ty))),
                ty: f.ty,
            })
            .collect()
    }
    fn register_layout(&mut self, name: &str, fields: Vec<Field>) {
        self.all_fields
            .entry(name.to_owned())
            .or_default()
            .push(fields.clone());
        for field in &fields {
            if !field.name.is_empty() && field.is_pointer {
                self.owners
                    .entry(field.name.clone())
                    .or_default()
                    .insert(name.to_owned());
            }
        }
        if fields.iter().any(|field| field.is_pointer) {
            self.layouts.insert(name.to_owned(), fields);
        }
    }
    fn pointer_field(&self, owner: &str, name: &str) -> bool {
        self.layouts.get(owner).is_some_and(|fields| {
            fields
                .iter()
                .any(|field| field.name == name && field.is_pointer)
        })
    }
    fn function(&self, name: &str, file: &str) -> Option<&'a Node> {
        let mut nodes: Vec<_> = self
            .ctx
            .named(name)
            .filter(|n| matches!(n.kind.as_str(), "function" | "method") && c_file(&n.file_path))
            .collect();
        nodes.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then(a.start_line.cmp(&b.start_line))
                .then(a.id.cmp(&b.id))
        });
        nodes
            .iter()
            .copied()
            .find(|node| node.file_path == file)
            .or_else(|| nodes.first().copied())
    }
    fn register_value(
        &mut self,
        owner: &str,
        body: &str,
        file: &str,
        env: &macros::Environment,
    ) -> Result<()> {
        self.ctx
            .steps(self.layouts.get(owner).map_or(0, Vec::len))?;
        let Some(layout) = self.layouts.get(owner).cloned() else {
            return Ok(());
        };
        let expanded = macros::expand(body, env, self.ctx)?;
        let mut body = trim(&expanded);
        if body.starts_with('{')
            && let Some(close) = matching(body, 0, b'{', b'}')
            && close > 0
            && trim(&body[close + 1..]).is_empty()
        {
            body = &body[1..close]
        }
        let mut position = 0;
        for item in split(body, b',')
            .into_iter()
            .map(trim)
            .filter(|s| !s.is_empty())
        {
            self.ctx.step()?;
            if let Some(m) = patterns().designated.captures(item)? {
                let field = m.get(1).unwrap().as_str();
                if self.pointer_field(owner, field)
                    && let Some(node) = self.function(m.get(2).unwrap().as_str(), file)
                {
                    self.registrations
                        .entry(format!("{owner}.{field}"))
                        .or_default()
                        .add(&node.id);
                }
                continue;
            }
            if let Some(field) = layout
                .iter()
                .find(|field| field.index == position && field.is_pointer)
                && let Some(m) = patterns().identifier.captures(item)?
                && let Some(node) = self.function(m.get(1).unwrap().as_str(), file)
            {
                self.registrations
                    .entry(format!("{owner}.{}", field.name))
                    .or_default()
                    .add(&node.id);
            }
            position += 1;
        }
        Ok(())
    }
    fn register_array(
        &mut self,
        name: &str,
        body: &str,
        file: &str,
        env: &macros::Environment,
    ) -> Result<()> {
        let expanded = macros::expand(body, env, self.ctx)?;
        for raw in split(&expanded, b',') {
            self.ctx.step()?;
            let mut item = trim(raw);
            if item.is_empty() {
                continue;
            }
            if let Some(m) = patterns().array_designated.captures(item)? {
                item = trim(m.get(1).unwrap().as_str())
            }
            if let Some(m) = patterns().cast.find(item)? {
                item = &item[m.end()..]
            }
            if let Some(m) = patterns().address.find(item)? {
                item = &item[m.end()..]
            }
            let Some(m) = patterns().word.captures(trim(item))? else {
                continue;
            };
            let Some(node) = self.function(m.get(1).unwrap().as_str(), file) else {
                continue;
            };
            let entries = self.arrays.entry(name.to_owned()).or_default();
            let i = entries
                .iter()
                .position(|entry| entry.0 == file)
                .unwrap_or_else(|| {
                    entries.push((file.to_owned(), Targets::default()));
                    entries.len() - 1
                });
            entries[i].1.add(&node.id);
        }
        Ok(())
    }
    fn environment(
        &mut self,
        file: &str,
        depth: usize,
        seen: &mut HashSet<String>,
        env: &mut macros::Environment,
    ) -> Result<()> {
        if !seen.insert(file.to_owned()) {
            return Ok(());
        }
        self.ctx.step()?;
        if !self.environments.contains_key(file) {
            self.ensure_text(file);
            let value = macros::parse(self.texts.get(file).map_or("", |f| f.text.as_str()))?;
            self.environments.insert(file.to_owned(), value);
        }
        let own = &self.environments[file];
        self.ctx
            .steps(own.functions.len() + own.objects.len() + own.defined.len())?;
        for (key, value) in &own.functions {
            env.functions
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        for (key, value) in &own.objects {
            env.objects
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        env.defined.extend(own.defined.iter().cloned());
        if depth > 0 {
            for include in self.local_includes(file) {
                self.environment(&include, depth - 1, seen, env)?;
            }
        }
        Ok(())
    }
    fn process_init(
        &mut self,
        owner: &str,
        body: &str,
        array: bool,
        file: &str,
        env: &macros::Environment,
    ) -> Result<()> {
        if array {
            for element in split(body, b',') {
                let text = trim(element);
                if text.starts_with('{') {
                    if let Some(close) = matching(text, 0, b'{', b'}')
                        && close > 0
                    {
                        self.register_value(owner, &text[1..close], file, env)?;
                    }
                } else if !text.is_empty() {
                    self.register_value(owner, text, file, env)?;
                }
            }
        } else {
            self.register_value(owner, body, file, env)?;
        }
        Ok(())
    }
    fn unit(&mut self, file: &str, text: &str, env: &macros::Environment) -> Result<()> {
        self.ctx.steps(text.len() / 32 + 1)?;
        if !text.contains('{') {
            return Ok(());
        }
        let mut cursor = 0;
        while let Some(m) = patterns().inline.captures_from_pos(text, cursor)? {
            self.ctx.step()?;
            cursor = m.get(0).unwrap().end();
            let tag = m.get(1).unwrap().as_str();
            let open = cursor - 1;
            let Some(close) = matching(text, open, b'{', b'}') else {
                continue;
            };
            let after = &text[close + 1..];
            let Some(vm) = patterns().inline_after.captures(after)? else {
                continue;
            };
            let fields = self.fields(prometeu_extract::cfnptr_fields(&text[open + 1..close]));
            if !fields.iter().any(|field| field.is_pointer) {
                continue;
            }
            if !self.layouts.contains_key(tag) {
                self.register_layout(tag, fields);
            }
            self.globals
                .insert(vm.get(1).unwrap().as_str().to_owned(), tag.to_owned());
            if vm.get(3).is_some() {
                let open = close + 1 + vm.get(0).unwrap().end() - 1;
                if let Some(end) = matching(text, open, b'{', b'}')
                    && end > 0
                {
                    self.process_init(tag, &text[open + 1..end], vm.get(2).is_some(), file, env)?;
                    cursor = end;
                }
            }
        }
        if !text.contains('=') {
            return Ok(());
        }
        cursor = 0;
        while let Some(m) = patterns().init.captures_from_pos(text, cursor)? {
            self.ctx.step()?;
            cursor = m.get(0).unwrap().end();
            let mut owner = m.get(1).unwrap().as_str().to_owned();
            if !self.layouts.contains_key(&owner) {
                owner = macros::type_name(&owner, env)?;
            }
            if !self.layouts.contains_key(&owner) {
                continue;
            }
            let open = cursor - 1;
            let Some(close) = matching(text, open, b'{', b'}') else {
                continue;
            };
            self.globals
                .insert(m.get(2).unwrap().as_str().to_owned(), owner.clone());
            self.process_init(
                &owner,
                &text[open + 1..close],
                m.get(3).is_some(),
                file,
                env,
            )?;
            cursor = close;
        }
        cursor = 0;
        while let Some(m) = patterns().array.captures_from_pos(text, cursor)? {
            self.ctx.step()?;
            cursor = m.get(0).unwrap().end();
            let elem = m.get(1).unwrap().as_str();
            if !(self.fn_types.contains(elem) && m.get(2).is_some()
                || self.fn_pointer_types.contains(elem))
            {
                continue;
            }
            let open = cursor - 1;
            let Some(close) = matching(text, open, b'{', b'}') else {
                continue;
            };
            self.register_array(
                m.get(3).unwrap().as_str(),
                &text[open + 1..close],
                file,
                env,
            )?;
            cursor = close;
        }
        Ok(())
    }
    fn receiver(&self, source: &str, name: &str, only_pointer: bool) -> Result<Option<String>> {
        if !self.receiver_patterns.borrow().contains_key(name) {
            if self.receiver_patterns.borrow().len() >= 4096 {
                self.receiver_patterns.borrow_mut().clear();
            }
            self.receiver_patterns.borrow_mut().insert(
                name.to_owned(),
                regex(&format!(
                    r"(?:(?:struct|union)\s+)?(\w+)\s*\*?\s*\b{}\b\s*(?:[,)=;]|\[)",
                    regex::escape(name)
                ))?,
            );
        }
        for m in self.receiver_patterns.borrow()[name].captures_iter(source) {
            self.ctx.step()?;
            let m = m?;
            let ty = m.get(1).unwrap().as_str();
            if if only_pointer {
                self.layouts.contains_key(ty)
            } else {
                !TYPE_KEYWORDS.contains(&ty)
            } {
                return Ok(Some(ty.to_owned()));
            }
        }
        Ok(if only_pointer {
            None
        } else {
            self.globals.get(name).cloned()
        })
    }
    fn chain(&self, source: &str, chain: &str) -> Result<Option<String>> {
        let chain = patterns().subscript.replace_all(chain, "");
        let parts = patterns()
            .member
            .split(&chain)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let parts: Vec<_> = parts.into_iter().filter(|s| !s.is_empty()).collect();
        let Some(&base) = parts.first() else {
            return Ok(None);
        };
        let mut ty = self.receiver(source, base, false)?;
        for part in &parts[1..] {
            let Some(owner) = ty else { return Ok(None) };
            ty = self
                .all_fields
                .get(&owner)
                .into_iter()
                .flatten()
                .find_map(|fields| {
                    fields
                        .iter()
                        .find(|field| field.name == *part && !field.ty.is_empty())
                        .map(|field| field.ty.clone())
                });
        }
        Ok(ty)
    }
}
pub(super) fn edges(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let files: Vec<_> = ctx
        .sources
        .keys()
        .filter(|path| c_file(path))
        .map(|path| (*path).to_owned())
        .collect();
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut state = State {
        ctx,
        files,
        texts: HashMap::new(),
        facts: HashMap::new(),
        includes: HashMap::new(),
        fn_pointer_types: HashSet::new(),
        fn_types: HashSet::new(),
        layouts: HashMap::new(),
        all_fields: HashMap::new(),
        owners: HashMap::new(),
        registrations: HashMap::new(),
        arrays: HashMap::new(),
        globals: HashMap::new(),
        environments: HashMap::new(),
        receiver_patterns: RefCell::new(HashMap::new()),
    };
    let mut fields = HashMap::new();
    let mut inline_tags = HashSet::new();
    let mut alias_names = HashSet::new();
    for file in state.files.clone() {
        let source = state.raw(&file).unwrap();
        if source.is_empty() {
            continue;
        }
        ctx.steps(source.len() / 32 + 1)?;
        let structs = ctx
            .in_file(&file)
            .filter(|n| matches!(n.kind.as_str(), "struct" | "union"))
            .map(|node| CfnptrStructIn {
                id: node.id.clone(),
                start_line: node.start_line,
                end_line: node.end_line,
            })
            .collect();
        let facts = prometeu_extract::cfnptr_scan_files(vec![CfnptrFileIn {
            text: source.to_owned(),
            structs,
        }])
        .pop()
        .unwrap();
        state
            .fn_pointer_types
            .extend(facts.fn_ptr_typedefs.iter().cloned());
        state
            .fn_types
            .extend(facts.fn_type_typedefs.iter().cloned());
        for st in &facts.structs {
            if st.parsed {
                fields.insert(st.id.clone(), st.fields.clone());
            }
        }
        inline_tags.extend(facts.inline_tags.iter().cloned());
        alias_names.extend(facts.alias_names.iter().cloned());
        let includes = facts
            .includes
            .iter()
            .filter(|path| includable(path))
            .filter_map(|path| state.resolved_include(&file, path))
            .collect();
        state.includes.insert(file.clone(), includes);
        state.facts.insert(file, facts);
    }
    for kind in ["struct", "union"] {
        let mut nodes: Vec<_> = ctx
            .nodes
            .iter()
            .filter(|n| n.kind == kind && c_file(&n.file_path))
            .collect();
        nodes.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then(a.start_line.cmp(&b.start_line))
                .then(a.id.cmp(&b.id))
        });
        for node in nodes {
            ctx.step()?;
            if let Some(fields) = fields.remove(&node.id) {
                let fields = state.fields(fields);
                state.register_layout(&node.name, fields);
            }
        }
    }
    let mut seen_includes = HashSet::new();
    let indexed: HashSet<_> = state.files.iter().cloned().collect();
    for file in state.files.clone() {
        ctx.step()?;
        let Some(facts) = state.facts.get(&file) else {
            continue;
        };
        let typedef =
            |name: &String| state.fn_pointer_types.contains(name) || state.fn_types.contains(name);
        let survives = facts.inline_ptr
            || facts.inline_types.iter().any(typedef)
            || facts.init_tokens.iter().any(|name| {
                state.layouts.contains_key(name)
                    || inline_tags.contains(name)
                    || alias_names.contains(name)
            })
            || facts.array_elems.iter().any(|elem| {
                if let Some(elem) = elem.strip_prefix('*') {
                    state.fn_pointer_types.contains(elem) || state.fn_types.contains(elem)
                } else {
                    state.fn_pointer_types.contains(elem)
                }
            });
        let includes = state.local_includes(&file);
        if !survives && includes.is_empty() {
            continue;
        }
        let mut env = macros::Environment::default();
        state.environment(&file, 2, &mut HashSet::new(), &mut env)?;
        if survives {
            state.ensure_text(&file);
            let text = state
                .texts
                .get(&file)
                .map_or(String::new(), |f| f.text.clone());
            state.unit(&file, &text, &env)?;
        }
        for target in includes {
            if seen_includes.contains(&(file.clone(), target.clone())) {
                continue;
            }
            state.ensure_text(&target);
            let Some(text) = state
                .texts
                .get(&target)
                .map(|file| file.text.clone())
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            if indexed.contains(&target) {
                let own = macros::parse(&text)?;
                if !env.defined.iter().any(|name| !own.defined.contains(name))
                    || !patterns().if_guard.is_match(&text)?
                {
                    continue;
                }
            }
            seen_includes.insert((file.clone(), target.clone()));
            let text = macros::conditionals(&text, &env.defined)?;
            let own = macros::parse(&text)?;
            let mut env = env.clone();
            env.functions.extend(own.functions);
            env.objects.extend(own.objects);
            state.unit(&target, &text, &env)?;
        }
    }
    let mut propagation = Vec::new();
    for file in &state.files {
        ctx.step()?;
        let Some(facts) = state.facts.get(file) else {
            continue;
        };
        if !facts.d_pairs.iter().any(|pair| {
            pair.split_once('\0').is_some_and(|(left, right)| {
                state.owners.contains_key(left) && state.owners.contains_key(right)
            })
        }) {
            continue;
        }
        let source = state.raw(file).unwrap_or("");
        let text = File::new(source);
        let mut functions: Vec<_> = ctx
            .in_file(file)
            .filter(|n| matches!(n.kind.as_str(), "function" | "method"))
            .collect();
        functions.sort_by(|a, b| a.start_line.cmp(&b.start_line).then(a.id.cmp(&b.id)));
        for function in functions {
            let body = text.slice(function);
            ctx.steps(body.len() / 32 + 1)?;
            for m in patterns().assignment.captures_iter(body) {
                let m = m?;
                let left = m.get(2).unwrap().as_str();
                let right = m.get(4).unwrap().as_str();
                if !state.owners.contains_key(left) || !state.owners.contains_key(right) {
                    continue;
                }
                let lt = state.receiver(body, m.get(1).unwrap().as_str(), true)?;
                let rt = state.receiver(body, m.get(3).unwrap().as_str(), true)?;
                if let (Some(lt), Some(rt)) = (lt, rt)
                    && state.pointer_field(&lt, left)
                    && state.pointer_field(&rt, right)
                {
                    propagation.push((format!("{lt}.{left}"), format!("{rt}.{right}")));
                }
            }
        }
    }
    for _ in 0..3 {
        let mut changed = false;
        for (to, from) in &propagation {
            ctx.step()?;
            let Some(from) = state.registrations.get(from).cloned() else {
                continue;
            };
            let target = state.registrations.entry(to.clone()).or_default();
            for id in &from.ids {
                ctx.step()?;
                changed |= target.add(id);
            }
        }
        if !changed {
            break;
        }
    }
    if state.registrations.is_empty() && state.arrays.is_empty() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for file in &state.files {
        ctx.step()?;
        let Some(facts) = state.facts.get(file) else {
            continue;
        };
        if !facts
            .dispatch_fields
            .iter()
            .any(|f| state.owners.contains_key(f))
            && !facts
                .array_dispatch_names
                .iter()
                .any(|name| state.arrays.contains_key(name))
        {
            continue;
        }
        let text = File::new(state.raw(file).unwrap_or(""));
        let mut functions: Vec<_> = ctx
            .in_file(file)
            .filter(|n| matches!(n.kind.as_str(), "function" | "method"))
            .collect();
        functions.sort_by(|a, b| a.start_line.cmp(&b.start_line).then(a.id.cmp(&b.id)));
        for function in functions {
            let body = text.slice(function);
            ctx.steps(body.len() / 32 + 1)?;
            let mut added = 0;
            let mut cursor = 0;
            let mut line = function.start_line;
            for m in patterns().dispatch.captures_iter(body) {
                if added >= 300 {
                    break;
                }
                let m = m?;
                ctx.step()?;
                let base = patterns()
                    .member_end
                    .replace(m.get(1).unwrap().as_str(), "");
                let base = trim(&base);
                let field = m.get(2).unwrap().as_str();
                let Some(owners) = state.owners.get(field).filter(|owners| !owners.is_empty())
                else {
                    continue;
                };
                let mut owner = state.chain(body, base)?;
                if owner.as_ref().is_none_or(|name| !owners.contains(name)) {
                    let chain = patterns().subscript.replace_all(base, "");
                    let parts = patterns()
                        .member
                        .split(&chain)
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    owner = state
                        .receiver(body, parts.last().copied().unwrap_or(""), true)?
                        .filter(|name| owners.contains(name));
                }
                if owner.as_ref().is_none_or(|name| !owners.contains(name)) {
                    owner = (owners.len() == 1).then(|| owners.iter().next().unwrap().clone())
                }
                let Some(owner) = owner else { continue };
                let Some(targets) = state.registrations.get(&format!("{owner}.{field}")) else {
                    continue;
                };
                let at = m.get(0).unwrap().start();
                line += body[cursor..at].bytes().filter(|&b| b == b'\n').count() as u32;
                cursor = at;
                for target in &targets.ids {
                    if target == &function.id || !seen.insert((function.id.clone(), target.clone()))
                    {
                        continue;
                    }
                    emit(
                        ctx,
                        &mut output,
                        function,
                        target,
                        line,
                        &format!("{owner}.{field}"),
                    )?;
                    added += 1;
                    if added >= 300 {
                        break;
                    }
                }
            }
            if state.arrays.is_empty() || added >= 300 {
                continue;
            }
            cursor = 0;
            line = function.start_line;
            for m in patterns().array_dispatch.captures_iter(body) {
                if added >= 300 {
                    break;
                }
                let m = m?;
                ctx.step()?;
                let name = m.get(1).unwrap().as_str();
                let Some(entries) = state.arrays.get(name) else {
                    continue;
                };
                let entry = if entries.len() == 1 {
                    entries.first()
                } else {
                    entries.iter().find(|entry| entry.0 == function.file_path)
                };
                let Some((_, targets)) = entry else { continue };
                let at = m.get(0).unwrap().start();
                line += body[cursor..at].bytes().filter(|&b| b == b'\n').count() as u32;
                cursor = at;
                for target in &targets.ids {
                    if target == &function.id || !seen.insert((function.id.clone(), target.clone()))
                    {
                        continue;
                    }
                    emit(
                        ctx,
                        &mut output,
                        function,
                        target,
                        line,
                        &format!("{name}[]"),
                    )?;
                    added += 1;
                    if added >= 300 {
                        break;
                    }
                }
            }
        }
    }
    Ok(output)
}
fn emit(
    ctx: &Context<'_>,
    output: &mut Vec<Edge>,
    function: &Node,
    target: &str,
    line: u32,
    via: &str,
) -> Result<()> {
    ctx.emit(output,Edge{source:function.id.clone(),target:target.to_owned(),kind:"calls".into(),line:Some(line),column:None,provenance:Some("heuristic".into()),metadata:Some(json!({"synthesizedBy":"fn-pointer-dispatch","via":via,"registeredAt":format!("{}:{line}",function.file_path)}))})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_function_pointer_records_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/c-fnptr.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let mut nodes = case["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = 0.into();
            }
            let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            let base: Vec<Edge> = serde_json::from_value(case["baseEdges"].clone()).unwrap();
            let sources: BTreeMap<_, _> = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let auxiliary: BTreeMap<_, _> = case["auxiliary"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let mut ctx = Context::new(&nodes, &base, &sources, &[]);
            ctx.auxiliary_sources = &auxiliary;
            assert_eq!(
                serde_json::to_value(edges(&ctx).unwrap()).unwrap(),
                case["expected"],
                "{}",
                case["id"]
            );
        }
    }
}
