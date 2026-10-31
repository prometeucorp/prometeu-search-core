use crate::{WatchHandle, parser};
use anyhow::{Context, Result, bail, ensure};
use ignore::WalkBuilder;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const EXCLUDED: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    ".venv",
    "venv",
    ".next",
    "coverage",
    ".prometeu-search",
];

/// A connection is bound permanently to one canonical worktree root.
pub struct Engine {
    pub(crate) root: PathBuf,
    pub(crate) database: PathBuf,
    connection: Connection,
}

#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub indexed: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub syntax_errors: usize,
    pub generation: u64,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub root: String,
    pub database: String,
    pub schema_version: u32,
    pub files: usize,
    pub symbols: usize,
    pub references: usize,
    pub generation: u64,
    pub last_indexed: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub snippet: String,
    pub snippet_truncated: bool,
    pub syntax_errors: bool,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub generation: u64,
    pub results: Vec<SearchHit>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Callers,
    Callees,
}

#[derive(Debug, Serialize)]
pub struct SymbolLocation {
    pub name: String,
    pub path: String,
    pub start_line: usize,
}

#[derive(Debug, Serialize)]
pub struct Relation {
    pub caller: SymbolLocation,
    pub target: String,
    pub call_line: usize,
    pub candidate: Option<SymbolLocation>,
    /// These are lexical candidates, not compiler-resolved call edges.
    pub resolution: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RelationsResult {
    pub generation: u64,
    pub relations: Vec<Relation>,
    pub truncated: bool,
}

impl Engine {
    pub fn open(root: &Path, database: Option<&Path>) -> Result<Self> {
        let root = root.canonicalize().context("cannot resolve project root")?;
        ensure!(root.is_dir(), "project root must be a directory");
        let database = match database {
            Some(path) => std::path::absolute(path)?,
            None => {
                let cache = dirs::cache_dir().context("no cache directory; pass --database")?;
                let key = blake3::hash(root.as_os_str().as_encoded_bytes());
                cache
                    .join("prometeu-search")
                    .join(key.to_hex().as_str())
                    .join("index.sqlite")
            }
        };
        let parent = database
            .parent()
            .context("database requires a parent directory")?;
        if !parent.exists() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
        ensure!(!database.is_symlink(), "database must not be a symlink");
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&database) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let database = database.canonicalize()?;
        let connection = Connection::open(&database)?;
        connection.busy_timeout(Duration::from_secs(30))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 0 {
            connection.execute_batch("BEGIN IMMEDIATE")?;
            let initialized = initialize(&connection, &root);
            if let Err(error) = initialized {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(error);
            }
            connection.execute_batch("COMMIT")?;
        } else {
            initialize(&connection, &root)?;
        }
        let journal: String =
            connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        if journal != "wal" {
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&database, fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self {
            root,
            database,
            connection,
        })
    }

