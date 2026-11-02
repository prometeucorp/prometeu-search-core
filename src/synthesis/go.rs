//! Go method sets, implicit interface satisfaction, and unambiguous promotion.
use super::{CALLBACK_CAP, Context, Edge, Node, Result};
use prometeu_extract::UnresolvedReference;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

pub(super) fn method_key(node: &Node) -> String {
    keyed_method(&node.name, &super::directory(&node.file_path))
}
fn keyed_method(name: &str, package: &str) -> String {
    static UPPER: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^\p{Lu}").unwrap());
    if UPPER.is_match(name) {
        name.into()
    } else {
        format!("{package}\0{name}")
    }
}
fn go_type(node: &Node) -> bool {
    node.language == "go" && matches!(node.kind.as_str(), "struct" | "interface" | "type_alias")
}
fn embedding(ctx: &Context<'_>, edge: &Edge) -> bool {
    edge.provenance.as_deref() != Some("heuristic")
        && matches!(edge.kind.as_str(), "extends" | "implements")
        && ctx.node(&edge.target).is_some_and(go_type)
        && (ctx
            .node(&edge.source)
            .is_none_or(|node| node.kind != "interface")
            || ctx
                .node(&edge.target)
                .is_some_and(|node| node.kind == "interface"))
}

fn list_items(text: &str, open: usize) -> Option<(Vec<String>, usize)> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut item = Vec::new();
    let mut items = Vec::new();
    let mut at = open;
    while at < bytes.len() {
        let ch = bytes[at];
        if ch == b'/' && matches!(bytes.get(at + 1), Some(b'/' | b'*')) {
            at = if bytes[at + 1] == b'/' {
                at + text[at..].find('\n')?
            } else {
                at + 2 + text[at + 2..].find("*/")? + 1
            };
            item.push(b' ');
            at += 1;
            continue;
        }
        if matches!(ch, b'"' | b'`' | b'\'') {
            let start = at;
            at += 1;
            while at < bytes.len() && bytes[at] != ch {
                at += if ch != b'`' && bytes[at] == b'\\' {
                    2
                } else {
                    1
                };
            }
            if at >= bytes.len() {
                return None;
            }
            item.extend_from_slice(&bytes[start..=at]);
            at += 1;
            continue;
        }
        if matches!(ch, b'(' | b'[' | b'{') {
            depth += 1;
            if depth == 1 {
                at += 1;
                continue;
            }
        } else if matches!(ch, b')' | b']' | b'}') {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                let value = String::from_utf8(item).ok()?;
                if !value.trim().is_empty() {
                    items.push(value.trim().into());
                }
                return Some((items, at));
            }
        } else if ch == b',' && depth == 1 {
            let value = String::from_utf8(std::mem::take(&mut item)).ok()?;
            if !value.trim().is_empty() {
                items.push(value.trim().into());
            }
            at += 1;
            continue;
        }
        item.push(ch);
        at += 1;
    }
    None
}
fn signature_parts(signature: Option<&str>) -> Option<(Vec<String>, Vec<String>)> {
    let signature = signature?.trim();
    if !signature.starts_with('(') {
        return None;
    }
    let (params, end) = list_items(signature, 0)?;
    let rest = signature[end + 1..].trim();
    if !rest.starts_with('(') {
        return Some((
            params,
            if rest.is_empty() {
                Vec::new()
            } else {
                vec![rest.into()]
            },
        ));
    }
    let (results, end) = list_items(rest, 0)?;
    rest[end + 1..]
        .trim()
        .is_empty()
        .then_some((params, results))
}
pub(super) fn signature_arity(signature: Option<&str>) -> Option<String> {
    signature_parts(signature)
        .map(|(params, results)| format!("{}/{}", params.len(), results.len()))
}
fn item_types(items: &[String]) -> Vec<String> {
    static NAMED: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\s+(\S[\s\S]*)$").unwrap());
    let named: Vec<_> = items
        .iter()
        .map(|item| {
            NAMED.captures(item).and_then(|c| {
                (!["func", "chan", "map", "struct", "interface"].contains(&&c[1]))
                    .then(|| c[2].to_string())
            })
        })
        .collect();
    if named.iter().all(Option::is_none) {
        return items.to_vec();
    }
    let mut next = String::new();
    let mut types = vec![String::new(); items.len()];
    for (i, name) in named.into_iter().enumerate().rev() {
        if let Some(name) = name {
            next = name;
        }
        types[i] = next.clone();
    }
    types
}
fn comparable_type(written: &str) -> Option<String> {
    static NESTED: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?-u:\b)(?:func\(|struct\{|interface\{)").unwrap());
    static QUALIFIER: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"[A-Za-z_][A-Za-z0-9_]*\.").unwrap());
    static BYTE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?-u:\b)uint8(?-u:\b)").unwrap());
    static RUNE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?-u:\b)int32(?-u:\b)").unwrap());
    let flat = written
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .replace("interface{}", "any");
    if NESTED.is_match(&flat) {
        return None;
    }
    let flat = QUALIFIER.replace_all(&flat, "");
    let flat = BYTE.replace_all(&flat, "byte");
    Some(RUNE.replace_all(&flat, "rune").into_owned())
}
fn method_types(signature: Option<&str>) -> Option<Vec<Option<String>>> {
    let (params, results) = signature_parts(signature)?;
    Some(
        item_types(&params)
            .into_iter()
            .chain(item_types(&results))
            .map(|t| comparable_type(&t))
            .collect(),
    )
}
fn grpc_arity(signature: Option<&str>) -> Option<String> {
    static OPTIONS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^(?:[A-Za-z_][A-Za-z0-9_]*\s+)?\.\.\.\s*(?:grpc\.)?CallOption$")
            .unwrap()
    });
    let (params, results) = signature_parts(signature)?;
    if !OPTIONS.is_match(params.last()?) {
        return None;
    }
    Some(format!(
        "{}/{}",
        params.len() - 1,
        if results.len() == 2 && results[0].starts_with('*') {
            2
        } else {
            1
        }
    ))
}
fn declaration(node: &Node, ctx: &Context<'_>) -> String {
    ctx.sources
        .get(node.file_path.as_str())
        .map_or(String::new(), |source| {
            source
                .lines()
                .skip(node.start_line.saturating_sub(1) as usize)
                .take(3)
                .collect::<Vec<_>>()
                .join("\n")
        })
}
fn parameters(node: &Node, ctx: &Context<'_>) -> Vec<String> {
    static OPEN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^\s*(?:type\s+)?[A-Za-z_][A-Za-z0-9_]*\s*\[").unwrap()
    });
    let text = declaration(node, ctx);
    let Some(open) = OPEN.find(&text) else {
        return Vec::new();
    };
    list_items(&text, open.end() - 1).map_or(Vec::new(), |(items, _)| {
        items
            .into_iter()
            .filter_map(|item| {
                let name = item
                    .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .next()?;
                (!name.is_empty()).then(|| name.to_owned())
            })
            .collect()
    })
}
fn true_alias(node: &Node, ctx: &Context<'_>) -> bool {
    static ALIAS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^\s*(?:type\s+)?[A-Za-z_][A-Za-z0-9_]*\s*(?:\[[^\]]*\]\s*)?=").unwrap()
    });
    ALIAS.is_match(&declaration(node, ctx))
}

