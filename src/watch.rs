use crate::Engine;
use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, event::ModifyKind};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// One watcher owns each database. Other clients take over when that owner exits.
pub struct WatchHandle {
    stop: Arc<AtomicBool>,
    owner: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

struct Session {
    engine: Engine,
    _watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<()>,
}

impl WatchHandle {
    pub(crate) fn start(root: &Path, database: &Path) -> Result<Self> {
        let lock_path = database.with_added_extension("watch.lock");
        anyhow::ensure!(
            !lock_path.is_symlink(),
            "watcher lock must not be a symlink"
        );
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        #[cfg(unix)]
        {
            use std::fs;
            use std::os::unix::fs::PermissionsExt;
            lock.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        let session = if acquire(&lock)? {
            Some(Session::open(root, database)?)
        } else {
            None
        };
        let stop = Arc::new(AtomicBool::new(false));
        let owner = Arc::new(AtomicBool::new(session.is_some()));
        let stopping = stop.clone();
        let ownership = owner.clone();
        let root = root.to_path_buf();
        let database = database.to_path_buf();
        let worker = thread::spawn(move || {
            let mut session = session;
            while !stopping.load(Ordering::Acquire) {
                if let Some(active) = session.take() {
                    ownership.store(true, Ordering::Release);
                    active.run(&stopping);
                    break;
                }
                match acquire(&lock) {
                    Ok(true) => match Session::open(&root, &database) {
                        Ok(active) => session = Some(active),
                        Err(error) => {
                            eprintln!(
                                "prometeu-search: watcher takeover failed; retrying: {error:#}"
                            );
                            let _ = fs2::FileExt::unlock(&lock);
                        }
                    },
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!("prometeu-search: watcher lock failed; retrying: {error:#}");
                    }
                }
                if session.is_none() {
                    thread::sleep(Duration::from_millis(100));
                }
            }
            ownership.store(false, Ordering::Release);
            // Release the lock after the watcher and its connection have stopped.
            drop(session);
            drop(lock);
        });
        Ok(Self {
            stop,
            owner,
            worker: Some(worker),
        })
    }

    pub fn is_owner(&self) -> bool {
        self.owner.load(Ordering::Acquire)
    }
}

fn acquire(lock: &File) -> Result<bool> {
    match fs2::FileExt::try_lock_exclusive(lock) {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

impl Session {
    fn open(root: &Path, database: &Path) -> Result<Self> {
        let engine = Engine::open(root, Some(database))?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let root_owned = root.to_path_buf();
        let mut cache_paths = vec![
            database.to_path_buf(),
            database.with_added_extension("watch.lock"),
        ];
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut name = database.as_os_str().to_os_string();
            name.push(suffix);
            cache_paths.push(PathBuf::from(name));
        }
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| match event {
                Ok(event) if relevant(&event, &root_owned, &cache_paths) => {
                    let _ = sender.try_send(());
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("prometeu-search: watcher error; queries still reconcile: {error}");
                    let _ = sender.try_send(());
                }
            })
            .context("cannot create file watcher")?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .context("cannot watch project root")?;
        Ok(Self {
            engine,
            _watcher: watcher,
            receiver,
        })
    }

    fn run(mut self, stopping: &AtomicBool) {
        if let Err(error) = self.engine.sync() {
            eprintln!("prometeu-search: initial index failed: {error:#}");
        }
        while !stopping.load(Ordering::Acquire) {
            if self
                .receiver
                .recv_timeout(Duration::from_millis(100))
                .is_err()
            {
                continue;
            }
            let burst = Instant::now();
            while burst.elapsed() < Duration::from_secs(2) && !stopping.load(Ordering::Acquire) {
                if self
                    .receiver
                    .recv_timeout(Duration::from_millis(250))
                    .is_err()
                {
                    break;
                }
            }
            if stopping.load(Ordering::Acquire) {
                break;
            }
            if let Err(error) = self.engine.sync() {
                eprintln!("prometeu-search: index update failed; queries retry: {error:#}");
            }
        }
    }
}

fn relevant(event: &Event, root: &Path, cache_paths: &[PathBuf]) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    if event.paths.is_empty() {
        return event.need_rescan();
    }
    event.paths.iter().any(|path| {
        if cache_paths.contains(path) {
            return false;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        if relative.components().any(|part| {
            [
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
            ]
            .iter()
            .any(|skip| part.as_os_str() == *skip)
        }) {
            return false;
        }
        // Renamed or deleted directories may no longer exist, including names such as src.v2.
        event.need_rescan()
            || matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(ModifyKind::Name(_))
                    | EventKind::Any
                    | EventKind::Other
            )
            || crate::parser::supports(path)
            || path.is_dir()
            || path.file_name().is_some_and(|name| {
                [".gitignore", ".ignore", ".prometeu-searchignore"]
                    .iter()
                    .any(|ignore| name == *ignore)
            })
    })
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
