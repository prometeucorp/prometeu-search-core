//! Native, bounded Git index reads. No Git process or writable repository handle.
use crate::source;
use anyhow::{Context, Result, bail, ensure};
use cap_fs_ext::DirExt;
use gix_index::{State, entry::Mode, hash::Kind};
use gix_object::FindExt;
use gix_odb::HeaderExt;
use std::collections::BTreeSet;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

#[path = "git_config.rs"]
mod config;

const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
pub(super) const MAX_RULE_BYTES: usize = 1024 * 1024;

pub(super) struct Repository {
    pub worktree: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
    pub tracked: BTreeSet<PathBuf>,
    pub directories: BTreeSet<PathBuf>,
    pub sparse_directories: BTreeSet<PathBuf>,
    pub gitlinks: BTreeSet<PathBuf>,
    pub ignore_case: bool,
    pub excludes_file: Option<PathBuf>,
    pub excludes_configured: bool,
    pub fingerprint: String,
}

impl Repository {
    pub fn discover(root: &Path) -> Result<Option<Self>> {
        for candidate in root.ancestors() {
            if let Some(git_dir) = git_directory(candidate)? {
                return Self::load(candidate.to_path_buf(), git_dir).map(Some);
            }
        }
        Ok(None)
    }

    pub fn at(root: &Path) -> Result<Option<Self>> {
        git_directory(root)?
            .map(|git_dir| Self::load(root.to_path_buf(), git_dir))
            .transpose()
    }

    fn load(worktree: PathBuf, git_dir: PathBuf) -> Result<Self> {
        let common_dir = match text(&git_dir.join("commondir"))? {
            Some(common) => absolute(&git_dir, common.trim())?,
            None => git_dir.clone(),
        };
        let mut fingerprint = blake3::Hasher::new();
        let settings = config::settings(
            Some(&worktree),
            Some(&git_dir),
            Some(&common_dir),
            &mut fingerprint,
        )?;
        let hash_kind = if settings
            .get("extensions.objectformat")
            .is_some_and(|value| value.eq_ignore_ascii_case("sha256"))
        {
            Kind::Sha256
        } else {
            Kind::Sha1
        };
        let mut index = read_index(&git_dir.join("index"), hash_kind, None, 0, &mut fingerprint)?
            .unwrap_or_default();
        expand_sparse(&worktree, &common_dir, hash_kind, &mut index)?;
        let mut repo = Self {
            worktree,
            git_dir,
            common_dir,
            tracked: BTreeSet::new(),
            directories: BTreeSet::new(),
            sparse_directories: BTreeSet::new(),
            gitlinks: BTreeSet::new(),
            ignore_case: config::truth(settings.get("core.ignorecase")),
            excludes_file: settings
                .get("core.excludesfile")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
            excludes_configured: settings.contains_key("core.excludesfile"),
            fingerprint: String::new(),
        };
        for entry in index {
            let Ok(path) = String::from_utf8(entry.path) else {
                continue;
            };
            let path = PathBuf::from(path);
            if entry.mode == Mode::COMMIT {
                repo.gitlinks.insert(path.clone());
            } else if entry.mode == Mode::DIR {
                repo.sparse_directories.insert(path.clone());
            } else {
                repo.tracked.insert(path.clone());
            }
            for parent in path.ancestors().skip(1) {
                repo.directories.insert(parent.to_path_buf());
            }
        }
        repo.fingerprint = fingerprint.finalize().to_hex().to_string();
        Ok(repo)
    }

    pub fn relative<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        path.strip_prefix(&self.worktree).ok()
    }
    pub fn tracked(&self, path: &Path) -> bool {
        self.relative(path)
            .is_some_and(|path| self.tracked.contains(path))
    }
    pub fn has_tracked_descendant(&self, path: &Path) -> bool {
        self.relative(path).is_some_and(|path| {
            self.directories.contains(path) || self.sparse_directories.contains(path)
        })
    }
}

