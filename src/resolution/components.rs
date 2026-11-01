//! Component bindings and compiler-provided names for Vue, Svelte, and Astro.
use super::super::Graph;
use crate::parser::Reference;

const VUE_MACROS: &[&str] = &[
    "defineProps",
    "defineEmits",
    "defineExpose",
    "defineOptions",
    "defineSlots",
    "defineModel",
    "withDefaults",
];
const NUXT_IMPORTS: &[&str] = &[
    "useRoute",
    "useRouter",
    "abortNavigation",
    "useFetch",
    "useAsyncData",
    "useLazyFetch",
    "useLazyAsyncData",
    "refreshNuxtData",
    "useState",
    "clearNuxtState",
    "useHead",
    "useSeoMeta",
    "useServerSeoMeta",
    "useRuntimeConfig",
    "useAppConfig",
    "useNuxtApp",
    "useCookie",
    "useError",
    "createError",
    "showError",
    "clearError",
    "definePageMeta",
    "defineNuxtConfig",
    "defineNuxtPlugin",
    "defineNuxtRouteMiddleware",
    "useRequestHeaders",
    "useRequestEvent",
    "useRequestFetch",
    "useRequestURL",
];
const SVELTE_RUNES: &[&str] = &[
    "$state",
    "$state.raw",
    "$state.snapshot",
    "$derived",
    "$derived.by",
    "$effect",
    "$effect.pre",
    "$effect.root",
    "$effect.tracking",
    "$props",
    "$bindable",
    "$inspect",
    "$host",
];

