//! Next proxy and middleware wiring, including the reference's static matcher subset.
use super::*;
use crate::frameworks::frontend::{href, object, paths};

const FILE_WITH_EXTENSION: &str = r".*\.[^/]*$";
const EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs"];

pub(super) fn edges(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.frameworks.iter().any(|name| name == "nextjs") {
        return Ok(Vec::new());
    }
    let mut apps: Vec<(String, Vec<(&Node, String)>)> = Vec::new();
    for route in ctx.nodes.iter().filter(|n| n.kind == "route") {
        ctx.step()?;
        let Some(file) = paths::next(&route.file_path)? else {
            continue;
        };
        if if file.kind == paths::Kind::Page {
            route.name != file.path
        } else {
            !route.name.ends_with(&format!(" {}", file.path))
        } {
            continue;
        }
        let i = apps
            .iter()
            .position(|(root, _)| *root == file.root)
            .unwrap_or_else(|| {
                apps.push((file.root, Vec::new()));
                apps.len() - 1
            });
        apps[i].1.push((route, file.path));
    }
    let mut output = Vec::new();
    for (root, routes) in apps {
        ctx.steps(routes.len())?;
        let dir = proxy_dir(&root, &routes);
        let Some((file, entry)) = proxy(ctx, &dir, &root)? else {
            continue;
        };
        let safe = comments::strip_for_regex(
            ctx.sources.get(file.as_str()).copied().unwrap_or(""),
            "typescript",
        );
        let entries = matcher(&safe)?
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| Ok((compile(&entry.source)?, entry)))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        for (route, path) in routes {
            ctx.step()?;
            let mut hit = None;
            if let Some(entries) = &entries {
                for (compiled, entry) in entries {
                    ctx.step()?;
                    if let Some(compiled) = compiled
                        && covers(compiled, &path, &route.file_path)?
                    {
                        hit = Some(entry);
                        break;
                    }
                }
                if hit.is_none() {
                    continue;
                }
            }
            let mut edge = call(route, entry, "next-proxy", "");
            edge.metadata = Some(json!({"synthesizedBy":"next-proxy"}));
            if let Some(hit) = hit {
                edge.metadata.as_mut().unwrap()["matcher"] = json!(hit.source);
            }
            edge.metadata.as_mut().unwrap()["registeredAt"] = json!(format!(
                "{file}:{}",
                hit.map_or(entry.start_line, |hit| ctx.line_at(&file, hit.at))
            ));
            ctx.emit(&mut output, edge)?;
        }
    }
    Ok(output)
}

fn proxy_dir(root: &str, routes: &[(&Node, String)]) -> String {
    for dir in ["pages/", "app/"] {
        if routes
            .iter()
            .any(|(route, _)| route.file_path.starts_with(&format!("{root}{dir}")))
        {
            return root.to_owned();
        }
        if routes
            .iter()
            .any(|(route, _)| route.file_path.starts_with(&format!("{root}src/{dir}")))
        {
            return format!("{root}src/");
        }
    }
    root.to_owned()
}

fn pins_before_16(ctx: &Context<'_>, root: &str) -> Result<bool> {
    for manifest in [format!("{root}package.json"), "package.json".to_owned()] {
        let Some(source) = ctx.manifests.get(manifest.as_str()) else {
            continue;
        };
        let Ok(package) = serde_json::from_str::<Value>(source) else {
            continue;
        };
        let range = ["dependencies", "devDependencies", "peerDependencies"]
            .into_iter()
            .find_map(|section| {
                package
                    .get(section)
                    .and_then(|s| s.get("next"))
                    .filter(|value| !value.is_null())
            });
        let Some(range) = range.and_then(Value::as_str) else {
            continue;
        };
        return Ok(regex(r"^[\s^~>=v]*(\d+)")?
            .captures(range)?
            .is_some_and(|found| found[1].parse::<f64>().unwrap_or(f64::INFINITY) < 16.0));
    }
    Ok(false)
}

fn proxy<'a>(ctx: &Context<'a>, dir: &str, root: &str) -> Result<Option<(String, &'a Node)>> {
    let find = |base: &str| {
        EXTENSIONS
            .iter()
            .map(|extension| format!("{dir}{base}.{extension}"))
            .find(|file| ctx.sources.contains_key(file.as_str()))
    };
    let middleware = find("middleware");
    let name = if middleware.is_some() {
        "middleware"
    } else {
        "proxy"
    };
    let file = if let Some(file) = middleware {
        file
    } else {
        let Some(file) = find("proxy") else {
            return Ok(None);
        };
        if pins_before_16(ctx, root)? {
            return Ok(None);
        }
        file
    };
    let entry_kind = |n: &&Node| matches!(n.kind.as_str(), "function" | "constant" | "variable");
    if let Some(entry) = ctx
        .in_file(&file)
        .filter(entry_kind)
        .find(|n| n.is_exported == Some(true) && n.name == name)
    {
        return Ok(Some((file, entry)));
    }
    let safe = comments::strip_for_regex(
        ctx.sources.get(file.as_str()).copied().unwrap_or(""),
        "typescript",
    );
    let mut local = None;
    for pattern in [
        r"\bexport\s+default\s+(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)",
        r"(?m)\bexport\s+default\s+([A-Za-z_$][\w$]*)\s*;?\s*$",
        r"\bexport\s*\{[^}]*?\b([A-Za-z_$][\w$]*)\s+as\s+(?:default|middleware|proxy)\b[^}]*\}(?!\s*from\b)",
    ] {
        if let Some(found) = regex(pattern)?.captures(&safe)? {
            local = Some(found[1].to_owned());
            break;
        }
    }
    let entry = local
        .as_deref()
        .and_then(|local| {
            ctx.in_file(&file)
                .filter(entry_kind)
                .find(|n| n.name == local)
        })
        .or_else(|| ctx.in_file(&file).find(|n| n.kind == "file"));
    Ok(entry.map(|entry| (file, entry)))
}

