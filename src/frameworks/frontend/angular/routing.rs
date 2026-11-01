//! App-scoped route tables, redirects, and command-array navigation.
use super::super::{navigation::Destination, tables};
use super::{
    AngularContext, Node, UnresolvedReference, commands_href, href, in_file_path, is_route, join,
    object, parse, regex, segments, static_string, strings, trim,
};
use anyhow::Result;
use std::collections::BTreeMap;
pub(crate) struct AngularTable<'a> {
    pub context: AngularContext<'a>,
    pub routes: tables::Rooted<'a>,
}
impl<'a> AngularTable<'a> {
    pub(crate) fn new(context: AngularContext<'a>) -> Result<Self> {
        let mut routes = tables::Rooted::new();
        let mut by_file = BTreeMap::new();
        let mut nodes = context
            .nodes
            .iter()
            .filter(|node| is_route(node) && node.name.starts_with('/'))
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| {
            (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
        });
        for node in nodes {
            let root = app_root(&context, &node.file_path)?;
            tables::add(routes.entry(root).or_default(), &node.name, node);
            by_file.entry(node.file_path.as_str()).or_insert(node);
        }
        let mut redirect_files = BTreeMap::new();
        for (file, node) in &by_file {
            let own = context.resolved_segments(in_file_path(node), file)?;
            let full = segments(&node.name);
            redirect_files.insert(
                file.to_string(),
                full[..full.len().saturating_sub(own.len())].to_vec(),
            );
        }
        if !by_file.is_empty() {
            let mounts = context.mounts()?;
            for file in &mounts.files {
                redirect_files
                    .entry(file.clone())
                    .or_insert_with(|| mounts.prefix(file).to_vec());
            }
        }
        let mut aliases = Vec::new();
        for (file, prefix) in redirect_files {
            let Some(content) = context.read(&file).filter(|s| s.contains("redirectTo")) else {
                continue;
            };
            let root = app_root(&context, &file)?;
            let root = if routes.contains_key(&root) {
                root
            } else if routes.len() == 1 {
                routes.keys().next().unwrap().clone()
            } else {
                continue;
            };
            for redirect in parse(content)?.redirects {
                let mut from = prefix.clone();
                from.extend(context.resolved_segments(&redirect.from, &file)?);
                let mut to = if redirect.absolute {
                    Vec::new()
                } else {
                    prefix.clone()
                };
                to.extend(context.resolved_segments(&redirect.to, &file)?);
                aliases.push((root.clone(), join(&from), join(&to)));
            }
        }
        for _ in 0..4 {
            let mut added = false;
            for (root, from, to) in &aliases {
                let table = routes.get_mut(root).unwrap();
                if !table.exact.contains_key(from)
                    && let Some(&target) = table.exact.get(to)
                {
                    table.exact.insert(from.clone(), target);
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        for (root, from, to) in &aliases {
            if from != "/**" {
                continue;
            }
            let table = routes.get_mut(root).unwrap();
            if !table.exact.contains_key("/")
                && let Some(&target) = table.exact.get(to)
            {
                table.exact.insert("/".into(), target);
            }
        }
        Ok(Self { context, routes })
    }
    pub(crate) fn for_file(&self, file: &str) -> Option<&tables::Table<'a>> {
        tables::for_file(&self.routes, file)
            .filter(|table| !table.exact.is_empty())
            .or_else(|| (self.routes.len() == 1).then(|| self.routes.values().next().unwrap()))
    }
    pub(crate) fn resolve(
        &self,
        reference: &UnresolvedReference,
        lines: &[&str],
    ) -> Result<Vec<Destination<'a>>> {
        let Some(nav) =
            regex(r"^(?:this\.)?_?[rR]outer\.(navigate|navigateByUrl|createUrlTree|parseUrl)$")?
                .captures(&reference.reference_name)?
        else {
            return Ok(Vec::new());
        };
        if reference.reference_kind != "calls" {
            return Ok(Vec::new());
        }
        let verb = &nav[1];
        let file = reference.file_path.as_deref().unwrap_or("");
        let Some(routes) = self.for_file(file).filter(|table| !table.exact.is_empty()) else {
            return Ok(Vec::new());
        };
        let Some(argument) = href::argument(lines, reference.line, reference.column, verb, 0)?
        else {
            return Ok(Vec::new());
        };
        if let Some(extras) = href::argument(lines, reference.line, reference.column, verb, 1)?
            && regex(r"\brelativeTo\b")?.is_match(&extras)?
        {
            return Ok(Vec::new());
        }
        let owner = self
            .context
            .nodes_in(file)
            .iter()
            .copied()
            .filter(|node| {
                node.kind == "class"
                    && node.start_line <= reference.line
                    && node.end_line >= reference.line
            })
            .max_by_key(|node| node.start_line);
        let Some(href) = self
            .context
            .destination(&argument, file, owner, 0, true)?
            .filter(|href| href.path.starts_with('/'))
        else {
            return Ok(Vec::new());
        };
        Ok(tables::destinations(&href, routes)?
            .into_iter()
            .map(|(node, href)| Destination {
                node,
                framework: "angular-router",
                metadata: serde_json::json!({"href":href.display,"navMethod":verb}),
            })
            .collect())
    }
}
fn app_root(context: &AngularContext<'_>, file: &str) -> Result<String> {
    let mut dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    loop {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        if ["angular.json", "nx.json"].iter().any(|name| {
            let path = format!("{prefix}{name}");
            context.manifests.contains_key(path.as_str())
                || context.sources.contains_key(path.as_str())
        }) {
            return Ok(prefix);
        }
        if dir.is_empty() {
            break;
        }
        dir = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
    }
    tables::app_root(file)
}
pub(crate) fn property_initializer(
    owner: &Node,
    name: &str,
    content: &str,
) -> Result<Option<String>> {
    let body = content
        .split('\n')
        .skip(owner.start_line.saturating_sub(1) as usize)
        .take((owner.end_line - owner.start_line + 1) as usize)
        .collect::<Vec<_>>()
        .join("\n");
    let Some(decl)=regex(&format!(r"(?:^|[\s;{{])(?:(?:public|private|protected|readonly|static|override)\s+)*{}\s*(?::[^=;\n]+)?=\s*",::regex::escape(name)))?.find(&body)?else{return Ok(None)};
    let start = decl.end();
    let mut i = start;
    while i < body.len() {
        let ch = body.as_bytes()[i];
        if matches!(ch, b'"' | b'\'' | b'`') {
            let Some(end) = object::skip_string(&body, i)? else {
                return Ok(None);
            };
            i = end + 1;
            continue;
        }
        if matches!(ch, b'[' | b'{' | b'(') {
            let Some(end) = object::matching(&body, i)? else {
                return Ok(None);
            };
            i = end + 1;
            continue;
        }
        if matches!(ch, b';' | b'\n') {
            break;
        }
        i += 1;
    }
    let value = trim(&body[start..i]);
    Ok((!value.is_empty()).then(|| value.into()))
}
fn arrow_return(function: &str) -> Result<Option<&str>> {
    let Some(arrow) = function.find("=>") else {
        return Ok(None);
    };
    let tail = function[arrow + 2..].trim_start_matches(crate::search::query_whitespace);
    let mut at = function.len() - tail.len();
    if function.as_bytes().get(at) == Some(&b'(') {
        at += 1;
    }
    if function.as_bytes().get(at) == Some(&b'[') {
        return Ok(object::matching(function, at)?
            .filter(|&end| end > at)
            .map(|end| &function[at..end + 1]));
    }
    if function.as_bytes().get(at) == Some(&b'{')
        && let Some(ret) = regex(r"\breturn\s*\[")?.find(&function[at..])?
    {
        let open = at + ret.end() - 1;
        return Ok(object::matching(function, open)?
            .filter(|&end| end > open)
            .map(|end| &function[open..end + 1]));
    }
    Ok(None)
}
impl AngularContext<'_> {
    pub(crate) fn destination(
        &self,
        expr: &str,
        file: &str,
        owner: Option<&Node>,
        depth: usize,
        from_root: bool,
    ) -> Result<Option<href::Href>> {
        Ok(strings::names_somewhere(
            self.destination_inner(expr, file, owner, depth, from_root)?,
        ))
    }
    fn destination_inner(
        &self,
        expr: &str,
        file: &str,
        owner: Option<&Node>,
        depth: usize,
        from_root: bool,
    ) -> Result<Option<href::Href>> {
        let text = trim(expr);
        if text.is_empty() || depth > 4 {
            return Ok(None);
        }
        if text.starts_with('[') {
            return commands_href(text, from_root);
        }
        if let Some(literal) = static_string(text)? {
            return Ok(href::to_href(&literal));
        }
        if text.starts_with(['\'', '"'])
            && let Some((head, _)) = text.split_once('+')
            && let Some(head) = static_string(head)?.filter(|head| head.starts_with('/'))
        {
            return Ok(href::to_href(&format!(
                "{}/{}",
                head.strip_suffix('/').unwrap_or(&head),
                href::HOLE
            )));
        }
        if let Some(concat) = regex(r"(?s)^(.*?)\.concat\s*\((.*)\)$")?.captures(text)? {
            let base = self.destination(&concat[1], file, owner, depth + 1, from_root)?;
            let added = concat[2]
                .split(',')
                .filter(|arg| !trim(arg).is_empty())
                .count();
            return Ok(if added > 0 {
                base.and_then(|base| {
                    href::to_href(&format!(
                        "{}{}",
                        base.path.strip_suffix('/').unwrap_or(&base.path),
                        format!("/{}", href::HOLE).repeat(added)
                    ))
                })
            } else {
                base
            });
        }
        if let Some(call) = regex(
            r"(?s)^((?:this\s*\.\s*)?[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)+)\s*\((.*)\)$",
        )?
        .captures(text)?
        {
            let name = compact(&call[1]);
            let name = name.strip_prefix("this.").unwrap_or(&name);
            let function = self.constant_text(name, file)?;
            return function
                .as_deref()
                .map(arrow_return)
                .transpose()?
                .flatten()
                .map(|array| commands_href(array, from_root))
                .transpose()
                .map(Option::flatten);
        }
        let Some(chain) =
            regex(r"^(?:this\s*\.\s*)?([A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*)$")?
                .captures(text)?
        else {
            return href::parse(text);
        };
        let chain = compact(&chain[1]);
        let parts = chain.split('.').collect::<Vec<_>>();
        if let Some(content) = self.read(file).filter(|s| !s.is_empty())
            && let Some(owner) = owner
            && (text.starts_with("this") || parts.len() == 1)
            && let Some(init) = property_initializer(owner, parts[0], content)?
        {
            let next = if parts.len() == 1 {
                init
            } else {
                format!("{init}.{}", parts[1..].join("."))
            };
            return self.destination(&next, file, Some(owner), depth + 1, from_root);
        }
        let Some(constant) = self
            .constant_text(&parts.join("."), file)?
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        self.destination(&constant, file, owner, depth + 1, from_root)
    }
}
fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|&ch| !crate::search::query_whitespace(ch))
        .collect()
}
