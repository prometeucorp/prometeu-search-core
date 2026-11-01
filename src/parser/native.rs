//! Adapt the pinned upstream extraction contract to the bounded engine.
use super::{ExtractionResult, Import, ParsedFile, Reference, Symbol, preprocess};
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

pub(super) fn language(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".app") || name.ends_with(".app.src") {
        return Some("erlang");
    }
    if path.file_name()?.to_str()? == "routes"
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "conf")
    {
        return Some("yaml");
    }
    if matches!(
        path.file_name()?.to_str()?,
        "Gemfile" | "Rakefile" | "Guardfile"
    ) {
        return Some("ruby");
    }
    Some(
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "ts" | "mts" | "cts" => "typescript",
            "tsx" => "tsx",
            "ets" => "arkts",
            "js" | "mjs" | "cjs" | "xsjs" | "xsjslib" => "javascript",
            "jsx" => "jsx",
            "py" | "pyw" => "python",
            "go" => "go",
            "rs" => "rust",
            "java" => "java",
            "c" | "h" => "c",
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "cu" | "cuh" | "metal" => "cpp",
            "cs" => "csharp",
            "php" | "module" | "install" | "theme" | "inc" => "php",
            "rb" | "rake" | "gemspec" | "ru" => "ruby",
            "swift" => "swift",
            "kt" | "kts" => "kotlin",
            "dart" => "dart",
            "scala" | "sc" => "scala",
            "lua" => "lua",
            "luau" => "luau",
            "r" => "r",
            "liquid" => "liquid",
            "xml" => "xml",
            "yaml" | "yml" => "yaml",
            "twig" => "twig",
            "properties" => "properties",
            "routes" => "yaml",
            "svelte" => "svelte",
            "vue" => "vue",
            "astro" => "astro",
            "cshtml" | "razor" => "razor",
            "pas" | "dpr" | "dpk" | "lpr" | "dfm" | "fmx" => "pascal",
            "m" | "mm" => "objc",
            "sol" => "solidity",
            "nix" => "nix",
            "cfc" | "cfm" => "cfml",
            "cfs" => "cfscript",
            "cbl" | "cob" | "cobol" | "cpy" => "cobol",
            "vb" => "vbnet",
            "erl" | "hrl" | "escript" => "erlang",
            "tf" | "tfvars" | "tofu" => "terraform",
            _ => return None,
        },
    )
}

pub(super) fn extract(path: &Path, source: &str) -> Result<ExtractionResult> {
    let language = detect_language(path, source, None)?;
    extract_as(path, source, language)
}

pub(super) fn extract_as(path: &Path, source: &str, language: &str) -> Result<ExtractionResult> {
    if matches!(language, "cfml" | "cfscript" | "cfquery") {
        return super::cfml::extract(path, source, language);
    }
    if language == "razor" {
        return super::razor::extract(path, source);
    }
    if matches!(language, "svelte" | "vue" | "astro") {
        return super::components::extract(path, source, language);
    }
    if matches!(language, "liquid" | "xml" | "yaml" | "twig" | "properties") {
        return super::formats::extract(path, source, language);
    }
    ensure!(
        prometeu_extract::LANGUAGES.contains(&language),
        "unsupported language: {language}"
    );
    let path_text = path
        .to_str()
        .context("source path must be UTF-8")?
        .replace(std::path::MAIN_SEPARATOR, "/");
    let prepared = preprocess::prepare(path, source, language)?;
    let mut result = match prometeu_extract::extract(&path_text, &prepared, language) {
        Ok(result) => result,
        Err(error) if error.starts_with("defer:") => {
            prometeu_extract::extract_recovering(&path_text, &prepared, language)
                .map_err(|error| anyhow!(error))?
        }
        Err(error) => bail!(error),
    };
    preprocess::finish(path, source, language, &mut result)?;
    Ok(result)
}

pub(super) fn parse(path: &Path, source: &str) -> Result<ParsedFile> {
    let language = detect_language(path, source, None)?;
    parse_as(path, source, language)
}

pub(super) fn parse_as(path: &Path, source: &str, language: &str) -> Result<ParsedFile> {
    parse_with_additions(path, source, language, &[], &[])
}

