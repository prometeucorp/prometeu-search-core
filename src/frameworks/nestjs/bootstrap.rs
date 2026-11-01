use super::{BTreeMap, HTTP, Hit, Node, Result, Scan, Scope, join, language, regex, trim};
use anyhow::ensure;
use std::collections::{HashMap, HashSet};
const NEUTRAL: &str = "\0neutral";
#[derive(Default)]
struct Config {
    prefix: String,
    excludes: Vec<String>,
    uri: bool,
    default_version: Option<String>,
    version_prefix: String,
}
struct Bootstrap {
    root: String,
    config: Config,
}
fn version(text: &str) -> Result<Option<String>> {
    let text = trim(text);
    if regex(r"^VERSION_NEUTRAL\b")?.is_match(text)? {
        return Ok(Some(NEUTRAL.into()));
    }
    Ok(regex(r#"^\[?\s*(['"`])([^'"`]*)\1"#)?
        .captures(text)?
        .map(|found| found[2].into()))
}
fn read(scan: &Scan, safe: &str) -> Result<Config> {
    let mut config = Config {
        version_prefix: "v".into(),
        ..Config::default()
    };
    if let Some(found) = regex(r"\.setGlobalPrefix\s*\(")?.find(safe)? {
        let args = scan
            .close(safe, found.end() - 1, true)?
            .map_or("", |end| &safe[found.end()..end]);
        if let Some(literal) = regex(r#"^\s*(['"`])([^'"`$]*)\1"#)?.captures(args)? {
            config.prefix = literal[2].trim_matches('/').into();
            if let Some(found) = regex(r"\bexclude\s*:\s*\[")?.find(args)? {
                let list = scan
                    .close(args, found.end() - 1, true)?
                    .map_or("", |end| &args[found.end()..end]);
                for item in scan.split(list, false)? {
                    let item = trim(item);
                    let path = regex(r#"^(['"])([^'"]*)\1$"#)?
                        .captures(item)?
                        .map(|found| found[2].to_owned())
                        .or(regex(r#"\bpath\s*:\s*(['"])([^'"]*)\1"#)?
                            .captures(item)?
                            .map(|found| found[2].to_owned()));
                    if let Some(path) = path {
                        config.excludes.push(path.trim_start_matches('/').into());
                    }
                }
            }
        }
    }
    if let Some(found) = regex(r"\.enableVersioning\s*\(")?.find(safe)? {
        let args = scan
            .close(safe, found.end() - 1, true)?
            .map_or("", |end| &safe[found.end()..end]);
        let ty = regex(r"\btype\s*:\s*VersioningType\s*\.\s*(\w+)")?.captures(args)?;
        config.uri = ty.is_none_or(|ty| &ty[1] == "URI");
        if let Some(found) = regex(r"\bdefaultVersion\s*:\s*")?.find(args)? {
            config.default_version = version(&args[found.end()..])?;
        }
        if let Some(found) =
            regex(r#"\bprefix\s*:\s*(false|(['"`])([^'"`]*)\2)"#)?.captures(args)?
        {
            config.version_prefix = if &found[1] == "false" {
                String::new()
            } else {
                found[3].into()
            };
        }
    }
    Ok(config)
}
fn excluded(path: &str, patterns: &[String]) -> Result<bool> {
    let route = path.trim_start_matches('/');
    for pattern in patterns {
        let wild = regex(r"(?:\{/?\*\w*\}|/?\(\.\*\)|/?\*\w*)$")?.find(pattern)?;
        let base = wild
            .map_or(pattern.as_str(), |wild| &pattern[..wild.start()])
            .trim_end_matches('/');
        if route == base || wild.is_some() && route.starts_with(&format!("{base}/")) {
            return Ok(true);
        }
    }
    Ok(false)
}
fn chain_start(safe: &str, at: usize, floor: usize) -> usize {
    let bytes = safe.as_bytes();
    let mut start = at;
    loop {
        let mut at = start;
        while at > floor {
            let ch = safe[..at].chars().next_back().unwrap();
            if !crate::search::query_whitespace(ch) {
                break;
            }
            at -= ch.len_utf8();
        }
        if at > 0 && bytes[at - 1] == b')' {
            let mut depth = 0;
            let mut cursor = at;
            let mut found = false;
            while cursor > floor {
                cursor -= 1;
                if bytes[cursor] == b')' {
                    depth += 1
                } else if bytes[cursor] == b'(' {
                    depth -= 1;
                    if depth == 0 {
                        at = cursor;
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                return start;
            }
        }
        let mut name = at;
        while name > floor
            && (bytes[name - 1].is_ascii_alphanumeric()
                || matches!(bytes[name - 1], b'_' | b'$' | b'.'))
        {
            name -= 1
        }
        if name == at || name == 0 || bytes[name - 1] != b'@' {
            return start;
        }
        start = name - 1;
    }
}
fn route_version(
    scan: &Scan,
    safe: &str,
    hit: &Hit<'_>,
    scope: &Scope,
) -> Result<Option<Option<String>>> {
    let from = chain_start(safe, hit.start, scope.start);
    let handler = scan.name_after(safe, hit.end, false)?;
    let to = if let Some(handler) = handler {
        regex(&format!(r"(?<![@\w$]){}\s*\(", regex::escape(&handler)))?
            .find(&safe[hit.end..])?
            .map_or(hit.end, |found| hit.end + found.start())
    } else {
        hit.end
    };
    if let Some(found) = regex(r"@Version\s*\(([^)]*)\)")?.captures(&safe[from..to])? {
        return Ok(Some(version(&found[1])?));
    }
    let from = chain_start(safe, scope.start, 0);
    let end = regex(r"\bclass\b")?
        .find(&safe[scope.start..])?
        .map_or_else(
            || crate::frameworks::rust::unit_end(safe, scope.start, 400),
            |found| scope.start + found.start(),
        );
    let head = &safe[from..end];
    if let Some(found) = regex(r"@Version\s*\(([^)]*)\)")?.captures(head)? {
        return Ok(Some(version(&found[1])?));
    }
    if let Some(found) =
        regex(r"@Controller\s*\(\s*\{[^)]*\bversion\s*:\s*([^,}]+)")?.captures(head)?
    {
        return Ok(Some(version(&found[1])?));
    }
    Ok(None)
}
fn tree(
    scan: &Scan,
    text: &str,
    parent: &str,
    prefixes: &mut HashMap<String, String>,
    depth: usize,
) -> Result<()> {
    ensure!(depth < 256, "module route nesting limit exceeded");
    for object in scan.split(text, true)? {
        let path = super::string(r#"(?:^|[,{\s])path\s*:\s*['"`]([^'"`]*)['"`]"#, object)?;
        let full = join(parent, &path);
        if let Some(found) =
            regex(r"(?:^|[,{\s])module\s*:\s*([A-Za-z_$][\w$]*)")?.captures(object)?
        {
            prefixes
                .entry(found[1].into())
                .or_insert_with(|| full.clone());
        }
        if let Some(children) = scan.array(object, "children")? {
            tree(scan, children, &full, prefixes, depth + 1)?;
        }
    }
    Ok(())
}
fn registrations(scan: &Scan, safe: &str, prefixes: &mut HashMap<String, String>) -> Result<()> {
    let mut resume = 0;
    for found in regex(r"\bRouterModule\s*\.\s*(?:register|forRoot|forChild)\s*\(")?.find_iter(safe)
    {
        let found = found?;
        if found.start() < resume {
            continue;
        }
        let Some(close) = scan.close(safe, found.end() - 1, false)? else {
            continue;
        };
        resume = close + 1;
        let args = trim(&safe[found.end()..close]);
        if args.starts_with('[')
            && let Some(end) = scan.close(args, 0, true)?
        {
            tree(scan, &args[1..end], "", prefixes, 0)?;
        }
    }
    Ok(())
}
fn controllers(
    scan: &Scan,
    safe: &str,
    seen: &mut HashSet<String>,
    out: &mut Vec<(String, String)>,
) -> Result<()> {
    for hit in scan.decorators(safe, "Module")? {
        let Some(class) = scan.name_after(safe, hit.end, true)? else {
            continue;
        };
        let Some(inner) = scan.array(hit.args, "controllers")? else {
            continue;
        };
        for name in inner.split(',').map(trim) {
            if regex(r"^[A-Za-z_$][\w$]*$")?.is_match(name)? && seen.insert(name.into()) {
                out.push((name.into(), class.clone()));
            }
        }
    }
    Ok(())
}
fn config_for<'a>(bootstraps: &'a [Bootstrap], file: &str) -> Option<&'a Config> {
    let root = bootstraps
        .iter()
        .filter(|bootstrap| file.starts_with(&bootstrap.root))
        .max_by_key(|bootstrap| bootstrap.root.len())
        .map(|bootstrap| bootstrap.root.as_str())
        .or_else(|| {
            let first = bootstraps.first()?;
            (bootstraps
                .iter()
                .all(|bootstrap| bootstrap.root == first.root))
            .then_some(first.root.as_str())
        })?;
    bootstraps
        .iter()
        .filter(|bootstrap| bootstrap.root == root)
        .find(|bootstrap| !bootstrap.config.prefix.is_empty() || bootstrap.config.uri)
        .or_else(|| bootstraps.iter().find(|bootstrap| bootstrap.root == root))
        .map(|bootstrap| &bootstrap.config)
}
pub(crate) fn post_extract(nodes: &[Node], files: &BTreeMap<String, String>) -> Result<Vec<Node>> {
    let scan = Scan::default();
    let mut prefixes = HashMap::new();
    let mut controller_modules = Vec::new();
    let mut seen = HashSet::new();
    let mut bootstraps = Vec::new();
    for (file, source) in files {
        if !regex(r"\.(m?[jt]s|cjs)$")?.is_match(file)? {
            continue;
        }
        let is_module = regex(r"\.module\.(m?[jt]s|cjs)$")?.is_match(file)?;
        if !is_module && !source.contains("NestFactory") {
            continue;
        }
        if source.is_empty() {
            continue;
        }
        let safe = crate::synthesis::comments::strip_for_regex(source, language(file));
        if is_module {
            registrations(&scan, &safe, &mut prefixes)?;
            controllers(&scan, &safe, &mut seen, &mut controller_modules)?;
        }
        if regex(r"\bNestFactory\s*\.\s*create")?.is_match(&safe)? {
            let root = regex(r"^((?:[^/]+/)*?)src/")?
                .captures(file)?
                .map_or(String::new(), |found| found[1].into());
            bootstraps.push(Bootstrap {
                root,
                config: read(&scan, &safe)?,
            });
        }
    }
    let mut plan = HashMap::<&str, (&Node, String, Option<&Config>, Option<Option<String>>)>::new();
    for (controller, module) in controller_modules {
        let Some(prefix) = prefixes
            .get(&module)
            .filter(|prefix| !prefix.is_empty() && *prefix != "/")
        else {
            continue;
        };
        let mut classes = nodes
            .iter()
            .filter(|node| node.kind == "class" && node.name == controller)
            .collect::<Vec<_>>();
        classes.sort_by(|a, b| {
            (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
        });
        for class in classes {
            for route in nodes {
                scan.step(1)?;
                if route.kind == "route"
                    && route.file_path == class.file_path
                    && route.start_line >= class.start_line
                    && route.start_line <= class.end_line
                {
                    plan.insert(&route.id, (route, prefix.clone(), None, None));
                }
            }
        }
    }
    for (file, source) in files {
        if !regex(r"\.(m?[jt]s|cjs)$")?.is_match(file)? || !source.contains("@Controller") {
            continue;
        }
        let routes = nodes
            .iter()
            .filter(|route| {
                route.kind == "route"
                    && route.file_path == *file
                    && regex(r"::(?:GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS|ALL):")
                        .unwrap()
                        .is_match(&route.qualified_name)
                        .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if routes.is_empty() {
            continue;
        }
        let config = config_for(&bootstraps, file);
        let safe = crate::synthesis::comments::strip_for_regex(source, language(file));
        let scopes = scan.scopes(&safe)?;
        let hits = scan.decorators(&safe, HTTP)?;
        for route in routes {
            scan.step(1)?;
            let Some(hit) = hits.iter().find(|hit| {
                safe[..hit.start]
                    .bytes()
                    .filter(|&byte| byte == b'\n')
                    .count() as u32
                    + 1
                    == route.start_line
            }) else {
                continue;
            };
            let Some(scope) = scopes.iter().find(|scope| {
                scope.start <= hit.start && hit.start < scope.end && scope.kind == "controller"
            }) else {
                continue;
            };
            let module = plan
                .get(route.id.as_str())
                .map_or(String::new(), |(_, prefix, _, _)| prefix.clone());
            let version = route_version(&scan, &safe, hit, scope)?;
            plan.insert(&route.id, (route, module, config, version));
        }
    }
    let mut updates = Vec::new();
    for (route, module, config, version) in plan.into_values() {
        let Some((_, tail)) = route.qualified_name.split_once("::") else {
            continue;
        };
        let Some((method, path)) = tail.split_once(':') else {
            continue;
        };
        let inner = join(&module, path);
        let mut path = inner.clone();
        if let Some(config) = config {
            let version = version.as_ref().unwrap_or(&config.default_version);
            let version = if config.uri {
                version
                    .as_deref()
                    .filter(|version| *version != NEUTRAL)
                    .map(|version| format!("{}{version}", config.version_prefix))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let prefix = if !config.prefix.is_empty() && !excluded(&inner, &config.excludes)? {
                config.prefix.as_str()
            } else {
                ""
            };
            path = join(&join(prefix, &version), &inner);
        }
        let name = format!("{method} {path}");
        if name != route.name {
            let mut node = route.clone();
            node.name = name;
            updates.push(node);
        }
    }
    Ok(updates)
}
