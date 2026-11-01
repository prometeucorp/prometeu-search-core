//! HTTP, job-queue, event, socket, and test-request channels between project code.
use super::{Context, Edge, ImportBinding, Node};
use anyhow::Result;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex};
mod events;
mod http;
mod queue;
mod requests;
mod scan;
const CHAIN: &str = r"(?<![A-Za-z_$])";
fn chain_start(source: &str, at: usize) -> bool {
    source[..at]
        .bytes()
        .rev()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
        .all(|byte| byte.is_ascii_digit())
}
const FANOUT: usize = 6;
fn regex(pattern: &str) -> Result<Arc<fancy_regex::Regex>> {
    static CACHE: LazyLock<Mutex<HashMap<String, Arc<fancy_regex::Regex>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut cache = CACHE.lock().expect("channel pattern cache");
    if let Some(compiled) = cache.get(pattern) {
        return Ok(compiled.clone());
    }
    let compiled = Arc::new(super::regex(pattern)?);
    if cache.len() < 256 {
        cache.insert(pattern.into(), compiled.clone());
    }
    Ok(compiled)
}
struct Facts {
    file: String,
    safe: String,
    clients: HashMap<String, Option<String>>,
    default_client: Option<Option<String>>,
    queues: HashMap<String, String>,
    socket_server: bool,
}
struct Session<'c, 'a> {
    ctx: &'c Context<'a>,
    facts: HashMap<String, Option<Rc<Facts>>>,
}
impl<'c, 'a> Session<'c, 'a> {
    fn facts(&mut self, file: &str) -> Result<Option<Rc<Facts>>> {
        if let Some(facts) = self.facts.get(file) {
            return Ok(facts.clone());
        }
        let Some(source) = self
            .ctx
            .sources
            .get(file)
            .filter(|source| !source.is_empty())
        else {
            self.facts.insert(file.into(), None);
            return Ok(None);
        };
        self.ctx.steps(source.len())?;
        let safe = super::comments::strip_for_regex(source, "typescript");
        let factory = r"(?:(?:axios|ky|got|ofetch|\$fetch|wretch|redaxios)\s*\.\s*(?:create|extend)|new\s+Axios|wretch)\s*\(";
        let base = |open| -> Result<Option<String>> {
            let args = scan::arguments(self.ctx, &safe, open)?;
            let config = args
                .as_ref()
                .and_then(|args| args.first())
                .copied()
                .unwrap_or("");
            let Some(found) =
                regex(r"\b(?:baseURL|baseUrl|prefixUrl|baseURI)\s*:\s*")?.find(config)?
            else {
                return Ok(None);
            };
            crate::frameworks::frontend::href::read_string(config, found.end())
        };
        let mut clients = HashMap::new();
        for found in regex(&format!(r"\b(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=;]*?)?=\s*(?:await\s+)?{factory}"))?.captures_iter(&safe) { let found = found?;clients.insert(found[1].into(),base(found.get(0).unwrap().end()-1)?); }
        let default_client = regex(&format!(r"\bexport\s+default\s+{factory}"))?
            .find(&safe)?
            .map(|found| base(found.end() - 1))
            .transpose()?;
        let mut queues = HashMap::new();
        for found in regex(r#"\b(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=;]*?)?=\s*(?:new\s+)?(?:Queue|Bull|BullQueue)\s*(?:<[^>]*>)?\s*\(\s*(['"`])([^'"`]+)\2"#)?.captures_iter(&safe) { let found = found?;queues.insert(found[1].into(),found[3].into()); }
        for found in regex(r#"@InjectQueue\s*\(\s*(['"`])([^'"`]+)\1\s*\)\s*(?:(?:private|public|protected|readonly)\s+)*([A-Za-z_$][\w$]*)"#)?.captures_iter(&safe) { let found = found?;queues.insert(found[3].into(),found[2].into()); }
        let socket_server = regex(r#"@WebSocketGateway\s*\(|@SubscribeMessage\s*\(|@WebSocketServer\s*\(|\bnew\s+(?:Server|SocketIOServer|WebSocketServer|WebSocket\.Server|WSServer)\b|\.on\s*\(\s*['"]connection['"]"#)?.is_match(&safe)?;
        let facts = Rc::new(Facts {
            file: file.into(),
            safe,
            clients,
            default_client,
            queues,
            socket_server,
        });
        self.facts.insert(file.into(), Some(facts.clone()));
        Ok(Some(facts))
    }
    fn imported(&mut self, file: &str, local: &str) -> Result<Option<(Rc<Facts>, ImportBinding)>> {
        let Some(binding) = self.ctx.imports.get(&(file, local)).copied().cloned() else {
            return Ok(None);
        };
        let Some(path) = binding
            .mapped_path
            .as_ref()
            .or(binding.resolved_path.as_ref())
        else {
            return Ok(None);
        };
        if !regex(r"\.(?:[cm]?[jt]sx?)$")?.is_match(path)? {
            return Ok(None);
        }
        Ok(self.facts(path)?.map(|facts| (facts, binding)))
    }
    fn queue(&mut self, facts: &Facts, receiver: &str) -> Result<Option<String>> {
        let chain = scan::compact(receiver);
        let chain = chain.strip_prefix("this.").unwrap_or(&chain);
        let head = chain.split('.').next().unwrap();
        if let Some(queue) = facts.queues.get(head) {
            return Ok(Some(queue.clone()));
        }
        Ok(self
            .imported(&facts.file, head)?
            .and_then(|(facts, binding)| facts.queues.get(&binding.imported_name).cloned()))
    }
    fn handler(&mut self, facts: &Facts, text: &str, line: u32) -> Result<Option<&'a Node>> {
        let text = text.trim_start_matches(crate::search::query_whitespace);
        let Some(found) = regex(r"^(?:(?:async\s+)?([A-Za-z_$][\w$.]*)\s*(?:[,)]|$)|(?:async\s*)?(?:\(|function\b|[A-Za-z_$][\w$]*\s*=>))")?.captures(text)? else { return Ok(None) };
        if let Some(name) = found.get(1) {
            let tail = name.as_str().rsplit('.').next().unwrap();
            let candidates: Vec<_> = self
                .ctx
                .named(tail)
                .filter(|node| matches!(node.kind.as_str(), "function" | "method"))
                .collect();
            self.ctx.steps(candidates.len())?;
            let local: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|node| node.file_path == facts.file)
                .collect();
            if local.len() == 1 {
                return Ok(local.first().copied());
            }
            if local.len() > 1 {
                return Ok(None);
            }
            if let Some((imported, _)) =
                self.imported(&facts.file, name.as_str().split('.').next().unwrap())?
            {
                let targets: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|node| node.file_path == imported.file)
                    .collect();
                if targets.len() == 1 {
                    return Ok(targets.first().copied());
                }
            }
            return Ok((candidates.len() == 1).then(|| candidates[0]));
        }
        if let Some(node) = self.ctx.enclosing(&facts.file, line)? {
            return Ok(Some(node));
        }
        Ok(self
            .ctx
            .in_file(&facts.file)
            .filter(|node| {
                matches!(node.kind.as_str(), "constant" | "variable")
                    && node.start_line <= line
                    && node.end_line >= line
            })
            .max_by_key(|node| node.start_line))
    }
}
struct Site<'a> {
    from: &'a Node,
    line: u32,
    column: u32,
    callee: String,
    method: String,
    path: http::Path,
}
fn column(ctx: &Context<'_>, file: &str, at: usize) -> u32 {
    let source = ctx.sources[file];
    let start = source[..at].rfind('\n').map_or(0, |at| at + 1);
    source[start..at].encode_utf16().count() as u32
}
fn emit(
    ctx: &Context<'_>,
    output: &mut Vec<Edge>,
    seen: &mut HashSet<(String, String)>,
    from: &Node,
    to: &Node,
    position: (u32, u32),
    metadata: Value,
) -> Result<()> {
    if !seen.insert((from.id.clone(), to.id.clone())) {
        return Ok(());
    }
    ctx.emit(
        output,
        Edge {
            source: from.id.clone(),
            target: to.id.clone(),
            kind: "calls".into(),
            line: Some(position.0),
            column: Some(position.1),
            provenance: Some("heuristic".into()),
            metadata: Some(metadata),
        },
    )
}
fn decorated<'a>(
    ctx: &Context<'a>,
    facts: &Facts,
    end: usize,
    class: Option<&Node>,
) -> Result<Option<&'a Node>> {
    let Some((name, at)) = scan::method_after(ctx, &facts.safe, end)? else {
        return Ok(None);
    };
    let line = ctx.line_at(&facts.file, at);
    Ok(ctx.in_file(&facts.file).find(|node| {
        matches!(node.kind.as_str(), "method" | "function")
            && node.name == name
            && node.start_line >= line.saturating_sub(1)
            && node.start_line <= line + 1
            && class.is_none_or(|class| {
                node.start_line >= class.start_line && node.end_line <= class.end_line
            })
    }))
}
pub(super) fn cross_tier(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    static CHANNELS: LazyLock<[regex::Regex; 3]> = LazyLock::new(|| {
        [
            r"\b(?:fetch|\$fetch|ofetch|axios|ky|got|useFetch|useSWR)\b|\.\s*(?:get|post|put|patch|delete|head|options|request|\$get|\$post)\s*[<(]",
            r"\.\s*add\s*\(|@Processor\s*\(|\bnew\s+Worker\s*[<(]|\.\s*process\s*\(",
            r"\.\s*(?:emit|emitAsync|on|once)\s*\(|@OnEvent\s*\(|@SubscribeMessage\s*\(",
        ].map(|pattern| regex::Regex::new(&crate::parser::javascript_regex::linear_pattern(pattern)).expect("constant channel probe"))
    });
    let mut session = Session {
        ctx,
        facts: HashMap::new(),
    };
    let routes = http::routes(ctx);
    let mut sites = Vec::new();
    let mut producers = Vec::new();
    let mut consumers = Vec::new();
    let mut dispatches = Vec::new();
    let mut handlers = Vec::new();
    for (&file, &source) in ctx.sources {
        if !regex(r"\.(?:[cm]?[jt]sx?)$")?.is_match(file)?
            || crate::classify::is_test_path(file)
            || prometeu_extract::is_generated_file(file)
            || source.is_empty()
        {
            continue;
        }
        let wants_http = !routes.is_empty() && CHANNELS[0].is_match(source);
        let wants_queue = CHANNELS[1].is_match(source);
        let wants_events = CHANNELS[2].is_match(source);
        if !wants_http && !wants_queue && !wants_events {
            continue;
        }
        let Some(facts) = session.facts(file)? else {
            continue;
        };
        if wants_http {
            http::collect(&mut session, file, &facts, &mut sites)?;
        }
        if wants_queue {
            queue::collect(&mut session, &facts, &mut producers, &mut consumers)?;
        }
        if wants_events {
            events::collect(&mut session, &facts, &mut dispatches, &mut handlers)?;
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    http::edges(ctx, &sites, &routes, &mut out, &mut seen, false)?;
    queue::pair(ctx, &producers, &consumers, &mut out, &mut seen)?;
    events::pair(ctx, &dispatches, &handlers, &mut out, &mut seen)?;
    Ok(out)
}
pub(super) fn test_requests(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    requests::edges(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    #[test]
    fn channels_match_independent_project_records() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/tier/cases.json"
        ))
        .unwrap();
        for project in corpus["projects"].as_array().unwrap() {
            let mut records = project["nodes"].clone();
            for node in records.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let nodes: Vec<Node> = serde_json::from_value(records).unwrap();
            let files: BTreeMap<_, _> = project["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, text)| (path.as_str(), text.as_str().unwrap()))
                .collect();
            let imports: Vec<ImportBinding> =
                serde_json::from_value(project["imports"].clone()).unwrap();
            let mut ctx = Context::new(&nodes, &[], &files, &[]);
            for binding in &imports {
                ctx.imports
                    .entry((&binding.file, &binding.local_name))
                    .or_insert(binding);
            }
            for (pass, actual) in [
                ("cross", cross_tier(&ctx)),
                ("requests", test_requests(&ctx)),
            ] {
                let actual =
                    actual.unwrap_or_else(|error| panic!("{} {pass}: {error:?}", project["id"]));
                let mut actual = serde_json::to_value(actual)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .clone();
                let mut expected = project["expected"][pass].as_array().unwrap().clone();
                let sort = |records: &mut Vec<Value>| {
                    records.sort_by_key(|value| serde_json::to_string(value).unwrap())
                };
                sort(&mut actual);
                sort(&mut expected);
                assert_eq!(actual, expected, "{} {pass}", project["id"]);
            }
        }
    }
    #[test]
    fn channel_readers_match_independent_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/tier/cases.json"
        ))
        .unwrap();
        let files = BTreeMap::new();
        let ctx = Context::new(&[], &[], &files, &[]);
        for sample in corpus["samples"].as_array().unwrap() {
            let args = &sample["args"];
            let actual = match sample["fn"].as_str().unwrap() {
                "clientPath" => serde_json::to_value(
                    http::path(args[0].as_str().unwrap(), args[1].as_str()).unwrap(),
                )
                .unwrap(),
                "scorePath" => json!(
                    http::score(
                        &serde_json::from_value::<Vec<String>>(args[0].clone()).unwrap(),
                        &serde_json::from_value::<Vec<String>>(args[1].clone()).unwrap()
                    )
                    .unwrap()
                ),
                "eventMatches" => json!(
                    events::matches(args[0].as_str().unwrap(), args[1].as_str().unwrap()).unwrap()
                ),
                "readRequestPath" => json!(
                    requests::read_path(
                        args[0].as_str().unwrap(),
                        args[1].as_u64().unwrap() as usize,
                        args[2].as_str().unwrap()
                    )
                    .unwrap()
                ),
                "leadingString" => json!(scan::leading(&ctx, args[0].as_str().unwrap()).unwrap()),
                "argumentsAt" => json!(
                    scan::arguments(
                        &ctx,
                        args[0].as_str().unwrap(),
                        args[1].as_u64().unwrap() as usize
                    )
                    .unwrap()
                ),
                name => panic!("unknown reference reader {name}"),
            };
            assert_eq!(actual, sample["expected"], "{sample}");
        }
    }
}
