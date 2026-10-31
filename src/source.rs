use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

pub(crate) fn open_root(path: &Path) -> io::Result<Dir> {
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir if names.is_empty() => anchor.push(component),
            Component::Normal(name) => names.push(name),
            _ => return Err(invalid_path()),
        }
    }
    if !anchor.is_absolute() {
        return Err(invalid_path());
    }
    let mut directory = Dir::open_ambient_dir(anchor, cap_std::ambient_authority())?;
    for name in names {
        directory = directory.open_dir_nofollow(name)?;
    }
    Ok(directory)
}

pub(crate) fn open(root: &Dir, relative: &Path) -> io::Result<std::fs::File> {
    let mut directory = root.try_clone()?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(invalid_path());
        };
        if components.peek().is_some() {
            directory = directory.open_dir_nofollow(name)?;
        } else {
            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            #[cfg(unix)]
            {
                use cap_fs_ext::OpenOptionsSyncExt;
                // A replaced source must not block the indexer by becoming a FIFO.
                options.nonblock(true);
            }
            return Ok(directory.open_with(name, &options)?.into_std());
        }
    }
    Err(invalid_path())
}

fn invalid_path() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid source path")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;
    use tempfile::TempDir;

    #[test]
    fn source_handles_read_nested_files_and_reject_parent_paths() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("nested")).unwrap();
        fs::write(temp.path().join("nested/file.ts"), "bootstrap();").unwrap();
        let root = open_root(&temp.path().canonicalize().unwrap()).unwrap();
        let mut content = String::new();
        open(&root, Path::new("nested/file.ts"))
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "bootstrap();");
        assert!(open(&root, Path::new("../file.ts")).is_err());
        assert!(open(&root, &temp.path().join("nested/file.ts")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn replaced_files_and_ancestors_cannot_redirect_source_reads() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("project");
        let outside = temp.path().join("outside");
        fs::create_dir_all(project.join("nested")).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(project.join("nested/file.ts"), "inside();").unwrap();
        fs::write(outside.join("file.ts"), "secret();").unwrap();
        let root_path = project.canonicalize().unwrap();
        let root = open_root(&root_path).unwrap();

        // Replace the leaf after traversal has identified a regular file.
        let leaf = project.join("nested/file.ts");
        assert!(fs::symlink_metadata(&leaf).unwrap().is_file());
        fs::remove_file(&leaf).unwrap();
        symlink(outside.join("file.ts"), &leaf).unwrap();
        assert!(open(&root, Path::new("nested/file.ts")).is_err());

        // Checking only the leaf would miss this ancestor replacement.
        fs::rename(project.join("nested"), project.join("saved")).unwrap();
        symlink(&outside, project.join("nested")).unwrap();
        assert!(open(&root, Path::new("nested/file.ts")).is_err());
        assert!(open_root(&root_path.join("nested")).is_err());

        // A root handle stays bound to the directory selected at startup.
        fs::write(project.join("file.ts"), "original();").unwrap();
        fs::rename(&project, temp.path().join("original")).unwrap();
        symlink(&outside, &project).unwrap();
        let mut content = String::new();
        open(&root, Path::new("file.ts"))
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "original();");
    }
}
