//! Vue Router's nested route configuration and named destination table.
use super::{href, object, regex, tables};
use anyhow::Result;
use prometeu_extract::{ExtractionResult, Node, supplemental::Context};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, serde::Serialize)]
struct Layout {
    component: String,
    spec: Option<String>,
}
#[derive(serde::Serialize)]
struct Entry {
    path: String,
    name: Option<String>,
    component: String,
    spec: Option<String>,
    layouts: Vec<Layout>,
    line: u32,
}

pub(super) fn extract(file: &str, content: &str) -> Result<ExtractionResult> {
    let language = if file.ends_with(".vue") {
        "vue"
    } else if regex(r"\.(?:ts|mts|cts)$")?.is_match(file)? {
        "typescript"
    } else {
        "javascript"
    };
    let ctx = Context::new(file, content, language);
    let mut result = ctx.result.clone();
    result.nodes.clear();
    for entry in parse(content, file)? {
        let mut node = super::page_node(&ctx, &entry.path);
        node.id = format!("route:{file}:{}:{}:vue", entry.line, entry.path);
        node.start_line = entry.line;
        node.end_line = entry.line;
        node.is_exported = None;
        let mut reference = crate::frameworks::routes::reference(
            &node,
            &component_name(&entry.component, entry.spec.as_deref()),
            "calls",
        );
        reference.candidates = Some(vec![entry.component]);
        result.unresolved_references.push(reference);
        for layout in entry.layouts {
            result
                .unresolved_references
                .push(crate::frameworks::routes::reference(
                    &node,
                    &format!(
                        "layout:{}",
                        component_name(&layout.component, layout.spec.as_deref())
                    ),
                    "references",
                ));
        }
        result.nodes.push(node);
    }
    Ok(result)
}
fn component_name(component: &str, spec: Option<&str>) -> String {
    spec.map_or_else(
        || component.to_owned(),
        |spec| format!("import:{spec}#{component}"),
    )
}
fn parse(content: &str, file: &str) -> Result<Vec<Entry>> {
    let factory = regex(
        r"\b(?:createRouter|createWebHistory|createWebHashHistory|createMemoryHistory)\s*\(|\bnew\s+(?:VueRouter|Router)\s*\(",
    )?;
    if !factory.is_match(content)?
        && !regex(r"\broutes\s*[:=]\s*\[")?.is_match(content)?
        && !regex(r#"\bfrom\s*['"]vue-router['"]"#)?.is_match(content)?
        && !regex(r"(?:^|/)(?:router|routes)(?:/|\.[cm]?[jt]s$)")?.is_match(file)?
    {
        return Ok(Vec::new());
    }
    let safe = crate::synthesis::comments::strip_for_regex(content, "typescript");
    let openers = regex(
        r"\broutes\s*[:=]\s*\[|\b(?:const|let|var)\s+(?:[A-Za-z_$][\w$]*)?(?:[Rr]outes?|[Rr]outers?)\s*(?::[^=;\n]*)?=\s*([\[{])",
    )?;
    let mut walker = Walker {
        safe: &safe,
        seen: HashSet::new(),
        out: Vec::new(),
    };
    let mut walked = HashSet::new();
    let mut from = 0;
    while let Some(found) = openers.find_from_pos(&safe, from)? {
        let open = found.end() - 1;
        from = found.end();
        let Some(close) = object::matching(&safe, open)? else {
            continue;
        };
        if !walked.insert(open) {
            continue;
        }
        if safe.as_bytes()[open] == b'{' {
            walker.visit(open, close, None, &[], 0)?;
        } else {
            for (start, end) in object::objects(&safe, open + 1, close)? {
                walker.visit(start, end, None, &[], 0)?;
            }
        }
        from = close;
    }
    Ok(walker.out)
}
struct Walker<'a> {
    safe: &'a str,
    seen: HashSet<String>,
    out: Vec<Entry>,
}
impl Walker<'_> {
    fn visit(
        &mut self,
        start: usize,
        end: usize,
        prefix: Option<&str>,
        layouts: &[Layout],
        depth: usize,
    ) -> Result<()> {
        let fields = object::fields(self.safe, start, end)?;
        let Some(path_field) = fields.get("path") else {
            return Ok(());
        };
        let Some(own) = href::read_string(path_field.text.trim_start(), 0)? else {
            return Ok(());
        };
        let mut path = if own.starts_with('/') {
            own.clone()
        } else if let Some(prefix) = prefix {
            if own.is_empty() {
                prefix.to_owned()
            } else {
                format!("{}/{own}", if prefix == "/" { "" } else { prefix })
            }
        } else {
            return Ok(());
        };
        if path.len() > 1 {
            path = path.trim_end_matches('/').to_owned();
        }
        let component = fields
            .get("component")
            .or_else(|| fields.get("components"))
            .map(|f| component(f.text))
            .transpose()?
            .flatten();
        let children = fields.get("children");
        let mut child_claims = false;
        if let Some(children) = children
            && depth < 12
            && let Some(offset) = self.safe[children.at..].find('[')
        {
            let open = children.at + offset;
            if let Some(close) = object::matching(self.safe, open)?
                && close > open
            {
                let mut inner = layouts.to_vec();
                if let Some(component) = &component {
                    inner.push(component.clone());
                }
                for (start, end) in object::objects(self.safe, open + 1, close)? {
                    let child_fields = object::fields(self.safe, start, end)?;
                    let child_own = child_fields
                        .get("path")
                        .map(|f| href::read_string(f.text.trim_start(), 0))
                        .transpose()?
                        .flatten();
                    if child_own
                        .as_deref()
                        .is_some_and(|own| own.is_empty() || own == path)
                    {
                        child_claims = true;
                    }
                    self.visit(start, end, Some(&path), &inner, depth + 1)?;
                }
            }
        }
        let Some(component) = component else {
            return Ok(());
        };
        if children.is_some() && (child_claims || fields.contains_key("redirect")) {
            return Ok(());
        }
        let name = fields
            .get("name")
            .map(|f| href::read_string(f.text.trim_start(), 0))
            .transpose()?
            .flatten();
        if !self
            .seen
            .insert(format!("{path} {}", name.as_deref().unwrap_or("")))
        {
            return Ok(());
        }
        self.out.push(Entry {
            path,
            name,
            component: component.component,
            spec: component.spec,
            layouts: layouts.to_vec(),
            line: 1 + self.safe[..path_field.at]
                .bytes()
                .filter(|&b| b == b'\n')
                .count() as u32,
        });
        Ok(())
    }
}
fn component(value: &str) -> Result<Option<Layout>> {
    if let Some(lazy) = regex(r#"\bimport\s*\(\s*['"`]([^'"`]+)['"`]"#)?.captures(value)? {
        let tail = lazy[1].rsplit('/').next().unwrap_or("");
        let component = regex(r"\.\w+$")?.replace(tail, "").into_owned();
        return Ok((!component.is_empty()).then(|| Layout {
            component,
            spec: Some(lazy[1].to_owned()),
        }));
    }
    Ok(regex(r"^\s*([A-Z][A-Za-z0-9_]*)\s*$")?
        .captures(value)?
        .map(|found| Layout {
            component: found[1].to_owned(),
            spec: None,
        }))
}