    /// Reconcile additions, edits, removals and changed ignore rules in one transaction.
    /// Unchanged content is never parsed again. Failed reads leave the previous snapshot intact.
    pub fn sync(&mut self) -> Result<SyncReport> {
        ensure!(self.root.is_dir(), "project root no longer exists");
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let known: HashMap<String, String> = {
            let mut statement = transaction.prepare("SELECT path, hash FROM files")?;
            statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        let root = self.root.clone();
        let database = self.database.clone();
        let walker = WalkBuilder::new(&root)
            .hidden(false)
            .follow_links(false)
            .require_git(false)
            .add_custom_ignore_filename(".prometeu-searchignore")
            .filter_entry(move |entry| {
                entry.depth() == 0
                    || (!EXCLUDED.iter().any(|name| entry.file_name() == *name)
                        && entry.path() != database)
            })
            .build();
        let mut seen = HashSet::new();
        let mut report = SyncReport {
            indexed: 0,
            removed: 0,
            unchanged: 0,
            syntax_errors: 0,
            generation: 0,
        };
        // ponytail: reconciliation hashes all eligible files; use event-path batches if scanning becomes a measured bottleneck.
        for entry in walker {
            let entry = entry.context("cannot enumerate project files")?;
            if !entry.file_type().is_some_and(|kind| kind.is_file())
                || !parser::supports(entry.path())
            {
                continue;
            }
            if entry.metadata()?.len() > MAX_FILE_BYTES {
                continue;
            }
            let canonical = entry.path().canonicalize()?;
            ensure!(
                canonical.starts_with(&self.root),
                "source escaped project root"
            );
            if entry.path().is_symlink() {
                continue;
            }
            let relative = entry.path().strip_prefix(&self.root)?;
            let Some(path) = relative.to_str() else {
                continue;
            };
            let path = path.replace(std::path::MAIN_SEPARATOR, "/");
            let mut bytes = Vec::new();
            fs::File::open(entry.path())
                .with_context(|| format!("cannot read {path}"))?
                .take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                continue;
            }
            let Ok(source) = std::str::from_utf8(&bytes) else {
                continue;
            };
            let hash = blake3::hash(&bytes).to_hex().to_string();
            seen.insert(path.clone());
            if known.get(&path) == Some(&hash) {
                report.unchanged += 1;
                continue;
            }
            let parsed = parser::parse(entry.path(), source)?;
            report.syntax_errors += usize::from(parsed.has_errors);
            transaction.execute("DELETE FROM files WHERE path = ?1", [&path])?;
            transaction.execute(
                "INSERT INTO files(path, hash, language, syntax_errors) VALUES (?1, ?2, ?3, ?4)",
                params![path, hash, parsed.language, parsed.has_errors],
            )?;
            let mut ids = Vec::with_capacity(parsed.symbols.len());
            for symbol in &parsed.symbols {
                let code = source
                    .get(symbol.start_byte..symbol.end_byte)
                    .context("parser returned invalid source offsets")?;
                transaction.execute("INSERT INTO symbols(path, name, terms, kind, start_line, end_line, code) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![path, symbol.name, words(&symbol.name).join(" "), symbol.kind, symbol.start_line as i64, symbol.end_line as i64, code])?;
                ids.push(transaction.last_insert_rowid());
            }
            for reference in &parsed.references {
                if let Some(caller) = reference.caller.and_then(|index| ids.get(index)) {
                    transaction.execute(
                        "INSERT OR IGNORE INTO refs(caller, target, line) VALUES (?1, ?2, ?3)",
                        params![caller, reference.target, reference.line as i64],
                    )?;
                }
            }
            report.indexed += 1;
        }
        for path in known.keys().filter(|path| !seen.contains(*path)) {
            transaction.execute("DELETE FROM files WHERE path = ?1", [path])?;
            report.removed += 1;
        }
        if report.indexed + report.removed > 0 {
            transaction.execute(
                "UPDATE metadata SET value = CAST(value AS INTEGER) + 1 WHERE key = 'generation'",
                [],
            )?;
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        transaction.execute(
            "INSERT OR REPLACE INTO metadata(key,value) VALUES ('last_indexed', ?1)",
            [now.to_string()],
        )?;
        report.generation = generation(&transaction)?;
        transaction.commit()?;
        Ok(report)
    }

    pub fn status(&self) -> Result<Status> {
        let snapshot = self.connection.unchecked_transaction()?;
        let last_indexed: Option<String> = snapshot
            .query_row(
                "SELECT value FROM metadata WHERE key = 'last_indexed'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(Status {
            root: self.root.to_string_lossy().into_owned(),
            database: self.database.to_string_lossy().into_owned(),
            schema_version: SCHEMA_VERSION,
            files: snapshot.query_row("SELECT count(*) FROM files", [], |r| r.get::<_, i64>(0))?
                as usize,
            symbols: snapshot
                .query_row("SELECT count(*) FROM symbols", [], |r| r.get::<_, i64>(0))?
                as usize,
            references: snapshot
                .query_row("SELECT count(*) FROM refs", [], |r| r.get::<_, i64>(0))?
                as usize,
            generation: generation(&snapshot)?,
            last_indexed: last_indexed.map(|s| s.parse()).transpose()?,
        })
    }

    /// Search an indexed snapshot. Hosts call `sync` before queries when freshness is required.
    pub fn search(&self, query: &str, limit: usize, max_bytes: usize) -> Result<SearchResult> {
        let snapshot = self.connection.unchecked_transaction()?;
        validate_limits(limit, max_bytes)?;
        ensure!(query.len() <= 1024, "query must be at most 1024 bytes");
        let terms: Vec<String> = words(query)
            .into_iter()
            .filter(|word| {
                ![
                    "the", "a", "an", "how", "does", "what", "where", "is", "in", "of", "to", "and",
                ]
                .contains(&word.as_str())
            })
            .take(16)
            .collect();
        ensure!(!terms.is_empty(), "query must contain searchable words");
        let expression = terms
            .iter()
            .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut statement = snapshot.prepare("SELECT s.id, s.name, s.kind, s.path, s.start_line, s.end_line, s.code, f.syntax_errors FROM symbols_fts JOIN symbols s ON s.id = symbols_fts.rowid JOIN files f ON f.path = s.path WHERE symbols_fts MATCH ?1 ORDER BY CASE WHEN lower(s.name) = lower(?2) THEN 0 ELSE 1 END, bm25(symbols_fts, 10.0, 8.0, 2.0, 0.5, 1.0), s.path, s.start_line LIMIT ?3")?;
        let rows = statement.query_map(params![expression, query, (limit + 1) as i64], |row| {
            let code: String = row.get(6)?;
            let snippet = clip(&code, 1600).to_string();
            Ok(SearchHit {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                path: row.get(3)?,
                start_line: row.get::<_, i64>(4)? as usize,
                end_line: row.get::<_, i64>(5)? as usize,
                snippet_truncated: snippet.len() < code.len(),
                snippet,
                syntax_errors: row.get(7)?,
            })
        })?;
        let mut result = SearchResult {
            generation: generation(&snapshot)?,
            results: Vec::new(),
            truncated: false,
        };
        'hits: for row in rows {
            if result.results.len() == limit {
                result.truncated = true;
                break;
            }
            result.results.push(row?);
            loop {
                let bytes = serde_json::to_vec(&result)?.len();
                if bytes <= max_bytes {
                    break;
                }
                let hit = result.results.last_mut().unwrap();
                if hit.snippet.is_empty() {
                    result.results.pop();
                    result.truncated = true;
                    break 'hits;
                }
                let room = hit.snippet.len().saturating_sub(bytes - max_bytes);
                hit.snippet = clip(&hit.snippet, room).to_string();
                hit.snippet_truncated = true;
            }
        }
        Ok(result)
    }

