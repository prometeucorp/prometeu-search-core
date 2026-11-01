//! Confined manifest and directory capture for one indexing transaction.
use super::workspace::{self, Manifests};
use anyhow::{Context, Result, ensure};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    io::{ErrorKind, Read},
    path::{Component, Path},
};

pub(crate) fn is_manifest(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                "package.json"
                    | "compile_commands.json"
                    | "artisan"
                    | "angular.json"
                    | "nx.json"
                    | "tsconfig.json"
                    | "tsconfig.base.json"
                    | "jsconfig.json"
                    | "deno.json"
                    | "deno.jsonc"
                    | "pnpm-workspace.yaml"
                    | "oh-package.json5"
                    | "requirements.txt"
                    | "requirements-dev.txt"
                    | "pyproject.toml"
                    | "setup.cfg"
                    | "Pipfile"
                    | "Cargo.toml"
                    | "Package.swift"
                    | "pom.xml"
                    | "build.gradle"
                    | "build.gradle.kts"
                    | "build.sbt"
                    | "application.conf"
                    | "composer.json"
                    | "Gemfile"
            ) || name.ends_with(".csproj")
        })
}
struct Capture<'a> {
    root: &'a Dir,
    path: &'a str,
    manifests: Manifests,
    directories: BTreeSet<String>,
    listed: HashMap<String, Vec<String>>,
    reads: usize,
    entries: usize,
    bytes: usize,
}
impl Capture<'_> {
    fn directory(&mut self, relative: &str) -> Result<Option<Dir>> {
        let Some(relative) = workspace::target(self.path, "", relative) else {
            return Ok(None);
        };
        let mut directory = self.root.try_clone()?;
        for component in Path::new(&relative).components() {
            let Component::Normal(name) = component else {
                return Ok(None);
            };
            directory = match directory.open_dir_nofollow(name) {
                Ok(dir) => dir,
                Err(error)
                    if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
                {
                    return Ok(None);
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("cannot inspect workspace directory {relative}"));
                }
            };
        }
        self.directories.insert(relative);
        Ok(Some(directory))
    }
    fn read_config(&mut self, path: &str) -> Result<bool> {
        if self.manifests.contains_key(path) {
            return Ok(true);
        }
        let file = match crate::source::open(self.root, Path::new(path)) {
            Ok(file) => file,
            Err(error)
                if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
            {
                return Ok(false);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("cannot inspect module configuration {path}"));
            }
        };
        if !file.metadata()?.is_file() {
            return Ok(false);
        }
        self.read_manifest(path)?;
        Ok(true)
    }
    fn read_manifest(&mut self, path: &str) -> Result<()> {
        if self.manifests.contains_key(path) {
            return Ok(());
        }
        let mut file = match crate::source::open(self.root, Path::new(path)) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read project manifest {path}"));
            }
        };
        ensure!(
            file.metadata()?.is_file(),
            "project manifest must be a regular file: {path}"
        );
        let mut bytes = Vec::new();
        (&mut file)
            .take(crate::index::MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= crate::index::MAX_FILE_BYTES,
            "project manifest exceeds 1 MiB: {path}"
        );
        self.bytes += bytes.len();
        ensure!(
            self.bytes <= 8 * 1024 * 1024,
            "project manifest snapshot exceeds 8 MiB"
        );
        self.manifests.insert(
            path.into(),
            String::from_utf8(bytes)
                .with_context(|| format!("project manifest must be UTF-8: {path}"))?,
        );
        Ok(())
    }
    fn list(&mut self, relative: &str) -> Result<Vec<String>> {
        let Some(relative) = workspace::target(self.path, "", relative) else {
            return Ok(Vec::new());
        };
        if let Some(names) = self.listed.get(&relative) {
            return Ok(names.clone());
        }
        self.reads += 1;
        ensure!(
            self.reads <= 50_000,
            "workspace directory-read budget exceeded"
        );
        let Some(directory) = self.directory(&relative)? else {
            return Ok(Vec::new());
        };
        let mut dirs = Vec::new();
        let mut manifests = Vec::new();
        for entry in directory.entries()? {
            let entry = entry?;
            self.entries += 1;
            ensure!(
                self.entries <= 500_000,
                "workspace directory-entry budget exceeded"
            );
            let filename = entry.file_name();
            let Some(name) = filename.to_str() else {
                continue;
            };
            let name = name.to_string();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                self.directories.insert(if relative.is_empty() {
                    name.clone()
                } else {
                    format!("{relative}/{name}")
                });
                dirs.push(name);
            } else if is_manifest(Path::new(&name)) {
                manifests.push(if relative.is_empty() {
                    name
                } else {
                    format!("{relative}/{name}")
                });
            }
        }
        for manifest in manifests {
            self.read_manifest(&manifest)?;
        }
        dirs.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        self.listed.insert(relative, dirs.clone());
        Ok(dirs)
    }
    fn target_manifests(&mut self, relative: &str) -> Result<()> {
        if self.directory(relative)?.is_none() {
            return Ok(());
        }
        for name in ["package.json", "oh-package.json5"] {
            self.read_manifest(&if relative.is_empty() {
                name.into()
            } else {
                format!("{relative}/{name}")
            })?;
        }
        Ok(())
    }
}
pub(crate) fn snapshot<'a>(
    root: &Dir,
    project_root: &Path,
    source_paths: impl IntoIterator<Item = &'a str>,
) -> Result<(Manifests, Vec<String>)> {
    let root_path = project_root
        .to_str()
        .context("project root must be UTF-8")?;
    let mut capture = Capture {
        root,
        path: root_path,
        manifests: BTreeMap::new(),
        directories: BTreeSet::from([String::new()]),
        listed: HashMap::new(),
        reads: 0,
        entries: 0,
        bytes: 0,
    };
    capture.list("")?;
    for directory in ["build", "cmake-build-debug", "cmake-build-release", "out"] {
        capture.read_config(&format!("{directory}/compile_commands.json"))?;
    }
    if let Some(source) = capture.manifests.get("Cargo.toml").cloned() {
        let members = super::cargo_workspace::expand_members(&source, |dir| capture.list(dir))?;
        for member in members {
            if let Some(member) = workspace::target(root_path, "", &member)
                && capture.directory(&member)?.is_some()
            {
                let manifest = if member.is_empty() {
                    "Cargo.toml".to_owned()
                } else {
                    format!("{member}/Cargo.toml")
                };
                capture.read_config(&manifest)?;
            }
        }
    }
    let patterns = workspace::read_globs(&capture.manifests, "");
    let excluded: Vec<_> = patterns
        .iter()
        .filter_map(|pattern| pattern.strip_prefix('!'))
        .collect();
    for pattern in patterns.iter().filter(|pattern| !pattern.starts_with('!')) {
        let directories = workspace::expand_glob(pattern, |dir| capture.list(dir))?;
        for dir in directories {
            let mut skip = false;
            for pattern in &excluded {
                if workspace::glob_matches(pattern, &dir)? {
                    skip = true;
                    break;
                }
            }
            if !skip && let Some(dir) = workspace::target(root_path, "", &dir) {
                capture.target_manifests(&dir)?;
            }
        }
    }
    let mut queue = VecDeque::from([(String::new(), 0)]);
    let mut visited = 0;
    let mut targets = Vec::new();
    while let Some((rel, depth)) = queue.pop_front() {
        visited += 1;
        if visited > 8000 {
            break;
        }
        let dirs = capture.list(&rel)?;
        if depth < 6 {
            for name in dirs {
                if !workspace::skip_dependency_dir(&name) {
                    queue.push_back((
                        if rel.is_empty() {
                            name
                        } else {
                            format!("{rel}/{name}")
                        },
                        depth + 1,
                    ));
                }
            }
        }
        for name in ["package.json", "oh-package.json5"] {
            let manifest = if rel.is_empty() {
                name.into()
            } else {
                format!("{rel}/{name}")
            };
            let Some(source) = capture.manifests.get(&manifest) else {
                continue;
            };
            let dependencies = if name == "package.json" {
                serde_json::from_str::<serde_json::Value>(source)
                    .ok()
                    .map_or_else(Vec::new, |pkg| workspace::npm_dependencies(&pkg))
            } else {
                workspace::ohpm_dependencies(source)?
            };
            for (_, target) in dependencies {
                if let Some(target) = workspace::target(root_path, &rel, &target) {
                    targets.push(target);
                }
            }
        }
    }
    targets.sort();
    targets.dedup();
    for target in targets {
        capture.target_manifests(&target)?;
    }
    let mut config_dirs = BTreeSet::from([String::new()]);
    for path in source_paths {
        let mut dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        while !dir.is_empty() {
            config_dirs.insert(dir.to_owned());
            dir = dir.rsplit_once('/').map_or("", |(dir, _)| dir);
        }
    }
    for dir in config_dirs {
        for name in super::aliases::CONFIGS.into_iter().chain([
            "package.json",
            "Cargo.toml",
            "deno.json",
            "deno.jsonc",
            "angular.json",
            "nx.json",
        ]) {
            capture.read_config(&if dir.is_empty() {
                name.into()
            } else {
                format!("{dir}/{name}")
            })?;
        }
    }
    let mut pending: Vec<_> = capture
        .manifests
        .keys()
        .filter(|path| super::aliases::CONFIGS.contains(&path.rsplit('/').next().unwrap_or("")))
        .map(|path| (path.clone(), 0usize))
        .collect();
    let mut checked = HashMap::new();
    while let Some((file, depth)) = pending.pop() {
        if depth > 32
            || checked
                .get(&file)
                .is_some_and(|&previous| previous <= depth)
        {
            continue;
        }
        checked.insert(file.clone(), depth);
        let Some(raw) = capture
            .manifests
            .get(&file)
            .map(|source| super::aliases::parse(source))
            .transpose()?
            .flatten()
        else {
            continue;
        };
        let specs: Vec<_> = super::aliases::extends(&raw)?
            .into_iter()
            .map(str::to_owned)
            .collect();
        for spec in specs {
            for target in super::aliases::extends_candidates(root_path, &file, &spec) {
                if capture.read_config(&target)? {
                    pending.push((target, depth + 1));
                    break;
                }
            }
        }
    }
    ensure!(
        capture.directories.iter().map(String::len).sum::<usize>() <= 8 * 1024 * 1024,
        "workspace directory snapshot exceeds 8 MiB"
    );
    Ok((capture.manifests, capture.directories.into_iter().collect()))
}
