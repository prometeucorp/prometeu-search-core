//! Literal HTTP destinations and deterministic route scoring.
use super::{Context, Edge, Facts, Node, Result, Session, Site, emit, regex, scan};
use crate::frameworks::frontend::href::HOLE;
use std::collections::HashSet;
const VERBS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "ALL", "ANY",
];
pub(super) struct Route<'a> {
    pub node: &'a Node,
    method: String,
    segments: Vec<String>,
}
#[derive(Debug, serde::Serialize)]
pub(super) struct Path {
    #[serde(rename = "segs")]
    pub segments: Vec<String>,
    pub suffix: bool,
    pub display: String,
}
pub(super) fn routes<'a>(ctx: &Context<'a>) -> Vec<Route<'a>> {
    ctx.nodes
        .iter()
        .filter(|node| node.kind == "route")
        .filter_map(|node| {
            let (method, path) = node.name.split_once(' ')?;
            let method = method.to_uppercase();
            let path = scan::trim(path);
            (VERBS.contains(&method.as_str()) && path.starts_with('/')).then(|| Route {
                node,
                method,
                segments: path
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .collect()
}
pub(super) fn score(client: &[String], route: &[String]) -> Result<Option<usize>> {
    let parameter = regex(r"^(?::|\{|\[|<|\*)|\?$")?;
    let catch_all = regex(r"^(?:\*|\[\.\.\.|\{\*|:[\w$]+\*$|\{[\w$]+:\*\}|\*[\w$]*$)")?;
    let mut score = 0;
    let mut at = 0;
    for segment in route {
        if catch_all.is_match(segment)? {
            if at >= client.len() {
                return Ok(None);
            }
            score += client.len() - at;
            at = client.len();
            continue;
        }
        let Some(value) = client.get(at) else {
            return Ok(None);
        };
        if parameter.is_match(segment)? {
            score += 2
        } else if value == segment {
            score += 3
        } else {
            return Ok(None);
        }
        at += 1;
    }
    Ok((at == client.len()).then_some(score))
}
pub(super) fn matched<'a, 'b>(
    ctx: &Context<'_>,
    site: &Site<'_>,
    routes: &'b [Route<'a>],
) -> Result<Option<&'b Route<'a>>> {
    let mut best = None;
    let mut best_score = None;
    let mut tied = false;
    for route in routes {
        ctx.step()?;
        if !matches!(route.method.as_str(), "ALL" | "ANY") && route.method != site.method {
            continue;
        }
        let segments = if site.path.suffix {
            if site.path.segments.len() < 2
                || route.segments.len() < site.path.segments.len()
                || route.segments.len() - site.path.segments.len() > 2
            {
                continue;
            }
            &route.segments[route.segments.len() - site.path.segments.len()..]
        } else {
            &route.segments
        };
        let Some(score) = score(&site.path.segments, segments)? else {
            continue;
        };
        if best_score.is_none_or(|old| score > old) {
            best = Some(route);
            best_score = Some(score);
            tied = false
        } else if best_score == Some(score) {
            tied = true
        }
    }
    Ok(if tied { None } else { best })
}
pub(super) fn path(raw: &str, base: Option<&str>) -> Result<Option<Path>> {
    let mut value = raw.split(['?', '#']).next().unwrap_or("").to_owned();
    let mut suffix = false;
    if let Some(absolute) =
        regex(r"(?i)^(?:[a-z][a-z0-9+.-]*:)?//[^/]*(/.*)?$")?.captures(&value)?
    {
        value = absolute.get(1).map_or("/", |found| found.as_str()).into();
    } else if !value.starts_with('/') {
        if let Some(rest) = value.strip_prefix(HOLE) {
            if !rest.starts_with('/') {
                return Ok(None);
            }
            value = rest.into();
            suffix = true;
        } else if let Some(base) = base {
            if let Some(base_path) = path(base, None)? {
                suffix = base_path.suffix;
                value = format!("/{}/{}", base_path.segments.join("/"), value);
            } else {
                if !base.contains(HOLE) {
                    return Ok(None);
                }
                suffix = true;
                value = format!("/{value}");
            }
        } else {
            return Ok(None);
        }
    } else if let Some(base) = base {
        if let Some(base_path) = path(base, None)? {
            if !base_path.segments.is_empty() {
                suffix = base_path.suffix;
                value = format!("/{}/{}", base_path.segments.join("/"), value);
            }
        } else if base.contains(HOLE) {
            suffix = true;
        }
    }
    let segments: Vec<String> = value
        .split('/')
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.contains(HOLE) {
                "*".into()
            } else {
                part.into()
            }
        })
        .collect();
    if !segments.is_empty() && segments.iter().all(|segment| segment == "*") {
        return Ok(None);
    }
    let display = format!(
        "/{}",
        segments
            .iter()
            .map(|segment| if segment == "*" { "${…}" } else { segment })
            .collect::<Vec<_>>()
            .join("/")
    );
    Ok(Some(Path {
        segments,
        suffix,
        display,
    }))
}
fn add<'a>(
    ctx: &Context<'a>,
    facts: &Facts,
    at: usize,
    open: usize,
    verb: Option<&str>,
    base: Option<&str>,
    sites: &mut Vec<Site<'a>>,
) -> Result<()> {
    let file = &facts.file;
    let line = ctx.line_at(file, at);
    if ctx
        .in_file(file)
        .any(|node| node.kind == "route" && node.start_line == line)
    {
        return Ok(());
    }
    let Some(from) = ctx.enclosing(file, line)? else {
        return Ok(());
    };
    let Some(args) = scan::arguments(ctx, &facts.safe, open)? else {
        return Ok(());
    };
    let Some(&first) = args.first().filter(|arg| !arg.is_empty()) else {
        return Ok(());
    };
    let (first, method) = if first
        .trim_start_matches(crate::search::query_whitespace)
        .starts_with('{')
    {
        let Some(url) = regex(r"\burl\s*:\s*")?.find(first)? else {
            return Ok(());
        };
        (
            &first[url.end()..],
            verb.map(str::to_owned)
                .or(scan::method(first)?)
                .unwrap_or("GET".into()),
        )
    } else {
        (
            first,
            verb.map(str::to_owned)
                .or(scan::method(&args[1..].join(","))?)
                .unwrap_or("GET".into()),
        )
    };
    let Some(literal) = scan::leading(ctx, first)? else {
        return Ok(());
    };
    let Some(path) = path(&literal, base)? else {
        return Ok(());
    };
    let callee = regex(r"<.*>$")?
        .replace(&scan::compact(&facts.safe[at..open]), "")
        .into_owned();
    sites.push(Site {
        from,
        line,
        column: super::column(ctx, file, at),
        callee,
        method,
        path,
    });
    Ok(())
}
pub(super) fn collect<'a>(
    session: &mut Session<'_, 'a>,
    file: &str,
    facts: &Facts,
    sites: &mut Vec<Site<'a>>,
) -> Result<()> {
    let generic = r"(?:<[^()<>]*(?:<[^()<>]*>[^()<>]*)*>)?";
    for found in regex(&format!(r"(?:(?:window|globalThis|global)\s*\.\s*)?\b(fetch|\$fetch|ofetch|axios|ky|got|useFetch|useSWR)\s*{generic}\s*\("))?.captures_iter(&facts.safe) {
        let found = found?;let text = found.get(0).unwrap();
        if text.start() > 0 && facts.safe.as_bytes()[text.start()-1] == b'.' && !regex(r"^(?:window|globalThis|global)\s*\.")?.is_match(text.as_str())? { continue }
        add(session.ctx,facts,text.start(),text.end()-1,None,None,sites)?;
    }
    let pattern = format!(
        r"{}((?:this\s*\.\s*)?[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*)\s*\.\s*(get|post|put|patch|delete|head|options|request|\$get|\$post|\$put|\$patch|\$delete)\s*{generic}\s*\(",
        super::CHAIN
    );
    for found in regex(&pattern)?.captures_iter(&facts.safe) {
        let found = found?;
        let text = found.get(0).unwrap();
        if !super::chain_start(&facts.safe, text.start()) {
            continue;
        }
        let receiver = scan::compact(&found[1]);
        let receiver = receiver.strip_prefix("this.").unwrap_or(&receiver);
        let head = receiver.split('.').next().unwrap();
        let tail = receiver.rsplit('.').next().unwrap();
        let client = if let Some(base) = facts.clients.get(head) {
            Some(base.clone())
        } else if let Some((imported, binding)) = session.imported(file, head)? {
            if binding.is_default {
                imported.default_client.clone()
            } else {
                imported.clients.get(&binding.imported_name).cloned()
            }
        } else {
            None
        };
        let client = if client.is_some() { client } else if regex(r"^(?:app|router|route|routes|express|fastify|koa|hono|elysia|apiRouter|v1|v2|r)$")?.is_match(head)? || regex(r"^(?:app|router|route|routes|express|fastify|koa|hono|elysia|apiRouter|v1|v2|r)$")?.is_match(tail)? { None }
        else if regex(r"^(?:axios|ky|got|superagent|http|https|httpClient|httpService|api|apiClient|client|restClient|request|agent|fetcher|instance|\$api|\$http|\$axios|axiosInstance|Axios|HttpClient|backend|server)$")?.is_match(tail)? { Some(None) } else { None };
        let Some(base) = client else { continue };
        let method = found[2].trim_start_matches('$').to_uppercase();
        add(
            session.ctx,
            facts,
            text.start(),
            text.end() - 1,
            if method == "REQUEST" {
                None
            } else {
                Some(&method)
            },
            base.as_deref(),
            sites,
        )?;
    }
    Ok(())
}
pub(super) fn edges(
    ctx: &Context<'_>,
    sites: &[Site<'_>],
    routes: &[Route<'_>],
    output: &mut Vec<Edge>,
    seen: &mut HashSet<(String, String)>,
    tests: bool,
) -> Result<()> {
    for site in sites {
        let Some(route) = matched(ctx, site, routes)? else {
            continue;
        };
        if !tests && route.node.id == site.from.id {
            continue;
        }
        let mut metadata = serde_json::json!({"synthesizedBy":if tests{"test-request"}else{"http-client"},"channel":"http","callee":site.callee,"method":site.method,"href":site.path.display,"registeredAt":format!("{}:{}",route.node.file_path,route.node.start_line)});
        if !tests {
            metadata["tier"] = "client→server".into();
        }
        emit(
            ctx,
            output,
            seen,
            site.from,
            route.node,
            (site.line, site.column),
            metadata,
        )?;
    }
    Ok(())
}