pub(crate) fn parse_with_additions(
    path: &Path,
    source: &str,
    language: &str,
    nodes: &[prometeu_extract::Node],
    references: &[prometeu_extract::UnresolvedReference],
) -> Result<ParsedFile> {
    let (mut extraction, mut columns) = if matches!(language, "vue" | "svelte" | "astro") {
        let (result, columns) = super::components::extract_projected(path, source, language)?;
        (result, Some(columns))
    } else if matches!(language, "cfml" | "cfscript" | "cfquery") {
        let (result, columns) = super::cfml::extract_projected(path, source, language)?;
        (result, Some(columns))
    } else {
        (extract_as(path, source, language)?, None)
    };
    let original_count = extraction.nodes.len();
    extraction.nodes.extend_from_slice(nodes);
    extraction
        .unresolved_references
        .extend_from_slice(references);
    if let Some(columns) = &mut columns {
        columns.extend(
            extraction.nodes[original_count..]
                .iter()
                .map(|node| (node.start_column, node.end_column)),
        );
    }
    let offsets = Offsets::new(source);
    let ids: HashMap<_, _> = extraction
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let parents: HashMap<_, _> = extraction
        .edges
        .iter()
        .filter(|e| e.kind == "contains")
        .filter_map(|e| Some((*ids.get(e.target.as_str())?, *ids.get(e.source.as_str())?)))
        .collect();
    let mut symbols = Vec::with_capacity(extraction.nodes.len());
    let free_cobol = language == "cobol" && prometeu_extract::cobol::is_free_format(source);
    for (index, node) in extraction.nodes.iter().enumerate() {
        let (mut start_column, mut end_column) = columns
            .as_ref()
            .map_or((node.start_column, node.end_column), |columns| {
                columns[index]
            });
        if free_cobol {
            start_column = start_column.saturating_sub(7);
            end_column = end_column.saturating_sub(7);
        }
        let inclusive_lines = matches!(language, "liquid" | "xml") && end_column == 0
            || language == "dart" && matches!(node.kind.as_str(), "function" | "method");
        let (start_byte, end_byte) = if node.kind == "file" {
            (0, source.len())
        } else {
            (
                offsets
                    .byte(node.start_line, start_column)
                    .with_context(|| {
                        format!(
                            "{} {} starts at {}:{}",
                            path.display(),
                            node.name,
                            node.start_line,
                            start_column
                        )
                    })?,
                if inclusive_lines {
                    offsets.line_end(node.end_line)?
                } else if language == "vbnet"
                    && !source.ends_with('\n')
                    && node.end_line as usize == offsets.lines.len() + 1
                    && end_column == 0
                {
                    source.len()
                } else {
                    offsets
                        .end_byte(node.end_line, end_column)
                        .with_context(|| {
                            format!(
                                "{} {} ends at {}:{}",
                                path.display(),
                                node.name,
                                node.end_line,
                                end_column
                            )
                        })?
                },
            )
        };
        ensure!(
            end_byte >= start_byte,
            "invalid extraction extent for {}",
            node.name
        );
        let container = parents
            .get(&index)
            .map(|&parent| &extraction.nodes[parent])
            .filter(|parent| parent.kind != "file")
            .map(|parent| parent.qualified_name.clone());
        let exported = node.is_exported == Some(true)
            || node.visibility.as_deref() == Some("public")
            || (node.is_exported.is_none()
                && !matches!(node.kind.as_str(), "file" | "import" | "parameter"));
        symbols.push(Symbol {
            name: node.name.clone(),
            kind: node.kind.clone(),
            start_line: node.start_line as usize,
            end_line: if end_column == 0 && !inclusive_lines {
                (node.end_line as usize)
                    .saturating_sub(1)
                    .max(node.start_line as usize)
            } else {
                node.end_line as usize
            },
            start_byte,
            end_byte,
            qualified_name: node.qualified_name.clone(),
            container,
            exports: if exported {
                vec![node.name.clone()]
            } else {
                Vec::new()
            },
        });
    }
    let mut parsed = ParsedFile {
        language: language.into(),
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        has_errors: !extraction.errors.is_empty(),
        extraction: None,
    };

    // Preserve existing scope/receiver evidence while the upstream resolver is ported.
    // The complete canonical extraction remains separate and is never rewritten here.
    let legacy = super::language(path)
        .filter(|(name, _)| *name == language || (*name == "javascript" && language == "jsx"))
        .map(|_| super::parse_legacy(path, source))
        .transpose()?;
    let had_legacy = legacy.is_some();
    if let Some(legacy) = legacy {
        let mut candidates: HashMap<(&str, usize), Vec<usize>> = HashMap::new();
        for (i, node) in extraction.nodes.iter().enumerate() {
            candidates
                .entry((&node.name, node.start_line as usize))
                .or_default()
                .push(i);
        }
        let remap: Vec<_> = legacy
            .symbols
            .iter()
            .map(|symbol| {
                let found = candidates
                    .get(&(symbol.name.as_str(), symbol.start_line))
                    .and_then(|indices| {
                        indices
                            .iter()
                            .copied()
                            .find(|&i| extraction.nodes[i].qualified_name == symbol.qualified_name)
                            .or_else(|| indices.first().copied())
                    });
                Some(found.unwrap_or_else(|| {
                    // Retain host-visible declarations (such as anonymous defaults
                    // and closure bindings) that the upstream extractor omits.
                    let index = parsed.symbols.len();
                    parsed.symbols.push(symbol.clone());
                    index
                }))
            })
            .collect();
        for (old, new) in legacy.symbols.iter().zip(&remap) {
            if let Some(index) = new {
                let symbol = &mut parsed.symbols[*index];
                symbol.container = old.container.clone();
                symbol.qualified_name = old.qualified_name.clone();
                symbol.exports = old.exports.clone();
            }
        }
        parsed.imports = legacy.imports;
        parsed.has_errors |= legacy.has_errors;
        for mut reference in legacy.references {
            reference.caller = reference.caller.and_then(|i| remap[i]);
            parsed.references.push(reference);
        }
    }
    for reference in &extraction.unresolved_references {
        // Existing resolution evidence includes local-binding exclusions absent
        // from raw name references. Keep those exclusions until the full resolver
        // consumes canonical records directly; never reintroduce shadowed calls.
        if had_legacy
            && ids
                .get(reference.from_node_id.as_str())
                .is_none_or(|&index| index < original_count)
        {
            continue;
        }
        if reference.reference_kind == "imports" {
            continue;
        }
        let normalized = reference
            .reference_name
            .replace("->", "::")
            .replace('.', "::");
        let (qualifier, target) = normalized
            .rsplit_once("::")
            .map_or((None, normalized.as_str()), |(q, t)| {
                (Some(q.to_owned()), t)
            });
        parsed.references.push(Reference {
            column: Some(reference.column as usize),
            caller: ids.get(reference.from_node_id.as_str()).copied(),
            target: target.into(),
            line: reference.line as usize,
            qualifier,
            kind: reference.reference_kind.clone(),
            receiver_type: None,
            shadowed: false,
            owner: None,
        });
    }
    if parsed.imports.is_empty() {
        for node in extraction.nodes.iter().filter(|n| n.kind == "import") {
            parsed.imports.extend(imports(node, language)?);
        }
    }
    if !parsed.symbols.iter().any(|symbol| symbol.kind == "file") {
        parsed.symbols.push(super::file_symbol(path, source));
    }
    parsed.extraction = Some(extraction);
    recover_owned_call_sites(path, source, &mut parsed)?;
    Ok(parsed)
}