pub(super) fn worktree_pointer(root: &Path) -> Result<bool> {
    let directory = source::open_root(root)?;
    match directory.symlink_metadata(".git") {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("cannot inspect embedded Git pointer"),
    }
    let Some(pointer) = text(&root.join(".git"))? else {
        return Ok(false);
    };
    let Some(pointer) = pointer
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(str::trim)
    else {
        return Ok(false);
    };
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?:^|[\\/])\.git[\\/](?:modules[\\/][^\\/]+[\\/])?worktrees[\\/]")
            .unwrap()
    });
    Ok(PATTERN.is_match(pointer))
}

fn git_directory(root: &Path) -> Result<Option<PathBuf>> {
    let directory = source::open_root(root)?;
    let metadata = match directory.symlink_metadata(".git") {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot inspect Git metadata"),
    };
    ensure!(!metadata.is_symlink(), "Git metadata must not be a symlink");
    if metadata.is_dir() {
        return Ok(Some(root.join(".git")));
    }
    if !metadata.is_file() {
        bail!("Git metadata must be a regular file or directory");
    }
    let Some(pointer) = text(&root.join(".git"))? else {
        return Ok(None);
    };
    let Some(path) = pointer
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(str::trim)
    else {
        return Ok(None);
    };
    Ok(Some(absolute(root, path)?))
}

pub(super) fn absolute(base: &Path, value: &str) -> Result<PathBuf> {
    let path = base.join(value);
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                ensure!(
                    result.pop(),
                    "Git metadata path escapes its filesystem root"
                );
            }
            Component::CurDir => {}
            part => result.push(part.as_os_str()),
        }
    }
    ensure!(result.is_absolute(), "Git metadata path must be absolute");
    Ok(result)
}

pub(super) fn text(path: &Path) -> Result<Option<String>> {
    let Some(bytes) = read(path, MAX_RULE_BYTES)? else {
        return Ok(None);
    };
    // Invalid rule/config text is not a meaningful pattern source.
    Ok(String::from_utf8(bytes)
        .ok()
        .filter(|text| !text.contains('\0')))
}

fn read(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    let parent = path
        .parent()
        .context("Git metadata has no parent directory")?;
    // Git metadata pointers may use platform aliases such as /var -> /private/var.
    // This canonicalizes metadata ancestors only; source reads remain pinned to the project handle.
    let parent = match std::fs::canonicalize(parent) {
        Ok(parent) => parent,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot locate Git metadata directory"),
    };
    let root = match source::open_root(&parent) {
        Ok(root) => root,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot open Git metadata directory"),
    };
    let name = path.file_name().context("Git metadata has no filename")?;
    let mut file = match source::open(&root, Path::new(name)) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot read Git metadata"),
    };
    ensure!(
        file.metadata()?.is_file(),
        "Git metadata must be a regular file"
    );
    let mut bytes = Vec::new();
    (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "Git metadata exceeds bounded read limit"
    );
    Ok(Some(bytes))
}

struct IndexEntry {
    path: Vec<u8>,
    mode: Mode,
    stage: u32,
    id: gix_index::hash::ObjectId,
}