struct Entry {
    source: String,
    at: usize,
}
fn matcher(safe: &str) -> Result<Option<Vec<Entry>>> {
    let Some(decl) = regex(r"\bexport\s+const\s+config\b[^=;]*=\s*")?.find(safe)? else {
        return Ok(None);
    };
    let open = decl.end();
    let Some(close) = (safe.as_bytes().get(open) == Some(&b'{'))
        .then(|| object::matching(safe, open))
        .transpose()?
        .flatten()
    else {
        return Ok(Some(Vec::new()));
    };
    let fields = object::fields(safe, open, close)?;
    let Some(field) = fields.get("matcher") else {
        return Ok(regex(r"\bmatcher\b")?
            .is_match(&safe[open..close])?
            .then(Vec::new));
    };
    // Reading the field's exact text also keeps quoted keys and shorthand excluded.
    if field.text.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let at = value_after_key(safe, field.at)?;
    if safe.as_bytes().get(at) != Some(&b'[') {
        return Ok(Some(string_entry(safe, at)?.into_iter().collect()));
    }
    let Some(end) = object::matching(safe, at)? else {
        return Ok(Some(Vec::new()));
    };
    let mut entries = Vec::new();
    let mut i = at + 1;
    let space = regex(r"^[\s,]")?;
    while i < end {
        if let Some(matched) = space.find(&safe[i..])? {
            i += matched.end();
            continue;
        }
        let entry = if safe.as_bytes()[i] == b'{' {
            match object::matching(safe, i)? {
                Some(close) => object::fields(safe, i, close)?
                    .get("source")
                    .map(|field| {
                        value_after_key(safe, field.at).and_then(|at| string_entry(safe, at))
                    })
                    .transpose()?
                    .flatten(),
                None => None,
            }
        } else {
            string_entry(safe, i)?
        };
        let Some(entry) = entry else {
            return Ok(Some(Vec::new()));
        };
        entries.push(entry);
        while i < end && safe.as_bytes()[i] != b',' {
            let close = match safe.as_bytes()[i] {
                b'\'' | b'"' | b'`' => Some(object::skip_string(safe, i)?),
                b'{' | b'[' | b'(' => Some(object::matching(safe, i)?),
                _ => None,
            };
            match close {
                Some(Some(end)) => i = end + 1,
                Some(None) => return Ok(Some(Vec::new())),
                None => i += safe[i..].chars().next().unwrap().len_utf8(),
            }
        }
    }
    Ok(Some(entries))
}
fn string_entry(safe: &str, at: usize) -> Result<Option<Entry>> {
    Ok(href::read_string(safe, at)?
        .filter(|source| !source.contains(href::HOLE))
        .map(|source| Entry { source, at }))
}
fn value_after_key(safe: &str, at: usize) -> Result<usize> {
    Ok(at
        + regex(r"^[A-Za-z_$][\w$]*\s*:\s*")?
            .find(object::prefix(&safe[at..], 128))?
            .map_or(0, |matched| matched.end()))
}

