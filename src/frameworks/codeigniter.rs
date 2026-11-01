//! PHP controller conventions, loader properties, views, and explicit routes.
use super::{
    extractors::regex,
    routes::{reference, route},
};
use anyhow::{Result, ensure};
use prometeu_extract::{ExtractionResult, Node, UnresolvedReference, supplemental::Context};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
};
const ROUTES: &str = r"(?:^|/)application/(?:modules/[^/]+/)?config/(?:[^/]+/)?routes\.php$";
const CONTROLLER: &str =
    r"^(?:.*?/)?(?:application/controllers|modules/([^/]+)/controllers)/(.+)\.php$";
const MODULE: &str = r"(?:^|/)modules/([^/]+)/";
const PROPERTY: &str = r"(?i)^(?:this->(?:_?ci->)?|_?ci->)([A-Za-z_]\w*)\.([A-Za-z_]\w*)$";
pub(crate) fn claims(name: &str) -> Result<bool> {
    Ok(name.starts_with("ci:") || regex(PROPERTY)?.is_match(name)?)
}
fn controller_url(path: &str) -> Result<Option<String>> {
    let Some(found) = regex(CONTROLLER)?.captures(path)? else {
        return Ok(None);
    };
    let path = found[2].to_lowercase();
    let module = found.get(1).map(|module| module.as_str().to_lowercase());
    Ok(Some(if let Some(module) = module {
        if !path.contains('/') && path == module {
            format!("/{module}")
        } else {
            format!("/{module}/{path}")
        }
    } else {
        format!("/{path}")
    }))
}
fn module(path: &str) -> Result<Option<String>> {
    Ok(regex(MODULE)?.captures(path)?.map(|found| found[1].into()))
}
fn awaited(handler: &str) -> String {
    let parts = handler
        .split('/')
        .filter(|part| !part.is_empty() && !part.contains('$'))
        .collect::<Vec<_>>();
    let last = parts.last().copied().unwrap_or(handler);
    if parts.len() > 1 {
        last.into()
    } else {
        let mut chars = last.chars();
        chars.next().map_or(String::new(), |first| {
            first.to_uppercase().chain(chars).collect()
        })
    }
}
pub(crate) fn extract(path: &str, source: &str) -> Result<ExtractionResult> {
    let ctx = Context::new(path, source, "php");
    let mut out = ctx.result.clone();
    out.nodes.clear();
    let explicit = regex(ROUTES)?.is_match(path)?;
    let url = if explicit {
        None
    } else {
        controller_url(path)?
    };
    if !explicit && url.is_none() {
        return Ok(out);
    }
    let safe = crate::synthesis::comments::strip_for_regex(source, "php");
    let mut add = |at: usize, verb: &str, url: &str, handler: String| {
        let line = ctx.position(at).0;
        let mut node = route(&ctx, line, verb, url, 0);
        node.qualified_name = format!("{path}::route:{verb}:{url}");
        out.unresolved_references
            .push(reference(&node, &handler, "references"));
        out.nodes.push(node);
    };
    if explicit {
        let root = &path[..path.find("application/").unwrap()];
        for found in regex(r#"\$route\s*\[\s*['"]([^'"]+)['"]\s*\](?:\s*\[\s*['"]([A-Za-z]+)['"]\s*\])?\s*=\s*['"]([^'"]*)['"]"#)?.captures_iter(&safe){let found=found?;let pattern=&found[1];let handler=&found[3];if matches!(pattern,"404_override"|"translate_uri_dashes")||handler.is_empty(){continue}let url=if pattern=="default_controller"{"/".into()}else{format!("/{}",pattern.trim_start_matches('/'))};let verb=found.get(2).map_or("ANY",|verb|verb.as_str()).to_uppercase();add(found.get(0).unwrap().start(),&verb,&url,format!("ci:route:{root}|{handler}:{}",awaited(handler)));}
    } else {
        let stem = path
            .rsplit('/')
            .next()
            .unwrap()
            .strip_suffix(".php")
            .unwrap()
            .to_lowercase();
        let mut concrete = false;
        for found in regex(r"(?m)^[ \t]*((?:abstract\s+|final\s+)*)class\s+([A-Za-z_]\w*)")?
            .captures_iter(&safe)
        {
            let found = found?;
            if found[2].to_lowercase() == stem {
                concrete = !found[1].contains("abstract");
                break;
            }
        }
        if !concrete {
            return Ok(out);
        }
        let mut seen = HashSet::new();
        let base = url.unwrap();
        for found in regex(r"(?m)^[ \t]*((?:(?:public|protected|private|static|final|abstract)\s+)*)function\s+&?\s*([A-Za-z_]\w*)\s*\(")?.captures_iter(&safe){let found=found?;let name=&found[2];if found[1].contains("private")||found[1].contains("protected")||name.starts_with('_'){continue}let url=if name.to_lowercase()=="index"{base.clone()}else{format!("{base}/{}",name.to_lowercase())};if seen.insert(url.clone()){add(found.get(0).unwrap().start(),"ANY",&url,format!("ci:method:{path}|{name}"));}}
    }
    Ok(out)
}

pub(crate) struct Bindings<'a> {
    nodes: &'a [Node],
    sources: &'a BTreeMap<&'a str, &'a str>,
    files: HashMap<&'a str, Vec<usize>>,
    controllers: HashMap<String, Vec<usize>>,
    loadable: HashMap<String, Vec<usize>>,
    aliases: RefCell<HashMap<String, HashMap<String, String>>>,
    work: Cell<usize>,
}
impl<'a> Bindings<'a> {
    pub(crate) fn new(nodes: &'a [Node], sources: &'a BTreeMap<&'a str, &'a str>) -> Result<Self> {
        let mut files = HashMap::<&str, Vec<usize>>::new();
        let mut controllers = HashMap::<String, Vec<usize>>::new();
        let mut loadable = HashMap::<String, Vec<usize>>::new();
        for (index, node) in nodes.iter().enumerate() {
            files.entry(&node.file_path).or_default().push(index);
            if node.kind != "class" || node.language != "php" {
                continue;
            }
            if let Some(url) = controller_url(&node.file_path)? {
                controllers.entry(url[1..].into()).or_default().push(index);
            } else if regex(r"(?:^|/)(?:models|libraries)/")?.is_match(&node.file_path)? {
                loadable
                    .entry(node.name.to_lowercase())
                    .or_default()
                    .push(index);
            }
        }
        for nodes_in_file in files.values_mut() {
            nodes_in_file.sort_by(|&a, &b| {
                (nodes[a].start_line, &nodes[a].id).cmp(&(nodes[b].start_line, &nodes[b].id))
            });
        }
        Ok(Self {
            nodes,
            sources,
            files,
            controllers,
            loadable,
            aliases: RefCell::new(HashMap::new()),
            work: Cell::new(0),
        })
    }
    fn step(&self, amount: usize) -> Result<()> {
        let work = self.work.get().saturating_add(amount);
        ensure!(
            work <= 10_000_000,
            "PHP framework binding work limit exceeded"
        );
        self.work.set(work);
        Ok(())
    }
    fn in_file(&self, path: &str) -> impl Iterator<Item = &'a Node> + '_ {
        self.files
            .get(path)
            .into_iter()
            .flatten()
            .map(|&index| &self.nodes[index])
    }
    fn method(
        &self,
        class: &'a Node,
        name: &str,
        supertypes: &impl Fn(&str) -> Vec<&'a Node>,
    ) -> Result<Option<&'a Node>> {
        let name = name.to_lowercase();
        let mut seen = HashSet::new();
        let mut level = vec![class];
        for _ in 0..6 {
            let mut next = Vec::new();
            for class in level {
                self.step(1)?;
                if !seen.insert(class.id.as_str()) {
                    continue;
                }
                let prefix = format!("{}::", class.qualified_name.to_lowercase());
                for node in self.in_file(&class.file_path) {
                    self.step(1)?;
                    if node.kind == "method"
                        && node.name.to_lowercase() == name
                        && node.qualified_name.to_lowercase().starts_with(&prefix)
                    {
                        return Ok(Some(node));
                    }
                }
                next.extend(
                    supertypes(&class.id)
                        .into_iter()
                        .filter(|node| node.kind == "class"),
                );
            }
            if next.is_empty() {
                break;
            }
            level = next;
        }
        Ok(None)
    }
    fn route_handler(
        &self,
        root: &str,
        uri: &str,
        supertypes: &impl Fn(&str) -> Vec<&'a Node>,
    ) -> Result<Option<&'a Node>> {
        let segments = uri
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        let lower = segments
            .iter()
            .map(|segment| segment.to_lowercase())
            .collect::<Vec<_>>();
        for length in (1..=lower.len()).rev() {
            self.step(1)?;
            let Some(classes) = self.controllers.get(&lower[..length].join("/")) else {
                continue;
            };
            let class = &self.nodes[*classes
                .iter()
                .find(|&&index| self.nodes[index].file_path.starts_with(root))
                .unwrap_or(&classes[0])];
            let method = segments.get(length).copied().unwrap_or("index");
            let target = if method.contains('$') {
                None
            } else {
                self.method(class, method, supertypes)?
            };
            return Ok(Some(target.unwrap_or(class)));
        }
        Ok(None)
    }
    fn view(&self, reference: &UnresolvedReference) -> Result<Option<&'a Node>> {
        let path = reference.file_path.as_deref().unwrap_or("");
        let Some(line) = self.sources.get(path).and_then(|source| {
            source
                .split('\n')
                .nth(reference.line.saturating_sub(1) as usize)
        }) else {
            return Ok(None);
        };
        let Some(found) =
            regex(r#"->\s*load\s*->\s*view\s*\(\s*['"]([\w./-]+)['"]"#)?.captures(line)?
        else {
            return Ok(None);
        };
        let view = &found[1];
        let file = if regex(r"\.\w+$")?.is_match(view)? {
            view.into()
        } else {
            format!("{view}.php")
        };
        let root = &path[..path.find("application/").unwrap_or(0)];
        let module = regex(MODULE)?.captures(path)?;
        let modules = module.as_ref().map_or_else(
            || format!("{root}application/modules/"),
            |module| {
                let full = module.get(0).unwrap();
                path[..full.end() - module[1].len() - 1].into()
            },
        );
        let mut candidates = Vec::new();
        if let Some(module) = module {
            candidates.push(format!("{modules}{}/views/{file}", &module[1]));
        }
        if let Some((module, file)) = file.split_once('/')
            && !module.is_empty()
        {
            candidates.push(format!("{modules}{module}/views/{file}"));
        }
        candidates.push(format!("{root}application/views/{file}"));
        for path in candidates {
            if let Some(node) = self.in_file(&path).find(|node| node.kind == "file") {
                return Ok(Some(node));
            }
        }
        Ok(None)
    }
    fn property(&self, name: &str, file: &str) -> Result<Option<&'a Node>> {
        if !self.loadable.contains_key(&name.to_lowercase())
            && !self.aliases.borrow().contains_key(file)
        {
            let mut aliases = HashMap::new();
            let source = self.sources.get(file).copied().unwrap_or("");
            self.step(source.len())?;
            for pattern in [
                r#"->\s*load\s*->\s*model\s*\(\s*['"]([\w/]+)['"]\s*,\s*['"](\w+)['"]"#,
                r#"->\s*load\s*->\s*library\s*\(\s*['"]([\w/]+)['"]\s*,[^,()]*,\s*['"](\w+)['"]"#,
            ] {
                for found in regex(pattern)?.captures_iter(source) {
                    let found = found?;
                    aliases.insert(
                        found[2].into(),
                        found[1].rsplit('/').next().unwrap().to_lowercase(),
                    );
                }
            }
            self.aliases.borrow_mut().insert(file.into(), aliases);
        }
        let alias = if self.loadable.contains_key(&name.to_lowercase()) {
            None
        } else {
            self.aliases
                .borrow()
                .get(file)
                .and_then(|aliases| aliases.get(name))
                .cloned()
        };
        let Some(classes) = self
            .loadable
            .get(&alias.unwrap_or_else(|| name.to_lowercase()))
        else {
            return Ok(None);
        };
        if classes.len() == 1 {
            return Ok(Some(&self.nodes[classes[0]]));
        }
        let Some(module_name) = module(file)? else {
            return Ok(None);
        };
        let mut own = Vec::new();
        for &index in classes {
            self.step(1)?;
            if module(&self.nodes[index].file_path)?.as_deref() == Some(module_name.as_str()) {
                own.push(index);
            }
        }
        Ok((own.len() == 1).then(|| &self.nodes[own[0]]))
    }
    pub(crate) fn resolve(
        &self,
        reference: &UnresolvedReference,
        supertypes: impl Fn(&str) -> Vec<&'a Node>,
    ) -> Result<Option<(String, f64)>> {
        self.step(1)?;
        let name = &reference.reference_name;
        let file = reference.file_path.as_deref().unwrap_or("");
        let target = if let Some(name) = name.strip_prefix("ci:method:") {
            let mut parts = name.split('|');
            let file = parts.next().unwrap();
            let method = parts.next().unwrap_or("");
            return Ok(self
                .in_file(file)
                .find(|node| node.kind == "method" && node.name == method)
                .map(|node| (node.id.clone(), 0.95)));
        } else if let Some(name) = name.strip_prefix("ci:route:") {
            let mut parts = name.split('|');
            let root = parts.next().unwrap();
            let handler = parts.next().unwrap_or("");
            let handler = handler.rsplit_once(':').map_or("", |(handler, _)| handler);
            self.route_handler(root, handler, &supertypes)?
        } else if let Some(found) = regex(PROPERTY)?.captures(name)? {
            if &found[1] == "load" && &found[2] == "view" {
                self.view(reference)?
            } else if let Some(class) = self.property(&found[1], file)? {
                self.method(class, &found[2], &supertypes)?
            } else {
                None
            }
        } else {
            None
        };
        Ok(target.map(|node| (node.id.clone(), 0.9)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn routes_and_loader_bindings_match_independent_reference() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/codeigniter/cases.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let extracted = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut actual =
                json!({"nodes":extracted.nodes,"references":extracted.unresolved_references});
            for node in actual["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(
                actual, case["expected"],
                "{} {}",
                case["path"], case["source"]
            );
        }
        for project in fixture["projects"].as_array().unwrap() {
            let mut nodes = project["nodes"].clone();
            for node in nodes.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0);
            }
            let mut nodes: Vec<Node> = serde_json::from_value(nodes).unwrap();
            nodes.sort_by(|a, b| {
                (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
            });
            let sources = project["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let bindings = Bindings::new(&nodes, &sources).unwrap();
            let by_id = nodes
                .iter()
                .map(|node| (node.id.as_str(), node))
                .collect::<HashMap<_, _>>();
            for query in project["resolutions"].as_array().unwrap() {
                let reference: UnresolvedReference =
                    serde_json::from_value(query["reference"].clone()).unwrap();
                let answer = bindings
                    .resolve(&reference, |id| {
                        project["superclasses"][id]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|id| by_id.get(id.as_str().unwrap()).copied())
                            .collect()
                    })
                    .unwrap();
                assert_eq!(
                    answer
                        .map(|(id, confidence)| json!({"id":id,"confidence":confidence}))
                        .unwrap_or(Value::Null),
                    query["expected"],
                    "{} {}",
                    project["name"],
                    query["reference"]
                );
            }
        }
    }
}
