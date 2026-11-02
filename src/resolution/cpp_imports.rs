//! C/C++ include search over the committed project inventory.
//! SPDX-License-Identifier: MIT. See crates/prometeu-extract/UPSTREAM.md.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Default)]
pub(crate) struct CppImports {
    files: HashSet<String>,
    basenames: HashMap<String, Vec<String>>,
    directories: Vec<String>,
}
impl CppImports {
    pub(crate) fn new(files: impl IntoIterator<Item = String>) -> Self {
        let files: HashSet<_> = files.into_iter().collect();
        let directories: BTreeSet<_> = files
            .iter()
            .filter_map(|path| {
                let (directory, remainder) = path.split_once('/')?;
                let convention = matches!(
                    directory.to_ascii_lowercase().as_str(),
                    "include" | "src" | "lib" | "api" | "inc"
                );
                let header = !remainder.contains('/')
                    && remainder.rsplit_once('.').is_some_and(|(_, extension)| {
                        matches!(
                            extension.to_ascii_lowercase().as_str(),
                            "h" | "hpp" | "hxx" | "hh"
                        )
                    });
                (convention || header).then(|| directory.to_owned())
            })
            .collect();
        let mut basenames: HashMap<String, Vec<String>> = HashMap::new();
        for file in &files {
            basenames
                .entry(file.rsplit('/').next().unwrap().to_owned())
                .or_default()
                .push(file.clone());
        }
        for files in basenames.values_mut() {
            files.sort();
        }
        Self {
            files,
            basenames,
            directories: directories.into_iter().collect(),
        }
    }
    /// Compilation databases replace convention roots, even when they contain no -I flags.
    pub(crate) fn configure(
        &mut self,
        root: &str,
        manifests: &BTreeMap<String, String>,
        directories: &[String],
    ) {
        for path in [
            "compile_commands.json",
            "build/compile_commands.json",
            "cmake-build-debug/compile_commands.json",
            "cmake-build-release/compile_commands.json",
            "out/compile_commands.json",
        ] {
            let Some(source) = manifests.get(path) else {
                continue;
            };
            let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(source) else {
                break;
            };
            let mut roots = Vec::new();
            for entry in entries {
                let directory = entry["directory"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(root);
                let args = entry["arguments"]
                    .as_array()
                    .map(|args| {
                        args.iter()
                            .filter_map(|arg| arg.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_else(|| shell_words(entry["command"].as_str().unwrap_or("")));
                let mut args = args.iter();
                while let Some(arg) = args.next() {
                    let include = if matches!(arg.as_str(), "-I" | "-isystem") {
                        args.next().map(String::as_str)
                    } else {
                        arg.strip_prefix("-I").filter(|s| !s.is_empty())
                    };
                    let Some(include) = include else {
                        continue;
                    };
                    let joined = if include.starts_with('/') {
                        include.to_owned()
                    } else {
                        format!("{directory}/{include}")
                    };
                    let absolute = lexical_absolute(&joined);
                    let Some(relative) = absolute
                        .strip_prefix(root.trim_end_matches('/'))
                        .and_then(|tail| tail.strip_prefix('/'))
                    else {
                        continue;
                    };
                    if !relative.is_empty() && !roots.iter().any(|entry| entry == relative) {
                        roots.push(relative.to_owned());
                    }
                }
            }
            self.directories = roots;
            return;
        }
        for directory in directories.iter().filter(|d| {
            !d.contains('/')
                && matches!(
                    d.to_ascii_lowercase().as_str(),
                    "include" | "src" | "lib" | "api" | "inc"
                )
        }) {
            if !self.directories.contains(directory) {
                self.directories.push(directory.clone());
            }
        }
        self.directories.sort();
    }
    pub(crate) fn resolve(&self, specifier: &str, from: &str) -> Option<String> {
        self.resolve_as(specifier, from, "cpp")
    }
    /// A lower-confidence filename candidate, kept distinct from include resolution.
    pub(crate) fn fallback(&self, specifier: &str, from: &str) -> Option<String> {
        if !specifier.contains('/')
            && !specifier.rsplit_once('.').is_some_and(|(_, extension)| {
                (1..=4).contains(&extension.len())
                    && extension.as_bytes()[0].is_ascii_alphabetic()
                    && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        {
            return None;
        }
        let basename = specifier.rsplit('/').next()?;
        let files = self.basenames.get(basename)?;
        if let Some(exact) = files.iter().find(|file| file.as_str() == specifier) {
            return Some(exact.clone());
        }
        let parent = from.rsplit_once('/').map_or("", |(parent, _)| parent);
        let mut suffix = files
            .iter()
            .filter(|file| file.ends_with(specifier))
            .collect::<Vec<_>>();
        suffix.sort_by_key(|file| {
            let directory = file.rsplit_once('/').map_or("", |(parent, _)| parent);
            let shared = parent
                .split('/')
                .zip(directory.split('/'))
                .take_while(|(a, b)| a == b)
                .count();
            (
                std::cmp::Reverse(parent == directory),
                std::cmp::Reverse(shared),
            )
        });
        suffix
            .first()
            .copied()
            .or_else(|| (files.len() == 1).then(|| &files[0]))
            .cloned()
    }
    pub(crate) fn resolve_as(&self, specifier: &str, from: &str, language: &str) -> Option<String> {
        let parent = from.rsplit_once('/').map_or("", |(parent, _)| parent);
        let sibling = crate::auxiliary_sources::normalize(parent, specifier);
        if let Some(sibling) = &sibling
            && self.files.contains(sibling)
        {
            return Some(sibling.clone());
        }
        if standard_header(specifier) {
            return None;
        }
        let extensions: &[&str] = if language == "c" {
            &[".h", ".c"]
        } else {
            &[".h", ".hpp", ".hxx", ".cpp", ".cc", ".cxx"]
        };
        let probe = |base: &str| {
            extensions
                .iter()
                .map(|extension| format!("{base}{extension}"))
                .chain(std::iter::once(base.to_owned()))
                .find(|path| self.files.contains(path))
        };
        if specifier.starts_with('.') {
            return sibling.as_deref().and_then(probe);
        }
        if let Some(path) = probe(specifier) {
            return Some(path);
        }
        self.directories
            .iter()
            .find_map(|directory| probe(&format!("{directory}/{specifier}")))
    }
}
fn lexical_absolute(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}
fn shell_words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = command.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        let Some(first) = chars.next() else {
            break;
        };
        let mut argument = String::new();
        if matches!(first, '\'' | '"') {
            while let Some(ch) = chars.next() {
                if ch == first {
                    break;
                }
                if first == '"' && ch == '\\' {
                    if let Some(next) = chars.next() {
                        argument.push(next);
                    }
                } else {
                    argument.push(ch);
                }
            }
        } else {
            argument.push(first);
            while chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                argument.push(chars.next().unwrap());
            }
        }
        out.push(argument);
    }
    out
}
fn standard_header(specifier: &str) -> bool {
    const HEADERS: &str = "assert.h complex.h ctype.h errno.h fenv.h float.h inttypes.h iso646.h limits.h locale.h math.h setjmp.h signal.h stdalign.h stdarg.h stdatomic.h stdbool.h stddef.h stdint.h stdio.h stdlib.h stdnoreturn.h string.h tgmath.h threads.h time.h uchar.h wchar.h wctype.h cassert ccomplex cctype cerrno cfenv cfloat cinttypes ciso646 climits clocale cmath csetjmp csignal cstdalign cstdarg cstdbool cstddef cstdint cstdio cstdlib cstring ctgmath ctime cuchar cwchar cwctype algorithm any array atomic barrier bit bitset charconv chrono codecvt compare complex concepts condition_variable coroutine deque exception execution expected filesystem format forward_list fstream functional future generator initializer_list iomanip ios iosfwd iostream istream iterator latch limits list locale map mdspan memory memory_resource mutex new numbers numeric optional ostream print queue random ranges ratio regex scoped_allocator semaphore set shared_mutex source_location span spanstream sstream stack stacktrace stdexcept stdfloat stop_token streambuf string string_view strstream syncstream system_error thread tuple type_traits typeindex typeinfo unordered_map unordered_set utility valarray variant vector version";
    HEADERS
        .split_whitespace()
        .any(|header| header == specifier || Some(header) == specifier.strip_suffix(".h"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn include_search_matches_independent_project_import_edges() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/cpp-resolution/cases.json"
        ))
        .unwrap();
        let mut checked = 0;
        for case in corpus["cases"].as_array().unwrap() {
            let nodes: HashMap<_, _> = case["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| (node["id"].as_str().unwrap(), node))
                .collect();
            let resolver = CppImports::new(case["sources"].as_object().unwrap().keys().cloned());
            for edge in case["allEdges"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|edge| edge["kind"] == "imports")
            {
                let from = nodes[edge["source"].as_str().unwrap()];
                let to = nodes[edge["target"].as_str().unwrap()];
                let specifier = edge["metadata"]["refName"].as_str().unwrap();
                assert_eq!(
                    resolver
                        .resolve_as(
                            specifier,
                            from["filePath"].as_str().unwrap(),
                            from["language"].as_str().unwrap()
                        )
                        .or_else(||resolver.fallback(specifier,from["filePath"].as_str().unwrap())).as_deref(),
                    to["filePath"].as_str(),
                    "{} {specifier}",
                    case["origin"]
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 82);
    }
    #[test]
    fn compilation_database_order_and_local_headers_are_preserved() {
        let mut resolver = CppImports::new(
            [
                "src/main.cpp",
                "src/api.h",
                "include/api.h",
                "custom/api.h",
                "custom/other.h",
                "two words/third.h",
                "include/fallback.h",
                "include/vector",
            ]
            .map(str::to_owned),
        );
        assert_eq!(
            resolver.resolve("api.h", "src/main.cpp"),
            Some("src/api.h".into())
        );
        assert_eq!(
            resolver.resolve("api.h", "main.cpp"),
            Some("custom/api.h".into())
        );
        assert_eq!(resolver.resolve("vector", "main.cpp"), None);
        let manifests=BTreeMap::from([("build/compile_commands.json".into(),serde_json::json!([{"directory":"/project/build","command":"clang -I ../custom -isystem \"/project/two words\" -I/usr/include"}]).to_string())]);
        resolver.configure("/project", &manifests, &[]);
        assert_eq!(
            resolver.resolve("api.h", "main.cpp"),
            Some("custom/api.h".into())
        );
        assert_eq!(
            resolver.resolve("other", "main.cpp"),
            Some("custom/other.h".into())
        );
        assert_eq!(
            resolver.resolve("third.h", "main.cpp"),
            Some("two words/third.h".into())
        );
        assert_eq!(resolver.resolve("fallback.h", "main.cpp"), None);
        assert_eq!(resolver.resolve("../../custom/api.h", "main.cpp"), None);
    }
    #[test]
    fn compilation_database_changes_commit_with_source_snapshot() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("project");
        std::fs::create_dir_all(root.join("build"))?;
        let root = root.canonicalize()?;
        for directory in ["one", "two"] {
            std::fs::create_dir_all(root.join(directory))?;
            std::fs::write(root.join(directory).join("api.h"), "void act();\n")?;
        }
        std::fs::write(
            root.join("main.cpp"),
            "#include <api.h>\nvoid run(){act();}\n",
        )?;
        let database = temp.path().join("index.sqlite");
        let config = root.join("build/compile_commands.json");
        let write_config = |directory: &str| {
            std::fs::write(&config,serde_json::json!([{"directory":root.to_str().unwrap(),"arguments":["clang","-I",directory]}]).to_string())
        };
        write_config("two")?;
        let mut engine = crate::Engine::open(&root, Some(&database))?;
        engine.sync()?;
        let selected = |engine: &crate::Engine| -> anyhow::Result<String> {
            let graph = crate::graph::Graph::load(&engine.connection, 1)?;
            let edge = graph
                .edges
                .iter()
                .find(|edge| edge.kind == "imports" && graph.nodes[edge.from].path == "main.cpp")
                .unwrap();
            Ok(graph.nodes[edge.to.unwrap()].path.clone())
        };
        assert_eq!(selected(&engine)?, "two/api.h");
        write_config("one")?;
        assert_eq!(selected(&engine)?, "two/api.h");
        drop(engine);
        let mut engine = crate::Engine::open(&root, Some(&database))?;
        assert_eq!(selected(&engine)?, "two/api.h");
        engine.sync()?;
        assert_eq!(selected(&engine)?, "one/api.h");
        Ok(())
    }
}
