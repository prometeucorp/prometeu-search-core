use crate::index::{EXCLUDED, MAX_FILE_BYTES, generation, store_file};
use crate::{Engine, SyncReport, parser, source};
use anyhow::{Context, Result, ensure};
use ignore::WalkBuilder;
use rusqlite::{OptionalExtension, params};
use std::collections::{HashMap, HashSet};
use std::fs::Metadata;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
pub(crate) const IGNORE_FILES: &[&str] = &[".gitignore", ".ignore", ".prometeu-searchignore"];

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
                || relative
                    .file_name()
                    .is_some_and(|name| IGNORE_FILES.contains(&name.to_str().unwrap_or("")))
                || self.root.join(relative).is_dir()
            {
                return self.sync();
            }
            let Some(name) = relative.to_str() else {
                continue;
            };
            let name = name.replace(std::path::MAIN_SEPARATOR, "/");
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
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
        let root = self.root.clone();
        let database = self.database.clone();
        let permitted = selected.as_ref().map(|paths| {
            paths
                .iter()
                .flat_map(|path| {
                    Path::new(path)
                        .ancestors()
                        .map(|parent| root.join(parent))
                        .collect::<Vec<_>>()
                })
                .collect::<HashSet<_>>()
        });
        // Pruning uses the same walker as recovery, preserving nested ignore precedence.
        // Only ancestor directories are enumerated; unrelated subtrees are never visited.
        let walker = WalkBuilder::new(&root)
            .hidden(false)
            .follow_links(false)
            .require_git(false)
            .add_custom_ignore_filename(".prometeu-searchignore")
            .filter_entry(move |entry| {
                entry.depth() == 0
                    || (!EXCLUDED.iter().any(|name| entry.file_name() == *name)
                        && entry.path() != database
                        && permitted
                            .as_ref()
                            .is_none_or(|paths| paths.contains(entry.path())))
            })
            .build();
        let mut seen = HashSet::new();
        let mut report = SyncReport {
            indexed: 0,
            removed: 0,
            unchanged: 0,
            hashed: 0,
            syntax_errors: 0,
            generation: 0,
        };
        for entry in walker {
            let entry = entry.context("cannot enumerate project files")?;
            if !entry.file_type().is_some_and(|kind| kind.is_file())
                || !parser::supports(entry.path())
            {
                continue;
            }
            let relative = entry.path().strip_prefix(&self.root)?;
            let Some(path) = relative.to_str() else {
                continue;
            };
            let path = path.replace(std::path::MAIN_SEPARATOR, "/");
            let mut file = source::open(&source_root, relative)
                .with_context(|| format!("cannot read {path}"))?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
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
            if known.get(&path).is_some_and(|(old, _)| *old == hash) {
                report.unchanged += 1;
            } else {
                let parsed = parser::parse(entry.path(), content)?;
                report.syntax_errors += usize::from(parsed.has_errors);
                store_file(&transaction, &path, content, &hash, &parsed)?;
                report.indexed += 1;
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
        if report.indexed + report.removed > 0 {
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
