//! Parse Git configuration from bounded snapshots, following explicit includes only.
use super::{absolute, text};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) fn settings(
    worktree: Option<&Path>,
    git_dir: Option<&Path>,
    common: Option<&Path>,
    fingerprint: &mut blake3::Hasher,
) -> Result<BTreeMap<String, String>> {
    let overrides = environment_overrides()?;
    let mut paths = global_paths();
    if let Some(common) = common {
        paths.push(common.join("config"));
    }
    let branch = git_dir
        .map(|dir| text(&dir.join("HEAD")))
        .transpose()?
        .flatten()
        .and_then(|head| {
            head.trim()
                .strip_prefix("ref: refs/heads/")
                .map(str::to_owned)
        });
    if let Some(branch) = &branch {
        fingerprint.update(branch.as_bytes());
    }
    let mut reader = Reader {
        git_dir,
        branch: branch.as_deref(),
        files: BTreeMap::new(),
        bytes: 0,
        remote_urls: Vec::new(),
        fingerprint,
    };
    let mut first = BTreeMap::new();
    for path in &paths {
        reader.read(path, 0, &mut first, false)?;
    }
    for (key, value) in &overrides {
        reader.environment(key, value, worktree, &mut first, false)?;
    }
    if truth(first.get("extensions.worktreeconfig"))
        && let Some(dir) = git_dir
    {
        let path = dir.join("config.worktree");
        reader.read(&path, 0, &mut first, false)?;
        paths.push(path);
    }
    let mut result = BTreeMap::new();
    for path in &paths {
        reader.read(path, 0, &mut result, true)?;
    }
    for (key, value) in &overrides {
        reader.environment(key, value, worktree, &mut result, true)?;
    }
    // Relative excludesFile values are interpreted from Git's working directory.
    if let Some(value) = result.get_mut("core.excludesfile")
        && !value.is_empty()
    {
        let base = worktree
            .map(Path::to_path_buf)
            .unwrap_or(std::env::current_dir()?);
        *value = expand(&base, value)?.to_string_lossy().into_owned();
    }
    Ok(result)
}

fn environment_overrides() -> Result<Vec<(String, String)>> {
    let mut overrides = Vec::new();
    if let Ok(count) = std::env::var("GIT_CONFIG_COUNT") {
        let count = count
            .parse::<usize>()
            .context("invalid Git environment configuration count")?;
        ensure!(count <= 1024, "Git environment configuration exceeds limit");
        for index in 0..count {
            let key = std::env::var(format!("GIT_CONFIG_KEY_{index}"))
                .context("missing Git environment configuration key")?;
            let value = std::env::var(format!("GIT_CONFIG_VALUE_{index}"))
                .context("missing Git environment configuration value")?;
            overrides.push((key, value));
        }
    }
    if let Ok(parameters) = std::env::var("GIT_CONFIG_PARAMETERS") {
        ensure!(
            parameters.len() <= 1024 * 1024,
            "Git environment configuration exceeds limit"
        );
        for argument in quoted_parameters(&parameters)? {
            let (key, value) = argument
                .split_once('=')
                .map_or((argument.as_str(), "true"), |(k, v)| (k, v));
            overrides.push((key.to_owned(), value.to_owned()));
        }
    }
    ensure!(
        overrides.len() <= 1024
            && overrides
                .iter()
                .map(|(k, v)| k.len() + v.len())
                .sum::<usize>()
                <= 1024 * 1024,
        "Git environment configuration exceeds limit"
    );
    for (key, _) in &overrides {
        ensure!(
            gix_config::KeyRef::parse_unvalidated(key.as_bytes().into()).is_some(),
            "invalid Git environment configuration key"
        );
    }
    Ok(overrides)
}

// Git passes either 'key=value' or 'key'='value', with shell single-quote escapes.
// This is a decoder only; parameters are never evaluated by a shell.
fn quoted_parameters(input: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut chars = input.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|ch| ch.is_ascii_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        ensure!(
            chars.peek() == Some(&'\''),
            "invalid Git quoted configuration parameters"
        );
        let mut argument = String::new();
        while let Some(&ch) = chars.peek() {
            if ch.is_ascii_whitespace() {
                break;
            }
            chars.next();
            match ch {
                '\'' => {
                    let mut closed = false;
                    for ch in chars.by_ref() {
                        if ch == '\'' {
                            closed = true;
                            break;
                        }
                        argument.push(ch);
                    }
                    ensure!(closed, "unterminated Git quoted configuration parameter");
                }
                '=' => argument.push('='),
                '\\' => {
                    let escaped = chars
                        .next()
                        .context("incomplete Git quoted configuration escape")?;
                    ensure!(
                        matches!(escaped, '\'' | '!'),
                        "invalid Git quoted configuration escape"
                    );
                    argument.push(escaped);
                }
                _ => anyhow::bail!("invalid Git quoted configuration parameters"),
            }
        }
        result.push(argument);
        ensure!(
            result.len() <= 1024,
            "Git environment configuration exceeds limit"
        );
    }
    Ok(result)
}

