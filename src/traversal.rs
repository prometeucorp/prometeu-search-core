//! Source-confined filesystem traversal with native Git index visibility.
use crate::{
    parser,
    project_config::{Config, Patterns},
};
use anyhow::{Context, Result, ensure};
use cap_fs_ext::DirExt;
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod defaults;
mod git;

#[derive(Clone)]
struct Rule {
    root: PathBuf,
    patterns: Patterns,
}

#[derive(Clone)]
struct Scope {
    repository: Option<Arc<git::Repository>>,
    root: PathBuf,
    defaults: defaults::Defaults,
    git_rules: Vec<Rule>,
    custom: Vec<Rule>,
    ignored_parent: bool,
    embedded: bool,
}

pub(crate) struct Traversal<'a> {
    root: &'a Path,
    source: &'a cap_std::fs::Dir,
    database: &'a Path,
    config: &'a Config,
    scope: Scope,
    pub fingerprint: String,
}

impl<'a> Traversal<'a> {
    pub fn prepare(
        root: &'a Path,
        source: &'a cap_std::fs::Dir,
        database: &'a Path,
        config: &'a Config,
    ) -> Result<Self> {
        let mut repository = git::Repository::discover(root)?.map(Arc::new);
        let mut rules = Vec::new();
        let mut fingerprint = blake3::Hasher::new();
        if let Some(repo) = repository.clone() {
            fingerprint.update(repo.fingerprint.as_bytes());
            let mut extra = read_patterns(&repo.common_dir.join("info/exclude"), &mut fingerprint)?;
            extra.extend(untracked_patterns(&repo, &mut fingerprint)?);
            rules.push(Rule {
                root: repo.worktree.clone(),
                patterns: Patterns::with_case(&extra, repo.ignore_case),
            });
            let ancestors: Vec<_> = root
                .ancestors()
                .take_while(|path| path.starts_with(&repo.worktree))
                .collect();
            for directory in ancestors.into_iter().rev() {
                if directory == root
                    && directory != repo.worktree
                    && decision(&rules, directory, true) == Some(true)
                {
                    repository = None;
                    rules.clear();
                    break;
                }
                let own = read_patterns(&directory.join(".gitignore"), &mut fingerprint)?;
                rules.push(Rule {
                    root: directory.to_path_buf(),
                    patterns: Patterns::with_case(&own, repo.ignore_case),
                });
            }
        }
        let project = project_patterns(root, repository.as_deref(), &mut fingerprint)?;
        let scope = Scope {
            repository,
            root: root.to_path_buf(),
            defaults: defaults::Defaults::new(&project),
            git_rules: rules,
            custom: Vec::new(),
            ignored_parent: false,
            embedded: false,
        };
        Ok(Self {
            root,
            source,
            database,
            config,
            scope,
            fingerprint: fingerprint.finalize().to_hex().to_string(),
        })
    }