#[derive(Default, Clone)]
struct Methods {
    names: HashSet<String>,
    opaque: HashSet<String>,
    arities: HashMap<String, Option<BTreeSet<String>>>,
    types: HashMap<String, Option<Vec<Vec<Option<String>>>>>,
    parameters: HashSet<String>,
}
#[derive(Default)]
struct Outside {
    known: Vec<BTreeMap<String, String>>,
    opaque: Vec<String>,
}
struct Model<'a> {
    ctx: &'a Context<'a>,
    own: HashMap<&'a str, BTreeMap<String, Vec<Option<&'a str>>>>,
    embeds: HashMap<&'a str, Vec<&'a str>>,
    outside: HashMap<String, Outside>,
    memo: HashMap<String, Methods>,
}
impl<'a> Model<'a> {
    fn new(ctx: &'a Context<'a>) -> Result<Self> {
        let mut own = HashMap::new();
        let mut embeds = HashMap::new();
        for node in ctx.nodes.iter().filter(|node| go_type(node)) {
            ctx.step()?;
            let mut names: BTreeMap<String, Vec<Option<&str>>> = BTreeMap::new();
            for method in ctx.methods(&node.id) {
                ctx.step()?;
                names
                    .entry(method_key(method))
                    .or_default()
                    .push(method.signature.as_deref());
            }
            own.insert(node.id.as_str(), names);
            embeds.insert(
                node.id.as_str(),
                ctx.outgoing(&node.id)
                    .filter(|edge| embedding(ctx, edge))
                    .map(|edge| edge.target.as_str())
                    .collect(),
            );
        }
        let mut import_nodes: HashMap<&str, Vec<&Node>> = HashMap::new();
        for node in ctx
            .nodes
            .iter()
            .filter(|node| node.language == "go" && node.kind == "import")
        {
            ctx.step()?;
            import_nodes.entry(&node.file_path).or_default().push(node);
        }
        let imports_by_file: HashMap<_, _> = import_nodes
            .into_iter()
            .map(|(path, nodes)| (path, imports(&nodes)))
            .collect();
        let mut outside = HashMap::<String, Outside>::new();
        for reference in ctx.unresolved {
            if reference.reference_kind != "extends"
                || reference.language.as_deref() != Some("go")
                || reference.file_path.is_none()
                || !own.contains_key(reference.from_node_id.as_str())
            {
                continue;
            }
            ctx.step()?;
            let path = reference.file_path.as_deref().unwrap();
            let (written, imported) = qualification(ctx, reference, imports_by_file.get(path))?;
            let known = if imported.is_some() {
                stdlib(imported.as_deref(), &reference.reference_name)
            } else if written.is_none() {
                stdlib(None, &reference.reference_name)
            } else {
                None
            };
            let row = outside.entry(reference.from_node_id.clone()).or_default();
            if let Some(known) = known {
                row.known.push(
                    known
                        .into_iter()
                        .map(|(name, arity)| {
                            (
                                keyed_method(
                                    &name,
                                    &format!("std:{}", imported.as_deref().unwrap_or("")),
                                ),
                                arity,
                            )
                        })
                        .collect(),
                );
            } else {
                row.opaque.push(if let Some(imported) = imported {
                    format!("{imported}.{}", reference.reference_name)
                } else if let Some(written) = written {
                    format!("{written}.{}", reference.reference_name)
                } else {
                    format!("{}#{}", super::directory(path), reference.reference_name)
                });
            }
        }
        Ok(Self {
            ctx,
            own,
            embeds,
            outside,
            memo: HashMap::new(),
        })
    }
    fn closure(&self, id: &'a str) -> Result<Vec<&'a str>> {
        let mut ordered = vec![id];
        let mut seen = HashSet::from([id]);
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            for &target in self.embeds.get(id).into_iter().flatten() {
                self.ctx.step()?;
                if seen.insert(target) {
                    ordered.push(target);
                    pending.push(target);
                }
            }
        }
        Ok(ordered)
    }
    fn methods(&mut self, id: &'a str) -> Result<Methods> {
        if let Some(methods) = self.memo.get(id) {
            return Ok(methods.clone());
        }
        let mut methods = Methods::default();
        for target in self.closure(id)? {
            self.ctx.step()?;
            if let Some(node) = self.ctx.node(target) {
                methods.parameters.extend(parameters(node, self.ctx));
            }
            for (key, signatures) in &self.own[target] {
                methods.names.insert(key.clone());
                for signature in signatures {
                    let known = methods
                        .arities
                        .entry(key.clone())
                        .or_insert_with(|| Some(BTreeSet::new()));
                    if let Some(arities) = known {
                        if let Some(arity) = signature_arity(*signature) {
                            arities.insert(arity);
                        } else {
                            *known = None;
                        }
                    }
                    let known = methods
                        .types
                        .entry(key.clone())
                        .or_insert_with(|| Some(Vec::new()));
                    if let Some(types) = known {
                        if let Some(read) = method_types(*signature) {
                            types.push(read);
                        } else {
                            *known = None;
                        }
                    }
                }
            }
            if let Some(outside) = self.outside.get(target) {
                methods.opaque.extend(outside.opaque.iter().cloned());
                for known in &outside.known {
                    for (key, arity) in known {
                        methods.names.insert(key.clone());
                        let known = methods
                            .arities
                            .entry(key.clone())
                            .or_insert_with(|| Some(BTreeSet::new()));
                        if let Some(known) = known {
                            known.insert(arity.clone());
                        }
                        methods.types.insert(key.clone(), None);
                    }
                }
            }
        }
        self.memo.insert(id.into(), methods.clone());
        Ok(methods)
    }
    fn wanted_arities(
        &mut self,
        id: &'a str,
        own: &Methods,
    ) -> Result<HashMap<String, Option<BTreeSet<String>>>> {
        let mut wanted = HashMap::new();
        for target in self.closure(id)? {
            for (name, signatures) in &self.own[target] {
                let entry = wanted.entry(name.clone()).or_insert_with(|| {
                    Some(
                        own.arities
                            .get(name)
                            .and_then(Clone::clone)
                            .unwrap_or_default(),
                    )
                });
                for signature in signatures {
                    let Some(served) = grpc_arity(*signature) else {
                        return Ok(own.arities.clone());
                    };
                    entry.as_mut().unwrap().insert(served);
                }
            }
        }
        for (name, arities) in &own.arities {
            wanted
                .entry(name.clone())
                .or_insert_with(|| arities.clone());
        }
        Ok(wanted)
    }
}

