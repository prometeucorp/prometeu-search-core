//! Framework answers computed once after declared inheritance is available.
use super::*;
#[derive(Default)]
pub(in crate::graph) struct FrameworkBindings {
    references: HashMap<(usize, String, String, usize, usize), Vec<usize>>,
    metadata: HashMap<(usize, usize, String, u32, u32), BindingMetadata>,
}
pub(in crate::graph) struct BindingMetadata {
    pub confidence: f64,
    pub framework: &'static str,
    pub resolved_by: &'static str,
    pub secondary: bool,
    pub extra: Option<serde_json::Value>,
    pub by_name: bool,
    pub uncertain: bool,
    pub edge_kind: Option<&'static str>,
}
impl FrameworkBindings {
    fn insert(
        &mut self,
        from: usize,
        reference: &prometeu_extract::UnresolvedReference,
        targets: Vec<usize>,
        confidence: f64,
        framework: &'static str,
        extra: Option<serde_json::Value>,
    ) {
        for (ordinal, &to) in targets.iter().enumerate() {
            self.metadata.insert(
                (
                    from,
                    to,
                    reference.reference_name.clone(),
                    reference.line,
                    reference.column,
                ),
                BindingMetadata {
                    confidence,
                    framework,
                    resolved_by: "framework",
                    secondary: ordinal > 0,
                    extra: (ordinal == 0).then(|| extra.clone()).flatten(),
                    by_name: false,
                    uncertain: false,
                    edge_kind: None,
                },
            );
        }
        self.references.insert(
            (
                from,
                reference.reference_name.clone(),
                reference.reference_kind.clone(),
                reference.line as usize,
                reference.column as usize,
            ),
            targets,
        );
    }
}
impl Graph {
    pub(in crate::graph) fn initialize_framework_bindings(
        &mut self,
        inheritance_only: bool,
    ) -> Result<()> {
        let active = |name: &str| self.frameworks.iter().any(|framework| framework == name);
        let python = ["django", "flask", "fastapi"]
            .iter()
            .any(|name| active(name));
        let javascript = self.files.values().any(|file| {
            matches!(
                file.analysis.language.as_str(),
                "typescript" | "tsx" | "javascript" | "jsx"
            )
        });
        let python_files = self
            .files
            .values()
            .any(|file| file.analysis.language == "python");
        let go_files = self
            .files
            .values()
            .any(|file| file.analysis.language == "go");
        let callable_values = self.files.values().any(|file| {
            super::function_refs::handled(&file.analysis.language)
                && file.analysis.extraction.as_ref().is_some_and(|extraction| {
                    extraction.unresolved_references.iter().any(|reference| {
                        reference.reference_kind == "function_ref"
                            || file.analysis.language == "pascal"
                                && reference.reference_name.starts_with('{')
                    })
                })
        });
        let cpp = self
            .files
            .values()
            .any(|file| matches!(file.analysis.language.as_str(), "c" | "cpp"));
        if inheritance_only && !python_files && !cpp {
            return Ok(());
        }
        let mobile_names = [
            "swiftui",
            "uikit",
            "vapor",
            "swift-objc-bridge",
            "react-native-bridge",
            "expo-modules-js",
        ]
        .into_iter()
        .filter(|name| active(name))
        .collect::<Vec<_>>();
        if !active("codeigniter")
            && !active("drupal")
            && !active("play")
            && !python
            && !javascript
            && !python_files
            && !callable_values
            && !cpp
            && !active("rust")
            && !active("go")
            && !go_files
            && mobile_names.is_empty()
        {
            return Ok(());
        }
        let mut nodes = self
            .files
            .values()
            .filter_map(|file| file.analysis.extraction.as_ref())
            .flat_map(|extraction| extraction.nodes.iter().cloned())
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| {
            (&a.file_path, a.start_line, &a.id).cmp(&(&b.file_path, b.start_line, &b.id))
        });
        let sources = self
            .files
            .iter()
            .map(|(path, file)| (path.as_str(), file.source.as_str()))
            .collect::<BTreeMap<_, _>>();
        let indices = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, _)| self.metadata(index).map(|node| (node.id.as_str(), index)))
            .collect::<HashMap<_, _>>();
        let mut references = Vec::new();
        for (path, file) in &self.files {
            let Some(extraction) = &file.analysis.extraction else {
                continue;
            };
            for raw in &extraction.unresolved_references {
                if raw.reference_kind == "function_ref"
                    && !(matches!(
                        file.analysis.language.as_str(),
                        "typescript" | "tsx" | "javascript" | "jsx" | "python" | "go"
                    ))
                {
                    continue;
                }
                if inheritance_only
                    && !matches!(raw.reference_kind.as_str(), "extends" | "implements")
                {
                    continue;
                }
                let mut raw = raw.clone();
                raw.file_path = Some(path.clone());
                if raw.language.is_none() {
                    raw.language = Some(file.analysis.language.clone());
                }
                references.push(raw);
            }
        }
        let mut cache = FrameworkBindings::default();
        let ci = active("codeigniter")
            .then(|| crate::frameworks::codeigniter::Bindings::new(&nodes, &sources))
            .transpose()?;
        let drupal =
            active("drupal").then(|| crate::frameworks::drupal::Bindings::new(&nodes, &sources));
        let canonical = nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect::<HashMap<_, _>>();
        let supertypes = |id: &str| {
            indices
                .get(id)
                .into_iter()
                .flat_map(|&index| &self.outgoing[index])
                .filter_map(|&edge| {
                    let edge = &self.edges[edge];
                    if !matches!(edge.kind.as_str(), "extends" | "implements") {
                        return None;
                    }
                    canonical.get(self.metadata(edge.to?)?.id.as_str()).copied()
                })
                .collect::<Vec<_>>()
        };
        let resolve_python_module = |specifier: &str, from: &str| {
            self.python_modules
                .as_ref()
                .and_then(|modules| modules.resolve(specifier, from))
        };
        if python {
            let bindings = crate::frameworks::python_resolution::Bindings::new(&nodes);
            let environment = crate::frameworks::python_resolution::Environment::new(
                &sources,
                &resolve_python_module,
                &supertypes,
            );
            for raw in &references {
                if !crate::frameworks::python_resolution::Bindings::handles(raw, &self.frameworks)?
                {
                    continue;
                }
                let Some(&from) = indices.get(raw.from_node_id.as_str()) else {
                    continue;
                };
                let (targets, method) = self.lookup(
                    &self.nodes[from].path,
                    &self.nodes[from].symbol.qualified_name,
                    &raw.reference_name,
                );
                let imported = if method == "import_match" && targets.len() == 1 {
                    self.metadata(targets[0])
                        .and_then(|node| canonical.get(node.id.as_str()))
                        .map(|&node| crate::frameworks::python_resolution::Answer {
                            node,
                            confidence: 0.9,
                            resolved_by: "import",
                            framework: None,
                        })
                } else {
                    None
                };
                if let Some(answer) =
                    bindings.choose(raw, &self.frameworks, &environment, imported)?
                    && let Some(&to) = indices.get(answer.node.id.as_str())
                {
                    cache.insert(
                        from,
                        raw,
                        vec![to],
                        answer.confidence,
                        answer.framework.unwrap_or(""),
                        None,
                    );
                    cache
                        .metadata
                        .get_mut(&(from, to, raw.reference_name.clone(), raw.line, raw.column))
                        .unwrap()
                        .resolved_by = answer.resolved_by;
                } else {
                    cache.insert(from, raw, Vec::new(), 0.0, "", None);
                }
            }
        }
        if python_files {
            for answer in super::python::module_imports(
                &nodes,
                &sources,
                &resolve_python_module,
                &supertypes,
                &references,
            )? {
                let raw = answer.reference;
                let Some((&from, &to)) = indices
                    .get(raw.from_node_id.as_str())
                    .zip(indices.get(answer.target.as_str()))
                else {
                    continue;
                };
                if cache
                    .references
                    .get(&(
                        from,
                        raw.reference_name.clone(),
                        raw.reference_kind.clone(),
                        raw.line as usize,
                        raw.column as usize,
                    ))
                    .is_some_and(|targets| {
                        targets.iter().any(|&target| {
                            cache
                                .metadata
                                .get(&(
                                    from,
                                    target,
                                    raw.reference_name.clone(),
                                    raw.line,
                                    raw.column,
                                ))
                                .is_some_and(|metadata| metadata.confidence >= answer.confidence)
                        })
                    })
                {
                    continue;
                }
                cache.insert(from, raw, vec![to], answer.confidence, "", None);
                cache
                    .metadata
                    .get_mut(&(from, to, raw.reference_name.clone(), raw.line, raw.column))
                    .unwrap()
                    .resolved_by = answer.resolved_by;
            }
            for answer in super::python::evidence_decisions(
                &nodes,
                &sources,
                &resolve_python_module,
                &supertypes,
                &references,
            )? {
                let raw = answer.reference;
                let Some(&from) = indices.get(raw.from_node_id.as_str()) else {
                    continue;
                };
                if answer.general
                    && cache
                        .references
                        .get(&(
                            from,
                            raw.reference_name.clone(),
                            raw.reference_kind.clone(),
                            raw.line as usize,
                            raw.column as usize,
                        ))
                        .is_some_and(|targets| {
                            targets.iter().any(|&to| {
                                cache
                                    .metadata
                                    .get(&(
                                        from,
                                        to,
                                        raw.reference_name.clone(),
                                        raw.line,
                                        raw.column,
                                    ))
                                    .is_some_and(|metadata| {
                                        !metadata.framework.is_empty()
                                            && metadata.confidence >= answer.confidence
                                    })
                            })
                        })
                {
                    continue;
                }
                let targets = answer
                    .target
                    .as_ref()
                    .and_then(|id| indices.get(id.as_str()))
                    .copied()
                    .into_iter()
                    .collect::<Vec<_>>();
                cache.insert(
                    from,
                    raw,
                    targets.clone(),
                    answer.confidence,
                    "",
                    answer.metadata,
                );
                if let Some(&to) = targets.first() {
                    let metadata = cache
                        .metadata
                        .get_mut(&(from, to, raw.reference_name.clone(), raw.line, raw.column))
                        .unwrap();
                    metadata.resolved_by = answer.resolved_by;
                    metadata.by_name = answer.by_name;
                }
            }
        }
        if cpp {
            for answer in super::cpp::bindings(&nodes, &sources, &references, &self.cpp_imports)? {
                let raw = answer.reference;
                let Some(&from) = indices.get(raw.from_node_id.as_str()) else {
                    continue;
                };
                let targets = answer
                    .targets
                    .iter()
                    .filter_map(|id| indices.get(id.as_str()).copied())
                    .collect::<Vec<_>>();
                cache.insert(from, &raw, targets.clone(), answer.confidence, "", None);
                let uncertain = targets.len() > 1 && answer.confidence < 0.7;
                for to in targets {
                    let metadata = cache
                        .metadata
                        .get_mut(&(from, to, raw.reference_name.clone(), raw.line, raw.column))
                        .unwrap();
                    metadata.resolved_by = answer.method;
                    metadata.uncertain = uncertain;
                }
            }
        }
        if inheritance_only {
            self.framework_bindings = cache;
            return Ok(());
        }
        let inheritance_edges = self
            .edges
            .iter()
            .filter(|edge| matches!(edge.kind.as_str(), "extends" | "implements"))
            .filter_map(|edge| {
                Some(prometeu_extract::Edge {
                    source: self.metadata(edge.from)?.id.clone(),
                    target: self.metadata(edge.to?)?.id.clone(),
                    kind: edge.kind.clone(),
                    line: Some(edge.line as u32),
                    column: edge.column,
                    provenance: None,
                    metadata: None,
                })
            })
            .collect::<Vec<_>>();
        if javascript {
            // Syntax identifies qualified local/parameter types even when the
            // line-based heuristic misses a lowercase namespace. Preserve the
            // structural lookup, including export visibility and re-exports.
            let mut namespace_receivers = HashSet::new();
            for (path, file) in &self.files {
                if !matches!(
                    file.analysis.language.as_str(),
                    "typescript" | "tsx" | "javascript" | "jsx"
                ) {
                    continue;
                }
                for reference in self.resolution_references(path) {
                    if reference.kind == "calls"
                        && reference
                            .qualifier
                            .as_deref()
                            .is_some_and(|qualifier| !qualifier.contains(['.', ':']))
                        && let Some((namespace, _)) = reference
                            .receiver_type
                            .as_deref()
                            .and_then(|receiver| receiver.split_once("::"))
                        && let Some(from) = reference
                            .caller
                            .and_then(|ordinal| file.nodes.get(ordinal).copied())
                        && self
                            .visible_import(
                                path,
                                &self.nodes[from].symbol.qualified_name,
                                namespace,
                            )
                            .is_some()
                    {
                        namespace_receivers.insert((from, reference.line, reference.column));
                    }
                }
            }
            let imports = self.synthesis_imports()?;
            let mut import_bindings = HashMap::new();
            for binding in &imports {
                import_bindings
                    .entry((binding.file.as_str(), binding.local_name.as_str()))
                    .or_insert(binding);
            }
            let defaults = std::cell::RefCell::new(HashMap::new());
            let resolve_import = |reference: &prometeu_extract::UnresolvedReference| {
                let resolved =
                    self.synthesis_import_reference(reference, &import_bindings, &defaults)?;
                if resolved.is_some()
                    || reference.reference_kind != "references"
                    || !reference.reference_name.contains('.')
                {
                    return Ok(resolved);
                }
                let Some(&from) = indices.get(reference.from_node_id.as_str()) else {
                    return Ok(None);
                };
                let (targets, method) = self.lookup(
                    &self.nodes[from].path,
                    &self.nodes[from].symbol.qualified_name,
                    &reference.reference_name.replace('.', "::"),
                );
                Ok((method == "import_match" && targets.len() == 1)
                    .then(|| self.metadata(targets[0]).map(|node| node.id.clone()))
                    .flatten())
            };
            for answer in super::javascript::bindings(
                &nodes,
                &sources,
                &inheritance_edges,
                &references,
                &imports,
                &resolve_import,
            )? {
                let raw = answer.reference;
                let target = answer.target;
                let Some(&from) = indices.get(raw.from_node_id.as_str()) else {
                    continue;
                };
                if raw.reference_kind == "calls"
                    && namespace_receivers.contains(&(
                        from,
                        raw.line as usize,
                        Some(raw.column as usize),
                    ))
                {
                    continue;
                }
                let targets = target
                    .as_ref()
                    .and_then(|id| indices.get(id.as_str()))
                    .copied()
                    .into_iter()
                    .collect::<Vec<_>>();
                cache.insert(from, &raw, targets.clone(), answer.confidence, "", None);
                if let Some(&to) = targets.first() {
                    let metadata = cache
                        .metadata
                        .get_mut(&(from, to, raw.reference_name.clone(), raw.line, raw.column))
                        .unwrap();
                    metadata.resolved_by = answer.resolved_by;
                    metadata.by_name = answer.by_name;
                }
            }
            for raw in references.iter().filter(|raw| {
                raw.reference_kind == "instantiates"
                    && matches!(
                        raw.language.as_deref(),
                        Some("typescript" | "tsx" | "javascript" | "jsx")
                    )
            }) {
                if let Some(target) = resolve_import(raw)?
                    && let Some((&from, &to)) = indices
                        .get(raw.from_node_id.as_str())
                        .zip(indices.get(target.as_str()))
                {
                    cache.insert(from, raw, vec![to], 0.9, "", None);
                    cache
                        .metadata
                        .get_mut(&(from, to, raw.reference_name.clone(), raw.line, raw.column))
                        .unwrap()
                        .resolved_by = "import";
                }
            }
        }
        if active("rust") {
            let modules = super::cargo_workspace::Modules::new(
                &nodes,
                &self.manifests,
                &self.workspace_directories,
            )?;
            for raw in &references {
                if raw.language.as_deref() != Some("rust") || raw.reference_kind != "imports" {
                    continue;
                }
                if let Some((node, confidence)) = modules.resolve(raw)
                    && confidence >= 0.9
                    && let Some((&from, &to)) = indices
                        .get(raw.from_node_id.as_str())
                        .zip(indices.get(node.id.as_str()))
                {
                    cache.insert(from, raw, vec![to], confidence, "rust", None);
                }
            }
        }
        if go_files {
            let package = |from: &str, source: &str| self.go_modules.package(from, source);
            let go_sources = self
                .auxiliary_sources
                .iter()
                .filter(|(path, _)| path.ends_with(".go"))
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .chain(sources.iter().map(|(&path, &source)| (path, source)))
                .collect::<BTreeMap<_, _>>();
            let bindings =
                crate::frameworks::go_bindings::Bindings::new(&nodes, &go_sources, &package)?;
            for raw in &references {
                let Some(&from) = indices.get(raw.from_node_id.as_str()) else {
                    continue;
                };
                let Some(answer) = bindings.select(raw, active("go"))? else {
                    continue;
                };
                let targets = answer
                    .node
                    .and_then(|node| indices.get(node.id.as_str()))
                    .copied()
                    .into_iter()
                    .collect::<Vec<_>>();
                cache.insert(
                    from,
                    raw,
                    targets.clone(),
                    answer.confidence,
                    if answer.resolved_by == "framework" {
                        "go"
                    } else {
                        ""
                    },
                    None,
                );
                if let Some(&to) = targets.first() {
                    cache
                        .metadata
                        .get_mut(&(from, to, raw.reference_name.clone(), raw.line, raw.column))
                        .unwrap()
                        .resolved_by = answer.resolved_by;
                }
            }
        }
        for raw in &references {
            let Some(&from) = indices.get(raw.from_node_id.as_str()) else {
                continue;
            };
            let mut result = None;
            if let Some(drupal) = &drupal
                && matches!(raw.language.as_deref(), Some("php" | "yaml"))
                && let Some((node, confidence)) = drupal.resolve(raw, supertypes)?
            {
                result = Some((node.id.clone(), confidence, "drupal"));
            }
            if result.is_none()
                && let Some(ci) = &ci
                && raw.language.as_deref() == Some("php")
                && let Some((id, confidence)) = ci.resolve(raw, supertypes)?
            {
                result = Some((id, confidence, "codeigniter"));
            }
            if let Some((id, confidence, framework)) = result
                && let Some(&to) = indices.get(id.as_str())
            {
                if framework == "drupal"
                    && confidence < 0.9
                    && raw.reference_name.starts_with("hook_")
                    && self
                        .names
                        .get(&raw.reference_name)
                        .into_iter()
                        .flatten()
                        .any(|&index| {
                            self.metadata(index).is_some_and(|node| {
                                node.language == "php"
                                    && node.kind == "function"
                                    && node.name == raw.reference_name
                            })
                        })
                {
                    continue;
                }
                cache.insert(from, raw, vec![to], confidence, framework, None);
            }
        }
        if active("play") {
            for (raw, id) in crate::frameworks::play::bindings(
                &nodes,
                &sources,
                &inheritance_edges,
                &references,
            )? {
                if let Some((&from, &to)) = indices
                    .get(raw.from_node_id.as_str())
                    .zip(indices.get(id.as_str()))
                {
                    cache.insert(from, &raw, vec![to], 0.9, "play", None);
                }
            }
        }
        if !mobile_names.is_empty() {
            let imports = self.synthesis_imports()?;
            let resolve_module = |specifier: &str, from: &str, language: &str| {
                let paths = self.module_files_as(from, specifier, language);
                Ok((paths.len() == 1).then(|| paths[0].clone()))
            };
            let mut bindings = crate::frameworks::mobile::Bindings::new(
                &nodes,
                &sources,
                &imports,
                &resolve_module,
            )?;
            for raw in &references {
                let Some(&from) = indices.get(raw.from_node_id.as_str()) else {
                    continue;
                };
                let mut best = None;
                for &framework in &mobile_names {
                    let applies = match framework {
                        "swiftui" | "uikit" | "vapor" => raw.language.as_deref() == Some("swift"),
                        "swift-objc-bridge" => {
                            matches!(raw.language.as_deref(), Some("swift" | "objc"))
                        }
                        _ => matches!(
                            raw.language.as_deref(),
                            Some("javascript" | "typescript" | "tsx" | "jsx")
                        ),
                    };
                    if !applies {
                        continue;
                    }
                    if let Some(answer) = bindings.resolve(framework, raw)?
                        && best.as_ref().is_none_or(
                            |(_, previous): &(_, crate::frameworks::mobile::Binding)| {
                                answer.confidence > previous.confidence
                            },
                        )
                    {
                        let early = answer.confidence >= 0.9;
                        best = Some((framework, answer));
                        if early {
                            break;
                        }
                    }
                }
                if let Some((framework, answer)) = best {
                    let exact = if answer.confidence < 0.9
                        && matches!(framework, "swiftui" | "uikit" | "vapor")
                    {
                        bindings.swift_exact_type(raw)?
                    } else {
                        None
                    };
                    let resolved_by = if exact.is_some() {
                        "exact-match"
                    } else {
                        "framework"
                    };
                    let answer = exact.unwrap_or(answer);
                    let Some(answer) = bindings.swift_type_target(raw, answer)? else {
                        cache.insert(from, raw, Vec::new(), 0.0, "", None);
                        continue;
                    };
                    let targets = std::iter::once(&answer.target_node_id)
                        .chain(
                            answer
                                .also_targets
                                .iter()
                                .map(|target| &target.target_node_id),
                        )
                        .filter_map(|id| indices.get(id.as_str()).copied())
                        .collect::<Vec<_>>();
                    if targets.is_empty() {
                        continue;
                    }
                    cache.insert(
                        from,
                        raw,
                        targets,
                        answer.confidence,
                        framework,
                        answer.metadata,
                    );
                    if let Some(&to) = indices.get(answer.target_node_id.as_str())
                        && let Some(metadata) = cache.metadata.get_mut(&(
                            from,
                            to,
                            raw.reference_name.clone(),
                            raw.line,
                            raw.column,
                        ))
                    {
                        metadata.resolved_by = resolved_by;
                    }
                }
            }
        }
        if callable_values {
            let scopes = crate::frameworks::python_resolution::Bindings::new(&nodes);
            let mut callable_work = 0usize;
            for (path, file) in &self.files {
                if !super::function_refs::handled(&file.analysis.language) {
                    continue;
                }
                for original in file
                    .analysis
                    .extraction
                    .iter()
                    .flat_map(|extraction| &extraction.unresolved_references)
                    .filter(|reference| {
                        reference.reference_kind == "function_ref"
                            || file.analysis.language == "pascal"
                                && reference.reference_name.starts_with('{')
                    })
                {
                    if file.analysis.language == "go" && original.reference_name.contains('.') {
                        continue;
                    }
                    let Some(&from) = indices.get(original.from_node_id.as_str()) else {
                        continue;
                    };
                    let mut raw = original.clone();
                    raw.file_path = Some(path.clone());
                    raw.language = Some(file.analysis.language.clone());
                    if file.analysis.language == "pascal" && raw.reference_name.starts_with('{') {
                        let answer = self.pascal_value(&raw)?;
                        let targets = answer
                            .as_ref()
                            .and_then(|answer| indices.get(answer.node.id.as_str()))
                            .copied()
                            .into_iter()
                            .collect::<Vec<_>>();
                        let confidence = answer.as_ref().map_or(0.0, |answer| answer.confidence);
                        let extra = answer
                            .as_ref()
                            .filter(|answer| answer.kind == "references")
                            .map(|_| serde_json::json!({"fnRef":true}));
                        cache.insert(from, &raw, targets.clone(), confidence, "", extra);
                        if let Some((&to, answer)) = targets.first().zip(answer) {
                            let metadata = cache
                                .metadata
                                .get_mut(&(
                                    from,
                                    to,
                                    raw.reference_name.clone(),
                                    raw.line,
                                    raw.column,
                                ))
                                .unwrap();
                            metadata.resolved_by = answer.resolved_by;
                            metadata.edge_kind = Some(answer.kind);
                        }
                        continue;
                    }
                    let (imported, method) = self.lookup(
                        path,
                        &self.nodes[from].symbol.qualified_name,
                        &raw.reference_name,
                    );
                    let imported = (method == "import_match" && imported.len() == 1)
                        .then(|| self.metadata(imported[0]))
                        .flatten()
                        .filter(|node| matches!(node.kind.as_str(), "function" | "method"));
                    let (target, confidence, resolved_by) = if let Some(node) = imported {
                        (Some(node), 0.9, "import")
                    } else if raw.reference_name.starts_with("this.") {
                        let (target, confidence) = self.this_callable_value(from, &raw);
                        (target, confidence, "function-ref")
                    } else {
                        let name = raw
                            .reference_name
                            .rsplit("::")
                            .next()
                            .unwrap_or(&raw.reference_name);
                        let named = self
                            .names
                            .get(name)
                            .into_iter()
                            .flatten()
                            .filter_map(|&index| self.metadata(index))
                            .filter_map(|node| canonical.get(node.id.as_str()).copied())
                            .collect::<Vec<_>>();
                        callable_work = callable_work.saturating_add(named.len());
                        ensure!(
                            callable_work <= 2_000_000,
                            "callable-value candidate work limit exceeded"
                        );
                        let (target, confidence) = super::function_refs::select(
                            &named,
                            self.metadata(from),
                            &sources,
                            &scopes,
                            &raw,
                        )?;
                        (target, confidence, "function-ref")
                    };
                    let targets: Vec<_> = target
                        .and_then(|node| indices.get(node.id.as_str()))
                        .copied()
                        .into_iter()
                        .collect();
                    cache.insert(from, &raw, targets.clone(), confidence, "", None);
                    if let Some(&to) = targets.first() {
                        cache
                            .metadata
                            .get_mut(&(from, to, raw.reference_name.clone(), raw.line, raw.column))
                            .unwrap()
                            .resolved_by = resolved_by;
                    }
                }
            }
        }
        self.framework_bindings = cache;
        Ok(())
    }
    pub(in crate::graph) fn cached_framework_reference(
        &self,
        from: usize,
        name: &str,
        kind: &str,
        line: usize,
        column: Option<usize>,
    ) -> Option<&[usize]> {
        self.framework_bindings
            .references
            .get(&(from, name.into(), kind.into(), line, column.unwrap_or(0)))
            .map(Vec::as_slice)
    }
    pub(in crate::graph) fn cached_framework_metadata(
        &self,
        from: usize,
        to: usize,
        name: &str,
        line: usize,
        column: Option<u32>,
    ) -> Option<&BindingMetadata> {
        self.framework_bindings
            .metadata
            .get(&(from, to, name.into(), line as u32, column?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use serde_json::Value;
    #[test]
    fn controller_and_loader_edges_match_reference_after_reopen() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/codeigniter/cases.json"
        ))
        .unwrap();
        for project in fixture["projects"].as_array().unwrap() {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("project");
            let database = temporary.path().join("index.sqlite");
            for (path, source) in project["files"].as_object().unwrap() {
                let path = root.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let mut engine = Engine::open(&root, Some(&database)).unwrap();
            engine.sync().unwrap();
            for reopened in [false, true] {
                if reopened {
                    drop(engine);
                    for path in project["files"].as_object().unwrap().keys() {
                        std::fs::write(root.join(path), "modified live source").unwrap();
                    }
                    engine = Engine::open(&root, Some(&database)).unwrap();
                }
                let graph = Graph::load(&engine.connection, 1).unwrap();
                let mut actual = graph
                    .canonical_edges
                    .iter()
                    .flatten()
                    .filter(|edge| {
                        edge.metadata
                            .as_ref()
                            .is_some_and(|metadata| metadata["framework"] == "codeigniter")
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let mut expected: Vec<prometeu_extract::Edge> =
                    serde_json::from_value(project["edges"].clone()).unwrap();
                let key = |edge: &prometeu_extract::Edge| {
                    (
                        edge.source.clone(),
                        edge.target.clone(),
                        edge.kind.clone(),
                        edge.line,
                        edge.column,
                    )
                };
                actual.sort_by_key(key);
                expected.sort_by_key(key);
                assert_eq!(actual, expected, "{} reopened={reopened}", project["name"]);
            }
        }
    }
}

#[cfg(test)]
#[path = "framework_bindings/mobile_tests.rs"]
mod mobile_tests;

#[cfg(test)]
#[path = "framework_bindings/play_tests.rs"]
mod play_tests;

#[cfg(test)]
#[path = "framework_bindings/python_tests.rs"]
mod python_tests;