    pub fn files(&self, selected: Option<&HashSet<String>>) -> Result<Vec<PathBuf>> {
        let permitted = selected.map(|paths| {
            paths
                .iter()
                .flat_map(|path| Path::new(path).ancestors().map(Path::to_path_buf))
                .collect::<HashSet<_>>()
        });
        let mut files = BTreeSet::new();
        let mut queue = VecDeque::from([(PathBuf::new(), self.scope.clone())]);
        let mut repos = HashSet::new();
        while let Some((relative, mut scope)) = queue.pop_front() {
            if permitted
                .as_ref()
                .is_some_and(|permitted| !permitted.contains(&relative))
            {
                continue;
            }
            let absolute = self.root.join(&relative);
            let directory = open_directory(self.source, &relative)?;
            let mut unused_hash = blake3::Hasher::new();
            if absolute != scope.root {
                let rules = read_patterns(&absolute.join(".gitignore"), &mut unused_hash)?;
                let ignore_case = scope
                    .repository
                    .as_ref()
                    .is_none_or(|repo| repo.ignore_case);
                scope.git_rules.push(Rule {
                    root: absolute.clone(),
                    patterns: Patterns::with_case(&rules, ignore_case),
                });
            }
            for filename in [".ignore", ".prometeu-searchignore"] {
                let patterns = read_patterns(&absolute.join(filename), &mut unused_hash)?;
                scope.custom.push(Rule {
                    root: absolute.clone(),
                    patterns: Patterns::new(&patterns),
                });
            }
            let mut entries = directory.entries()?.collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = relative.join(entry.file_name());
                if hard_excluded(&path)
                    || permitted
                        .as_ref()
                        .is_some_and(|permitted| !permitted.contains(&path))
                {
                    continue;
                }
                let absolute = self.root.join(&path);
                if absolute == self.database {
                    continue;
                }
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    continue;
                }
                let is_dir = kind.is_dir();
                if !is_dir && !kind.is_file() {
                    continue;
                }
                if self.config.exclude.matches(&path, is_dir) {
                    continue;
                }
                if decision(&scope.custom, &absolute, is_dir) == Some(true) {
                    continue;
                }
                let scoped = absolute.strip_prefix(&scope.root)?;
                let ignored_by_scope = scope.defaults.excludes(scoped, is_dir)
                    || (scope.embedded && defaults::excluded(&path, is_dir));
                let ignored_by_git = scope.ignored_parent
                    || decision(&scope.git_rules, &absolute, is_dir) == Some(true);
                if is_dir {
                    if ignored_by_scope
                        || (scope.repository.is_some()
                            && ignored_by_git
                            && !scope
                                .repository
                                .as_ref()
                                .unwrap()
                                .has_tracked_descendant(&absolute))
                    {
                        if scope.repository.is_some()
                            && !defaults::excluded(&path, true)
                            && !self.config.settings.include_ignored.is_empty()
                        {
                            for repo in self.ignored_repositories(&path)? {
                                if repos.insert(repo.clone()) {
                                    queue.push_back((repo.clone(), self.embedded_scope(&repo)?));
                                }
                            }
                        }
                        continue;
                    }
                    if scope.repository.is_some() {
                        if git::worktree_pointer(&absolute)? {
                            continue;
                        }
                        if let Some(repo) = git::Repository::at(&absolute)? {
                            if !defaults::excluded(&path, true) && repos.insert(path.clone()) {
                                queue.push_back((path.clone(), self.repo_scope(&path, repo)?));
                            }
                            continue;
                        }
                        // An uninitialized submodule is an opaque gitlink, not a source directory.
                        if scope.repository.as_ref().is_some_and(|repo| {
                            repo.relative(&absolute)
                                .is_some_and(|path| repo.gitlinks.contains(path))
                        }) {
                            continue;
                        }
                    } else if ignored_by_git {
                        continue;
                    }
                    let mut child = scope.clone();
                    child.ignored_parent = ignored_by_git;
                    queue.push_back((path, child));
                } else {
                    let manifest = path
                        .file_name()
                        .is_some_and(|name| name == "go.mod" || name == "package.json");
                    if !manifest
                        && (ignored_by_scope
                            || (ignored_by_git
                                && !scope
                                    .repository
                                    .as_ref()
                                    .is_some_and(|repo| repo.tracked(&absolute))))
                    {
                        continue;
                    }
                    if potential(self.config, &path) {
                        files.insert(path);
                    }
                }
            }
        }
        self.force_included(permitted.as_ref(), &mut files)?;
        Ok(files.into_iter().collect())
    }

    fn repo_scope(&self, path: &Path, repo: git::Repository) -> Result<Scope> {
        let root = self.root.join(path);
        let mut fingerprint = blake3::Hasher::new();
        let project = project_patterns(&root, Some(&repo), &mut fingerprint)?;
        let mut rules = read_patterns(&repo.common_dir.join("info/exclude"), &mut fingerprint)?;
        rules.extend(untracked_patterns(&repo, &mut fingerprint)?);
        rules.extend(read_patterns(&root.join(".gitignore"), &mut fingerprint)?);
        let rules = Patterns::with_case(&rules, repo.ignore_case);
        Ok(Scope {
            repository: Some(Arc::new(repo)),
            root: root.clone(),
            defaults: defaults::Defaults::new(&project),
            git_rules: vec![Rule {
                root,
                patterns: rules,
            }],
            custom: Vec::new(),
            ignored_parent: false,
            embedded: true,
        })
    }

    fn embedded_scope(&self, path: &Path) -> Result<Scope> {
        let repo = git::Repository::at(&self.root.join(path))?
            .context("embedded repository disappeared during reconciliation")?;
        self.repo_scope(path, repo)
    }

    fn ignored_repositories(&self, under: &Path) -> Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        let mut queue = VecDeque::from([(under.to_path_buf(), 0)]);
        let mut examined = 0;
        let opted = self.config.include_ignored.matches(under, true);
        while let Some((path, depth)) = queue.pop_front() {
            examined += 1;
            if examined > 2000 {
                break;
            }
            if hard_excluded(&path)
                || defaults::excluded(&path, true)
                || self.config.exclude.matches(&path, true)
            {
                continue;
            }
            let absolute = self.root.join(&path);
            if git::worktree_pointer(&absolute)? {
                continue;
            }
            if git::Repository::at(&absolute)?.is_some() {
                if opted || self.config.include_ignored.matches(&path, true) {
                    found.push(path);
                }
                continue;
            }
            if depth >= 4 {
                continue;
            }
            let directory = open_directory(self.source, &path)?;
            let mut children = Vec::new();
            for entry in directory.entries()? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    children.push(path.join(entry.file_name()));
                }
            }
            children.sort();
            queue.extend(children.into_iter().map(|path| (path, depth + 1)));
        }
        Ok(found)
    }

    fn force_included(
        &self,
        permitted: Option<&HashSet<PathBuf>>,
        files: &mut BTreeSet<PathBuf>,
    ) -> Result<()> {
        let roots = crate::project_config::static_roots(&self.config.settings.include);
        if roots.is_empty() {
            return Ok(());
        }
        let mut queue = VecDeque::from([PathBuf::new()]);
        while let Some(relative) = queue.pop_front() {
            if permitted.is_some_and(|paths| !paths.contains(&relative)) {
                continue;
            }
            let directory = open_directory(self.source, &relative)?;
            for entry in directory.entries()? {
                let entry = entry?;
                let path = relative.join(entry.file_name());
                let kind = entry.file_type()?;
                let is_dir = kind.is_dir();
                if kind.is_symlink()
                    || hard_excluded(&path)
                    || defaults::excluded(&path, is_dir)
                    || self.config.exclude.matches(&path, is_dir)
                    || self.root.join(&path) == self.database
                    || permitted.is_some_and(|paths| !paths.contains(&path))
                {
                    continue;
                }
                if is_dir {
                    if roots
                        .iter()
                        .any(|prefix| prefix.starts_with(&path) || path.starts_with(prefix))
                    {
                        queue.push_back(path);
                    }
                } else if kind.is_file()
                    && self.config.include.matches(&path, false)
                    && potential(self.config, &path)
                {
                    files.insert(path);
                }
            }
        }
        Ok(())
    }
}