    /// Resolve by name at query time so newly indexed targets can satisfy old references.
    pub fn related(
        &self,
        symbol: &str,
        direction: Direction,
        limit: usize,
        max_bytes: usize,
    ) -> Result<RelationsResult> {
        let snapshot = self.connection.unchecked_transaction()?;
        validate_limits(limit, max_bytes)?;
        ensure!(
            !symbol.is_empty() && symbol.len() <= 1024,
            "symbol must contain 1 to 1024 bytes"
        );
        let predicate = match direction {
            Direction::Callers => "r.target = ?1",
            Direction::Callees => "s.name = ?1",
        };
        let sql = format!(
            "SELECT s.name, s.path, s.start_line, r.target, r.line, t.name, t.path, t.start_line FROM refs r JOIN symbols s ON s.id = r.caller LEFT JOIN symbols t ON t.name = r.target WHERE {predicate} ORDER BY s.path, r.line, t.path, t.start_line LIMIT ?2"
        );
        let mut statement = snapshot.prepare(&sql)?;
        let rows = statement.query_map(params![symbol, (limit + 1) as i64], |row| {
            let name: Option<String> = row.get(5)?;
            let candidate = name
                .map(|name| {
                    Ok::<_, rusqlite::Error>(SymbolLocation {
                        name,
                        path: row.get(6)?,
                        start_line: row.get::<_, i64>(7)? as usize,
                    })
                })
                .transpose()?;
            Ok(Relation {
                caller: SymbolLocation {
                    name: row.get(0)?,
                    path: row.get(1)?,
                    start_line: row.get::<_, i64>(2)? as usize,
                },
                target: row.get(3)?,
                call_line: row.get::<_, i64>(4)? as usize,
                resolution: if candidate.is_some() {
                    "name_match"
                } else {
                    "unresolved"
                },
                candidate,
            })
        })?;
        let mut result = RelationsResult {
            generation: generation(&snapshot)?,
            relations: Vec::new(),
            truncated: false,
        };
        for row in rows {
            if result.relations.len() == limit {
                result.truncated = true;
                break;
            }
            result.relations.push(row?);
            if serde_json::to_vec(&result)?.len() > max_bytes {
                result.relations.pop();
                result.truncated = true;
                break;
            }
        }
        Ok(result)
    }