/// Recover older projected call sites from committed source, then retain
/// the canonical owner without discarding import or shadowing evidence.
pub(crate) fn recover_owned_call_sites(
    path: &Path,
    source: &str,
    parsed: &mut ParsedFile,
) -> Result<()> {
    if !matches!(
        parsed.language.as_str(),
        "typescript" | "tsx" | "javascript" | "jsx"
    ) {
        return Ok(());
    }
    let Some(extraction) = &parsed.extraction else {
        return Ok(());
    };
    let projected = parsed
        .references
        .iter()
        .filter(|reference| reference.kind == "instantiates")
        .filter_map(|reference| {
            Some((reference.line, reference.column?, reference.target.as_str()))
        })
        .collect::<std::collections::HashSet<_>>();
    let missing = extraction
        .unresolved_references
        .iter()
        .filter(|raw| raw.reference_kind == "instantiates")
        .filter(|raw| {
            !projected.contains(&(
                raw.line as usize,
                raw.column as usize,
                raw.reference_name.rsplit('.').next().unwrap_or(""),
            ))
        })
        .map(|raw| {
            (
                raw.line as usize,
                raw.reference_name.rsplit('.').next().unwrap_or(""),
            )
        })
        .collect::<std::collections::HashSet<_>>();
    if parsed.references.iter().any(|reference| {
        reference.kind == "calls"
            && reference.column.is_none()
            && (reference.qualifier.as_deref() == Some("this")
                || missing.contains(&(reference.line, reference.target.as_str())))
    }) && super::language(path).is_some()
    {
        let recovered = super::parse_legacy(path, source)?;
        for (reference, recovered) in parsed.references.iter_mut().zip(recovered.references) {
            if (recovered.kind == "instantiates" || recovered.qualifier.as_deref() == Some("this"))
                && reference.kind == "calls"
                && reference.column.is_none()
                && reference.target == recovered.target
                && reference.line == recovered.line
                && reference.qualifier == recovered.qualifier
            {
                reference.kind = recovered.kind;
                reference.column = recovered.column;
                reference.shadowed = recovered.shadowed;
            }
        }
    }
    let ids = extraction
        .nodes
        .iter()
        .enumerate()
        .map(|(ordinal, node)| (node.id.as_str(), ordinal))
        .collect::<HashMap<_, _>>();
    let sites = extraction
        .unresolved_references
        .iter()
        .filter(|raw| matches!(raw.reference_kind.as_str(), "calls" | "instantiates"))
        .map(|raw| {
            (
                (
                    raw.line as usize,
                    raw.column as usize,
                    raw.reference_kind.as_str(),
                    raw.reference_name.rsplit('.').next().unwrap_or(""),
                ),
                raw,
            )
        })
        .collect::<HashMap<_, _>>();
    for reference in &mut parsed.references {
        if (reference.kind == "instantiates" || reference.qualifier.as_deref() == Some("this"))
            && let Some(column) = reference.column
            && let Some(raw) = sites.get(&(
                reference.line,
                column,
                reference.kind.as_str(),
                reference.target.as_str(),
            ))
        {
            reference.caller = ids.get(raw.from_node_id.as_str()).copied();
        }
    }
    Ok(())
}