fn closing_paren(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0i64;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' => {
                while i < bytes.len() && bytes[i] != b']' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
fn bars(source: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut from = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' => {
                while i < bytes.len() && bytes[i] != b']' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' => {
                let Some(close) = closing_paren(source, i) else {
                    return vec![source];
                };
                i = close;
            }
            b'|' => {
                out.push(&source[from..i]);
                from = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&source[from..]);
    out
}
fn exclusion(branch: &str) -> Result<Option<String>> {
    if regex(r"^(?:\[\^\?\]\*|\.\*|\.\+|\[[^\]]*\]\+)\\\.")?.is_match(branch)? {
        return Ok(Some(FILE_WITH_EXTENSION.to_owned()));
    }
    let Some(literal) = regex(r"^((?:[\w/~-]|\\?\.)+)(\$|\(\?:\/\|\$\))?$")?.captures(branch)?
    else {
        return Ok(None);
    };
    Ok(Some(format!(
        "{}{}",
        regex::escape(&literal[1].replace(r"\.", ".")),
        literal.get(2).map_or("", |m| m.as_str())
    )))
}
fn group(body: &str) -> Result<Option<String>> {
    if [".*", ".+"].contains(&body) {
        return Ok(Some(body.to_owned()));
    }
    if body.starts_with("(?!") {
        let Some(close) = closing_paren(body, 0) else {
            return Ok(None);
        };
        let tail = &body[close + 1..];
        if ![".*", ".+"].contains(&tail) {
            return Ok(None);
        }
        let mut branches = Vec::new();
        for branch in bars(&body[3..close]) {
            let Some(part) = exclusion(branch)? else {
                return Ok(None);
            };
            if !branches.contains(&part) {
                branches.push(part);
            }
        }
        return Ok(Some(format!("(?!{}){tail}", branches.join("|"))));
    }
    let options = bars(body.strip_prefix("?:").unwrap_or(body));
    let literal = regex(r"^[\w./~-]+$")?;
    for option in &options {
        if !literal.is_match(option)? {
            return Ok(None);
        }
    }
    Ok(Some(format!(
        "(?:{})",
        options
            .into_iter()
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join("|")
    )))
}
fn compile(source: &str) -> Result<Option<fancy_regex::Regex>> {
    if !source.starts_with('/') || source.encode_utf16().count() > 2048 {
        return Ok(None);
    }
    let parameter = regex(r"^/:[A-Za-z_]\w*")?;
    let literal = regex(r"^[\w./~-]$")?;
    let mut out = String::new();
    let mut i = 0;
    let bytes = source.as_bytes();
    while i < bytes.len() {
        if let Some(param) = parameter.find(&source[i..])? {
            i += param.end();
            let mut body = "[^/]+".to_owned();
            if bytes.get(i) == Some(&b'(') {
                let Some(close) = closing_paren(source, i) else {
                    return Ok(None);
                };
                let Some(parsed) = group(&source[i + 1..close])? else {
                    return Ok(None);
                };
                body = parsed;
                i = close + 1;
            }
            if matches!(bytes.get(i), Some(b'?' | b'*' | b'+')) {
                out.push_str(&format!("(?:/{body}){}", bytes[i] as char));
                i += 1;
            } else {
                out.push('/');
                out.push_str(&body);
            }
            continue;
        }
        if bytes[i] == b'(' {
            let Some(close) = closing_paren(source, i) else {
                return Ok(None);
            };
            let Some(parsed) = group(&source[i + 1..close])? else {
                return Ok(None);
            };
            if matches!(bytes.get(close + 1), Some(b'?' | b'*' | b'+')) {
                return Ok(None);
            }
            out.push_str(&parsed);
            i = close + 1;
            continue;
        }
        let ch = source[i..].chars().next().unwrap();
        if !literal.is_match(&ch.to_string())? {
            return Ok(None);
        }
        out.push_str(&regex::escape(&ch.to_string()));
        i += ch.len_utf8();
    }
    if regex(r"\.[*+]|\)[*+?]")?
        .find_iter(&out.replace(FILE_WITH_EXTENSION, ""))
        .collect::<std::result::Result<Vec<_>, _>>()?
        .len()
        > 2
    {
        return Ok(None);
    }
    Ok(Some(regex(&format!("^{out}/?$"))?))
}
fn covers(matcher: &fancy_regex::Regex, path: &str, file: &str) -> Result<bool> {
    let parts: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
    let mut filled: Vec<_> = parts
        .iter()
        .map(|part| {
            if part.starts_with(':') {
                "\u{1}"
            } else {
                *part
            }
        })
        .collect();
    if matcher.is_match(&format!("/{}", filled.join("/")))? {
        return Ok(true);
    }
    if let Some(last) = parts.last()
        && last.ends_with('*')
        && file.contains(&format!("[[...{}]]", &last[1..last.len() - 1]))
    {
        filled.pop();
        return Ok(matcher.is_match(&format!("/{}", filled.join("/")))?);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proxy_matchers_match_original_pure_tests() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/synthesis/next-proxy.json"
        ))
        .unwrap();
        for case in corpus["compile"].as_array().unwrap() {
            assert_eq!(
                compile(case["source"].as_str().unwrap()).unwrap().is_some(),
                case["expected"].as_bool().unwrap(),
                "{case}"
            );
        }
        for case in corpus["covers"].as_array().unwrap() {
            let pattern = compile(case["source"].as_str().unwrap()).unwrap().unwrap();
            assert_eq!(
                covers(
                    &pattern,
                    case["path"].as_str().unwrap(),
                    case["file"].as_str().unwrap()
                )
                .unwrap(),
                case["expected"].as_bool().unwrap(),
                "{case}"
            );
        }
        for case in corpus["read"].as_array().unwrap() {
            let actual = matcher(case["source"].as_str().unwrap())
                .unwrap()
                .map(|entries| {
                    entries
                        .into_iter()
                        .map(|entry| json!({"source":entry.source,"at":entry.at}))
                        .collect::<Vec<_>>()
                });
            assert_eq!(json!(actual), case["expected"], "{case}");
        }
    }
}