fn read_index(
    path: &Path,
    kind: Kind,
    expected: Option<&gix_index::hash::ObjectId>,
    depth: usize,
    fingerprint: &mut blake3::Hasher,
) -> Result<Option<Vec<IndexEntry>>> {
    ensure!(depth < 4, "Git shared index nesting exceeds limit");
    let Some(bytes) = read(path, MAX_INDEX_BYTES)? else {
        return Ok(None);
    };
    fingerprint.update(&bytes);
    let size = kind.len_in_bytes();
    ensure!(bytes.len() >= 12 + size, "truncated Git index");
    let checksum = &bytes[bytes.len() - size..];
    ensure!(
        expected.is_none_or(|expected| expected.as_slice() == checksum),
        "Git shared index identity mismatch"
    );
    if checksum.iter().any(|&byte| byte != 0) {
        let mut hasher = gix_index::hash::hasher(kind);
        hasher.update(&bytes[..bytes.len() - size]);
        let hash = hasher
            .try_finalize()
            .map_err(|error| anyhow::anyhow!("Git index checksum failure: {error}"))?;
        ensure!(hash.as_slice() == checksum, "Git index checksum mismatch");
    }
    let (state, _) = State::from_bytes(
        &bytes,
        std::time::UNIX_EPOCH.into(),
        kind,
        gix_index::decode::Options {
            thread_limit: Some(1),
            alloc_limit_bytes: Some(MAX_INDEX_BYTES),
            ..Default::default()
        },
    )
    .map_err(|error| anyhow::anyhow!("invalid Git index: {error}"))?;
    let mut entries = Vec::new();
    for entry in state.entries() {
        let raw: &[u8] = entry.path(&state).as_ref();
        ensure!(
            raw.is_empty()
                || (!raw.starts_with(b"/")
                    && !raw.contains(&b'\\')
                    && raw
                        .split(|&byte| byte == b'/')
                        .all(|part| part != b"." && part != b"..")),
            "invalid Git index source path"
        );
        entries.push(IndexEntry {
            path: raw.to_vec(),
            mode: entry.mode,
            stage: entry.stage_raw(),
            id: entry.id,
        });
    }
    if let Some(link) = state.link() {
        let shared_path = path
            .parent()
            .context("Git index has no parent")?
            .join(format!("sharedindex.{}", link.shared_index_checksum));
        let mut shared = read_index(
            &shared_path,
            kind,
            Some(&link.shared_index_checksum),
            depth + 1,
            fingerprint,
        )?
        .context("missing Git shared index")?;
        if let Some(bitmaps) = &link.bitmaps {
            let mut replacements = Vec::new();
            ensure!(
                bitmaps
                    .replace
                    .for_each_set_bit(|index| {
                        replacements.push(index);
                        Some(())
                    })
                    .is_some(),
                "invalid Git index replacement bitmap"
            );
            ensure!(
                replacements.len() <= entries.len(),
                "Git replacement bitmap exceeds index entries"
            );
            for (index, replacement) in replacements.iter().copied().zip(&entries) {
                ensure!(
                    replacement.path.is_empty(),
                    "Git replacement entry must inherit its path"
                );
                let entry = shared
                    .get_mut(index)
                    .context("Git replacement index outside shared entries")?;
                entry.mode = replacement.mode;
                entry.stage = replacement.stage;
                entry.id = replacement.id;
            }
            let mut deleted = vec![false; shared.len()];
            ensure!(
                bitmaps
                    .delete
                    .for_each_set_bit(|index| deleted.get_mut(index).map(|flag| *flag = true))
                    .is_some(),
                "invalid Git index deletion bitmap"
            );
            let mut result: Vec<_> = shared
                .into_iter()
                .enumerate()
                .filter(|(index, _)| !deleted[*index])
                .map(|(_, entry)| entry)
                .collect();
            result.extend(entries.into_iter().skip(replacements.len()));
            result.sort_by(|a, b| a.path.cmp(&b.path).then(a.stage.cmp(&b.stage)));
            return Ok(Some(result));
        }
        return Ok(Some(shared));
    }
    ensure!(
        entries.iter().all(|entry| !entry.path.is_empty()),
        "Git index entry has no path"
    );
    Ok(Some(entries))
}