pub(super) fn implements(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["go"]) {
        return Ok(Vec::new());
    }
    let mut model = Model::new(ctx)?;
    let mut types: Vec<_> = ctx
        .nodes
        .iter()
        .filter(|node| {
            node.language == "go" && matches!(node.kind.as_str(), "struct" | "type_alias")
        })
        .collect();
    let mut interfaces: Vec<_> = ctx
        .nodes
        .iter()
        .filter(|node| node.language == "go" && node.kind == "interface")
        .collect();
    types.sort_by_key(|node| {
        (
            &node.file_path,
            node.start_line,
            node.start_column,
            &node.id,
        )
    });
    interfaces.sort_by_key(|node| {
        (
            &node.file_path,
            node.start_line,
            node.start_column,
            &node.id,
        )
    });
    let aliases: HashSet<_> = types
        .iter()
        .filter(|node| node.kind == "type_alias" && true_alias(node, ctx))
        .map(|node| node.name.clone())
        .collect();
    let mut candidates = Vec::new();
    for node in types {
        let methods = model.methods(&node.id)?;
        if !methods.names.is_empty() || !methods.opaque.is_empty() {
            candidates.push((node, methods));
        }
    }
    let mut edges = Vec::new();
    for interface in interfaces {
        ctx.step()?;
        let want = model.methods(&interface.id)?;
        if want.names.is_empty() && want.opaque.is_empty() {
            continue;
        }
        let arities = model.wanted_arities(&interface.id, &want)?;
        let mut immediate = Vec::new();
        let mut through = Vec::new();
        let mut covering = 0;
        let mut sole = None;
        for (node, have) in &candidates {
            ctx.step()?;
            if immediate.len() >= CALLBACK_CAP {
                break;
            }
            if !have.names.is_superset(&want.names) {
                continue;
            }
            if arities.iter().any(|(name, want)| {
                match (want, have.arities.get(name).and_then(Option::as_ref)) {
                    (Some(want), Some(have)) => want.is_disjoint(have),
                    _ => false,
                }
            }) {
                continue;
            }
            let open = |ty: &Option<String>| {
                ty.as_ref().is_none_or(|ty| {
                    ty.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                        .any(|word| {
                            aliases.contains(word)
                                || want.parameters.contains(word)
                                || have.parameters.contains(word)
                        })
                })
            };
            let fits = want.types.iter().all(|(name, want)| {
                match (want, have.types.get(name).and_then(Option::as_ref)) {
                    (Some(want), Some(have)) => want.iter().any(|want| {
                        have.iter().any(|have| {
                            have.len() != want.len()
                                || have
                                    .iter()
                                    .zip(want)
                                    .all(|(a, b)| a == b || open(a) || open(b))
                        })
                    }),
                    _ => true,
                }
            });
            if !fits {
                continue;
            }
            covering += 1;
            if model
                .embeds
                .get(node.id.as_str())
                .is_some_and(|targets| targets.contains(&interface.id.as_str()))
            {
                continue;
            }
            if !have.opaque.is_superset(&want.opaque) {
                if covering == 1 {
                    sole = Some(*node);
                }
                continue;
            }
            if !want.opaque.is_empty()
                || !want
                    .names
                    .iter()
                    .all(|key| model.own[node.id.as_str()].contains_key(key))
            {
                through.push(*node);
            } else {
                immediate.push(*node);
            }
        }
        if let Some(node) = sole.filter(|_| covering == 1 && !want.names.is_empty()) {
            through.push(node);
        }
        let mut seen = HashSet::new();
        for node in immediate
            .into_iter()
            .chain(through)
            .filter(|node| seen.insert(node.id.as_str()))
            .take(CALLBACK_CAP)
        {
            ctx.emit(&mut edges,Edge{source:node.id.clone(),target:interface.id.clone(),kind:"implements".into(),line:Some(node.start_line),column:None,provenance:Some("heuristic".into()),metadata:Some(json!({"synthesizedBy":"go-implements","via":interface.name,"registeredAt":format!("{}:{}",node.file_path,node.start_line)}))})?;
        }
    }
    Ok(edges)
}

