//! Declared package ownership and external dependencies from committed manifests.
use super::workspace::{Manifests, WorkspacePackages, entries, jsonc};
use anyhow::Result;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
struct Scope {
    own: HashSet<String>,
    dependencies: HashSet<String>,
}
pub(crate) struct PackageScopes(BTreeMap<String, Scope>);
impl PackageScopes {
    pub(crate) fn new(manifests: &Manifests) -> Result<Self> {
        let mut scopes: BTreeMap<String, Scope> = BTreeMap::new();
        for (path, source) in manifests {
            let (directory, file) = path.rsplit_once('/').unwrap_or(("", path));
            if file == "package.json" {
                let Ok(json) = serde_json::from_str::<Value>(source) else {
                    continue;
                };
                let scope = scopes.entry(directory.into()).or_default();
                if let Some(name) = json
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    scope.own.insert(name.into());
                }
                for field in [
                    "dependencies",
                    "devDependencies",
                    "peerDependencies",
                    "optionalDependencies",
                ] {
                    let Some(deps) = json.get(field) else {
                        continue;
                    };
                    for (name, version) in entries(deps) {
                        if version.as_str().is_some_and(|v| {
                            ["workspace:", "file:", "link:", "portal:"]
                                .iter()
                                .any(|p| v.starts_with(p))
                        }) {
                            scope.own.insert(name);
                        } else {
                            scope.dependencies.insert(name);
                        }
                    }
                }
            } else if matches!(file, "deno.json" | "deno.jsonc") {
                let Some(json) = jsonc::parse(source)? else {
                    continue;
                };
                let Some(imports) = json.get("imports") else {
                    continue;
                };
                let scope = scopes.entry(directory.into()).or_default();
                for (name, target) in entries(imports) {
                    if target.as_str().is_some_and(|v| {
                        ["jsr:", "npm:", "http:", "https:"]
                            .iter()
                            .any(|p| v.starts_with(p))
                    }) {
                        scope
                            .dependencies
                            .insert(name.strip_suffix('/').unwrap_or(&name).into());
                    }
                }
            }
        }
        Ok(Self(scopes))
    }
    pub(super) fn external(
        &self,
        source: &str,
        from: &str,
        workspaces: Option<&WorkspacePackages>,
    ) -> bool {
        static BUILTINS: std::sync::LazyLock<HashSet<&'static str>> = std::sync::LazyLock::new(
            || {
                "_http_agent _http_client _http_common _http_incoming _http_outgoing _http_server _stream_duplex _stream_passthrough _stream_readable _stream_transform _stream_wrap _stream_writable _tls_common _tls_wrap assert assert/strict async_hooks buffer child_process cluster console constants crypto dgram diagnostics_channel dns dns/promises domain events fs fs/promises http http2 https inspector inspector/promises module net os path path/posix path/win32 perf_hooks process punycode querystring readline readline/promises repl stream stream/consumers stream/promises stream/web string_decoder sys timers timers/promises tls trace_events tty url util util/types v8 vm wasi worker_threads zlib".split(' ').collect()
            },
        );
        if ["node:", "bun:", "jsr:", "npm:", "http:", "https:", "@std/"]
            .iter()
            .any(|p| source.starts_with(p))
            || BUILTINS.contains(source)
        {
            return true;
        }
        let name = if source.starts_with('@') {
            source.split('/').take(2).collect::<Vec<_>>().join("/")
        } else {
            source.split('/').next().unwrap_or("").into()
        };
        let normalized = from.replace('\\', "/");
        let mut directory = normalized.rsplit_once('/').map_or("", |(dir, _)| dir);
        let mut own = false;
        let mut dependency = false;
        let provider = if source.starts_with("astro:") {
            Some("astro")
        } else if ["$app", "$env", "$service-worker"].iter().any(|p| {
            source == *p
                || source
                    .strip_prefix(p)
                    .is_some_and(|tail| tail.starts_with('/'))
        }) {
            Some("@sveltejs/kit")
        } else {
            None
        };
        let mut provider_own = false;
        loop {
            if let Some(scope) = self.0.get(directory) {
                own |= scope.own.contains(&name);
                dependency |= scope.dependencies.contains(&name);
                provider_own |= provider.is_some_and(|p| scope.own.contains(p));
            }
            if directory.is_empty() {
                break;
            }
            directory = directory.rsplit_once('/').map_or("", |(dir, _)| dir);
        }
        if own {
            return false;
        }
        if dependency {
            return true;
        }
        provider.is_some_and(|provider| {
            !provider_own && !workspaces.is_some_and(|ws| ws.by_name.contains_key(provider))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_package_ownership_matches_independent_reference() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/workspace/imports.json"
        ))
        .unwrap();
        let empty = PackageScopes::new(&BTreeMap::new()).unwrap();
        for name in corpus["builtins"].as_array().unwrap() {
            assert!(empty.external(name.as_str().unwrap(), "main.ts", None));
        }
        for case in corpus["packages"].as_array().unwrap() {
            let manifests = serde_json::from_value(case["files"].clone()).unwrap();
            let scopes = PackageScopes::new(&manifests).unwrap();
            let directories = manifests
                .keys()
                .filter_map(|path| path.rsplit_once('/').map(|(dir, _)| dir.into()))
                .collect::<Vec<_>>();
            let workspace =
                WorkspacePackages::from_snapshot(&manifests, &directories, "/fixture").unwrap();
            for query in case["expected"].as_array().unwrap() {
                assert_eq!(
                    scopes.external(
                        query["source"].as_str().unwrap(),
                        query["from"].as_str().unwrap(),
                        workspace.as_ref()
                    ),
                    query["external"].as_bool().unwrap(),
                    "{} {query}",
                    case["name"]
                );
            }
        }
    }
}