pub(super) fn truth(value: Option<&String>) -> bool {
    value.is_some_and(|value| {
        matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "on")
            || value.parse::<i64>().is_ok_and(|number| number != 0)
    })
}
pub(super) fn default_excludes() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .map(|dir| dir.join("git/ignore"))
}
fn global_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if !std::env::var("GIT_CONFIG_NOSYSTEM")
        .ok()
        .is_some_and(|value| !matches!(value.as_str(), "" | "0" | "false" | "no" | "off"))
    {
        paths.push(
            std::env::var_os("GIT_CONFIG_SYSTEM")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/etc/gitconfig")),
        );
    }
    if let Some(global) = std::env::var_os("GIT_CONFIG_GLOBAL") {
        if !global.is_empty() {
            paths.push(PathBuf::from(global));
        }
    } else {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        {
            paths.push(xdg.join("git/config"));
        }
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".gitconfig"));
        }
    }
    paths
}
fn expand(base: &Path, value: &str) -> Result<PathBuf> {
    if value == "~" {
        return dirs::home_dir().context("cannot resolve Git home directory");
    }
    if let Some(tail) = value.strip_prefix("~/") {
        return Ok(dirs::home_dir()
            .context("cannot resolve Git home directory")?
            .join(tail));
    }
    absolute(base, value)
}