fn stdlib(package: Option<&str>, name: &str) -> Option<BTreeMap<String, String>> {
    static TABLE: LazyLock<BTreeMap<String, BTreeMap<String, String>>> =
        LazyLock::new(|| serde_json::from_str(include_str!("go_stdlib.json")).unwrap());
    let Some(package) = package else {
        return (name == "error").then(|| BTreeMap::from([("Error".into(), "0/1".into())]));
    };
    Some(
        TABLE
            .get(package)?
            .get(name)?
            .split(' ')
            .filter_map(|entry| {
                entry
                    .split_once(':')
                    .map(|(name, arity)| (name.into(), arity.into()))
            })
            .collect(),
    )
}

fn qualification(
    ctx: &Context<'_>,
    reference: &UnresolvedReference,
    imports: Option<&HashMap<String, String>>,
) -> Result<(Option<String>, Option<String>)> {
    let name = reference.reference_name.rsplit('.').next().unwrap_or("");
    if name.is_empty()
        || !name.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_')
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Ok((None, None));
    }
    let path = reference.file_path.as_deref().unwrap_or("");
    let written = if let Some((head, _)) = reference.reference_name.split_once('.') {
        (head.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_')
            && head
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'))
        .then(|| head.to_owned())
    } else {
        let line = ctx
            .line(path, reference.line)
            .unwrap_or("")
            .trim_end_matches(['\r', '\n']);
        let at = line
            .char_indices()
            .scan(0, |units, (at, ch)| {
                let old = *units;
                *units += ch.len_utf16();
                Some((at, old))
            })
            .find(|(_, units)| *units == reference.column as usize)
            .map_or(line.len(), |(at, _)| at);
        if line.get(at..).is_some_and(|tail| tail.starts_with(name)) {
            super::regex(r"(?:^|[^\w.]|\.{3})([A-Za-z_]\w*)\.$")?
                .captures(&line[..at])?
                .map(|c| c[1].into())
        } else if !super::regex(&format!(r"(?<![\w.]){}\b", regex::escape(name)))?.is_match(line)? {
            super::regex(&format!(
                r"(?:^|[^\w.]|\.{{3}})([A-Za-z_]\w*)\.{}\b",
                regex::escape(name)
            ))?
            .captures(line)?
            .map(|c| c[1].into())
        } else {
            None
        }
    };
    let imported = written
        .as_ref()
        .and_then(|written| imports.and_then(|imports| imports.get(written)))
        .cloned();
    Ok((written, imported))
}
fn imports(nodes: &[&Node]) -> HashMap<String, String> {
    let specs: Vec<_> = nodes
        .iter()
        .map(|node| {
            let alias = node
                .signature
                .as_deref()
                .and_then(|signature| {
                    signature
                        .find(['"', '\'', '`'])
                        .map(|at| signature[..at].trim())
                })
                .filter(|alias| !alias.is_empty())
                .map(str::to_owned);
            (alias, node.name.clone())
        })
        .collect();
    let explicit: HashSet<_> = specs
        .iter()
        .filter_map(|(alias, _)| {
            alias
                .as_deref()
                .filter(|alias| !matches!(*alias, "." | "_"))
        })
        .collect();
    let mut imports = HashMap::new();
    for (alias, path) in &specs {
        let name = alias
            .as_deref()
            .filter(|alias| *alias != ".")
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
        if (alias.is_some() && alias.as_deref() != Some(".")) || !explicit.contains(name) {
            imports
                .entry(name.to_owned())
                .or_insert_with(|| path.clone());
        }
    }
    let mut assumed: HashMap<String, Vec<&str>> = HashMap::new();
    for (alias, path) in &specs {
        if alias.is_some() {
            continue;
        }
        let mut parts = path.rsplit('/');
        let last = parts.next().unwrap_or(path);
        let base = if last.strip_prefix('v').is_some_and(|version| {
            !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit())
        }) {
            parts.next().unwrap_or(last)
        } else {
            last
        };
        let base = base.strip_prefix("go-").unwrap_or(base);
        let name = base
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
            .collect::<String>();
        if !name.is_empty() && !imports.contains_key(&name) {
            assumed.entry(name).or_default().push(path);
        }
    }
    for (name, paths) in assumed {
        if paths.len() == 1 {
            imports.insert(name, paths[0].into());
        }
    }
    imports
}

