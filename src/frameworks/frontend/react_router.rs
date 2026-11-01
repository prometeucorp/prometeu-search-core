//! React Router destination tables and navigation calls.
use super::{href, navigation::Destination, react_tables::Context, regex, tables};
use anyhow::Result;
use prometeu_extract::{Node, UnresolvedReference};
use serde_json::json;
pub(crate) fn table(nodes: &[Node]) -> Result<tables::Rooted<'_>> {
    let mut rooted = tables::Rooted::new();
    let mut shortened = Vec::new();
    for node in nodes.iter().filter(|node| node.kind == "route") {
        let prefix = format!("route:{}:{}:", node.file_path, node.start_line);
        let Some(rest) = node.id.strip_prefix(&prefix) else {
            continue;
        };
        if !matches!(node.language.as_str(), "tsx" | "jsx")
            || !node.name.starts_with('/')
            || node.name.ends_with('*')
        {
            continue;
        }
        if !rest.starts_with("table:")
            && rest != node.name
            && !(node
                .signature
                .as_deref()
                .is_some_and(|signature| signature.starts_with("route-parts:"))
                && !regex(r"^[A-Z]+:")?.is_match(rest)?)
        {
            continue;
        }
        let root = tables::app_root(&node.file_path)?;
        let path = if node.name.len() > 1 {
            node.name.strip_suffix('/').unwrap_or(&node.name)
        } else {
            &node.name
        };
        tables::add(rooted.entry(root.clone()).or_default(), path, node);
        let mut segments = path.split('/').skip(1).collect::<Vec<_>>();
        while segments.len() > 1
            && segments
                .last()
                .is_some_and(|segment| segment.starts_with(':') && segment.ends_with('?'))
        {
            segments.pop();
            shortened.push((root.clone(), format!("/{}", segments.join("/")), node));
        }
    }
    for (root, path, node) in shortened {
        let table = rooted.get_mut(&root).unwrap();
        if !table.exact.contains_key(&path) {
            tables::add(table, &path, node);
        }
    }
    Ok(rooted)
}
pub(crate) fn verb(name: &str) -> Result<Option<&str>> {
    Ok(
        regex(r"^(?:history|navigate|router)\.(?:push|replace|navigate)$|^(?:navigate|redirect)$")?
            .is_match(name)?
            .then(|| name.rsplit('.').next().unwrap()),
    )
}
pub(crate) struct Router<'a> {
    pub context: Context<'a>,
    pub routes: tables::Rooted<'a>,
}
impl<'a> Router<'a> {
    pub(crate) fn new(context: Context<'a>) -> Result<Self> {
        let routes = table(context.nodes)?;
        Ok(Self { context, routes })
    }
    pub(crate) fn resolve(
        &self,
        reference: &UnresolvedReference,
        lines: &[&str],
        start: Option<u32>,
    ) -> Result<Vec<Destination<'a>>> {
        if reference.reference_kind != "calls"
            || !matches!(
                reference.language.as_deref(),
                Some("typescript" | "javascript" | "tsx" | "jsx")
            )
        {
            return Ok(Vec::new());
        }
        let Some(verb) = verb(&reference.reference_name)? else {
            return Ok(Vec::new());
        };
        let file = reference.file_path.as_deref().unwrap_or("");
        let Some(routes) =
            tables::for_file(&self.routes, file).filter(|routes| !routes.exact.is_empty())
        else {
            return Ok(Vec::new());
        };
        let Some(arg) = href::argument(lines, reference.line, reference.column, verb, 0)? else {
            return Ok(Vec::new());
        };
        let mut destination = href::parse(&arg)?;
        if destination.is_none()
            && let Some(configured) = self.context.configured_href(&arg, file)?
        {
            destination = href::parse(&configured)?;
        }
        if destination.is_none() {
            destination = href::via_local(
                lines,
                reference.line,
                reference.column,
                verb,
                start.unwrap_or(reference.line.saturating_sub(40).max(1)),
            )?;
        }
        let Some(destination) = destination else {
            return Ok(Vec::new());
        };
        Ok(tables::destinations(&destination, routes)?
            .into_iter()
            .map(|(node, href)| Destination {
                node,
                framework: "react-router",
                metadata: json!({"href":href.display,"navMethod":verb}),
            })
            .collect())
    }
}
