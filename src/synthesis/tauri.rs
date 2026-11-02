//! Source-proven desktop IPC commands and event subscriptions; no runtime dependency.
use super::*;
use crate::frameworks::frontend::href;
mod frontend;
mod rust;
const CLIENT_TO_SERVER: &str = "client→server";
const SERVER_TO_CLIENT: &str = "server→client";
fn trim(text: &str) -> &str {
    text.trim_matches(crate::search::query_whitespace)
}
fn callable(node: &Node) -> bool {
    matches!(node.kind.as_str(), "function" | "method" | "component")
}
fn within(file: &str, root: &str) -> bool {
    root.is_empty()
        || file
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/'))
}
fn same_scope(a: Option<&str>, b: Option<&str>) -> bool {
    a.is_none() || b.is_none() || a == b
}
fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|v| v == value) {
        values.push(value.to_owned());
    }
}
fn close_bracket(code: &str, open: usize) -> Option<usize> {
    let closer = |b| match b {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        _ => None,
    };
    let mut stack = vec![closer(*code.as_bytes().get(open)?)?];
    for (i, byte) in code.bytes().enumerate().skip(open + 1) {
        if let Some(close) = closer(byte) {
            stack.push(close)
        } else if stack.last() == Some(&byte) {
            stack.pop();
            if stack.is_empty() {
                return Some(i);
            }
        }
    }
    None
}
fn open_bracket(code: &str, close: usize) -> Option<usize> {
    let end = *code.as_bytes().get(close)?;
    let open = match end {
        b')' => b'(',
        b']' => b'[',
        _ => b'{',
    };
    let mut depth = 0i32;
    for i in (0..=close).rev() {
        if code.as_bytes()[i] == end {
            depth += 1
        } else if code.as_bytes()[i] == open {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}
fn arguments(code: &str, open: usize) -> Vec<(usize, usize)> {
    let Some(close) = close_bracket(code, open) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    let mut start = open + 1;
    let mut i = start;
    while i < close {
        match code.as_bytes()[i] {
            b'(' | b'[' | b'{' => {
                let Some(end) = close_bracket(code, i) else {
                    return spans;
                };
                i = end
            }
            b',' => {
                spans.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if !trim(&code[start..close]).is_empty() {
        spans.push((start, close));
    }
    spans
}
fn skip_space(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    at + text[at..].len()
        - text[at..]
            .trim_start_matches(crate::search::query_whitespace)
            .len()
}
fn byte_at(text: &str, units: usize) -> usize {
    let mut count = 0;
    for (i, ch) in text.char_indices() {
        if count >= units {
            return i;
        }
        count += ch.len_utf16();
    }
    text.len()
}
struct File<'a> {
    path: &'a str,
    source: &'a str,
    text: String,
    code: String,
    nodes: Vec<&'a Node>,
    lines: Vec<usize>,
}
impl<'a> File<'a> {
    fn new(ctx: &Context<'a>, path: &'a str, source: &'a str, language: &str) -> Result<Self> {
        ctx.steps(source.len() / 32 + 1)?;
        let text = comments::strip_for_regex(source, language);
        let code = comments::blank_string_contents(&text)?;
        let mut nodes: Vec<_> = ctx.in_file(path).collect();
        nodes.sort_by(|a, b| a.start_line.cmp(&b.start_line).then(a.id.cmp(&b.id)));
        let mut lines = vec![0];
        lines.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        Ok(Self {
            path,
            source,
            text,
            code,
            nodes,
            lines,
        })
    }
    fn line(&self, offset: usize) -> u32 {
        self.lines.partition_point(|&start| start <= offset) as u32
    }
    fn column(&self, offset: usize) -> u32 {
        let line = self.line(offset);
        self.source[self.offset(line)..offset]
            .encode_utf16()
            .count() as u32
    }
    fn offset(&self, line: u32) -> usize {
        self.lines
            .get((line as usize).min(self.lines.len()).saturating_sub(1))
            .copied()
            .unwrap_or(0)
    }
    fn position(&self, line: u32, column: u32) -> usize {
        let start = self.offset(line);
        start + byte_at(&self.source[start..], column as usize)
    }
    fn limit(&self, start: usize, units: usize) -> usize {
        start + byte_at(&self.source[start..], units)
    }
    fn before(&self, end: usize, units: usize) -> usize {
        let mut count = 0;
        for (i, ch) in self.source[..end].char_indices().rev() {
            count += ch.len_utf16();
            if count >= units {
                return i;
            }
        }
        0
    }
    fn owner(&self, offset: usize) -> Option<&'a Node> {
        let line = self.line(offset);
        let column = self.column(offset);
        let mut best: Option<&Node> = None;
        for &node in &self.nodes {
            if !callable(node)
                || node.start_line > line
                || node.end_line < line
                || node.start_line == line && node.start_column > column
                || node.end_line == line && node.end_column <= column
            {
                continue;
            }
            if best.is_none_or(|old| {
                node.start_line > old.start_line
                    || node.start_line == old.start_line && node.start_column >= old.start_column
            }) {
                best = Some(node)
            }
        }
        best.or_else(|| {
            self.nodes
                .iter()
                .copied()
                .filter(|node| {
                    matches!(node.kind.as_str(), "constant" | "variable")
                        && node.start_line <= line
                        && node.end_line >= line
                })
                .min_by_key(|node| node.end_line - node.start_line)
        })
        .or_else(|| self.nodes.iter().copied().find(|node| node.kind == "file"))
    }
    fn enclosing(&self, line: u32) -> Option<&'a Node> {
        let mut best: Option<&Node> = None;
        for &node in &self.nodes {
            if callable(node)
                && node.start_line <= line
                && node.end_line >= line
                && best.is_none_or(|old| node.start_line >= old.start_line)
            {
                best = Some(node)
            }
        }
        best
    }
}
#[derive(Clone)]
struct Command<'a> {
    nodes: Vec<&'a Node>,
    registered: String,
}
#[derive(Default)]
struct Table<'a> {
    keys: Vec<String>,
    values: HashMap<String, Option<Command<'a>>>,
}
impl<'a> Table<'a> {
    fn add(&mut self, key: &str, command: Command<'a>) {
        if let Some(known) = self.values.get_mut(key) {
            if known.as_ref().is_some_and(|old| {
                old.nodes.len() != command.nodes.len()
                    || old
                        .nodes
                        .iter()
                        .any(|node| !command.nodes.iter().any(|other| other.id == node.id))
            }) {
                *known = None
            }
        } else {
            self.keys.push(key.to_owned());
            self.values.insert(key.to_owned(), Some(command));
        }
    }
    fn get(&self, key: &str) -> Option<&Command<'a>> {
        self.values.get(key).and_then(Option::as_ref)
    }
}
struct App<'a> {
    root: String,
    commands: Table<'a>,
    open_list: Option<String>,
}
#[derive(Clone)]
struct EventStruct<'a> {
    node: &'a Node,
    event: String,
    scope: Option<String>,
}
#[derive(Clone)]
struct Endpoint<'a> {
    node: &'a Node,
    event: String,
    file: String,
    line: u32,
    column: u32,
    rust: bool,
    scope: Option<String>,
}
struct Side<'a> {
    roots: Vec<String>,
    apps: Vec<App<'a>>,
    plugins: Table<'a>,
    any_app: Table<'a>,
    by_event: HashMap<String, Vec<EventStruct<'a>>>,
    by_type: HashMap<String, Vec<EventStruct<'a>>>,
    by_camel: HashMap<String, Vec<EventStruct<'a>>>,
    emitters: Vec<Endpoint<'a>>,
    listeners: Vec<Endpoint<'a>>,
}
impl<'a> Side<'a> {
    fn scope(&self, file: &str) -> Option<String> {
        self.roots.iter().find(|root| within(file, root)).cloned()
    }
    fn event<'b>(
        &self,
        index: &'b HashMap<String, Vec<EventStruct<'a>>>,
        name: &str,
        scope: Option<&str>,
    ) -> Option<&'b EventStruct<'a>> {
        let found = index.get(name)?;
        found
            .iter()
            .find(|event| event.scope.as_deref() == scope)
            .or_else(|| {
                found
                    .iter()
                    .find(|event| same_scope(event.scope.as_deref(), scope))
            })
    }
    fn table(&self, file: &str) -> &Table<'a> {
        self.apps
            .iter()
            .find(|app| {
                within(file, &app.root)
                    && (!app.root.is_empty()
                        || self.apps.len() == 1
                        || !app.commands.keys.is_empty())
            })
            .map_or(&self.any_app, |app| &app.commands)
    }
}
struct Collected<'a> {
    edges: Vec<Edge>,
    seen: HashSet<(String, String)>,
    emitters: Vec<Endpoint<'a>>,
    listeners: Vec<Endpoint<'a>>,
    subscribed: Vec<(&'a Node, EventStruct<'a>, u32, u32)>,
    subscriptions: HashSet<(String, String)>,
}
impl<'a> Collected<'a> {
    fn command(
        &mut self,
        ctx: &Context<'_>,
        owner: &Node,
        key: &str,
        command: &Command<'a>,
        callee: &str,
        position: (u32, u32),
    ) -> Result<()> {
        let (line, column) = position;
        for target in &command.nodes {
            ctx.step()?;
            if !self.seen.insert((owner.id.clone(), target.id.clone())) {
                continue;
            }
            ctx.emit(&mut self.edges,Edge{source:owner.id.clone(),target:target.id.clone(),kind:"calls".into(),line:Some(line),column:Some(column),provenance:Some("heuristic".into()),metadata:Some(json!({"synthesizedBy":"tauri-command","channel":"ipc","tier":CLIENT_TO_SERVER,"callee":callee,"command":key,"registeredAt":command.registered}))})?;
        }
        Ok(())
    }
    fn pair(&mut self, ctx: &Context<'_>) -> Result<()> {
        let mut events = Vec::new();
        let mut emitters: HashMap<&str, Vec<&Endpoint<'_>>> = HashMap::new();
        let mut listeners: HashMap<&str, Vec<&Endpoint<'_>>> = HashMap::new();
        for endpoint in &self.emitters {
            if !emitters.contains_key(endpoint.event.as_str()) {
                events.push(endpoint.event.as_str());
            }
            emitters.entry(&endpoint.event).or_default().push(endpoint);
        }
        for endpoint in &self.listeners {
            listeners.entry(&endpoint.event).or_default().push(endpoint);
        }
        for event in events {
            let from = &emitters[event];
            let Some(to) = listeners.get(event) else {
                continue;
            };
            if from
                .iter()
                .map(|e| &e.node.id)
                .collect::<HashSet<_>>()
                .len()
                > 16
                || to.iter().map(|e| &e.node.id).collect::<HashSet<_>>().len() > 16
            {
                continue;
            }
            for e in from {
                for l in to {
                    ctx.step()?;
                    if e.node.id == l.node.id
                        || !same_scope(e.scope.as_deref(), l.scope.as_deref())
                        || !self.seen.insert((e.node.id.clone(), l.node.id.clone()))
                    {
                        continue;
                    }
                    let mut metadata = json!({"synthesizedBy":"tauri-event","channel":"event","event":event,"registeredAt":format!("{}:{}",l.file,l.line)});
                    if e.rust != l.rust {
                        metadata["tier"] = if e.rust {
                            SERVER_TO_CLIENT
                        } else {
                            CLIENT_TO_SERVER
                        }
                        .into();
                    }
                    ctx.emit(
                        &mut self.edges,
                        Edge {
                            source: e.node.id.clone(),
                            target: l.node.id.clone(),
                            kind: "calls".into(),
                            line: Some(e.line),
                            column: Some(e.column),
                            provenance: Some("heuristic".into()),
                            metadata: Some(metadata),
                        },
                    )?;
                }
            }
        }
        for (owner, event, line, column) in &self.subscribed {
            ctx.emit(&mut self.edges,Edge{source:owner.id.clone(),target:event.node.id.clone(),kind:"references".into(),line:Some(*line),column:Some(*column),provenance:Some("heuristic".into()),metadata:Some(json!({"synthesizedBy":"tauri-event","channel":"event","event":event.event,"registeredAt":format!("{}:{}",event.node.file_path,event.node.start_line)}))})?;
        }
        Ok(())
    }
}
fn words(name: &str) -> Result<Vec<String>> {
    static WORDS: std::sync::LazyLock<fancy_regex::Regex> =
        std::sync::LazyLock::new(|| regex(r"[A-Z]+(?![a-z])|[A-Z]?[a-z0-9]+|[0-9]+").unwrap());
    let mut out = Vec::new();
    for part in name.split('_') {
        for m in WORDS.find_iter(part) {
            out.push(m?.as_str().to_lowercase());
        }
    }
    Ok(out)
}
fn camel(name: &str) -> Result<String> {
    Ok(words(name)?
        .into_iter()
        .enumerate()
        .map(|(index, word)| {
            if index == 0 {
                word
            } else {
                format!("{}{}", word[..1].to_uppercase(), &word[1..])
            }
        })
        .collect())
}
fn kebab(name: &str) -> Result<String> {
    Ok(words(name)?.join("-"))
}
pub(super) fn edges(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let active = ["tauri", "tauri_specta"].iter().any(|name| {
        ctx.named(name)
            .any(|node| node.language == "rust" && node.kind == "import")
    }) || ctx.nodes.iter().any(|node| {
        node.language == "rust"
            && node.kind == "function"
            && node
                .decorators
                .as_ref()
                .is_some_and(|d| d.iter().any(|d| d == "tauri::command"))
    });
    if !active {
        return Ok(Vec::new());
    }
    let side = rust::read(ctx)?;
    let mut out = Collected {
        edges: Vec::new(),
        seen: HashSet::new(),
        emitters: side.emitters.clone(),
        listeners: side.listeners.clone(),
        subscribed: Vec::new(),
        subscriptions: HashSet::new(),
    };
    frontend::read(ctx, &side, &mut out)?;
    out.pair(ctx)?;
    Ok(out.edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_command_and_event_records_match_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/tauri.json"
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
                .map(|(p, s)| (p.as_str(), s.as_str().unwrap()))
                .collect();
            let manifests: BTreeMap<_, _> = case["assets"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(p, s)| (p.as_str(), s.as_str().unwrap()))
                .collect();
            let imports: Vec<ImportBinding> =
                serde_json::from_value(case["imports"].clone()).unwrap();
            let resolve = |specifier: &str, file: &str, language: &str| {
                case["modules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| {
                        m["source"] == specifier && m["file"] == file && m["language"] == language
                    })
                    .and_then(|m| m["resolved"].as_str().map(str::to_owned))
            };
            let mut ctx = Context::new(&nodes, &base, &sources, &[]);
            ctx.manifests = &manifests;
            ctx.resolve_module = &resolve;
            for binding in &imports {
                ctx.file_imports
                    .entry(&binding.file)
                    .or_default()
                    .push(binding);
                ctx.imports
                    .entry((&binding.file, &binding.local_name))
                    .or_insert(binding);
            }
            let actual = serde_json::to_value(edges(&ctx).unwrap()).unwrap();
            if actual != case["expected"] {
                std::fs::write(
                    "/tmp/prometeu-tauri-actual.json",
                    serde_json::to_string_pretty(&actual).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    "/tmp/prometeu-tauri-expected.json",
                    serde_json::to_string_pretty(&case["expected"]).unwrap(),
                )
                .unwrap();
            }
            assert_eq!(
                actual, case["expected"],
                "{} {}",
                case["id"], case["origin"]
            );
        }
    }
}