/// Sparse indexes store trees instead of individual files. Expand only trees whose
/// directories are materialized; absent sparse checkout content never needs a read.
fn expand_sparse(
    worktree: &Path,
    common_dir: &Path,
    kind: Kind,
    index: &mut Vec<IndexEntry>,
) -> Result<()> {
    let mut queue: Vec<_> = index
        .iter()
        .filter(|entry| entry.mode == Mode::DIR)
        .map(|entry| (entry.path.clone(), entry.id, 0usize))
        .collect();
    if queue.is_empty() {
        return Ok(());
    }
    let source_root = source::open_root(worktree)?;
    let mut store = None;
    let mut buffer = Vec::new();
    let mut bytes = 0usize;
    let mut path_bytes: usize = index.iter().map(|entry| entry.path.len()).sum();
    while let Some((prefix, id, depth)) = queue.pop() {
        let Ok(relative) = std::str::from_utf8(&prefix) else {
            continue;
        };
        if !materialized_directory(&source_root, Path::new(relative))? {
            continue;
        }
        ensure!(depth < 128, "sparse Git tree nesting exceeds limit");
        if store.is_none() {
            source::open_root(&common_dir.join("objects").canonicalize()?)?;
            store = Some(gix_odb::at_opts(
                common_dir.join("objects"),
                kind,
                None,
                gix_odb::store::init::Options {
                    alloc_limit_bytes: Some(MAX_INDEX_BYTES),
                    ..Default::default()
                },
            )?);
        }
        let store = store.as_ref().unwrap();
        let header = store
            .header(id)
            .map_err(|error| anyhow::anyhow!("cannot read sparse Git tree header: {error}"))?;
        ensure!(
            header.kind() == gix_object::Kind::Tree && header.size() <= MAX_INDEX_BYTES as u64,
            "invalid or oversized sparse Git tree"
        );
        bytes = bytes.saturating_add(header.size() as usize);
        ensure!(
            bytes <= 128 * 1024 * 1024,
            "sparse Git tree snapshot exceeds limit"
        );
        let object = store
            .find(&id, &mut buffer)
            .map_err(|error| anyhow::anyhow!("cannot read sparse Git tree: {error}"))?;
        ensure!(
            object.kind == gix_object::Kind::Tree,
            "sparse Git entry is not a tree"
        );
        for entry in gix_object::TreeRefIter::from_bytes(object.data, kind) {
            let entry =
                entry.map_err(|error| anyhow::anyhow!("invalid sparse Git tree: {error}"))?;
            let name: &[u8] = entry.filename.as_ref();
            ensure!(
                !name.is_empty()
                    && name != b"."
                    && name != b".."
                    && !name.contains(&b'/')
                    && !name.contains(&0),
                "invalid sparse Git path"
            );
            let mut path = prefix.clone();
            if !path.ends_with(b"/") {
                path.push(b'/');
            }
            path.extend_from_slice(name);
            path_bytes = path_bytes.saturating_add(path.len());
            ensure!(
                path_bytes <= MAX_INDEX_BYTES,
                "sparse Git paths exceed snapshot limit"
            );
            if entry.mode.is_tree() {
                queue.push((path, entry.oid.to_owned(), depth + 1));
            } else {
                ensure!(
                    index.len() < 1_000_000,
                    "sparse Git entry count exceeds limit"
                );
                index.push(IndexEntry {
                    path,
                    mode: if entry.mode.is_commit() {
                        Mode::COMMIT
                    } else {
                        Mode::FILE
                    },
                    stage: 0,
                    id: entry.oid.to_owned(),
                });
            }
        }
    }
    Ok(())
}
fn materialized_directory(root: &cap_std::fs::Dir, path: &Path) -> Result<bool> {
    let mut directory = root.try_clone()?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            bail!("invalid sparse Git directory");
        };
        match directory.symlink_metadata(name) {
            Ok(metadata) if !metadata.is_dir() || metadata.is_symlink() => return Ok(false),
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error).context("cannot inspect sparse Git directory"),
        }
        directory = directory.open_dir_nofollow(name)?;
    }
    Ok(true)
}

pub(super) fn global_excludes(fingerprint: &mut blake3::Hasher) -> Result<Option<PathBuf>> {
    Ok(config::settings(None, None, None, fingerprint)?
        .get("core.excludesfile")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from))
}
pub(super) fn default_excludes() -> Option<PathBuf> {
    config::default_excludes()
}