pub(super) fn detect_language(
    path: &Path,
    source: &str,
    extension_override: Option<&str>,
) -> Result<&'static str> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let special = name.to_ascii_lowercase().ends_with(".app")
        || name.to_ascii_lowercase().ends_with(".app.src")
        || (name == "routes"
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "conf"))
        || path
            .extension()
            .is_some_and(|extension| extension == "routes");
    let language = if special {
        language(path)
    } else if let Some(override_language) = extension_override {
        Some(
            *super::LANGUAGES
                .iter()
                .find(|&&language| language == override_language)
                .with_context(|| format!("unsupported language override: {override_language}"))?,
        )
    } else {
        language(path)
    }
    .with_context(|| format!("unsupported language: {}", path.display()))?;
    if matches!(language, "javascript" | "jsx") && preprocess::has_flow_pragma(source)? {
        return Ok("tsx");
    }
    if language == "c"
        && path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("h"))
    {
        // Same header signals as upstream grammars.ts; the base-clause scan covers
        // the whole source because generated C-compatible preambles can be long.
        static CPP: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"\bnamespace\b|\bclass\s+\w+\s*[:{]|\b(?:class|struct)\s+[A-Z][A-Z0-9_]+\s+\w+\s*(?:final\s*)?[:{]|\btemplate\s*<|\b(?:public|private|protected)\s*:|\bvirtual\b|\busing\s+(?:namespace\b|\w+\s*=)").unwrap()
        });
        static BASE: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"\b(?:class|struct)\s+\w+\s*(?:final\s*)?:\s*(?:(?:public|protected|private|virtual)\s+)*[A-Za-z_][\w:]*(?:\s*<[^{};]*>)?\s*[{,]").unwrap()
        });
        static COMMENTS: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r"/\*[\s\S]*?\*/|//[^\n]*").unwrap());
        let end = source
            .char_indices()
            .scan(0, |units, (at, ch)| {
                *units += ch.len_utf16();
                Some((at, *units))
            })
            .find(|(_, units)| *units > 8192)
            .map_or(source.len(), |(at, _)| at);
        if CPP.is_match(&source[..end]) || BASE.is_match(&COMMENTS.replace_all(source, " ")) {
            return Ok("cpp");
        }
        static OBJC: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"@(?:interface|implementation|protocol|synthesize)\b").unwrap()
        });
        if OBJC.is_match(&source[..end]) {
            return Ok("objc");
        }
    }
    if language == "php"
        && extension_override.is_none()
        && path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("inc"))
        && pascal_include(source)?
    {
        return Ok("pascal");
    }
    Ok(language)
}