struct Reader<'a> {
    git_dir: Option<&'a Path>,
    branch: Option<&'a str>,
    files: BTreeMap<PathBuf, Option<String>>,
    bytes: usize,
    remote_urls: Vec<String>,
    fingerprint: &'a mut blake3::Hasher,
}
impl Reader<'_> {
    fn environment(
        &mut self,
        key: &str,
        value: &str,
        worktree: Option<&Path>,
        settings: &mut BTreeMap<String, String>,
        conditional: bool,
    ) -> Result<()> {
        self.fingerprint.update(&(key.len() as u64).to_le_bytes());
        self.fingerprint.update(key.as_bytes());
        self.fingerprint.update(&(value.len() as u64).to_le_bytes());
        self.fingerprint.update(value.as_bytes());
        let folded = key.to_ascii_lowercase();
        if !conditional && folded.starts_with("remote.") && folded.ends_with(".url") {
            self.remote_urls.push(value.into());
        }
        let base = worktree
            .map(Path::to_path_buf)
            .unwrap_or(std::env::current_dir()?);
        let include = folded == "include.path"
            || folded.starts_with("includeif.")
                && folded.ends_with(".path")
                && self.matches(
                    &key[10..key.len() - 5],
                    &base.join("environment.config"),
                    conditional,
                );
        if include && !value.is_empty() {
            self.read(&expand(&base, value)?, 0, settings, conditional)?;
        } else {
            settings.insert(folded, value.into());
        }
        Ok(())
    }
    fn read(
        &mut self,
        path: &Path,
        depth: usize,
        settings: &mut BTreeMap<String, String>,
        conditional: bool,
    ) -> Result<()> {
        ensure!(depth <= 10, "Git configuration include depth exceeds limit");
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            absolute(
                &std::env::current_dir()?,
                path.to_str().context("Git metadata path must be UTF-8")?,
            )?
        };
        if !self.files.contains_key(&path) {
            ensure!(
                self.files.len() < 4096,
                "Git configuration file count exceeds limit"
            );
            let content = text(&path)?;
            if let Some(content) = &content {
                self.bytes += content.len();
                ensure!(
                    self.bytes <= 8 * 1024 * 1024,
                    "Git configuration snapshot exceeds limit"
                );
                self.fingerprint.update(path.as_os_str().as_encoded_bytes());
                self.fingerprint.update(content.as_bytes());
            }
            self.files.insert(path.clone(), content);
        }
        let Some(content) = self.files.get(&path).and_then(Option::as_ref) else {
            return Ok(());
        };
        let file = gix_config::File::from_bytes_no_includes(
            content.as_bytes(),
            gix_config::file::Metadata::default(),
            Default::default(),
        )
        .map_err(|error| {
            anyhow::anyhow!("invalid Git configuration {}: {error}", path.display())
        })?;
        for section in file.sections() {
            let header = section.header();
            let name = header.name().to_string().to_ascii_lowercase();
            let subsection = header.subsection_name().map(ToString::to_string);
            let include = name == "include" && subsection.is_none()
                || name == "includeif"
                    && subsection
                        .as_ref()
                        .is_some_and(|condition| self.matches(condition, &path, conditional));
            if include {
                for value in section.values("path") {
                    let value = std::str::from_utf8(value.as_ref())
                        .context("Git include path must be UTF-8")?;
                    if !value.is_empty() {
                        let child = expand(
                            path.parent().context("Git configuration has no parent")?,
                            value,
                        )?;
                        self.read(&child, depth + 1, settings, conditional)?;
                    }
                }
            } else if name == "remote" {
                if !conditional {
                    self.remote_urls
                        .extend(section.values("url").into_iter().map(|url| url.to_string()));
                }
            } else if subsection.is_none() && matches!(name.as_str(), "core" | "extensions") {
                for key in section.value_names() {
                    if let Some(value) = section.value_implicit(&key) {
                        settings.insert(
                            format!("{name}.{}", key.to_ascii_lowercase()),
                            value.map_or_else(|| "true".into(), |value| value.to_string()),
                        );
                    }
                }
            }
        }
        Ok(())
    }
    fn matches(&self, condition: &str, path: &Path, conditional: bool) -> bool {
        let Some((prefix, value)) = condition.split_once(':') else {
            return false;
        };
        let mode = gix_glob::wildmatch::Mode::NO_MATCH_SLASH_LITERAL;
        let matches = |pattern: &str, text: &str, mode| {
            gix_glob::wildmatch(pattern.as_bytes().into(), text.as_bytes().into(), mode)
        };
        match prefix {
            "onbranch" => self
                .branch
                .is_some_and(|branch| matches(&directory_pattern(value), branch, mode)),
            "hasconfig" => {
                conditional
                    && value.strip_prefix("remote.*.url:").is_some_and(|pattern| {
                        self.remote_urls
                            .iter()
                            .any(|url| matches(pattern, url, mode))
                    })
            }
            "gitdir" | "gitdir/i" => {
                let Some(git_dir) = self.git_dir else {
                    return false;
                };
                let mut pattern = if value.starts_with("~/") {
                    dirs::home_dir()
                        .map(|home| home.join(&value[2..]).to_string_lossy().into_owned())
                        .unwrap_or_else(|| value.into())
                } else if let Some(tail) = value.strip_prefix("./") {
                    path.parent()
                        .unwrap_or(Path::new(""))
                        .join(tail)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    value.into()
                };
                if !Path::new(&pattern).is_absolute() {
                    pattern = format!("**/{pattern}");
                }
                pattern = directory_pattern(&pattern);
                let mode = if prefix == "gitdir/i" {
                    mode | gix_glob::wildmatch::Mode::IGNORE_CASE
                } else {
                    mode
                };
                matches(&pattern, &git_dir.to_string_lossy(), mode)
                    || git_dir.canonicalize().ok().is_some_and(|directory| {
                        matches(&pattern, &directory.to_string_lossy(), mode)
                    })
            }
            _ => false,
        }
    }
}
fn directory_pattern(value: &str) -> String {
    if value.ends_with('/') {
        format!("{value}**")
    } else {
        value.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_parameters_decode_without_shell_evaluation_and_reject_malformed_input() {
        assert_eq!(quoted_parameters("'core.ignoreCase=true' 'core.excludesFile'='path with '\\''quotes'\\'' and $(literal)' ").unwrap(),["core.ignoreCase=true","core.excludesFile=path with 'quotes' and $(literal)"]);
        for malformed in [
            "unquoted=value",
            "'unclosed",
            "'core.x'='value'junk",
            "'core.x'='value'\\q",
        ] {
            assert!(quoted_parameters(malformed).is_err(), "{malformed}");
        }
        assert!(!truth(Some(&String::new())));
        assert!(truth(Some(&"true".into())));
    }
}
