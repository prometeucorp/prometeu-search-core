use super::regex;
use anyhow::Result;
use prometeu_extract::Node;

pub(crate) fn resolve<'a>(
    origin: &str,
    name: &str,
    kind: &str,
    named: impl Fn(&str) -> Vec<&'a Node>,
    in_file: impl Fn(&str) -> Vec<&'a Node>,
) -> Result<Option<(&'a Node, f64)>> {
    if let Some(found) = regex(r"^([A-Z][a-zA-Z]+)::(\w+)$")?.captures(name)? {
        for path in [
            format!("app/Models/{}.php", &found[1]),
            format!("app/{}.php", &found[1]),
        ] {
            let nodes = in_file(&path);
            let target = nodes
                .iter()
                .find(|node| node.kind == "method" && node.name == found[2])
                .or_else(|| {
                    nodes
                        .iter()
                        .find(|node| node.kind == "class" && node.name == found[1])
                });
            if let Some(node) = target {
                return Ok(Some((*node, 0.85)));
            }
        }
    }
    if regex(
        r"^(Auth|Cache|DB|Log|Mail|Queue|Session|Storage|Validator|Route|Request|Response)::(\w+)$",
    )?
    .is_match(name)?
        || [
            "route", "view", "config", "env", "app", "abort", "redirect", "response", "request",
            "session", "url", "asset", "mix",
        ]
        .contains(&name)
    {
        return Ok(None);
    }
    let classes = |controller: &str| {
        let relative = controller.replace('\\', "/");
        let short = relative.rsplit('/').next().unwrap();
        let nodes = named(short)
            .into_iter()
            .filter(|node| node.kind == "class" && node.file_path.contains("Controllers"))
            .collect::<Vec<_>>();
        (relative, nodes)
    };
    if kind == "imports" && origin.contains(":RESOURCE:") {
        let (relative, nodes) = classes(name);
        let suffix = format!("/{}.php", relative.to_lowercase());
        let target = nodes
            .iter()
            .find(|node| format!("/{}", node.file_path.to_lowercase()).ends_with(&suffix))
            .or_else(|| {
                if nodes.len() == 1 {
                    nodes.first()
                } else {
                    None
                }
            });
        if let Some(node) = target {
            return Ok(Some((*node, 0.9)));
        }
    }
    if let Some(found) = regex(r"^([A-Za-z_][\w\\]*)@(\w+)$")?.captures(name)? {
        let method = |path: &str| {
            in_file(path)
                .into_iter()
                .find(|node| node.kind == "method" && node.name == found[2])
        };
        let (relative, nodes) = classes(&found[1]);
        if let Some(node) = method(&format!("app/Http/Controllers/{relative}.php")) {
            return Ok(Some((node, 0.9)));
        }
        if relative.contains('/') {
            let suffix = format!("/{}.php", relative.to_lowercase());
            for node in &nodes {
                if format!("/{}", node.file_path.to_lowercase()).ends_with(&suffix)
                    && let Some(node) = method(&node.file_path)
                {
                    return Ok(Some((node, 0.9)));
                }
            }
            if nodes.len() > 1 {
                return Ok(None);
            }
        }
        for node in nodes {
            if let Some(node) = method(&node.file_path) {
                return Ok(Some((node, 0.9)));
            }
        }
    }
    Ok(None)
}
