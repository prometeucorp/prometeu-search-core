//! Pinned reference default exclusions and source-directory exceptions.
//! SPDX-License-Identifier: MIT. Provenance: crates/prometeu-extract/UPSTREAM.md.
use crate::project_config::Patterns;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const DIRECTORIES: &[&str] = &[
    "node_modules",
    "bower_components",
    "jspm_packages",
    "web_modules",
    ".yarn",
    ".pnpm-store",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".vite",
    ".parcel-cache",
    ".angular",
    ".docusaurus",
    "storybook-static",
    ".vinxi",
    ".nitro",
    "out-tsc",
    ".vercel",
    ".netlify",
    ".wrangler",
    "dist",
    "build",
    "out",
    ".output",
    "coverage",
    ".nyc_output",
    "__pycache__",
    "__pypackages__",
    ".venv",
    "venv",
    ".pixi",
    ".pdm-build",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
    ".hypothesis",
    ".ipynb_checkpoints",
    ".eggs",
    "target",
    ".gradle",
    "obj",
    "vendor",
    ".build",
    "Pods",
    "Carthage",
    "DerivedData",
    ".swiftpm",
    ".dart_tool",
    ".pub-cache",
    ".cxx",
    ".externalNativeBuild",
    "vcpkg_installed",
    ".bloop",
    ".metals",
    "lua_modules",
    ".luarocks",
    "__history",
    "__recovery",
    ".cache",
];
const JVM: &[&str] = &[
    "**/src/*/java/**/build/",
    "**/src/*/kotlin/**/build/",
    "**/src/*/scala/**/build/",
];

fn patterns() -> Vec<String> {
    let mut patterns: Vec<_> = DIRECTORIES.iter().map(|name| format!("{name}/")).collect();
    patterns.extend(["*.egg-info/", "cmake-build-*/", "bazel-*/"].map(str::to_string));
    patterns.extend(
        [
            "anim",
            "animator",
            "color",
            "drawable",
            "font",
            "layout",
            "menu",
            "mipmap",
            "navigation",
            "transition",
            "values",
            "xml",
        ]
        .map(|name| format!("**/res/{name}*/")),
    );
    patterns.extend(JVM.iter().map(|pattern| format!("!{pattern}")));
    patterns.push("!**/src/**/build/".into());
    patterns
}

#[derive(Clone)]
pub(super) struct Defaults {
    combined: Patterns,
    project: Patterns,
}

impl Defaults {
    pub fn new(project: &[String]) -> Self {
        let mut all = patterns();
        all.extend(project.iter().cloned());
        Self {
            combined: Patterns::new(&all),
            project: Patterns::new(project),
        }
    }
    pub fn excludes(&self, path: &Path, directory: bool) -> bool {
        if self.combined.matches(path, directory) {
            return true;
        }
        let Some(folder) = build_output(path, directory) else {
            return false;
        };
        for ancestor in path
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(&folder))
        {
            if self
                .project
                .decision(ancestor, ancestor != path || directory)
                == Some(false)
            {
                return false;
            }
        }
        true
    }
}

pub(super) fn excluded(path: &Path, directory: bool) -> bool {
    static DEFAULTS: LazyLock<Defaults> = LazyLock::new(|| Defaults::new(&[]));
    DEFAULTS.excludes(path, directory)
}

fn build_output(path: &Path, directory: bool) -> Option<PathBuf> {
    if directory {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    if !name.contains('.') {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let hand_written = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            [
                "ts", "mts", "cts", "tsx", "ets", "jsx", "vue", "svelte", "astro", "go", "cs", "rs",
            ]
            .contains(&extension.to_ascii_lowercase().as_str())
        });
    let declaration = [".d.ts", ".d.mts", ".d.cts", ".d.ets"]
        .iter()
        .any(|suffix| lower.ends_with(suffix));
    if hand_written && !declaration {
        return None;
    }
    let mut folder = PathBuf::new();
    let mut inside_src = false;
    for part in path.parent()?.components() {
        folder.push(part.as_os_str());
        if part.as_os_str().eq_ignore_ascii_case("src") {
            inside_src = true;
        } else if inside_src && part.as_os_str().eq_ignore_ascii_case("build") {
            static JVM_PACKAGES: LazyLock<Patterns> = LazyLock::new(|| {
                Patterns::new(
                    &JVM.iter()
                        .map(|pattern| (*pattern).into())
                        .collect::<Vec<_>>(),
                )
            });
            return (!JVM_PACKAGES.matches(&folder, true)).then_some(folder);
        }
    }
    None
}
