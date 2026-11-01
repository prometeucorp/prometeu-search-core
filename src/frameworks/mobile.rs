//! Snapshot-based Swift and JavaScript/native framework bindings.
use super::frontend::ModuleResolver;
use super::source_pattern as regex;
use crate::synthesis::ImportBinding;
use anyhow::{Result, ensure};
use prometeu_extract::{Node, UnresolvedReference};
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
mod objc;
mod react_native;
mod swift;
mod type_visibility;
mod vapor;
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Target {
    pub target_node_id: String,
}
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Binding {
    pub target_node_id: String,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub also_targets: Vec<Target>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}
fn binding(node: &Node, confidence: f64) -> Binding {
    Binding {
        target_node_id: node.id.clone(),
        confidence,
        also_targets: Vec::new(),
        metadata: None,
    }
}
struct Context<'a> {
    nodes: &'a [Node],
    by_id: HashMap<&'a str, &'a Node>,
    sources: &'a BTreeMap<&'a str, &'a str>,
    line_checkpoints: HashMap<&'a str, Vec<usize>>,
    by_name: HashMap<&'a str, Vec<&'a Node>>,
    by_file: HashMap<&'a str, Vec<&'a Node>>,
    qualified: HashMap<&'a str, Vec<&'a Node>>,
    imports: HashMap<&'a str, Vec<&'a ImportBinding>>,
    resolve_module: &'a ModuleResolver<'a>,
    work: Cell<usize>,
}
impl<'a> Context<'a> {
    fn step(&self, count: usize) -> Result<()> {
        let used = self.work.get().saturating_add(count);
        ensure!(used <= 10_000_000, "mobile binding work limit exceeded");
        self.work.set(used);
        Ok(())
    }
    fn named(&self, name: &str) -> impl Iterator<Item = &'a Node> + '_ {
        self.by_name.get(name).into_iter().flatten().copied()
    }
    fn in_file(&self, path: &str) -> impl Iterator<Item = &'a Node> + '_ {
        self.by_file.get(path).into_iter().flatten().copied()
    }
    // One checkpoint per 64 lines bounds lookup scans without a per-character
    // position table, even for newline-heavy source snapshots.
    fn line(&self, path: &str, line: usize) -> &'a str {
        let Some(&start) = self
            .line_checkpoints
            .get(path)
            .and_then(|points| points.get(line / 64))
        else {
            return "";
        };
        self.source(path)[start..]
            .split('\n')
            .nth(line % 64)
            .unwrap_or("")
    }
    fn source(&self, path: &str) -> &'a str {
        self.sources.get(path).copied().unwrap_or("")
    }
}
#[derive(Clone)]
struct NativeBinding {
    module: String,
    ty: Option<String>,
}
pub(crate) struct Bindings<'a> {
    ctx: Context<'a>,
    visibility: type_visibility::Visibility,
    objc: Option<HashMap<String, Vec<&'a Node>>>,
    native: Option<react_native::Maps<'a>>,
    expo: HashMap<(String, String), Option<NativeBinding>>,
}
impl<'a> Bindings<'a> {
    pub(crate) fn new(
        nodes: &'a [Node],
        sources: &'a BTreeMap<&'a str, &'a str>,
        imports: &'a [ImportBinding],
        resolve_module: &'a ModuleResolver<'a>,
    ) -> Result<Self> {
        let mut ctx = Context {
            nodes,
            by_id: HashMap::new(),
            sources,
            line_checkpoints: HashMap::new(),
            by_name: HashMap::new(),
            by_file: HashMap::new(),
            qualified: HashMap::new(),
            imports: HashMap::new(),
            resolve_module,
            work: Cell::new(0),
        };
        for (&path, &source) in sources {
            let mut points = vec![0];
            let mut line = 0usize;
            for (offset, byte) in source.bytes().enumerate() {
                if byte == b'\n' {
                    line += 1;
                    if line.is_multiple_of(64) {
                        points.push(offset + 1)
                    }
                }
            }
            ctx.line_checkpoints.insert(path, points);
        }
        for node in nodes {
            ctx.by_id.insert(&node.id, node);
            ctx.by_name.entry(&node.name).or_default().push(node);
            ctx.by_file.entry(&node.file_path).or_default().push(node);
            ctx.qualified
                .entry(&node.qualified_name)
                .or_default()
                .push(node);
        }
        for import in imports {
            ctx.imports.entry(&import.file).or_default().push(import);
        }
        Ok(Self {
            ctx,
            visibility: type_visibility::Visibility::default(),
            objc: None,
            native: None,
            expo: HashMap::new(),
        })
    }
    pub(crate) fn resolve(
        &mut self,
        framework: &str,
        reference: &UnresolvedReference,
    ) -> Result<Option<Binding>> {
        self.ctx.step(1)?;
        match framework {
            "swiftui" | "uikit" => swift::resolve(&self.ctx, framework, reference),
            "vapor" => vapor::resolve(&self.ctx, reference),
            "swift-objc-bridge" => {
                if self.objc.is_none() {
                    self.objc = Some(objc::index(&self.ctx)?);
                }
                objc::resolve(&self.ctx, self.objc.as_ref().unwrap(), reference)
            }
            "react-native-bridge" => {
                if self.native.is_none() {
                    self.native = Some(react_native::build(&self.ctx)?);
                }
                react_native::resolve(&self.ctx, self.native.as_ref().unwrap(), reference)
            }
            "expo-modules-js" => self.expo_reference(reference),
            _ => Ok(None),
        }
    }
    pub(crate) fn swift_type_target(
        &mut self,
        reference: &UnresolvedReference,
        result: Binding,
    ) -> Result<Option<Binding>> {
        self.visibility.gate(&self.ctx, reference, result)
    }
    pub(crate) fn swift_exact_type(
        &self,
        reference: &UnresolvedReference,
    ) -> Result<Option<Binding>> {
        swift::exact_type(&self.ctx, reference)
    }
    fn expo_binding(&mut self, name: &str, file: &str) -> Result<Option<NativeBinding>> {
        let key = (file.to_owned(), name.to_owned());
        if let Some(found) = self.expo.get(&key) {
            return Ok(found.clone());
        }
        let native = r#"\brequire(?:Optional)?NativeModule\s*(?:<\s*([A-Za-z_$][\w$]*)[^>]*>)?\s*\(\s*['"]([A-Za-z_$][\w$]*)['"]\s*\)"#;
        let parse = |source: &str, pattern: &str| -> Result<Option<NativeBinding>> {
            self.ctx.step(source.len() / 32 + 1)?;
            let safe = crate::synthesis::comments::strip_for_regex(source, "typescript");
            Ok(regex(pattern)?.captures(&safe)?.map(|m| NativeBinding {
                module: m[2].into(),
                ty: m.get(1).map(|m| m.as_str().to_owned()),
            }))
        };
        let source = self.ctx.source(file);
        let mut found = if source.contains("NativeModule") {
            parse(
                source,
                &format!(
                    r"\b(?:const|let|var)\s+{}\s*(?::[^=]+)?=\s*{native}",
                    regex::escape(name)
                ),
            )?
        } else {
            None
        };
        if found.is_none()
            && let Some(import) = self
                .ctx
                .imports
                .get(file)
                .into_iter()
                .flatten()
                .find(|m| m.local_name == name)
            && import.source.starts_with('.')
            && let Some(target) =
                (self.ctx.resolve_module)(&import.source, file, script_language(file))?
        {
            let source = self.ctx.source(&target);
            if source.contains("NativeModule") {
                let pattern = if import.is_default {
                    format!(r"\bexport\s+default\s+{native}")
                } else {
                    format!(
                        r"\bexport\s+(?:const|let)\s+{}\s*(?::[^=]+)?=\s*{native}",
                        regex::escape(&import.imported_name)
                    )
                };
                found = parse(source, &pattern)?;
            }
        }
        self.expo.insert(key, found.clone());
        Ok(found)
    }
    fn expo_reference(&mut self, reference: &UnresolvedReference) -> Result<Option<Binding>> {
        if reference.reference_kind != "calls" {
            return Ok(None);
        }
        let Some(m) = regex(r"^([A-Za-z_$][\w$]*)\.([A-Za-z_$][\w$]*)$")?
            .captures(&reference.reference_name)?
        else {
            return Ok(None);
        };
        let Some(found) = self.expo_binding(&m[1], reference.file_path.as_deref().unwrap_or(""))?
        else {
            return Ok(None);
        };
        let name = &m[2];
        self.ctx.step(self.ctx.named(name).count())?;
        let mut declarations: Vec<_> = self
            .ctx
            .named(name)
            .filter(|node| {
                node.id.starts_with("expo-module:")
                    && node
                        .qualified_name
                        .ends_with(&format!("::{}.{name}", found.module))
            })
            .collect();
        declarations.sort_by_key(|node| usize::from(node.language != "swift"));
        if let Some(&first) = declarations.first() {
            let mut result = binding(first, 0.95);
            result.also_targets = declarations[1..]
                .iter()
                .map(|node| Target {
                    target_node_id: node.id.clone(),
                })
                .collect();
            return Ok(Some(result));
        }
        Ok(found.ty.and_then(|ty| {
            self.ctx
                .named(name)
                .find(|node| {
                    matches!(node.kind.as_str(), "method" | "property" | "field")
                        && node.qualified_name.ends_with(&format!("{ty}::{name}"))
                })
                .map(|node| binding(node, 0.9))
        }))
    }
}
fn script_language(file: &str) -> &'static str {
    if file.ends_with(".tsx") {
        "tsx"
    } else if [".ts", ".mts", ".cts"]
        .iter()
        .any(|ext| file.ends_with(ext))
    {
        "typescript"
    } else if file.ends_with(".jsx") {
        "jsx"
    } else {
        "javascript"
    }
}
pub(crate) use react_native::extract;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framework_snapshots_accept_large_repository_totals() {
        let template: Node = serde_json::from_value(json!({
            "id":"n0","name":"run","qualifiedName":"run","kind":"function",
            "filePath":"main.swift","language":"swift","startLine":1,"endLine":1,
            "startColumn":0,"endColumn":1,"updatedAt":0
        }))
        .unwrap();
        let nodes = (0..100_001)
            .map(|index| Node {
                id: format!("n{index}"),
                ..template.clone()
            })
            .collect::<Vec<_>>();
        let source = " ".repeat(1024 * 1024);
        let paths = (0..65)
            .map(|index| format!("payload_{index}.txt"))
            .collect::<Vec<_>>();
        let sources = paths
            .iter()
            .map(|path| (path.as_str(), source.as_str()))
            .collect();
        let module = |_: &str, _: &str, _: &str| Ok(None);
        let mobile = Bindings::new(&nodes, &sources, &[], &module).unwrap();
        assert_eq!(mobile.ctx.by_id["n100000"].id, "n100000");
        drop(mobile);
        let package = |_: &str, _: &str| None;
        let go = crate::frameworks::go_bindings::Bindings::new(&nodes, &sources, &package).unwrap();
        drop(go);
        assert!(
            crate::frameworks::play::bindings(&nodes, &sources, &[], &[])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn matches_independent_mobile_resolvers_and_macro_records() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/frameworks/mobile.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let mut raw = case["nodes"].clone();
            for node in raw.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0)
            }
            let nodes: Vec<Node> = serde_json::from_value(raw).unwrap();
            let sources_owned: BTreeMap<String, String> =
                serde_json::from_value(case["sources"].clone()).unwrap();
            let sources = sources_owned
                .iter()
                .map(|(p, s)| (p.as_str(), s.as_str()))
                .collect();
            let imports: Vec<ImportBinding> =
                serde_json::from_value(case["imports"].clone()).unwrap();
            let module = |specifier: &str, from: &str, language: &str| -> Result<Option<String>> {
                let answer = case["moduleAnswers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| {
                        a["specifier"] == specifier
                            && a["from"] == from
                            && a["language"] == language
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "unrecorded module lookup: {specifier} {from} {language} {}",
                            case["id"]
                        )
                    });
                Ok(answer["answer"].as_str().map(str::to_owned))
            };
            let mut bindings = Bindings::new(&nodes, &sources, &imports, &module).unwrap();
            let references: Vec<UnresolvedReference> =
                serde_json::from_value(case["references"].clone()).unwrap();
            let actual: Vec<_> = references
                .iter()
                .map(|r| {
                    bindings
                        .resolve(case["framework"].as_str().unwrap(), r)
                        .unwrap()
                })
                .collect();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "{} {}",
                case["id"],
                case["origin"]
            );
        }
        for case in corpus["exactTypes"].as_array().unwrap() {
            let mut raw = case["nodes"].clone();
            for node in raw.as_array_mut().unwrap() {
                node["updatedAt"] = json!(0)
            }
            let nodes: Vec<Node> = serde_json::from_value(raw).unwrap();
            let owned: BTreeMap<String, String> =
                serde_json::from_value(case["sources"].clone()).unwrap();
            let sources = owned
                .iter()
                .map(|(p, s)| (p.as_str(), s.as_str()))
                .collect();
            let module = |_: &str, _: &str, _: &str| Ok(None);
            let bindings = Bindings::new(&nodes, &sources, &[], &module).unwrap();
            let reference = serde_json::from_value(case["reference"].clone()).unwrap();
            assert_eq!(
                serde_json::to_value(bindings.swift_exact_type(&reference).unwrap()).unwrap(),
                case["expected"],
                "{}",
                case["origin"]
            );
        }
        for case in corpus["extracts"].as_array().unwrap() {
            let mut actual = serde_json::to_value(
                extract(
                    case["file"].as_str().unwrap(),
                    case["source"].as_str().unwrap(),
                )
                .unwrap()
                .nodes,
            )
            .unwrap();
            for node in actual.as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("updatedAt");
            }
            assert_eq!(actual, case["expected"]["nodes"], "{}", case["origin"]);
        }
    }
}