fn pascal_include(source: &str) -> Result<bool> {
    static PHP: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)<\?(?:php|=)").unwrap());
    if PHP.is_match(source) {
        return Ok(false);
    }
    static SIGNALS: LazyLock<Vec<fancy_regex::Regex>> = LazyLock::new(|| {
        let name = r"[a-z_]\w*(?:<[^>\n]*>)?";
        let qualified = r"[a-z_][\w.]*(?:<[^>\n]*>)?";
        let comment = r"(?://[^\n]*|\{[^$}\n][^}\n]*\}[ \t]*)?";
        let section = format!(r"[ \t]*{comment}(?:\r?\n[ \t]*{comment})+");
        [
            r"\{(?:\$(?:[a-z]\w*[ \t]+[^\s}]|(?:else|endif|ifend)[ \t]*\}|[a-z][+-][,}])|%MainUnit\b)".into(),
            format!(r"(?:class[ \t]+)?(?:(?:procedure|constructor|destructor)[ \t]+{qualified}[ \t]*(?:\([^()]*\)[ \t]*)?|function[ \t]+{qualified}[ \t]*(?:(?:\([^()]*\)[ \t]*)?:[ \t]*[\w.]+(?:<[^>\n]*>)?[ \t]*)?);"),
            r"unit[ \t]+[a-z_][\w.]*[ \t]*;".into(),
            r"uses\s+[a-z_][\w.]*(?:\s*,\s*[a-z_][\w.]*)*\s*;".into(),
            format!(r"(?:const|resourcestring)(?:{section}[a-z_]\w*[ \t]*(?::[^=;:\n]+)?=|[ \t]+[a-z_]\w*[ \t]*:[^=;:\n]+=)"),
            format!(r"(?:var|threadvar)(?:{section}|[ \t]+)[a-z_]\w*(?:[ \t]*,[ \t]*[a-z_]\w*)*[ \t]*:(?!:)[^;\n]*;"),
            format!(r"type(?:{section}{name}[ \t]*=|[ \t]+{name}[ \t]*=[ \t]*(?:packed[ \t]+)?(?:class|record|object|interface|dispinterface|set[ \t]+of|array|reference[ \t]+to|procedure|function)\b)"),
        ].into_iter().map(|pattern| fancy_regex::Regex::new(&super::javascript_regex::pattern(&format!(r"(?im)^[ \t\u{{FEFF}}]*{pattern}"))).unwrap()).collect()
    });
    for signal in SIGNALS.iter() {
        if signal.is_match(source)? {
            return Ok(true);
        }
    }
    static BEGIN: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?im)^[ \t\u{FEFF}]*begin\b").unwrap());
    static END: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?im)^[ \t\u{FEFF}]*end[ \t]*[;.][ \t]*$").unwrap());
    Ok(BEGIN.is_match(source) && END.is_match(source))
}

