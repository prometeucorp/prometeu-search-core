use crate::index::{MAX_FILE_BYTES, generation, store_file};
use crate::{Engine, SyncReport, parser, project_config, source};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::Metadata;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
pub(crate) const IGNORE_FILES: &[&str] = &[".gitignore", ".ignore", ".prometeu-searchignore"];
const THEME_MARKERS: &[&str] = &["layout/theme.liquid", "config/settings_schema.json"];

impl Engine {
    /// Hash every eligible file and atomically reconcile the complete worktree.
    pub fn sync(&mut self) -> Result<SyncReport> {
        let report = self.reconcile(None, false)?;
        self.last_full_sync = Some(Instant::now());
        Ok(report)
    }

    /// Reconcile metadata before queries, with a full hash check at least every minute.
    /// The first refresh on each connection always performs a full reconciliation.
    pub fn refresh(&mut self) -> Result<SyncReport> {
        if self
            .last_full_sync
            .is_none_or(|last| last.elapsed() >= RECOVERY_INTERVAL)
        {
            self.sync()
        } else {
            self.reconcile(None, true)
        }
    }

    /// Reconcile a batch of changed paths without visiting unrelated subtrees.
    /// Directory and ignore-rule changes require a complete recovery scan.
    pub fn sync_paths(&mut self, paths: &[PathBuf]) -> Result<SyncReport> {
        let mut selected = HashSet::new();
        let known_directories: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key='project_directories'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let known_directories: HashSet<String> = known_directories
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();
        let known_manifests: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key='project_manifests'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        ensure!(
            known_manifests
                .as_ref()
                .is_none_or(|json| json.len() <= 64 * 1024 * 1024),
            "cached manifest inventory exceeds limit"
        );
        let known_manifests: std::collections::HashMap<String, serde::de::IgnoredAny> =
            known_manifests
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?
                .unwrap_or_default();
        for path in paths {
            let relative = if path.is_absolute() {
                path.strip_prefix(&self.root)
                    .context("changed path must be inside the project root")?
            } else {
                path.as_path()
            };
            ensure!(
                relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
                "changed path must not contain parent or root components"
            );
            if relative.as_os_str().is_empty()
                || relative.file_name().is_some_and(|name| {
                    IGNORE_FILES.contains(&name.to_str().unwrap_or(""))
                        || name == project_config::FILENAME
                        || name == "go.mod"
                })
                || crate::graph::is_project_manifest(relative)
                || crate::traversal::scope_metadata(relative)
                || self.root.join(relative).is_dir()
                || THEME_MARKERS
                    .iter()
                    .any(|marker| relative.ends_with(marker))
            {
                return self.sync();
            }
            let Some(name) = relative.to_str() else {
                continue;
            };
            let name = name.replace(std::path::MAIN_SEPARATOR, "/");
            // Empty workspace targets can affect package resolution without containing source.
            if known_directories.contains(&name) || known_manifests.contains_key(&name) {
                return self.sync();
            }
            // A deleted directory can have a source-like suffix, so consult indexed paths.
            let descendant: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE path >= ?1 AND path < ?2)",
                params![format!("{name}/"), format!("{name}0")],
                |row| row.get(0),
            )?;
            if descendant {
                return self.sync();
            }
            selected.insert(name);
        }
        if selected.len() > 4096 {
            return self.sync();
        }
        self.reconcile(Some(selected), false)
    }

    fn reconcile(
        &mut self,
        selected: Option<HashSet<String>>,
        metadata_only: bool,
    ) -> Result<SyncReport> {
        let source_root = source::open_root(&self.root).context("cannot open project root")?;
        let config = project_config::Config::load(&source_root)?;
        let traversal = crate::traversal::Traversal::prepare(
            &self.root,
            &source_root,
            &self.database,
            &config,
        )?;
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let active_frameworks = crate::frameworks::active(&transaction)?;
        let previous_config: Option<String> = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key='project_config_content'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let config_changed = previous_config
            .as_deref()
            .is_some_and(|previous| previous != config.content_fingerprint)
            || (previous_config.is_none() && config.content_fingerprint != "absent");
        let previous_scope: Option<String> = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key='project_scope_fingerprint'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let scope_changed = previous_scope
            .as_deref()
            .is_some_and(|previous| previous != traversal.fingerprint);
        let selected = if config_changed || scope_changed {
            None
        } else {
            selected
        };
        let metadata_only = metadata_only && !config_changed && !scope_changed;
        let mut known: HashMap<String, (String, String)> = HashMap::new();
        if let Some(paths) = &selected {
            let mut statement = transaction.prepare("SELECT f.hash, coalesce(s.signature, '') FROM files f LEFT JOIN file_state s ON s.path=f.path WHERE f.path=?1")?;
            for path in paths {
                if let Some(state) = statement
                    .query_row([path], |row| Ok((row.get(0)?, row.get(1)?)))
                    .optional()?
                {
                    known.insert(path.clone(), state);
                }
            }
        } else {
            let mut statement = transaction.prepare("SELECT f.path, f.hash, coalesce(s.signature, '') FROM files f LEFT JOIN file_state s ON s.path=f.path")?;
            known = statement
                .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?
                .collect::<rusqlite::Result<_>>()?;
        }
        let mut seen = HashSet::new();
        let mut go_unread = if let Some(selected) = &selected {
            crate::auxiliary_sources::load(&transaction)?
                .into_iter()
                .filter(|(path, _)| path.ends_with(".go") && !selected.contains(path))
                .collect::<BTreeMap<_, _>>()
        } else {
            BTreeMap::new()
        };
        let mut go_modules = BTreeMap::new();
        let mut module_bytes = 0;
        let mut parsed_paths = HashSet::new();
        let mut themes = HashMap::new();
        let mut report = SyncReport {
            indexed: 0,
            removed: 0,
            unchanged: 0,
            hashed: 0,
            syntax_errors: 0,
            generation: 0,
        };
        for relative in traversal.files(selected.as_ref())? {
            let relative = relative.as_path();
            if relative.file_name().is_some_and(|name| name == "go.mod") && selected.is_none() {
                let mut module = source::open(&source_root, relative)
                    .with_context(|| format!("cannot read {}", relative.display()))?;
                ensure!(
                    module.metadata()?.is_file(),
                    "project manifest must be a regular file"
                );
                let mut bytes = Vec::new();
                (&mut module)
                    .take(MAX_FILE_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                module_bytes += bytes.len();
                ensure!(
                    bytes.len() as u64 <= MAX_FILE_BYTES && module_bytes <= 8 * 1024 * 1024,
                    "Go module configuration exceeds snapshot limit"
                );
                if let (Some(path), Ok(content)) = (relative.to_str(), String::from_utf8(bytes)) {
                    go_modules.insert(path.replace(std::path::MAIN_SEPARATOR, "/"), content);
                }
            }
            let liquid_json = theme_json(&source_root, relative, &mut themes)?;
            let override_language = config.language(relative);
            if !parser::supports(relative) && !liquid_json && override_language.is_none() {
                continue;
            }
            let Some(path) = relative.to_str() else {
                continue;
            };
            let path = path.replace(std::path::MAIN_SEPARATOR, "/");
            let mut file = source::open(&source_root, relative)
                .with_context(|| format!("cannot read {path}"))?;
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            if metadata.len() > MAX_FILE_BYTES {
                if path.ends_with(".go") && !path.ends_with("_test.go") {
                    go_unread.insert(path, crate::auxiliary_sources::go_declarations(&mut file)?);
                }
                continue;
            }
            let stamp = signature(&metadata);
            if metadata_only
                && !stamp.is_empty()
                && known.get(&path).is_some_and(|(_, old)| *old == stamp)
            {
                seen.insert(path);
                report.unchanged += 1;
                continue;
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                continue;
            }
            let Ok(content) = std::str::from_utf8(&bytes) else {
                continue;
            };
            let hash = blake3::hash(&bytes).to_hex().to_string();
            report.hashed += 1;
            seen.insert(path.clone());
            if !config_changed && known.get(&path).is_some_and(|(old, _)| *old == hash) {
                report.unchanged += 1;
            } else {
                let language = if liquid_json {
                    "liquid"
                } else {
                    parser::detect_language(relative, content, override_language)?
                };
                let parsed =
                    parser::parse_with_frameworks(relative, content, language, &active_frameworks)
                        .with_context(|| format!("cannot extract {path}"))?;
                report.syntax_errors += usize::from(parsed.has_errors);
                store_file(&transaction, &path, content, &hash, &parsed)?;
                transaction.execute(
                    "UPDATE files SET deprioritized=?2 WHERE path=?1",
                    params![path, config.deprioritize.matches(relative, false)],
                )?;
                report.indexed += 1;
                parsed_paths.insert(path.clone());
            }
            // A write during our read must be checked again on the next refresh.
            let stable_stamp = if signature(&file.metadata()?) == stamp {
                stamp
            } else {
                String::new()
            };
            transaction.execute(
                "INSERT OR REPLACE INTO file_state(path,signature) VALUES(?1,?2)",
                params![path, stable_stamp],
            )?;
        }
        for path in known.keys().filter(|path| !seen.contains(*path)) {
            transaction.execute("DELETE FROM files WHERE path=?1", [path])?;
            report.removed += 1;
        }
        let mut module_changed = false;
        if selected.is_none() {
            let modules = serde_json::to_string(&go_modules)?;
            let previous: Option<String> = transaction
                .query_row(
                    "SELECT value FROM metadata WHERE key='resolution_go_modules'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            module_changed = previous
                .as_deref()
                .map_or(!go_modules.is_empty(), |previous| previous != modules);
            transaction.execute(
                "INSERT OR REPLACE INTO metadata(key,value) VALUES('resolution_go_modules',?1)",
                [modules],
            )?;
            let (manifests, directories) = crate::graph::workspace_snapshot(
                &source_root,
                &self.root,
                seen.iter().map(String::as_str),
            )?;
            let directories_json = serde_json::to_string(&directories)?;
            let previous_directories: Option<String> = transaction
                .query_row(
                    "SELECT value FROM metadata WHERE key='project_directories'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            module_changed |= previous_directories
                .as_deref()
                .is_some_and(|previous| previous != directories_json);
            transaction.execute(
                "INSERT OR REPLACE INTO metadata(key,value) VALUES('project_directories',?1)",
                [directories_json],
            )?;
            let manifests_json = serde_json::to_string(&manifests)?;
            let previous_manifests: Option<String> = transaction
                .query_row(
                    "SELECT value FROM metadata WHERE key='project_manifests'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            module_changed |= previous_manifests
                .as_deref()
                .map_or(!manifests.is_empty(), |previous| previous != manifests_json);
            transaction.execute(
                "INSERT OR REPLACE INTO metadata(key,value) VALUES('project_manifests',?1)",
                [manifests_json],
            )?;
        }
        for (key, value) in [
            ("project_scope_fingerprint", traversal.fingerprint.clone()),
            ("project_config_content", config.content_fingerprint),
            ("project_config_fingerprint", config.fingerprint),
        ] {
            transaction.execute(
                "INSERT OR REPLACE INTO metadata(key,value) VALUES(?1,?2)",
                params![key, value],
            )?;
        }
        module_changed |=
            crate::auxiliary_sources::reconcile(&transaction, &source_root, go_unread)?;
        let frameworks_changed =
            crate::frameworks::reconcile(&transaction, &mut report, &parsed_paths)?;
        if report.indexed + report.removed > 0
            || config_changed
            || scope_changed
            || module_changed
            || frameworks_changed
        {
            transaction.execute(
                "UPDATE metadata SET value=CAST(value AS INTEGER)+1 WHERE key='generation'",
                [],
            )?;
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        transaction.execute(
            "INSERT OR REPLACE INTO metadata(key,value) VALUES('last_indexed',?1)",
            [now.to_string()],
        )?;
        report.generation = generation(&transaction)?;
        transaction.commit()?;
        Ok(report)
    }
}

fn theme_json(
    root: &cap_std::fs::Dir,
    path: &Path,
    cache: &mut HashMap<PathBuf, bool>,
) -> Result<bool> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return Ok(false);
    }
    for folder in path.ancestors().skip(1) {
        if !folder.file_name().is_some_and(|name| {
            name.eq_ignore_ascii_case("templates") || name.eq_ignore_ascii_case("sections")
        }) {
            continue;
        }
        let directory = folder.parent().unwrap_or_else(|| Path::new(""));
        let is_theme = if let Some(&is_theme) = cache.get(directory) {
            is_theme
        } else {
            let mut is_theme = false;
            for marker in THEME_MARKERS {
                let marker = directory.join(marker);
                match source::open(root, &marker).and_then(|file| file.metadata()) {
                    Ok(metadata) if metadata.is_file() => {
                        is_theme = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("cannot inspect theme marker {}", marker.display())
                        });
                    }
                }
            }
            cache.insert(directory.into(), is_theme);
            is_theme
        };
        if is_theme {
            return Ok(true);
        }
    }
    Ok(false)
}

fn signature(metadata: &Metadata) -> String {
    let Ok(modified) = metadata.modified() else {
        return String::new();
    };
    let modified = match modified.duration_since(UNIX_EPOCH) {
        Ok(value) => value.as_nanos().to_string(),
        Err(error) => format!("-{}", error.duration().as_nanos()),
    };
    let stamp = format!("{}:{modified}", metadata.len());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{stamp}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec()
        )
    }
    #[cfg(not(unix))]
    stamp
}
