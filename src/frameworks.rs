//! Framework declarations and detection over committed project inputs.
mod aspnet;
mod astro;
pub(crate) mod codeigniter;
mod dependencies;
pub(crate) mod drupal;
pub(crate) use dependencies::declared_borrowed as declared_dependencies;
pub(crate) mod express;
mod extractors;
pub(crate) use extractors::regex as source_pattern;
mod finalize;
pub(crate) mod frontend;
pub(crate) mod go_bindings;
pub(crate) mod laravel;
pub(crate) mod mobile;
pub(crate) mod nestjs;
mod nuxt;
pub(crate) mod play;
mod python;
pub(crate) mod python_resolution;
pub(crate) mod rails;
mod routes;
mod rust;
mod spring;
mod vapor;
use crate::{SyncReport, index::store_file, parser};
use anyhow::{Context, Result};
use prometeu_extract::ExtractionResult;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

const KEY: &str = "extraction_frameworks";
const FABRIC_LANGUAGES: &[&str] = &[
    "typescript",
    "tsx",
    "javascript",
    "jsx",
    "objc",
    "java",
    "kotlin",
];

pub(crate) fn append(
    path: &Path,
    source: &str,
    language: &str,
    frameworks: &[String],
    result: &mut ExtractionResult,
) -> Result<()> {
    if frameworks.is_empty() {
        return Ok(());
    }
    let path = path
        .to_str()
        .context("source path must be UTF-8")?
        .replace(std::path::MAIN_SEPARATOR, "/");
    // Preserve the reference registry order independently of the caller's name order.
    if language == "php" && frameworks.iter().any(|name| name == "laravel") {
        let extracted = laravel::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    for framework in ["drupal", "codeigniter"] {
        if frameworks.iter().any(|name| name == framework)
            && (language == "php" || framework == "drupal" && language == "yaml")
        {
            let extracted = if framework == "drupal" {
                drupal::extract(&path, source)?
            } else {
                codeigniter::extract(&path, source)?
            };
            result.nodes.extend(extracted.nodes);
            result
                .unresolved_references
                .extend(extracted.unresolved_references);
        }
    }
    if matches!(language, "javascript" | "typescript")
        && frameworks.iter().any(|name| name == "express")
    {
        let extracted = express::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if matches!(language, "javascript" | "typescript")
        && frameworks.iter().any(|name| name == "nestjs")
    {
        let extracted = nestjs::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    for framework in [
        "react",
        "tanstack-router",
        "nextjs",
        "svelte",
        "nuxt",
        "vue-router",
        "angular-router",
        "astro",
        "expo-router",
    ] {
        let applies = if framework == "svelte" {
            language == "svelte"
        } else if framework == "angular-router" {
            matches!(language, "typescript" | "javascript")
        } else if framework == "vue-router" {
            matches!(language, "typescript" | "javascript" | "vue")
        } else if matches!(framework, "nuxt" | "astro") {
            true
        } else {
            matches!(language, "typescript" | "javascript" | "tsx" | "jsx")
        };
        if applies && frameworks.iter().any(|name| name == framework) {
            let extracted = if framework == "astro" {
                astro::extract(&path, source)?
            } else {
                frontend::extract(&path, source, framework)?
            };
            result.nodes.extend(extracted.nodes);
            result
                .unresolved_references
                .extend(extracted.unresolved_references);
        }
    }
    if language == "python" {
        for framework in ["django", "flask", "fastapi"] {
            if frameworks.iter().any(|name| name == framework) {
                let extracted = python::extract(&path, source, framework)?;
                result.nodes.extend(extracted.nodes);
                result
                    .unresolved_references
                    .extend(extracted.unresolved_references);
            }
        }
    }
    if language == "ruby" && frameworks.iter().any(|name| name == "rails") {
        let extracted = rails::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if language == "csharp" && frameworks.iter().any(|name| name == "aspnet") {
        let extracted = aspnet::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if matches!(language, "java" | "kotlin" | "yaml" | "properties")
        && frameworks.iter().any(|name| name == "spring")
    {
        let extracted = spring::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if frameworks.iter().any(|name| name == "play") && matches!(language, "scala" | "java" | "yaml")
    {
        let extracted = play::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if language == "go" && frameworks.iter().any(|name| name == "go") {
        let extracted = routes::go(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if language == "rust" && frameworks.iter().any(|name| name == "rust") {
        let extracted = rust::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if language == "go" && frameworks.iter().any(|name| name == "goframe") {
        let extracted = routes::goframe(&path, source)?;
        result.nodes.extend(extracted.nodes);
    }
    if language == "swift" && frameworks.iter().any(|name| name == "vapor") {
        let extracted = vapor::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if frameworks.iter().any(|name| name == "expo-modules")
        && matches!(language, "swift" | "kotlin")
    {
        result.nodes.extend(extractors::expo(&path, source)?);
    }
    if language == "objc" && frameworks.iter().any(|name| name == "react-native-bridge") {
        let extracted = mobile::extract(&path, source)?;
        result.nodes.extend(extracted.nodes);
        result
            .unresolved_references
            .extend(extracted.unresolved_references);
    }
    if frameworks.iter().any(|name| name == "fabric-view") && FABRIC_LANGUAGES.contains(&language) {
        result.nodes.extend(extractors::fabric(&path, source)?);
    }
    Ok(())
}

pub(crate) fn active(connection: &Connection) -> Result<Vec<String>> {
    let stored: Option<String> = connection
        .query_row("SELECT value FROM metadata WHERE key=?1", [KEY], |row| {
            row.get(0)
        })
        .optional()?;
    stored.map_or_else(
        || Ok(Vec::new()),
        |stored| Ok(serde_json::from_str(&stored)?),
    )
}

fn detect(connection: &Connection) -> Result<Vec<String>> {
    let manifests: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='project_manifests'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let manifests: BTreeMap<String, String> = manifests
        .map(|value| serde_json::from_str(&value))
        .transpose()?
        .unwrap_or_default();
    let expo = regex::Regex::new(r#"["']expo-modules-core["']\s*:"#).unwrap();
    let fabric = regex::Regex::new(r#"["']react-native["']\s*:"#).unwrap();
    let mut found_expo = manifests
        .get("package.json")
        .is_some_and(|source| expo.is_match(source));
    let found_fabric = manifests.iter().any(|(path, source)| {
        let parts: Vec<_> = path.split('/').collect();
        (path == "package.json"
            || (parts.len() == 3
                && ["packages", "apps", "modules", "libraries"].contains(&parts[0])
                && parts[2] == "package.json"))
            && fabric.is_match(source)
    });
    if !found_expo {
        let mut statement = connection.prepare("SELECT path FROM files ORDER BY path LIMIT 200")?;
        let paths = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for path in paths {
            if !path.ends_with(".swift") && !path.ends_with(".kt") {
                continue;
            }
            let source: String = connection.query_row(
                "SELECT code FROM symbols WHERE path=?1 AND kind='file' ORDER BY ordinal LIMIT 1",
                [path],
                |row| row.get(0),
            )?;
            if extractors::is_expo(&source)? {
                found_expo = true;
                break;
            }
        }
    }
    let mut statement = connection.prepare("SELECT path FROM files ORDER BY path")?;
    let paths = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let source = |path: &str| -> Result<Option<String>> {
        if let Some(source) = manifests.get(path) {
            return Ok(Some(source.clone()));
        }
        Ok(connection
            .query_row(
                "SELECT code FROM symbols WHERE path=?1 AND kind='file' ORDER BY ordinal LIMIT 1",
                [path],
                |row| row.get(0),
            )
            .optional()?)
    };
    let mut found_django = paths.iter().any(|path| path == "manage.py");
    for path in ["requirements.txt", "setup.py", "pyproject.toml"] {
        found_django |=
            source(path)?.is_some_and(|source| source.to_lowercase().contains("django"));
    }
    let flask = regex::Regex::new(r"(?i)\bflask\b")?;
    let fastapi = regex::Regex::new(r"(?i)\bfastapi\b")?;
    let mut found_flask = false;
    let mut found_fastapi = false;
    for path in ["requirements.txt", "pyproject.toml", "Pipfile", "setup.py"] {
        if let Some(source) = source(path)? {
            found_flask |= flask.is_match(&source);
            if matches!(path, "requirements.txt" | "pyproject.toml") {
                found_fastapi |= fastapi.is_match(&source);
            }
        }
    }
    let flask_entry = regex::Regex::new(r"(?:^|/)(app|application|main|wsgi|__init__)\.py$")?;
    let flask_create = regex::Regex::new(r"\bFlask\s*\(")?;
    let flask_import = regex::Regex::new(r"\bimport\s+flask\b|\bfrom\s+flask\b")?;
    if !found_flask {
        for path in paths
            .iter()
            .filter(|path| flask_entry.is_match(path))
            .take(50)
        {
            if let Some(source) = source(path)?
                && flask_create.is_match(&source)
                && flask_import.is_match(&source)
            {
                found_flask = true;
                break;
            }
        }
    }
    for path in ["app.py", "main.py", "api.py"] {
        found_fastapi |= source(path)?.is_some_and(|source| source.contains("FastAPI("));
    }
    if !found_fastapi {
        let mut looked = 0;
        for path in &paths {
            let base = path.rsplit('/').next().unwrap();
            if matches!(
                base,
                "requirements.txt" | "pyproject.toml" | "requirements-dev.txt"
            ) {
                if source(path)?.is_some_and(|source| fastapi.is_match(&source)) {
                    found_fastapi = true;
                    break;
                }
                looked += 1;
            } else if matches!(base, "main.py" | "app.py" | "api.py")
                && path.split('/').count() <= 4
            {
                if source(path)?.is_some_and(|source| source.contains("FastAPI(")) {
                    found_fastapi = true;
                    break;
                }
                looked += 1;
            }
            if looked >= 40 {
                break;
            }
        }
    }
    let modules: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='resolution_go_modules'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let modules: BTreeMap<String, String> = modules
        .map(|json| serde_json::from_str(&json))
        .transpose()?
        .unwrap_or_default();
    let go_module = modules.get("go.mod");
    let found_go = paths.iter().any(|path| path.ends_with(".go"))
        || go_module.is_some_and(|text| !text.is_empty());
    let found_goframe = go_module.is_some_and(|text| text.contains("github.com/gogf/gf"));
    let mut found_vapor = source("Package.swift")?.is_some_and(|text| text.contains("vapor"));
    if !found_vapor {
        for path in paths.iter().filter(|path| path.ends_with(".swift")) {
            if source(path)?.is_some_and(|text| text.contains("import Vapor")) {
                found_vapor = true;
                break;
            }
        }
    }
    let mut found_aspnet = paths.iter().any(|path| path == "Startup.cs");
    for path in paths.iter().filter(|path| path.ends_with(".csproj")) {
        found_aspnet |= source(path)?.is_some_and(|text| {
            [
                "Microsoft.AspNetCore",
                "Microsoft.NET.Sdk.Web",
                "System.Web.Mvc",
            ]
            .iter()
            .any(|marker| text.contains(marker))
        });
    }
    found_aspnet |= source("Program.cs")?.is_some_and(|text| {
        ["WebApplication", "CreateHostBuilder", "UseStartup"]
            .iter()
            .any(|marker| text.contains(marker))
    });
    if !found_aspnet {
        let file_pattern = regex::Regex::new(
            r"(Controller|Program|Startup|Endpoints?|Extensions)\.cs$|(?:^|/)Endpoints/[^/]+\.cs$",
        )?;
        let attribute =
            regex::Regex::new(r"\[(ApiController|Route|Http(Get|Post|Put|Patch|Delete))\b")?;
        for path in paths.iter().filter(|path| file_pattern.is_match(path)) {
            if source(path)?.is_some_and(|text| {
                attribute.is_match(&text)
                    || [
                        "ControllerBase",
                        ": Controller",
                        "MapControllers",
                        "WebApplication",
                        "Microsoft.AspNetCore",
                        "IEndpointGroup",
                        "EndpointGroupBase",
                        "RouteGroupBuilder",
                    ]
                    .iter()
                    .any(|marker| text.contains(marker))
            }) {
                found_aspnet = true;
                break;
            }
        }
    }
    let rails_gem = extractors::regex(r#"\bgem\s+["'](?:rails|railties)["']"#)?;
    let found_rails = source("Gemfile")?
        .map(|source| -> Result<bool> { Ok(rails_gem.is_match(&source)?) })
        .transpose()?
        .unwrap_or(false)
        || paths.iter().any(|path| {
            matches!(
                path.as_str(),
                "config/application.rb"
                    | "app/controllers/application_controller.rb"
                    | "config/routes.rb"
            ) || path.ends_with("/config/routes.rb")
        });
    let mut found_spring = false;
    for path in ["pom.xml", "build.gradle", "build.gradle.kts"] {
        found_spring |= source(path)?
            .is_some_and(|text| text.contains("spring-boot") || text.contains("springframework"));
    }
    if !found_spring {
        for path in paths.iter().filter(|path| path.ends_with(".java")) {
            if source(path)?.is_some_and(|text| {
                [
                    "@SpringBootApplication",
                    "@RestController",
                    "@Service",
                    "@Repository",
                ]
                .iter()
                .any(|marker| text.contains(marker))
            }) {
                found_spring = true;
                break;
            }
        }
    }
    let declared = dependencies::declared(&manifests, &paths);
    let mut found_express = ["express", "fastify", "koa", "hapi", "@hapi/hapi"]
        .iter()
        .any(|name| declared.contains(*name));
    if !found_express {
        for path in paths.iter().filter(|path| {
            ["routes", "controllers", "middleware"]
                .iter()
                .any(|part| path.contains(part))
        }) {
            if source(path)?.is_some_and(|source| {
                ["express", "app.get", "router.get"]
                    .iter()
                    .any(|pattern| source.contains(pattern))
            }) {
                found_express = true;
                break;
            }
        }
    }
    let mut found_nest = declared.iter().any(|name| name.starts_with("@nestjs/"));
    if !found_nest {
        for path in paths.iter().filter(|path| {
            [
                ".controller.ts",
                ".controller.js",
                ".module.ts",
                ".resolver.ts",
                ".gateway.ts",
            ]
            .iter()
            .any(|suffix| path.ends_with(suffix))
        }) {
            if source(path)?.is_some_and(|source| {
                [
                    "@nestjs/",
                    "@Controller",
                    "@Module(",
                    "@Resolver(",
                    "@WebSocketGateway(",
                ]
                .iter()
                .any(|marker| source.contains(marker))
            }) {
                found_nest = true;
                break;
            }
        }
    }
    let next_config = regex::Regex::new(r"(?:^|/)next\.config\.[cm]?[jt]s$")?;
    let nuxt_config = regex::Regex::new(r"(?:^|/)nuxt\.config\.[cm]?[jt]s$")?;
    let expo_layout = regex::Regex::new(r"(?:^|/)(?:src/)?app/_layout\.(?:tsx|jsx|ts|js)$")?;
    let expo_config = regex::Regex::new(r"^app\.(?:json|config\.(?:js|ts))$")?;
    let mut found_next = declared.contains("next");
    if !found_next && paths.iter().any(|path| next_config.is_match(path)) {
        for path in &paths {
            if frontend::paths::next(path)?.is_some() {
                found_next = true;
                break;
            }
        }
    }
    let found_expo_router = declared.contains("expo-router")
        || (paths.iter().any(|path| expo_layout.is_match(path))
            && paths.iter().any(|path| expo_config.is_match(path)));
    let root_dependency = |names: &[&str]| {
        manifests
            .get("package.json")
            .and_then(|source| serde_json::from_str::<serde_json::Value>(source).ok())
            .is_some_and(|package| {
                names.iter().any(|name| {
                    let value = package["devDependencies"]
                        .get(*name)
                        .or_else(|| package["dependencies"].get(*name));
                    value.is_some_and(|value| match value {
                        serde_json::Value::Null => false,
                        serde_json::Value::Bool(value) => *value,
                        serde_json::Value::String(value) => !value.is_empty(),
                        serde_json::Value::Number(value) => value.as_f64() != Some(0.0),
                        _ => true,
                    })
                })
            })
    };
    let found_svelte = paths.iter().any(|path| path.ends_with(".svelte"))
        || root_dependency(&["svelte", "@sveltejs/kit"]);
    let found_vue = paths.iter().any(|path| path.ends_with(".vue"))
        || root_dependency(&["vue", "nuxt", "@nuxt/kit"]);
    let found_cics: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM symbols s JOIN files f ON f.path=s.path WHERE f.language='cobol' AND s.kind='module')", [], |row| row.get(0))?;
    let found_drupal = drupal::detect(
        &manifests
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect(),
        &paths.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let found_ci = paths
        .iter()
        .any(|path| path == "system/core/CodeIgniter.php")
        || if let Some(path) = paths
            .iter()
            .find(|path| path.ends_with("application/config/routes.php"))
        {
            source(path)?.is_some_and(|source| {
                source_pattern(r#"\$route\s*\[\s*['"]default_controller['"]\s*\]"#)
                    .unwrap()
                    .is_match(&source)
                    .unwrap_or(false)
            })
        } else {
            false
        };
    let found_play = source("build.sbt")?
        .map(|source| -> Result<bool> {
            Ok(
                source_pattern(r#"(?i)playframework|"play"|sbt-plugin|PlayScala|PlayJava"#)?
                    .is_match(&source)?,
            )
        })
        .transpose()?
        .unwrap_or(false)
        || manifests.contains_key("conf/application.conf")
        || paths.iter().any(|path| play::is_routes(path));
    let mut found_swiftui = paths
        .iter()
        .any(|path| path.ends_with(".xcodeproj") || path.ends_with(".xcworkspace"));
    let mut found_uikit = false;
    for path in paths.iter().filter(|path| path.ends_with(".swift")) {
        if let Some(source) = source(path)? {
            found_swiftui |= source.contains("import SwiftUI");
            found_uikit |= ["import UIKit", "UIViewController", "UIView"]
                .iter()
                .any(|marker| source.contains(marker));
        }
    }
    let found_swift_objc = paths.iter().any(|path| path.ends_with(".swift"))
        && paths
            .iter()
            .any(|path| path.ends_with(".m") || path.ends_with(".mm"));
    let mut found_rn = manifests
        .get("package.json")
        .map(|source| -> Result<bool> {
            Ok(source_pattern(r#"["']react-native["']\s*:"#)?.is_match(source)?)
        })
        .transpose()?
        .unwrap_or(false);
    if !found_rn {
        for path in paths.iter().take(200) {
            let pattern = if path.ends_with(".m") || path.ends_with(".mm") {
                Some(r"RCT_EXPORT_MODULE\b|RCT_EXTERN_(?:REMAP_)?MODULE\b")
            } else if path.ends_with(".ts") || path.ends_with(".tsx") {
                Some(r"TurboModuleRegistry\.(?:get|getEnforcing)\s*<")
            } else {
                None
            };
            if let Some(pattern) = pattern
                && source(path)?
                    .map(|source| -> Result<bool> {
                        Ok(source_pattern(pattern)?.is_match(&source)?)
                    })
                    .transpose()?
                    .unwrap_or(false)
            {
                found_rn = true;
                break;
            }
        }
    }
    let mut found = Vec::new();
    for (name, detected) in [
        (
            "laravel",
            manifests.contains_key("artisan")
                || paths.iter().any(|path| path == "app/Http/Kernel.php"),
        ),
        ("drupal", found_drupal),
        ("codeigniter", found_ci),
        ("express", found_express),
        ("nestjs", found_nest),
        (
            "react",
            ["react", "next", "react-native"]
                .iter()
                .any(|name| declared.contains(*name))
                || paths
                    .iter()
                    .any(|path| path.ends_with(".jsx") || path.ends_with(".tsx")),
        ),
        (
            "react-router",
            ["react-router", "react-router-dom", "react-router-native"]
                .iter()
                .any(|name| declared.contains(*name)),
        ),
        (
            "angular-router",
            ["@angular/router", "@angular/core"]
                .iter()
                .any(|name| declared.contains(*name)),
        ),
        (
            "tanstack-router",
            [
                "@tanstack/react-router",
                "@tanstack/solid-router",
                "@tanstack/router",
                "@tanstack/react-start",
                "@tanstack/start",
            ]
            .iter()
            .any(|name| declared.contains(*name)),
        ),
        ("nextjs", found_next),
        (
            "vue-router",
            ["vue-router", "nuxt", "nuxt3"]
                .iter()
                .any(|name| declared.contains(*name)),
        ),
        (
            "nuxt",
            ["nuxt", "nuxt3", "@nuxt/kit"]
                .iter()
                .any(|name| declared.contains(*name))
                || paths.iter().any(|path| nuxt_config.is_match(path)),
        ),
        (
            "astro",
            paths.iter().any(|path| path.ends_with(".astro")) || root_dependency(&["astro"]),
        ),
        ("vue", found_vue),
        ("svelte", found_svelte),
        ("sveltekit-router", declared.contains("@sveltejs/kit")),
        ("django", found_django),
        ("flask", found_flask),
        ("fastapi", found_fastapi),
        ("rails", found_rails),
        ("spring", found_spring),
        ("play", found_play),
        ("go", found_go),
        ("goframe", found_goframe),
        ("rust", manifests.contains_key("Cargo.toml")),
        ("aspnet", found_aspnet),
        ("vapor", found_vapor),
        ("swiftui", found_swiftui),
        ("uikit", found_uikit),
        ("swift-objc-bridge", found_swift_objc),
        ("react-native-bridge", found_rn),
        ("expo-modules-js", found_expo),
        ("expo-router", found_expo_router),
        ("cics", found_cics),
        (
            "terraform",
            paths.iter().any(|path| {
                [".tf", ".tfvars", ".tofu"]
                    .iter()
                    .any(|suffix| path.ends_with(suffix))
            }),
        ),
    ] {
        if detected {
            found.push(name.into());
        }
    }
    if found_expo {
        found.push("expo-modules".into());
    }
    if found_fabric {
        found.push("fabric-view".into());
    }
    let order = [
        "laravel",
        "drupal",
        "codeigniter",
        "express",
        "nestjs",
        "react",
        "react-router",
        "tanstack-router",
        "nextjs",
        "svelte",
        "sveltekit-router",
        "vue",
        "nuxt",
        "vue-router",
        "angular-router",
        "astro",
        "django",
        "flask",
        "fastapi",
        "rails",
        "spring",
        "play",
        "go",
        "goframe",
        "rust",
        "aspnet",
        "swiftui",
        "uikit",
        "vapor",
        "swift-objc-bridge",
        "react-native-bridge",
        "expo-modules",
        "expo-modules-js",
        "expo-router",
        "fabric-view",
        "cics",
        "terraform",
    ];
    found.sort_by_key(|name: &String| order.iter().position(|entry| *entry == name).unwrap());
    Ok(found)
}

/// Re-extract stored source when project-wide detection changes, in the caller's transaction.
pub(crate) fn reconcile(
    transaction: &Transaction<'_>,
    report: &mut SyncReport,
    parsed_paths: &HashSet<String>,
) -> Result<bool> {
    let previous = active(transaction)?;
    let current = detect(transaction)?;
    if current == previous {
        return finalize::run(transaction, &current);
    }
    let mut statement=transaction.prepare("SELECT f.path,f.hash,f.language,f.deprioritized,coalesce(s.signature,'') FROM files f LEFT JOIN file_state s ON s.path=f.path ORDER BY f.path")?;
    let files = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    for (path, hash, language, deprioritized, signature) in files {
        let applies = |names: &[String]| {
            names.iter().any(|name| match name.as_str() {
                "react" | "tanstack-router" | "nextjs" | "expo-router" => matches!(
                    language.as_str(),
                    "typescript" | "javascript" | "tsx" | "jsx"
                ),
                "svelte" => language == "svelte",
                "nuxt" | "astro" => true,
                "vue-router" => matches!(language.as_str(), "vue" | "typescript" | "javascript"),
                "laravel" | "codeigniter" => language == "php",
                "drupal" => matches!(language.as_str(), "php" | "yaml"),
                "angular-router" => matches!(language.as_str(), "typescript" | "javascript"),
                "express" | "nestjs" => matches!(language.as_str(), "typescript" | "javascript"),
                "django" | "flask" | "fastapi" => language == "python",
                "rails" => language == "ruby",
                "spring" => matches!(language.as_str(), "java" | "kotlin" | "yaml" | "properties"),
                "play" => matches!(language.as_str(), "scala" | "java" | "yaml"),
                "react-native-bridge" => language == "objc",
                "go" | "goframe" => language == "go",
                "rust" => language == "rust",
                "aspnet" => language == "csharp",
                "vapor" => language == "swift",
                "expo-modules" => matches!(language.as_str(), "swift" | "kotlin"),
                "fabric-view" => FABRIC_LANGUAGES.contains(&language.as_str()),
                _ => false,
            })
        };
        if !applies(&previous) && !applies(&current) {
            continue;
        }
        let source: String = transaction.query_row(
            "SELECT code FROM symbols WHERE path=?1 AND kind='file' ORDER BY ordinal LIMIT 1",
            [&path],
            |row| row.get(0),
        )?;
        let parsed = parser::parse_with_frameworks(Path::new(&path), &source, &language, &current)?;
        store_file(transaction, &path, &source, &hash, &parsed)?;
        transaction.execute(
            "UPDATE files SET deprioritized=?2 WHERE path=?1",
            params![path, deprioritized],
        )?;
        transaction.execute(
            "INSERT OR REPLACE INTO file_state(path,signature) VALUES(?1,?2)",
            params![path, signature],
        )?;
        if !parsed_paths.contains(&path) {
            report.indexed += 1;
            report.unchanged = report.unchanged.saturating_sub(1);
            report.syntax_errors += usize::from(parsed.has_errors);
        }
    }
    transaction.execute(
        "INSERT OR REPLACE INTO metadata(key,value) VALUES(?1,?2)",
        params![KEY, serde_json::to_string(&current)?],
    )?;
    finalize::run(transaction, &current)?;
    Ok(true)
}
