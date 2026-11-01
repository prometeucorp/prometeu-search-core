//! Play route declarations and Java/Scala controller ownership.
use super::{
    extractors::regex,
    routes::{reference, route},
};
use anyhow::{Result, ensure};
use prometeu_extract::{ExtractionResult, Node, UnresolvedReference, supplemental};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
mod arguments;
mod java;
mod types;
type Overloads<'a> = Vec<(&'a Node, Option<(usize, usize)>)>;

pub(crate) fn is_routes(path: &str) -> bool {
    path == "conf/routes" || path.ends_with("/conf/routes") || path.ends_with(".routes")
}
pub(crate) fn claims(name: &str) -> Result<bool> {
    Ok(regex(r"^[A-Za-z_]\w*\.[A-Za-z_]\w*$")?.is_match(name)?)
}
pub(crate) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let ctx = supplemental::Context::new(path, source, "scala");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    if !is_routes(path) {
        return Ok(out);
    }
    let route_line = regex(r"^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+(\S+)\s+(.+)$")?;
    for (index, line) in source.split('\n').enumerate() {
        let line = line.trim_matches(crate::search::query_whitespace);
        if line.is_empty() || line.starts_with('#') || line.starts_with("->") {
            continue;
        }
        let Some(m) = route_line.captures(line)? else {
            continue;
        };
        let fqn = m[3]
            .split('(')
            .next()
            .unwrap()
            .trim_matches(crate::search::query_whitespace);
        let parts = fqn.split('.').filter(|s| !s.is_empty()).collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        ensure!(
            out.nodes.len() < 100_000,
            "Play route expansion exceeds limit"
        );
        let node = route(&ctx, index as u32 + 1, &m[1], &m[2], 0);
        out.unresolved_references.push(reference(
            &node,
            &parts[parts.len() - 2..].join("."),
            "references",
        ));
        out.nodes.push(node);
    }
    Ok(out)
}

