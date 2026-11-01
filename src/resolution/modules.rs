//! Shared ES-module lookup for graph references and route finalization.
use super::{Aliases, PackageScopes, WorkspacePackages, workspace};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::BTreeMap;

pub(super) struct Modules<'a> {
    pub workspace: Option<&'a WorkspacePackages>,
    pub aliases: &'a Aliases,
    pub packages: &'a PackageScopes,
    pub browser: bool,
}
impl Modules<'_> {
    pub(super) fn resolve(
        &self,
        from: &str,
        module: &str,
        language: &str,
        exists: impl Fn(&str) -> bool,
    ) -> Option<String> {
        if !workspace::esm(language) {
            return None;
        }
        if module.starts_with('.') {
            let parent = from.rsplit_once('/').map_or("", |(parent, _)| parent);
            let path = super::super::normalize_path(&format!("{parent}/{module}"))?;
            return workspace::indexed(&path, language, &exists, false);
        }
        let local_package = self.workspace.is_some_and(|ws| {
            ws.resolve(module, Some(from))
                .is_some_and(|path| !path.is_empty())
        });
        let builtin = [
            "fs",
            "path",
            "os",
            "crypto",
            "http",
            "https",
            "url",
            "util",
            "events",
            "stream",
            "child_process",
            "buffer",
        ]
        .contains(&module);
        let external = !local_package
            && (builtin
                || !self.aliases.prefix(module)
                    && !["@/", "~/", "src/"]
                        .iter()
                        .any(|prefix| module.starts_with(prefix)));
        if external {
            let candidates = self.aliases.own(from, module);
            if self.packages.external(module, from, self.workspace) {
                if !matches!(language, "typescript" | "tsx" | "arkts") {
                    return None;
                }
                return candidates
                    .into_iter()
                    .map(|candidate| format!("{candidate}.d.ts"))
                    .find(|candidate| exists(candidate));
            }
            return candidates
                .iter()
                .find_map(|candidate| workspace::indexed(candidate, language, &exists, false));
        }
        if let Some(found) = self
            .aliases
            .aliases(from, module)
            .iter()
            .find_map(|candidate| workspace::indexed(candidate, language, &exists, false))
        {
            return Some(found);
        }
        if let Some(targets) = self.workspace.and_then(|ws| ws.targets(module, Some(from)))
            && let Some(found) = targets.indexed(language, self.browser, &exists)
        {
            return Some(found);
        }
        for (prefix, replacement) in [
            ("@/", "src/"),
            ("~/", "src/"),
            ("@src/", "src/"),
            ("src/", "src/"),
            ("@app/", "app/"),
            ("app/", "app/"),
        ] {
            if let Some(tail) = module.strip_prefix(prefix)
                && let Some(found) =
                    workspace::indexed(&format!("{replacement}{tail}"), language, &exists, false)
            {
                return Some(found);
            }
        }
        workspace::indexed(module, language, &exists, false)
    }
}

pub(crate) struct ProjectModules<'a> {
    sources: &'a BTreeMap<String, String>,
    workspace: Option<WorkspacePackages>,
    aliases: Aliases,
    packages: PackageScopes,
}
impl<'a> ProjectModules<'a> {
    pub(crate) fn new(
        connection: &Connection,
        sources: &'a BTreeMap<String, String>,
    ) -> Result<Self> {
        let manifests = workspace::load_manifests(connection)?;
        let root: String =
            connection.query_row("SELECT value FROM metadata WHERE key='root'", [], |row| {
                row.get(0)
            })?;
        Ok(Self {
            sources,
            workspace: WorkspacePackages::load(connection, &manifests)?,
            aliases: Aliases::new(&manifests, &root)?,
            packages: PackageScopes::new(&manifests)?,
        })
    }
    pub(crate) fn resolve(
        &self,
        from: &str,
        module: &str,
        language: &str,
    ) -> Result<Option<String>> {
        let browser = if self
            .workspace
            .as_ref()
            .and_then(|ws| ws.targets(module, Some(from)))
            .is_some_and(|targets| !targets.browser.is_empty())
        {
            self.sources
                .get(from)
                .map(|source| workspace::browser_source(source))
                .transpose()?
                .unwrap_or(false)
        } else {
            false
        };
        Ok(Modules {
            workspace: self.workspace.as_ref(),
            aliases: &self.aliases,
            packages: &self.packages,
            browser,
        }
        .resolve(from, module, language, |file| {
            self.sources.contains_key(file)
        }))
    }
}