fn pascal_case(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

impl Graph {
    fn owns_function(&self, path: &str, name: &str) -> bool {
        self.files[path].nodes.iter().any(|&index| {
            let node = &self.nodes[index].symbol;
            node.name == name && matches!(node.kind.as_str(), "function" | "constant" | "variable")
        })
    }

    fn component_in(&self, path: &str) -> Option<usize> {
        let file = self.files.get(path)?;
        file.nodes
            .iter()
            .copied()
            .find(|&index| self.nodes[index].symbol.kind == "component")
            .or_else(|| {
                file.nodes.iter().copied().find(|&index| {
                    let node = &self.nodes[index].symbol;
                    node.kind == "function"
                        && node.name.starts_with(|c: char| c.is_ascii_uppercase())
                })
            })
    }

    fn vue_route_component(&self, path: &str, name: &str) -> Option<usize> {
        let lazy = name
            .strip_prefix("import:")
            .and_then(|value| value.rsplit_once('#'));
        let specifier = lazy.map(|pair| pair.0).or_else(|| {
            self.files[path]
                .script_imports
                .iter()
                .find(|import| import.local_name == name)
                .map(|import| import.source.as_str())
        });
        let name = lazy.map_or(name, |pair| pair.1);
        if let Some(specifier) = specifier {
            let file = self
                .module_files_as(path, specifier, "vue")
                .into_iter()
                .next()
                .or_else(|| {
                    let prefix =
                        regex::Regex::new(r"^(?:[@#~$][A-Za-z0-9_-]*/|\.{1,2}/)+").unwrap();
                    let rest = prefix.replace(specifier, "");
                    if rest == specifier || rest.is_empty() {
                        return None;
                    }
                    let root = crate::frameworks::frontend::tables::app_root(path).ok()?;
                    let mut hits = self.files.keys().filter(|file| {
                        file.starts_with(&root)
                            && [
                                "",
                                ".vue",
                                ".tsx",
                                ".jsx",
                                ".ts",
                                ".js",
                                "/index.vue",
                                "/index.tsx",
                                "/index.ts",
                                "/index.js",
                            ]
                            .iter()
                            .any(|extension| {
                                let tail = format!("{rest}{extension}");
                                *file == &tail || file.ends_with(&format!("/{tail}"))
                            })
                    });
                    let first = hits.next()?;
                    hits.next().is_none().then(|| first.clone())
                });
            if let Some(component) = file.and_then(|file| self.component_in(&file)) {
                return Some(component);
            }
        }
        if name == "index" {
            return None;
        }
        let candidates: Vec<_> = self
            .named(path, name)
            .into_iter()
            .filter(|&index| {
                let node = &self.nodes[index];
                node.symbol.kind == "component"
                    || (node.symbol.kind == "function" && node.path.ends_with(".vue"))
            })
            .collect();
        if candidates.len() == 1 {
            return candidates.first().copied();
        }
        let root = crate::frameworks::frontend::tables::app_root(path).ok()?;
        let mut near = candidates
            .into_iter()
            .filter(|&index| self.nodes[index].path.starts_with(&root));
        let first = near.next()?;
        near.next().is_none().then_some(first)
    }

    fn named_component(&self, path: &str, name: &str, vue: bool) -> Option<usize> {
        let candidates: Vec<_> = self
            .named(path, name)
            .into_iter()
            .filter(|&index| {
                let node = &self.nodes[index];
                node.symbol.kind == "component" && (!vue || node.path.ends_with(".vue"))
            })
            .collect();
        if let Some((folder, _)) = path.rsplit_once('/') {
            let folder = format!("{folder}/");
            if let Some(&index) = candidates
                .iter()
                .find(|&&index| self.nodes[index].path.starts_with(&folder))
            {
                return Some(index);
            }
        }
        (candidates.len() == 1).then(|| candidates[0])
    }

    pub(super) fn component_reference(
        &self,
        path: &str,
        caller: usize,
        reference: &Reference,
    ) -> Option<(Vec<usize>, &'static str)> {
        let language = self.files[path].analysis.language.as_str();
        let name = reference.qualifier.as_ref().map_or_else(
            || reference.target.clone(),
            |qualifier| format!("{qualifier}.{}", reference.target),
        );
        let active = |framework: &str| self.frameworks.iter().any(|name| name == framework);
        let bound = |target, framework| Some((vec![target], framework));
        let script = matches!(
            language,
            "vue" | "javascript" | "typescript" | "tsx" | "jsx"
        );
        if active("vue") && script {
            if VUE_MACROS.contains(&name.as_str())
                || (NUXT_IMPORTS.contains(&name.as_str()) && !self.owns_function(path, &name))
            {
                return bound(caller, "vue");
            }
            if reference.kind == "imports" && self.module_files(path, &name).is_empty() {
                if language != "vue"
                    && ["@/", "~/"].iter().any(|prefix| name.starts_with(prefix))
                    && let Some(target) = self
                        .module_files_as(path, &name, "vue")
                        .into_iter()
                        .find_map(|file| {
                            self.files.get(&file).and_then(|file| {
                                file.nodes
                                    .iter()
                                    .copied()
                                    .find(|&index| self.nodes[index].symbol.kind == "file")
                            })
                        })
                {
                    return bound(target, "vue");
                }
                if ["#imports", "#components", "#app", "#build", "#head"]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                {
                    return bound(caller, "vue");
                }
            }
            if reference.kind == "calls"
                && pascal_case(&name)
                && let Some(component) = self.named_component(path, &name, true)
            {
                return bound(component, "vue");
            }
        }
        if active("vue-router")
            && self
                .metadata(caller)
                .is_some_and(|node| node.id.starts_with("route:") && node.id.ends_with(":vue"))
            && (reference.kind == "calls"
                || (reference.kind == "references" && name.starts_with("layout:")))
            && let Some(component) =
                self.vue_route_component(path, name.strip_prefix("layout:").unwrap_or(&name))
        {
            return bound(component, "vue-router");
        }
        if active("nuxt")
            && script
            && reference.kind == "calls"
            && name == "navigateTo"
            && !self.owns_function(path, &name)
            && !self.files[path]
                .script_imports
                .iter()
                .any(|import| import.local_name == name)
        {
            return Some((Vec::new(), "nuxt"));
        }
        if active("astro") {
            if name == "Astro"
                || name.starts_with("Astro.")
                || (reference.kind == "imports"
                    && [
                        "astro:content",
                        "astro:assets",
                        "astro:actions",
                        "astro:env",
                        "astro:i18n",
                        "astro:middleware",
                        "astro:transitions",
                        "astro:components",
                        "astro:schema",
                    ]
                    .iter()
                    .any(|prefix| name.starts_with(prefix)))
            {
                return bound(caller, "astro");
            }
            if path.ends_with(".astro")
                && pascal_case(&name)
                && matches!(reference.kind.as_str(), "calls" | "references")
                && let Some(component) = self.named_component(path, &name, false)
            {
                return bound(component, "astro");
            }
        }
        if active("svelte") && matches!(language, "svelte" | "typescript" | "javascript") {
            if SVELTE_RUNES.contains(&name.as_str()) {
                return bound(caller, "svelte");
            }
            if language == "svelte"
                && name.starts_with('$')
                && !name.starts_with("$$")
                && let Some(target) = self.named(path, &name[1..]).into_iter().find(|&index| {
                    matches!(
                        self.nodes[index].symbol.kind.as_str(),
                        "variable" | "constant"
                    )
                })
            {
                return bound(target, "svelte");
            }
            if reference.kind == "imports" {
                if let Some(tail) = name.strip_prefix("$lib/") {
                    for extension in ["", ".ts", ".js", ".svelte", "/index.ts", "/index.js"] {
                        if let Some(file) = self.files.get(&format!("src/lib/{tail}{extension}"))
                            && let Some(target) = file
                                .nodes
                                .iter()
                                .copied()
                                .filter_map(|index| self.metadata(index).map(|node| (index, node)))
                                .min_by(|(_, a), (_, b)| {
                                    (a.start_line, &a.id).cmp(&(b.start_line, &b.id))
                                })
                                .map(|(index, _)| index)
                        {
                            return bound(target, "svelte");
                        }
                    }
                }
                if [
                    "$app/navigation",
                    "$app/stores",
                    "$app/environment",
                    "$app/forms",
                    "$app/paths",
                    "$env/static/private",
                    "$env/static/public",
                    "$env/dynamic/private",
                    "$env/dynamic/public",
                ]
                .iter()
                .any(|prefix| name.starts_with(prefix))
                {
                    return bound(caller, "svelte");
                }
            }
            if reference.kind == "calls"
                && pascal_case(&name)
                && let Some(component) = self.named_component(path, &name, false)
            {
                return bound(component, "svelte");
            }
        }
        None
    }

    pub(super) fn framework_confidence(
        &self,
        from: usize,
        to: usize,
        name: &str,
        line: usize,
        column: Option<u32>,
    ) -> f64 {
        if let Some(metadata) = self.cached_framework_metadata(from, to, name, line, column) {
            return metadata.confidence;
        }
        if let Some(confidence) = self.infrastructure_confidence(from, name) {
            return confidence;
        }
        if let Some(confidence) = self.react_confidence(from, to, name) {
            return confidence;
        }
        if let Some((target, confidence)) = self.express_reference(&self.nodes[from].path, name)
            && target == to
        {
            return confidence;
        }
        if self.files[&self.nodes[from].path].analysis.language == "php" && name.contains("::") {
            return 0.85;
        }
        if let Some((target, confidence)) = self.nest_reference(&self.nodes[from].path, name)
            && target == to
        {
            return confidence;
        }
        if let Some((target, confidence)) = self.rails_reference(&self.nodes[from].path, name)
            && target == to
        {
            return confidence;
        }
        let origin = self.metadata(from);
        if origin.is_some_and(|node| node.kind == "route")
            && (matches!(
                self.files[&self.nodes[from].path]
                    .analysis
                    .language
                    .as_str(),
                "vue" | "astro" | "java" | "kotlin"
            ) || origin.is_some_and(|node| node.id.ends_with(":vue")))
        {
            return 0.95;
        }
        if origin.is_some_and(|node| {
            node.id.starts_with("spring-value:") || node.id.starts_with("spring-cp:")
        }) {
            return 0.85;
        }
        if from == to {
            return 1.0;
        }
        if self.nodes[to].symbol.kind == "component" {
            return 0.8;
        }
        if self.files[&self.nodes[from].path].analysis.language == "svelte"
            && name.starts_with('$')
            && !name.starts_with("$$")
            && !name.contains('/')
        {
            return 0.85;
        }
        0.9
    }

    pub(super) fn framework_name(
        &self,
        from: usize,
        raw: Option<&prometeu_extract::UnresolvedReference>,
    ) -> Option<&'static str> {
        if let Some(raw) = raw
            && self
                .react_reference(
                    from,
                    &raw.reference_name,
                    &raw.reference_kind,
                    raw.line as usize,
                    Some(raw.column as usize),
                )
                .is_some()
        {
            return Some("react");
        }
        if let Some(raw) = raw
            && let Some(targets) = self.cached_framework_reference(
                from,
                &raw.reference_name,
                &raw.reference_kind,
                raw.line as usize,
                Some(raw.column as usize),
            )
            && let Some(metadata) = targets.first().and_then(|&to| {
                self.cached_framework_metadata(
                    from,
                    to,
                    &raw.reference_name,
                    raw.line as usize,
                    Some(raw.column),
                )
            })
        {
            return (!metadata.framework.is_empty()).then_some(metadata.framework);
        }
        if let Some(raw) = raw
            && self
                .express_reference(&self.nodes[from].path, &raw.reference_name)
                .is_some()
        {
            return Some("express");
        }
        if let Some(raw) = raw
            && self
                .nest_reference(&self.nodes[from].path, &raw.reference_name)
                .is_some()
        {
            return Some("nestjs");
        }
        let node = self.metadata(from)?;
        if node.kind == "route"
            && matches!(node.language.as_str(), "vue" | "astro")
            && !node.id.ends_with(":vue")
        {
            return Some(if node.language == "vue" {
                "nuxt"
            } else {
                "astro"
            });
        }
        if let Some(raw) = raw {
            let reference = Reference {
                column: None,
                caller: None,
                target: raw.reference_name.clone(),
                kind: raw.reference_kind.clone(),
                line: raw.line as usize,
                qualifier: None,
                receiver_type: None,
                shadowed: false,
                owner: None,
            };
            if let Some((_, framework)) =
                self.component_reference(&self.nodes[from].path, from, &reference)
            {
                return Some(framework);
            }
        }
        match self.files[&self.nodes[from].path]
            .analysis
            .language
            .as_str()
        {
            "java" | "kotlin" => Some("spring"),
            "swift" => Some("vapor"),
            "php" => Some("laravel"),
            "ruby" => Some("rails"),
            "terraform" => Some("terraform"),
            "cobol" => Some("cics"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use serde_json::Value;

    #[test]
    fn component_bindings_match_independent_framework_resolvers() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/frameworks/components.json"
        ))
        .unwrap();
        for project in fixture["projects"].as_array().unwrap() {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("project");
            for (path, source) in project["files"].as_object().unwrap() {
                let path = root.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, source.as_str().unwrap()).unwrap();
            }
            let database = temporary.path().join("index.sqlite");
            let mut engine = Engine::open(&root, Some(&database)).unwrap();
            engine.sync().unwrap();
            for reopened in [false, true] {
                if reopened {
                    engine = Engine::open(&root, Some(&database)).unwrap();
                }
                let graph = Graph::load(&engine.connection, 1).unwrap();
                for query in project["queries"].as_array().unwrap() {
                    let raw: prometeu_extract::UnresolvedReference =
                        serde_json::from_value(query["reference"].clone()).unwrap();
                    let from = graph
                        .nodes
                        .iter()
                        .enumerate()
                        .find(|(index, _)| {
                            graph
                                .metadata(*index)
                                .is_some_and(|node| node.id == raw.from_node_id)
                        })
                        .unwrap()
                        .0;
                    let reference = Reference {
                        column: None,
                        caller: None,
                        target: raw.reference_name.clone(),
                        kind: raw.reference_kind.clone(),
                        line: raw.line as usize,
                        qualifier: None,
                        receiver_type: None,
                        shadowed: false,
                        owner: None,
                    };
                    let result = graph.component_reference(
                        raw.file_path.as_ref().unwrap(),
                        from,
                        &reference,
                    );
                    let targets = result.as_ref().map(|pair| pair.0.as_slice()).unwrap_or(&[]);
                    let expected = &query["expected"];
                    if expected.is_null() {
                        assert!(targets.is_empty(), "{} {query}", project["name"]);
                    } else {
                        assert_eq!(targets.len(), 1, "{} {query}", project["name"]);
                        let to = targets[0];
                        assert_eq!(
                            graph.metadata(to).unwrap().id,
                            expected["target"].as_str().unwrap(),
                            "{} {query}",
                            project["name"]
                        );
                        assert_eq!(
                            graph.framework_confidence(
                                from,
                                to,
                                &raw.reference_name,
                                raw.line as usize,
                                Some(raw.column)
                            ),
                            expected["confidence"].as_f64().unwrap(),
                            "{} {query}",
                            project["name"]
                        );
                        assert_eq!(
                            graph.framework_name(from, Some(&raw)),
                            query["framework"].as_str(),
                            "{} {query}",
                            project["name"]
                        );
                    }
                    assert_eq!(
                        result
                            .as_ref()
                            .is_some_and(|(targets, _)| targets.is_empty()),
                        query["owned"].as_bool().unwrap(),
                        "{} {query}",
                        project["name"]
                    );
                }
            }
        }
    }
}