fn imports(node: &prometeu_extract::Node, language: &str) -> Result<Vec<Import>> {
    if matches!(
        language,
        "typescript" | "tsx" | "javascript" | "jsx" | "arkts" | "vue" | "svelte" | "astro"
    ) && let Some(signature) = node.signature.as_deref()
    {
        return Ok(super::parse_legacy(Path::new("import.ts"), signature)?.imports);
    }
    let mut result = Vec::new();
    let mut add = |local: &str, imported: &str, module: &str| {
        result.push(Import {
            local: local.into(),
            imported: imported.into(),
            module: module.into(),
            scope: String::new(),
            reexport: false,
        })
    };
    let signature = node.signature.as_deref().unwrap_or("").trim();
    if language == "python" {
        if let Some((module, names)) = signature
            .strip_prefix("from ")
            .and_then(|s| s.split_once(" import "))
        {
            for name in names
                .trim_matches(['(', ')'])
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let (imported, local) = name
                    .split_once(" as ")
                    .map_or((name, name), |(i, l)| (i.trim(), l.trim()));
                add(local, imported, module);
            }
        } else if let Some(names) = signature.strip_prefix("import ") {
            for name in names.split(',').map(str::trim) {
                let (module, local) = name
                    .split_once(" as ")
                    .map_or((name, name.split('.').next().unwrap_or(name)), |(m, l)| {
                        (m.trim(), l.trim())
                    });
                if module == node.name {
                    add(local, "*", module);
                }
            }
        }
    } else {
        let local = node
            .name
            .rsplit(['.', '/', ':'])
            .find(|s| !s.is_empty())
            .unwrap_or(&node.name);
        add(local, "*", &node.name);
    }
    Ok(result)
}

struct Offsets<'a> {
    source: &'a str,
    /// Byte and UTF-16 offsets for each line start.
    lines: Vec<(usize, usize)>,
    /// Non-ASCII input needs a UTF-16-to-byte map. ASCII columns are byte offsets.
    utf16: Option<Vec<usize>>,
}
impl<'a> Offsets<'a> {
    fn new(source: &'a str) -> Self {
        let mut lines = vec![(0, 0)];
        let utf16 = if source.is_ascii() {
            lines.extend(source.match_indices('\n').map(|(at, _)| (at + 1, at + 1)));
            None
        } else {
            let mut offsets = Vec::with_capacity(source.len() + 1);
            for (at, ch) in source.char_indices() {
                offsets.push(at);
                if ch.len_utf16() == 2 {
                    offsets.push(usize::MAX);
                }
                if ch == '\n' {
                    lines.push((at + 1, offsets.len()));
                }
            }
            offsets.push(source.len());
            Some(offsets)
        };
        Self {
            source,
            lines,
            utf16,
        }
    }
    fn byte(&self, line: u32, column: u32) -> Result<usize> {
        let index = line.checked_sub(1).context("zero extraction line")? as usize;
        let &(start, units) = self
            .lines
            .get(index)
            .context("extraction line is outside source")?;
        let end_units = self.lines.get(index + 1).map_or_else(
            || {
                self.utf16
                    .as_ref()
                    .map_or(self.source.len(), |map| map.len() - 1)
            },
            |(_, end)| end - 1,
        );
        ensure!(
            column as usize <= end_units - units,
            "extraction column is outside source line"
        );
        let offset = self
            .utf16
            .as_ref()
            .map_or(start + column as usize, |map| map[units + column as usize]);
        ensure!(
            offset != usize::MAX,
            "extraction column splits a Unicode character"
        );
        Ok(offset)
    }
    fn end_byte(&self, line: u32, column: u32) -> Result<usize> {
        // Some upstream walkers extend endLine over the body but retain the
        // signature's endColumn. Keep that metadata intact; source projection
        // uses the ending line when the column cannot address that line.
        match self.byte(line, column) {
            Ok(offset) => Ok(offset),
            Err(_) => self.line_end(line),
        }
    }
    fn line_end(&self, line: u32) -> Result<usize> {
        let index = line.checked_sub(1).context("zero extraction line")? as usize;
        ensure!(
            index < self.lines.len(),
            "extraction line is outside source"
        );
        Ok(self
            .lines
            .get(index + 1)
            .map_or(self.source.len(), |(byte, _)| byte - 1))
    }
}
