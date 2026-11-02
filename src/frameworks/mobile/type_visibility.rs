//! The reference's final Swift type-target gate, applied after candidate selection.
use super::*;
const TYPES: &[&str] = &[
    "class",
    "struct",
    "enum",
    "interface",
    "protocol",
    "type_alias",
];
const GENERICS: &str = r"<[^<>]*(?:<[^<>]*>[^<>]*)*>";
const SEGMENT: &str = r"[A-Za-z_]\w*(?:<[^<>]*(?:<[^<>]*>[^<>]*)*>)?\s*\.\s*";
#[derive(Default)]
pub(super) struct Visibility {
    columns: HashMap<(usize, usize), Columns>,
    extensions: HashMap<String, bool>,
    declarations: HashMap<(String, String, String, String), Option<String>>,
    clauses: HashMap<String, Vec<String>>,
    inherited: HashMap<String, HashSet<String>>,
}
struct Columns {
    ascii: bool,
    points: Vec<(usize, usize)>,
}
fn columns<'a>(cache: &'a mut HashMap<(usize, usize), Columns>, text: &str) -> &'a Columns {
    // Snapshot strings remain borrowed and immutable for the binding's lifetime.
    // Slice identity avoids copying lines into a second source cache.
    cache
        .entry((text.as_ptr() as usize, text.len()))
        .or_insert_with(|| {
            let ascii = text.is_ascii();
            let mut points = Vec::new();
            if !ascii {
                let mut units = 0;
                let mut previous = 0;
                points.push((0, 0));
                for (at, ch) in text.char_indices() {
                    if units >= previous + 64 {
                        points.push((units, at));
                        previous = units;
                    }
                    units += ch.len_utf16();
                }
            }
            Columns { ascii, points }
        })
}
fn byte(columns: &Columns, text: &str, units: usize) -> usize {
    if columns.ascii {
        return units.min(text.len());
    }
    let at = columns
        .points
        .partition_point(|(position, _)| *position <= units)
        .saturating_sub(1);
    let (mut count, start) = columns.points[at];
    for (at, ch) in text[start..].char_indices() {
        if count >= units {
            return start + at;
        }
        count += ch.len_utf16();
    }
    text.len()
}
fn slice<'a>(columns: &Columns, text: &'a str, start: usize, end: usize) -> &'a str {
    let start = byte(columns, text, start);
    let end = byte(columns, text, end);
    &text[start.min(end)..end]
}
fn head(
    cache: &mut HashMap<(usize, usize), Columns>,
    ctx: &Context<'_>,
    node: &Node,
    following: u32,
) -> String {
    let text = ctx.line(&node.file_path, node.start_line.saturating_sub(1) as usize);
    let positions = columns(cache, text);
    let mut parts = vec![&text[byte(positions, text, node.start_column as usize)..]];
    for line in node.start_line..node.end_line.min(node.start_line.saturating_add(following)) {
        parts.push(
            ctx.line(&node.file_path, line as usize)
                .trim_end_matches('\r'),
        );
    }
    parts.join(" ")
}
fn strip_strings(text: &str) -> Result<String> {
    Ok(regex(r#""(?:[^"\\\n]|\\.)*""#)?
        .replace_all(text, "\"\"")
        .into_owned())
}
fn parent(node: &Node) -> Option<&str> {
    let (path, _) = node.qualified_name.rsplit_once("::")?;
    (!path.is_empty() && !path.contains('/') && !path.ends_with(".swift")).then_some(path)
}
fn written(
    cache: &mut HashMap<(usize, usize), Columns>,
    ctx: &Context<'_>,
    r: &UnresolvedReference,
) -> Result<String> {
    let name = &r.reference_name;
    if name.contains('.') {
        return Ok(name.clone());
    }
    let text = ctx.line(
        r.file_path.as_deref().unwrap_or(""),
        r.line.saturating_sub(1) as usize,
    );
    if text.is_empty() {
        return Ok(name.clone());
    }
    ctx.step(text.len() / 32 + 1)?;
    let positions = columns(cache, text);
    let starts = |column| text[byte(positions, text, column)..].starts_with(name);
    let column = r.column as usize;
    let column =
        if !starts(column) && regex(r"^[ \t]*&[ \t]")?.is_match(text)? && starts(column + 1) {
            column + 1
        } else {
            column
        };
    let qualifier = if starts(column) {
        regex(&format!("((?:{SEGMENT})+)$"))?
            .captures(slice(positions, text, column.saturating_sub(240), column))?
            .map(|m| m[1].to_owned())
    } else {
        regex(&format!(r"^((?:{SEGMENT})+){}\b", regex::escape(name)))?
            .captures(slice(positions, text, column, column.saturating_add(240)))?
            .map(|m| m[1].to_owned())
    };
    let Some(qualifier) = qualifier else {
        return Ok(name.clone());
    };
    let clean = regex(GENERICS)?.replace_all(&qualifier, "");
    let clean = regex(r"\s+")?.replace_all(&clean, "");
    Ok(format!("{clean}{name}"))
}
fn written_path(written: &str, name: &str) -> Option<String> {
    let parts: Vec<_> = written.split('.').collect();
    let at = parts.iter().rposition(|part| *part == name)?;
    Some(parts[..=at].join("::"))
}
fn shared_dirs(a: &str, b: &str) -> usize {
    let a: Vec<_> = a.split('/').collect();
    let b: Vec<_> = b.split('/').collect();
    a[..a.len().saturating_sub(1)]
        .iter()
        .zip(&b[..b.len().saturating_sub(1)])
        .take_while(|(a, b)| a == b)
        .count()
}
impl Visibility {
    fn extension(&mut self, ctx: &Context<'_>, node: &Node) -> Result<bool> {
        if node.language != "swift" || node.kind != "class" {
            return Ok(false);
        }
        if let Some(answer) = self.extensions.get(&node.id) {
            return Ok(*answer);
        }
        let text = head(&mut self.columns, ctx, node, 4);
        ctx.step(text.len() / 32 + 1)?;
        let text = strip_strings(&text)?;
        let answer = regex(r"\b(extension|class|struct|enum|actor|protocol)\b")?
            .captures(&text)?
            .is_some_and(|m| &m[1] == "extension");
        self.extensions.insert(node.id.clone(), answer);
        Ok(answer)
    }
    fn supertypes(&mut self, ctx: &Context<'_>, node: &Node) -> Result<Vec<String>> {
        if let Some(answer) = self.clauses.get(&node.id) {
            return Ok(answer.clone());
        }
        let text = head(&mut self.columns, ctx, node, 6);
        ctx.step(text.len() / 32 + 1)?;
        let text = strip_strings(&text)?;
        let clause=regex(r"\b(?:class|struct|enum|actor|protocol|extension)\s+[A-Za-z_][\w.]*\s*(?:<[^{}]*?>)?\s*:\s*([^{]*?)\s*(?:\bwhere\b[^{]*)?\{")?.captures(&text)?.map(|m|m[1].to_owned());
        let mut names = Vec::new();
        if let Some(clause) = clause {
            for part in clause.split(',') {
                let name = regex(GENERICS)?.replace_all(part, "");
                let name = name
                    .trim_matches(crate::search::query_whitespace)
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .trim_matches(crate::search::query_whitespace);
                if regex(r"^[A-Za-z_]\w*$")?.is_match(name)? {
                    names.push(name.to_owned())
                }
            }
        }
        self.clauses.insert(node.id.clone(), names.clone());
        Ok(names)
    }
    fn inherited(&mut self, ctx: &Context<'_>, name: &str) -> Result<HashSet<String>> {
        if let Some(answer) = self.inherited.get(name) {
            return Ok(answer.clone());
        }
        let mut found = HashSet::new();
        let mut frontier = vec![name.to_owned()];
        for _ in 0..4 {
            let mut next = Vec::new();
            for own in frontier {
                for node in ctx.named(&own) {
                    ctx.step(1)?;
                    if node.language != "swift" || !TYPES.contains(&node.kind.as_str()) {
                        continue;
                    }
                    for sup in self.supertypes(ctx, node)? {
                        if sup != name && found.insert(sup.clone()) {
                            next.push(sup)
                        }
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        self.inherited.insert(name.to_owned(), found.clone());
        Ok(found)
    }
    fn scope(
        &mut self,
        ctx: &Context<'_>,
        target: &Node,
        from: Option<&Node>,
    ) -> Result<Option<usize>> {
        let Some(parent) = parent(target) else {
            return Ok(Some(0));
        };
        let Some(from) = from else { return Ok(None) };
        let scope = &from.qualified_name;
        let inside = |path: &str| {
            let parts: Vec<_> = path.split("::").collect();
            (0..parts.len()).any(|at| {
                let suffix = parts[at..].join("::");
                *scope == suffix || scope.starts_with(&format!("{suffix}::"))
            })
        };
        let depth = parent.split("::").count();
        if inside(&target.qualified_name) {
            return Ok(Some(depth + 1));
        }
        if inside(parent) {
            return Ok(Some(depth));
        }
        let name = parent.rsplit("::").next().unwrap_or("");
        for part in scope.split("::") {
            if self.inherited(ctx, part)?.contains(name) {
                return Ok(Some(depth));
            }
        }
        Ok(None)
    }
    fn fits(ctx: &Context<'_>, node: &Node, path: &str) -> Result<bool> {
        let qn = &node.qualified_name;
        if *qn == path || qn.ends_with(&format!("::{path}")) {
            return Ok(true);
        }
        if !path.ends_with(&format!("::{qn}")) {
            return Ok(false);
        }
        if qn.contains("::") {
            return Ok(true);
        }
        let head = &path[..path.len() - qn.len() - 2];
        let parent = head.rsplit("::").next().unwrap_or("");
        ctx.step(ctx.by_file.get(node.file_path.as_str()).map_or(0, Vec::len))?;
        if ctx
            .in_file(&node.file_path)
            .any(|p| p.name == parent && TYPES.contains(&p.kind.as_str()) && p.id != node.id)
        {
            return Ok(true);
        }
        ctx.step(ctx.by_name.get(head).map_or(0, Vec::len))?;
        Ok(!head.contains("::")
            && !ctx
                .named(head)
                .any(|n| n.language == "swift" && TYPES.contains(&n.kind.as_str())))
    }
    fn declaration<'a>(
        &mut self,
        ctx: &Context<'a>,
        name: &str,
        written: &str,
        r: &UnresolvedReference,
        from: Option<&Node>,
    ) -> Result<Option<&'a Node>> {
        let file = r.file_path.as_deref().unwrap_or("");
        let key = (
            file.to_owned(),
            from.map_or("", |n| n.qualified_name.as_str()).to_owned(),
            written.to_owned(),
            name.to_owned(),
        );
        if let Some(found) = self.declarations.get(&key) {
            return Ok(found.as_deref().and_then(|id| ctx.by_id.get(id).copied()));
        }
        let bare = !written.contains('.');
        let path = (!bare).then(|| written_path(written, name)).flatten();
        let mut scored = Vec::new();
        for node in ctx.named(name) {
            ctx.step(1)?;
            if node.language != "swift"
                || !TYPES.contains(&node.kind.as_str())
                || self.extension(ctx, node)?
            {
                continue;
            }
            if let Some(path) = &path {
                if Self::fits(ctx, node, path)? {
                    scored.push((node, 0))
                }
                continue;
            }
            let depth = if bare {
                self.scope(ctx, node, from)?
            } else {
                Some(0)
            };
            if let Some(depth) = depth {
                scored.push((node, depth))
            }
        }
        let deepest = scored.iter().map(|(_, depth)| *depth).max();
        let mut by_file: HashMap<&str, usize> = HashMap::new();
        let mut declarations: Vec<&Node> = Vec::new();
        for (node, depth) in scored {
            if Some(depth) != deepest {
                continue;
            }
            if let Some(&at) = by_file.get(node.file_path.as_str()) {
                let old = declarations[at];
                if node.end_line.saturating_sub(node.start_line)
                    > old.end_line.saturating_sub(old.start_line)
                {
                    declarations[at] = node
                }
            } else {
                by_file.insert(&node.file_path, declarations.len());
                declarations.push(node)
            }
        }
        let found = if declarations.len() == 1 {
            Some(declarations[0])
        } else if let Some(own) = declarations.iter().find(|node| node.file_path == file) {
            Some(*own)
        } else {
            let nearest = declarations
                .iter()
                .map(|n| shared_dirs(&n.file_path, file))
                .max();
            let mut fits = declarations
                .iter()
                .filter(|n| Some(shared_dirs(&n.file_path, file)) == nearest);
            let first = fits.next().copied();
            if fits.next().is_none() { first } else { None }
        };
        self.declarations.insert(key, found.map(|n| n.id.clone()));
        Ok(found)
    }
    pub(super) fn gate(
        &mut self,
        ctx: &Context<'_>,
        r: &UnresolvedReference,
        mut result: Binding,
    ) -> Result<Option<Binding>> {
        if r.language.as_deref() != Some("swift") || r.reference_kind == "imports" {
            return Ok(Some(result));
        }
        let Some(&target) = ctx.by_id.get(result.target_node_id.as_str()) else {
            return Ok(Some(result));
        };
        if target.language != "swift" || !TYPES.contains(&target.kind.as_str()) {
            return Ok(Some(result));
        }
        let from = ctx.by_id.get(r.from_node_id.as_str()).copied();
        let written = written(&mut self.columns, ctx, r)?;
        if !self.extension(ctx, target)? {
            if !written.contains('.') {
                if self.scope(ctx, target, from)?.is_some() {
                    return Ok(Some(result));
                }
            } else {
                let path = written_path(&written, &target.name);
                if match path {
                    None => true,
                    Some(path) => Self::fits(ctx, target, &path)?,
                } {
                    return Ok(Some(result));
                }
            }
        }
        let Some(declared) = self.declaration(ctx, &target.name, &written, r, from)? else {
            return Ok(None);
        };
        result.target_node_id = declared.id.clone();
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_swift_type_targets_match_original_strategy_results() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/frameworks/swift-type-gate.json"
        ))
        .unwrap();
        let mut differences = Vec::new();
        for case in corpus["cases"].as_array().unwrap() {
            let mut raw = case["nodes"].clone();
            for node in raw.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes: Vec<Node> = serde_json::from_value(raw).unwrap();
            let sources: BTreeMap<&str, &str> = case["sources"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let resolve_module = |_: &str, _: &str, _: &str| Ok(None);
            let mut bindings = Bindings::new(&nodes, &sources, &[], &resolve_module).unwrap();
            for answer in case["answers"].as_array().unwrap() {
                let reference = serde_json::from_value(answer["reference"].clone()).unwrap();
                let actual = if let Some(input) = answer["input"].as_object() {
                    let result = Binding {
                        target_node_id: input["targetNodeId"].as_str().unwrap().into(),
                        confidence: input["confidence"].as_f64().unwrap(),
                        also_targets: Vec::new(),
                        metadata: Some(json!({"preserved":true})),
                    };
                    bindings.swift_type_target(&reference,result).unwrap().map(|binding|{assert_eq!(binding.metadata,Some(json!({"preserved":true})));json!({"targetNodeId":binding.target_node_id,"confidence":binding.confidence})})
                } else {
                    None
                };
                let actual = actual.unwrap_or(Value::Null);
                if actual != answer["expected"] {
                    differences.push(json!({"origin":case["origin"],"reference":reference,"input":answer["input"],"actual":actual,"expected":answer["expected"]}));
                }
            }
        }
        if !differences.is_empty() {
            std::fs::write(
                "/tmp/prometeu-swift-gate-differences.json",
                serde_json::to_string_pretty(&differences).unwrap(),
            )
            .unwrap();
        }
        assert!(
            differences.is_empty(),
            "{} Swift gate answers differ; see /tmp/prometeu-swift-gate-differences.json",
            differences.len()
        );
    }
}