    pub fn watch(&self) -> Result<WatchHandle> {
        WatchHandle::start(&self.root, &self.database)
    }
}

fn initialize(connection: &Connection, root: &Path) -> Result<()> {
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    ensure!(
        version == 0 || version == SCHEMA_VERSION,
        "unsupported database schema {version}; choose a new database or remove the derived index"
    );
    if version == 0 {
        let existing: i64 = connection.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            existing == 0,
            "refusing to initialize an unrelated SQLite database"
        );
        connection.execute_batch("CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE files(path TEXT PRIMARY KEY, hash TEXT NOT NULL, language TEXT NOT NULL, syntax_errors INTEGER NOT NULL);
            CREATE TABLE symbols(id INTEGER PRIMARY KEY, path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE, name TEXT NOT NULL, terms TEXT NOT NULL, kind TEXT NOT NULL, start_line INTEGER NOT NULL, end_line INTEGER NOT NULL, code TEXT NOT NULL);
            CREATE INDEX symbols_name ON symbols(name);
            CREATE INDEX symbols_path ON symbols(path);
            CREATE TABLE refs(caller INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE, target TEXT NOT NULL, line INTEGER NOT NULL, UNIQUE(caller,target,line));
            CREATE INDEX refs_target ON refs(target);
            CREATE VIRTUAL TABLE symbols_fts USING fts5(name,terms,path,kind,code,content='symbols',content_rowid='id',tokenize='unicode61');
            CREATE TRIGGER symbols_insert AFTER INSERT ON symbols BEGIN INSERT INTO symbols_fts(rowid,name,terms,path,kind,code) VALUES(new.id,new.name,new.terms,new.path,new.kind,new.code); END;
            CREATE TRIGGER symbols_delete AFTER DELETE ON symbols BEGIN INSERT INTO symbols_fts(symbols_fts,rowid,name,terms,path,kind,code) VALUES('delete',old.id,old.name,old.terms,old.path,old.kind,old.code); END;
            INSERT INTO metadata VALUES('generation','0');")?;
        connection.execute(
            "INSERT INTO metadata VALUES('root',?1)",
            [root.to_str().context("project root must be valid UTF-8")?],
        )?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    let stored: String =
        connection.query_row("SELECT value FROM metadata WHERE key='root'", [], |r| {
            r.get(0)
        })?;
    if Path::new(&stored) != root {
        bail!("database belongs to a different project root: {stored}");
    }
    Ok(())
}

fn generation(connection: &Connection) -> Result<u64> {
    let value: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='generation'",
        [],
        |r| r.get(0),
    )?;
    Ok(value.parse()?)
}

fn validate_limits(limit: usize, max_bytes: usize) -> Result<()> {
    ensure!((1..=50).contains(&limit), "limit must be between 1 and 50");
    ensure!(
        (1024..=65536).contains(&max_bytes),
        "max_bytes must be between 1024 and 65536"
    );
    Ok(())
}

fn clip(value: &str, max: usize) -> &str {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn words(text: &str) -> Vec<String> {
    let mut split = String::new();
    let mut lower = false;
    for ch in text.chars() {
        if lower && ch.is_uppercase() {
            split.push(' ');
        }
        lower = ch.is_lowercase();
        split.push(ch);
    }
    split
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}
