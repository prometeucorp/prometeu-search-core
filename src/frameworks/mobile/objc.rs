use super::*;
const GENERIC: &[&str] = &[
    "init",
    "description",
    "debugDescription",
    "hash",
    "isEqual",
    "isEqualTo",
    "copy",
    "mutableCopy",
    "class",
    "self",
    "count",
    "length",
    "value",
    "name",
    "data",
    "string",
    "object",
    "add",
    "remove",
    "update",
    "load",
    "save",
    "reload",
    "cancel",
    "start",
    "stop",
    "pause",
    "resume",
    "close",
    "open",
    "show",
    "hide",
    "toString",
    "dealloc",
    "release",
    "retain",
    "autorelease",
];
pub(super) fn names(selector: &str) -> Result<Vec<String>> {
    let first = selector
        .trim_end_matches(':')
        .split(':')
        .next()
        .unwrap_or("");
    if first.is_empty() {
        return Ok(Vec::new());
    }
    let mut names = vec![first.to_owned()];
    let mut add = |name: &str| {
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        }
    };
    if first.starts_with("initWith") {
        add("init")
    }
    if let Some(m) = regex(r"^([a-z][a-zA-Z0-9]*?)(?:With|For|By|In|On|At|From|To|Of|As)[A-Z]")?
        .captures(first)?
    {
        add(&m[1]);
    }
    if !selector.trim_end_matches(':').contains(':')
        && first
            .strip_prefix("set")
            .is_some_and(|tail| tail.as_bytes().first().is_some_and(u8::is_ascii_uppercase))
        && selector.ends_with(':')
    {
        let tail = &first[3..];
        let lower = format!("{}{}", tail[..1].to_lowercase(), &tail[1..]);
        add(&lower);
    }
    Ok(names)
}
pub(super) fn index<'a>(ctx: &Context<'a>) -> Result<HashMap<String, Vec<&'a Node>>> {
    let mut map: HashMap<String, Vec<&Node>> = HashMap::new();
    for node in ctx
        .nodes
        .iter()
        .filter(|node| node.kind == "method" && node.language == "objc")
    {
        ctx.step(1)?;
        for name in names(&node.name)? {
            if name == node.name && !node.name.contains(':') || GENERIC.contains(&name.as_str()) {
                continue;
            }
            map.entry(name).or_default().push(node);
        }
    }
    Ok(map)
}
pub(super) fn resolve(
    ctx: &Context<'_>,
    map: &HashMap<String, Vec<&Node>>,
    reference: &UnresolvedReference,
) -> Result<Option<Binding>> {
    let name = reference.reference_name.rsplit('.').next().unwrap_or("");
    match reference.language.as_deref() {
        Some("swift") => Ok(map
            .get(name)
            .and_then(|nodes| nodes.first())
            .map(|node| binding(node, 0.6))),
        Some("objc") if name.contains(':') => {
            for name in names(name)? {
                for node in ctx.named(&name).filter(|node| {
                    node.language == "swift" && matches!(node.kind.as_str(), "method" | "function")
                }) {
                    let start = node.start_line.saturating_sub(4) as usize;
                    let window = (start..node.start_line as usize)
                        .map(|index| {
                            let line = ctx.line(&node.file_path, index);
                            line.strip_suffix('\r').unwrap_or(line)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    ctx.step(window.len() / 32 + 1)?;
                    if !regex(r"@nonobjc\b")?.is_match(&window)?
                        && regex(r"@objc\b")?.is_match(&window)?
                    {
                        return Ok(Some(binding(node, 0.6)));
                    }
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}