fn potential(config: &Config, path: &Path) -> bool {
    parser::supports(path)
        || config.language(path).is_some()
        || path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        || path.file_name().is_some_and(|name| name == "go.mod")
}

fn decision(rules: &[Rule], path: &Path, directory: bool) -> Option<bool> {
    let mut result = None;
    for rule in rules {
        if let Ok(relative) = path.strip_prefix(&rule.root)
            && let Some(matched) = rule.patterns.decision(relative, directory)
        {
            result = Some(matched);
        }
    }
    result
}

fn project_patterns(
    root: &Path,
    repo: Option<&git::Repository>,
    fingerprint: &mut blake3::Hasher,
) -> Result<Vec<String>> {
    let mut patterns = read_patterns(&root.join(".gitignore"), fingerprint)?;
    if let Some(repo) = repo.filter(|repo| repo.worktree == root) {
        patterns.extend(read_patterns(
            &repo.git_dir.join("info/exclude"),
            fingerprint,
        )?);
    }
    patterns.extend(global_patterns(repo, fingerprint)?);
    Ok(patterns)
}

fn global_patterns(
    repo: Option<&git::Repository>,
    fingerprint: &mut blake3::Hasher,
) -> Result<Vec<String>> {
    let path = match repo {
        Some(repo) => repo.excludes_file.clone(),
        None => git::global_excludes(fingerprint)?,
    };
    path.map(|path| read_patterns(&path, fingerprint))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn untracked_patterns(
    repo: &git::Repository,
    fingerprint: &mut blake3::Hasher,
) -> Result<Vec<String>> {
    let path = if repo.excludes_configured {
        repo.excludes_file.clone()
    } else {
        git::default_excludes()
    };
    path.map(|path| read_patterns(&path, fingerprint))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn read_patterns(path: &Path, fingerprint: &mut blake3::Hasher) -> Result<Vec<String>> {
    let Some(text) = git::text(path)? else {
        return Ok(Vec::new());
    };
    fingerprint.update(path.as_os_str().as_encoded_bytes());
    fingerprint.update(text.as_bytes());
    Ok(text.lines().map(str::to_string).collect())
}

fn open_directory(root: &cap_std::fs::Dir, path: &Path) -> Result<cap_std::fs::Dir> {
    let mut directory = root.try_clone()?;
    for part in path.components() {
        ensure!(
            matches!(part, std::path::Component::Normal(_)),
            "invalid traversal path"
        );
        directory = directory.open_dir_nofollow(part.as_os_str())?;
    }
    Ok(directory)
}

pub(crate) fn hard_excluded(path: &Path) -> bool {
    path.components().any(|part| {
        part.as_os_str() == ".git"
            || part.as_os_str().to_str().is_some_and(|name| {
                name == ".prometeu-search" || name.starts_with(".prometeu-search-")
            })
    })
}

pub(crate) fn scope_metadata(path: &Path) -> bool {
    if path.file_name().is_some_and(|name| name == ".git") {
        return true;
    }
    if !path.components().any(|part| part.as_os_str() == ".git") {
        return false;
    }
    path.file_name().is_some_and(|name| {
        [
            "index",
            "config",
            "config.worktree",
            "commondir",
            "exclude",
            "sparse-checkout",
        ]
        .iter()
        .any(|value| name == *value)
    })
}