struct Context<'a> {
    sources: &'a BTreeMap<&'a str, &'a str>,
    lines: HashMap<&'a str, Vec<&'a str>>,
    supers: HashMap<&'a str, Vec<&'a Node>>,
    scopes: RefCell<HashMap<String, types::Scope>>,
    ancestor_cache: RefCell<HashMap<String, HashSet<String>>>,
    overloads: RefCell<HashMap<String, Option<Overloads<'a>>>>,
    parameters: RefCell<HashMap<String, Option<Vec<java::Parameter>>>>,
    files: HashMap<&'a str, Vec<&'a Node>>,
    names: HashMap<&'a str, Vec<&'a Node>>,
    qualified: HashMap<&'a str, Vec<&'a Node>>,
    imports: HashMap<&'a str, Vec<(String, String)>>,
    packages: HashSet<&'a str>,
    work: Cell<usize>,
}
impl<'a> Context<'a> {
    fn new(
        nodes: &'a [Node],
        sources: &'a BTreeMap<&'a str, &'a str>,
        edges: &[prometeu_extract::Edge],
    ) -> Result<Self> {
        let mut files = HashMap::<&str, Vec<&Node>>::new();
        let mut names = HashMap::<&str, Vec<&Node>>::new();
        let mut qualified = HashMap::<&str, Vec<&Node>>::new();
        let mut packages = HashSet::new();
        for node in nodes {
            files.entry(&node.file_path).or_default().push(node);
            names.entry(&node.name).or_default().push(node);
            qualified
                .entry(&node.qualified_name)
                .or_default()
                .push(node);
            if node.kind == "namespace"
                && matches!(node.language.as_str(), "java" | "kotlin" | "scala")
            {
                packages.insert(node.qualified_name.as_str());
            }
        }
        for rows in files.values_mut() {
            rows.sort_by(|a, b| (a.start_line, &a.id).cmp(&(b.start_line, &b.id)));
        }
        for rows in names.values_mut().chain(qualified.values_mut()) {
            rows.sort_by(|a, b| {
                (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
            });
        }
        let block = regex(r"/\*[\s\S]*?\*/")?;
        let line = regex(r"//[^\n]*")?;
        let import =
            regex(r"(?m)^\s*import\s+(static\s+)?([\w.]+(?:\.\*)?)(?:\s+as\s+([\w]+))?\s*(?:;|$)")?;
        let mut imports = HashMap::new();
        for (&file, &source) in sources {
            if !files.get(file).is_some_and(|nodes| {
                nodes
                    .iter()
                    .any(|node| matches!(node.language.as_str(), "java" | "kotlin"))
            }) {
                continue;
            }
            let stripped = block.replace_all(source, "");
            let stripped = line.replace_all(&stripped, "");
            let mut mappings = Vec::new();
            for m in import.captures_iter(&stripped) {
                let m = m?;
                let fqn = &m[2];
                if fqn.ends_with(".*") {
                    continue;
                }
                let local = m
                    .get(3)
                    .map_or_else(|| fqn.rsplit('.').next().unwrap(), |m| m.as_str());
                if !local.is_empty() {
                    mappings.push((local.to_owned(), fqn.to_owned()))
                }
            }
            imports.insert(file, mappings);
        }
        let by_id = nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect::<HashMap<_, _>>();
        let mut supers = HashMap::<&str, Vec<&Node>>::new();
        for edge in edges {
            if matches!(edge.kind.as_str(), "extends" | "implements")
                && let (Some(from), Some(to)) = (
                    by_id.get(edge.source.as_str()),
                    by_id.get(edge.target.as_str()),
                )
            {
                supers.entry(from.id.as_str()).or_default().push(to);
            }
        }
        let lines = sources
            .iter()
            .map(|(&file, &source)| {
                (
                    file,
                    source
                        .split('\n')
                        .map(|line| line.strip_suffix('\r').unwrap_or(line))
                        .collect(),
                )
            })
            .collect();
        Ok(Self {
            sources,
            lines,
            supers,
            scopes: RefCell::new(HashMap::new()),
            ancestor_cache: RefCell::new(HashMap::new()),
            overloads: RefCell::new(HashMap::new()),
            parameters: RefCell::new(HashMap::new()),
            files,
            names,
            qualified,
            imports,
            packages,
            work: Cell::new(0),
        })
    }
    fn step(&self, count: usize) -> Result<()> {
        let work = self.work.get().saturating_add(count);
        ensure!(work <= 10_000_000, "Play resolution work limit exceeded");
        self.work.set(work);
        Ok(())
    }
    fn in_file(&self, file: &str) -> Vec<&'a Node> {
        self.files.get(file).cloned().unwrap_or_default()
    }
    fn named(&self, name: &str) -> Vec<&'a Node> {
        self.names.get(name).cloned().unwrap_or_default()
    }
    fn qualified(&self, name: &str) -> Vec<&'a Node> {
        self.qualified.get(name).cloned().unwrap_or_default()
    }
    fn lines(&self, file: &str) -> &[&'a str] {
        self.lines.get(file).map_or(&[], Vec::as_slice)
    }
    fn source(&self, file: &str) -> &'a str {
        self.sources.get(file).copied().unwrap_or("")
    }
    fn outside(&self, name: &str, reference: &UnresolvedReference) -> bool {
        let file = reference.file_path.as_deref().unwrap_or("");
        let Some(binding) = self
            .imports
            .get(file)
            .into_iter()
            .flatten()
            .find(|(local, _)| local == name)
        else {
            return false;
        };
        let call = reference.reference_kind == "calls" && name == reference.reference_name;
        if self.in_file(file).into_iter().any(|node| {
            node.name == name
                && if call {
                    matches!(node.kind.as_str(), "method" | "function")
                } else {
                    matches!(
                        node.kind.as_str(),
                        "class"
                            | "interface"
                            | "enum"
                            | "struct"
                            | "trait"
                            | "type_alias"
                            | "annotation"
                    )
                }
        }) {
            return false;
        }
        let parts = binding.1.split('.').collect::<Vec<_>>();
        !(1..parts.len()).any(|end| self.packages.contains(parts[..end].join(".").as_str()))
    }
}
impl<'a> Context<'a> {
    fn parameters(&self, node: &Node) -> Result<Option<Vec<java::Parameter>>> {
        if let Some(params) = self.parameters.borrow().get(&node.id) {
            return Ok(params.clone());
        }
        let params = if node.language == "java" {
            java::parameters(node.signature.as_deref())?
        } else {
            None
        };
        self.parameters
            .borrow_mut()
            .insert(node.id.clone(), params.clone());
        Ok(params)
    }
    fn overloads(&self, node: &'a Node) -> Result<Option<Overloads<'a>>> {
        if let Some(set) = self.overloads.borrow().get(&node.id) {
            return Ok(set.clone());
        }
        let owner = node
            .qualified_name
            .rsplit_once("::")
            .map_or("", |(owner, _)| owner);
        let siblings = self
            .in_file(&node.file_path)
            .into_iter()
            .filter(|other| {
                other.id != node.id
                    && other.name == node.name
                    && matches!(other.kind.as_str(), "method" | "function")
                    && other
                        .qualified_name
                        .rsplit_once("::")
                        .map_or("", |(owner, _)| owner)
                        == owner
            })
            .collect::<Vec<_>>();
        let mut set = None;
        if !siblings.is_empty() && matches!(node.kind.as_str(), "method" | "function") {
            let mut rows = vec![(node, arguments::arity(self, node)?)];
            for sibling in siblings {
                rows.push((sibling, arguments::arity(self, sibling)?))
            }
            set = Some(rows)
        }
        self.overloads
            .borrow_mut()
            .insert(node.id.clone(), set.clone());
        Ok(set)
    }
    fn retarget(&self, node: &'a Node, reference: &UnresolvedReference) -> Result<&'a Node> {
        if reference.reference_kind != "calls"
            || !matches!(reference.language.as_deref(), Some("java" | "scala"))
        {
            return Ok(node);
        }
        let Some(set) = self.overloads(node)? else {
            return Ok(node);
        };
        let Some(own) = set[0].1 else { return Ok(node) };
        let Some(args) = arguments::list(
            self,
            reference.file_path.as_deref().unwrap_or(""),
            reference.line,
            reference.column,
            &node.name,
        )?
        else {
            return Ok(node);
        };
        if reference.language.as_deref() == Some("java") {
            let nodes = set.iter().map(|(node, _)| *node).collect::<Vec<_>>();
            if let Some(fitting) = java::fitting(self, &nodes, &args, reference)? {
                return Ok(
                    if fitting.iter().any(|other| other.id == node.id)
                        || fitting.len() > 1
                            && node.visibility.as_deref() == Some("private")
                            && Some(&node.file_path) != reference.file_path.as_ref()
                    {
                        node
                    } else {
                        fitting[0]
                    },
                );
            }
        }
        let count = arguments::split(&args).len();
        if count >= own.0 && count <= own.1 {
            return Ok(node);
        }
        let fitting = set[1..]
            .iter()
            .filter(|(_, arity)| arity.is_some_and(|(min, max)| count >= min && count <= max))
            .collect::<Vec<_>>();
        Ok(if fitting.len() == 1 {
            fitting[0].0
        } else {
            node
        })
    }
    fn resolve(&self, reference: &UnresolvedReference) -> Result<Option<&'a Node>> {
        let Some(m) =
            regex(r"^([A-Za-z_]\w*)\.([A-Za-z_]\w*)$")?.captures(&reference.reference_name)?
        else {
            return Ok(None);
        };
        let file = reference.file_path.as_deref().unwrap_or("");
        let route = is_routes(file);
        if !route && reference.language.as_deref() == Some("java") && self.outside(&m[1], reference)
        {
            return Ok(None);
        }
        let owners = self
            .named(&m[1])
            .into_iter()
            .filter(|node| {
                if route {
                    matches!(node.kind.as_str(), "class" | "module")
                } else {
                    matches!(
                        node.kind.as_str(),
                        "class" | "module" | "interface" | "enum"
                    )
                }
            })
            .collect::<Vec<_>>();
        let groups = if route {
            vec![owners]
        } else {
            vec![
                owners
                    .iter()
                    .copied()
                    .filter(|node| Some(&node.language) == reference.language.as_ref())
                    .collect(),
                owners
                    .iter()
                    .copied()
                    .filter(|node| Some(&node.language) != reference.language.as_ref())
                    .collect(),
            ]
        };
        for group in groups {
            let mut members = Vec::new();
            let mut seen = HashSet::new();
            for owner in group {
                let qualified = format!("{}::{}", owner.qualified_name, &m[2]);
                for node in self.in_file(&owner.file_path) {
                    self.step(1)?;
                    if matches!(node.kind.as_str(), "method" | "function")
                        && node.name == m[2]
                        && node.qualified_name == qualified
                        && seen.insert(&node.id)
                    {
                        members.push(node)
                    }
                }
            }
            let Some(&pick) = members
                .iter()
                .find(|node| !route && node.file_path == file)
                .or_else(|| members.first())
            else {
                continue;
            };
            return Ok(Some(if route {
                pick
            } else {
                self.retarget(pick, reference)?
            }));
        }
        Ok(None)
    }
}

pub(crate) fn bindings(
    nodes: &[Node],
    sources: &BTreeMap<&str, &str>,
    edges: &[prometeu_extract::Edge],
    references: &[UnresolvedReference],
) -> Result<Vec<(UnresolvedReference, String)>> {
    let context = Context::new(nodes, sources, edges)?;
    let mut out = Vec::new();
    for reference in references {
        if !matches!(
            reference.language.as_deref(),
            Some("scala" | "java" | "yaml")
        ) || !claims(&reference.reference_name)?
        {
            continue;
        }
        if let Some(target) = context.resolve(reference)? {
            out.push((reference.clone(), target.id.clone()))
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