#[derive(Clone)]
pub(super) struct Promotion<'a> {
    pub methods: Vec<&'a Node>,
    pub line: u32,
}
pub(super) struct Promotions<'a> {
    ctx: &'a Context<'a>,
    memo: HashMap<(String, String), Option<Promotion<'a>>>,
}
impl<'a> Promotions<'a> {
    pub(super) fn new(ctx: &'a Context<'a>) -> Self {
        Self {
            ctx,
            memo: HashMap::new(),
        }
    }
    fn provides(&self, id: &str, key: &str) -> Result<bool> {
        if self
            .ctx
            .node(id)
            .is_none_or(|node| node.kind != "interface")
        {
            return Ok(self
                .ctx
                .methods(id)
                .iter()
                .any(|node| method_key(node) == key));
        }
        let mut seen = HashSet::from([id.to_owned()]);
        let mut pending = vec![id.to_owned()];
        while let Some(id) = pending.pop() {
            self.ctx.step()?;
            if self
                .ctx
                .methods(&id)
                .iter()
                .any(|node| method_key(node) == key)
            {
                return Ok(true);
            }
            for edge in self
                .ctx
                .outgoing(&id)
                .filter(|edge| embedding(self.ctx, edge))
            {
                if seen.insert(edge.target.clone()) {
                    pending.push(edge.target.clone());
                }
            }
        }
        Ok(false)
    }
    pub(super) fn get(&mut self, id: &str, key: &str) -> Result<Option<Promotion<'a>>> {
        let memo_key = (id.into(), key.into());
        if let Some(found) = self.memo.get(&memo_key) {
            return Ok(found.clone());
        }
        let mut level = vec![(id.to_owned(), 1usize, 0u32)];
        let mut met = HashSet::from([id.to_owned()]);
        let mut depth = 0;
        let mut found = None;
        while !level.is_empty() {
            let mut next = Vec::<(String, usize, u32)>::new();
            let mut positions = HashMap::new();
            for (id, paths, line) in level {
                self.ctx.step()?;
                if self
                    .ctx
                    .node(&id)
                    .is_some_and(|node| node.kind == "interface")
                {
                    continue;
                }
                let mut targets = HashSet::new();
                for edge in self
                    .ctx
                    .outgoing(&id)
                    .filter(|edge| embedding(self.ctx, edge))
                {
                    self.ctx.step()?;
                    if met.contains(&edge.target) || !targets.insert(&edge.target) {
                        continue;
                    }
                    if let Some(&at) = positions.get(&edge.target) {
                        let at: usize = at;
                        next[at].1 = next[at].1.saturating_add(paths);
                    } else {
                        positions.insert(edge.target.clone(), next.len());
                        next.push((
                            edge.target.clone(),
                            paths,
                            if depth == 0 {
                                edge.line.unwrap_or(0)
                            } else {
                                line
                            },
                        ));
                    }
                }
            }
            let mut paths = 0usize;
            let mut provider = None;
            for (id, count, line) in &next {
                met.insert(id.clone());
                if self.provides(id, key)? {
                    paths = paths.saturating_add(*count);
                    provider = Some((id, *line));
                }
            }
            if let Some((id, line)) = provider {
                if paths == 1
                    && self
                        .ctx
                        .node(id)
                        .is_some_and(|node| node.kind != "interface")
                {
                    found = Some(Promotion {
                        methods: self
                            .ctx
                            .methods(id)
                            .into_iter()
                            .filter(|node| method_key(node) == key)
                            .collect(),
                        line,
                    });
                }
                break;
            }
            level = next;
            depth += 1;
        }
        self.memo.insert(memo_key, found.clone());
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn go_conformance_matches_independent_reference() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/go/cases.json"
        ))
        .unwrap();
        let mut failures = Vec::new();
        for case in fixture["cases"].as_array().unwrap() {
            let mut nodes = case["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            let edges: Vec<Edge> = serde_json::from_value(case["baseEdges"].clone()).unwrap();
            let unresolved: Vec<UnresolvedReference> =
                serde_json::from_value(case["unresolved"].clone()).unwrap();
            let sources = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let ctx = Context::new(&nodes, &edges, &sources, &unresolved);
            let actual = implements(&ctx).unwrap();
            let expected: Vec<Edge> = serde_json::from_value(case["expected"].clone()).unwrap();
            let normalize = |edges: Vec<Edge>| {
                let mut edges: Vec<_> = edges
                    .iter()
                    .map(|edge| serde_json::to_string(edge).unwrap())
                    .collect();
                edges.sort();
                edges
            };
            let actual = normalize(actual);
            let expected = normalize(expected);
            if actual != expected {
                failures.push(format!(
                    "{} missing {:?}, extra {:?}",
                    case["id"],
                    expected
                        .iter()
                        .filter(|edge| !actual.contains(edge))
                        .collect::<Vec<_>>(),
                    actual
                        .iter()
                        .filter(|edge| !expected.contains(edge))
                        .collect::<Vec<_>>()
                ));
            }
            for row in case["arities"].as_array().unwrap() {
                assert_eq!(
                    serde_json::to_value(signature_arity(row["signature"].as_str())).unwrap(),
                    row["arity"],
                    "{}: {}",
                    case["id"],
                    row["signature"]
                );
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