#[derive(Default)]
pub(crate) struct App<'a> {
    pub routes: tables::Table<'a>,
    pub names: HashMap<String, &'a Node>,
}
pub(crate) type Table<'a> = BTreeMap<String, App<'a>>;
pub(crate) fn table<'a>(nodes: &'a [Node], sources: &BTreeMap<&str, &str>) -> Result<Table<'a>> {
    let mut table = Table::new();
    let mut files: Vec<(&str, String, Vec<&Node>)> = Vec::new();
    let mut file_index = HashMap::new();
    for node in nodes.iter().filter(|n| n.kind == "route") {
        let config = node.id
            == format!(
                "route:{}:{}:{}:vue",
                node.file_path, node.start_line, node.name
            );
        let nuxt = node.language == "vue"
            && format!("/{}", node.file_path).contains("/pages/")
            && node.id == format!("route:{}:{}:1", node.file_path, node.name);
        if !(config || nuxt) || !node.name.starts_with('/') {
            continue;
        }
        let root = tables::app_root(&node.file_path)?;
        tables::add(
            &mut table.entry(root.clone()).or_default().routes,
            &node.name,
            node,
        );
        if config {
            let index = *file_index
                .entry(node.file_path.as_str())
                .or_insert_with(|| {
                    files.push((&node.file_path, root, Vec::new()));
                    files.len() - 1
                });
            files[index].2.push(node);
        }
    }
    for (file, root, nodes) in files {
        let Some(source) = sources.get(file).filter(|s| !s.is_empty()) else {
            continue;
        };
        let by_path: HashMap<_, _> = nodes
            .into_iter()
            .map(|node| (node.name.as_str(), node))
            .collect();
        for entry in parse(source, file)? {
            if let Some(name) = entry.name.filter(|name| !name.is_empty())
                && let Some(&node) = by_path.get(entry.path.as_str())
            {
                table
                    .get_mut(&root)
                    .unwrap()
                    .names
                    .entry(name)
                    .or_insert(node);
            }
        }
    }
    Ok(table)
}
pub(crate) fn for_file<'a, 'b>(table: &'b Table<'a>, file: &str) -> Option<&'b App<'a>> {
    table
        .iter()
        .filter(|(root, _)| file.starts_with(root.as_str()))
        .max_by_key(|(root, _)| root.len())
        .map(|(_, app)| app)
}
pub(crate) fn name(expression: &str) -> Result<Option<String>> {
    let args = expression.trim();
    if !args.starts_with('{') {
        return Ok(None);
    }
    let Some(found) = regex(r#"\bname\s*:\s*['"`]"#)?.find(args)? else {
        return Ok(None);
    };
    href::read_string(args, found.end() - 1)
}
pub(crate) fn path_object(expression: &str) -> Result<Option<href::Href>> {
    let args = expression.trim();
    if !args.starts_with('{') {
        return Ok(None);
    }
    let Some(found) = regex(r#"\bpath\s*:\s*['"`]"#)?.find(args)? else {
        return Ok(None);
    };
    Ok(href::read_string(args, found.end() - 1)?.and_then(|literal| href::to_href(&literal)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn vue_routes_match_original_extraction_and_helpers() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/frontend/vue-router.json"
        ))
        .unwrap();
        for case in corpus["parse"].as_array().unwrap() {
            assert_eq!(
                json!(
                    parse(
                        case["source"].as_str().unwrap(),
                        case["file"].as_str().unwrap()
                    )
                    .unwrap()
                ),
                case["expected"],
                "{case}"
            );
        }
        for case in corpus["names"].as_array().unwrap() {
            assert_eq!(
                json!(name(case["source"].as_str().unwrap()).unwrap()),
                case["expected"],
                "{case}"
            );
        }
        for case in corpus["extraction"].as_array().unwrap() {
            let result = extract(
                case["path"].as_str().unwrap(),
                case["source"].as_str().unwrap(),
            )
            .unwrap();
            let mut actual =
                json!({"nodes":result.nodes,"references":result.unresolved_references});
            for node in actual["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(actual, case["expected"], "{case}");
        }
    }
}
