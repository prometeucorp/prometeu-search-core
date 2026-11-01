use super::*;
use prometeu_extract::ExtractionResult;
const BUILTINS: &[&str] = &[
    "addListener",
    "removeListeners",
    "remove",
    "invalidate",
    "startObserving",
    "stopObserving",
];
struct Export {
    module: String,
    class: Option<String>,
    name: String,
    native: String,
    line: u32,
}
fn exports(source: &str, external: bool) -> Result<Vec<Export>> {
    let (module, class) = if external {
        let remap=regex(r"RCT_EXTERN_REMAP_MODULE\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*,\s*([A-Za-z_][A-Za-z0-9_]*)")?.captures(source)?;
        let plain =
            regex(r"RCT_EXTERN_MODULE\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)")?.captures(source)?;
        (
            remap
                .as_ref()
                .map(|m| m[1].to_owned())
                .or_else(|| plain.as_ref().map(|m| m[1].to_owned())),
            remap
                .as_ref()
                .map(|m| m[2].to_owned())
                .or_else(|| plain.as_ref().map(|m| m[1].to_owned())),
        )
    } else {
        let explicit = regex(r"RCT_EXPORT_MODULE\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)?\s*\)")?
            .captures(source)?
            .and_then(|m| m.get(1).map(|s| s.as_str().to_owned()));
        let class = regex(r"@implementation\s+([A-Za-z_][A-Za-z0-9_]*)")?
            .captures(source)?
            .map(|m| m[1].to_owned());
        (
            explicit.or_else(|| {
                class.as_deref().map(|c| {
                    c.strip_prefix("RCT")
                        .filter(|v| !v.is_empty())
                        .unwrap_or(c)
                        .to_owned()
                })
            }),
            class,
        )
    };
    let Some(module) = module else {
        return Ok(Vec::new());
    };
    let newlines: Vec<_> = source
        .match_indices('\n')
        .map(|(offset, _)| offset)
        .collect();
    let mut out = Vec::new();
    for remap in [false, true] {
        let name = match (external, remap) {
            (false, false) => "RCT_EXPORT_METHOD",
            (false, true) => "RCT_REMAP_METHOD",
            (true, false) => "RCT_EXTERN_METHOD",
            (true, true) => "RCT_EXTERN_REMAP_METHOD",
        };
        let pattern = format!(
            r"{name}\s*\(\s*([A-Za-z_][A-Za-z0-9_]*){}",
            if remap {
                r"\s*,\s*([A-Za-z_][A-Za-z0-9_]*)"
            } else {
                ""
            }
        );
        for m in regex(&pattern)?.captures_iter(source) {
            let m = m?;
            out.push(Export {
                module: module.clone(),
                class: class.clone(),
                name: m[1].to_owned(),
                native: m[if remap { 2 } else { 1 }].to_owned(),
                line: 1 + newlines.partition_point(|&offset| offset < m.get(0).unwrap().start())
                    as u32,
            });
        }
    }
    Ok(out)
}
pub(crate) fn extract(file: &str, source: &str) -> Result<ExtractionResult> {
    let mut result = ExtractionResult {
        nodes: Vec::new(),
        edges: Vec::new(),
        unresolved_references: Vec::new(),
        errors: Vec::new(),
        duration_ms: 0.0,
    };
    if !(file.ends_with(".m") || file.ends_with(".mm"))
        || !regex(r"RCT_EXPORT_MODULE\b")?.is_match(source)?
    {
        return Ok(result);
    }
    let mut seen = HashSet::new();
    for e in exports(source, false)? {
        if !seen.insert(e.name.clone()) {
            continue;
        }
        result.nodes.push(serde_json::from_value(json!({"id":format!("rn-export:{file}:{}.{}",e.module,e.name),"kind":"method","name":e.name,"qualifiedName":format!("{file}::{}.{}",e.module,e.name),"filePath":file,"language":"objc","startLine":e.line,"endLine":e.line,"startColumn":0,"endColumn":0,"isExported":true,"docstring":format!("RCT_EXPORT_METHOD {} (module {})",e.native,e.module),"signature":format!("RCT_EXPORT_METHOD({}:…)",e.native),"updatedAt":0}))?);
    }
    Ok(result)
}
struct Method<'a> {
    module: String,
    node: &'a Node,
}
pub(super) struct Maps<'a> {
    methods: HashMap<String, Vec<Method<'a>>>,
    aliases: HashMap<String, String>,
}
fn alias(
    source: &str,
    aliases: &mut HashMap<String, String>,
    ambiguous: &mut HashSet<String>,
) -> Result<()> {
    let mut bind = |name: &str, module: &str| {
        if ambiguous.contains(name) {
            return;
        }
        if aliases.get(name).is_some_and(|prior| prior != module) {
            aliases.remove(name);
            ambiguous.insert(name.to_owned());
        } else {
            aliases.insert(name.to_owned(), module.to_owned());
        }
    };
    for m in regex(
        r"\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=;]+)?=\s*NativeModules\.([A-Z][\w$]*)",
    )?
    .captures_iter(source)
    {
        let m = m?;
        bind(&m[1], &m[2]);
    }
    for m in
        regex(r"\b(?:const|let|var)\s*\{([^}]+)\}\s*=\s*NativeModules\b")?.captures_iter(source)
    {
        let m = m?;
        for part in m[1].split(',') {
            if let Some(m) = regex(r"^([A-Za-z_$][\w$]*)(?:\s*:\s*([A-Za-z_$][\w$]*))?$")?
                .captures(part.trim())?
            {
                bind(m.get(2).map_or(&m[1], |v| v.as_str()), &m[1]);
            }
        }
    }
    Ok(())
}
pub(super) fn build<'a>(ctx: &Context<'a>) -> Result<Maps<'a>> {
    let mut maps = Maps {
        methods: HashMap::new(),
        aliases: HashMap::new(),
    };
    let mut ambiguous = HashSet::new();
    let mut objc: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut jvm: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut swift: HashMap<&str, Vec<&Node>> = HashMap::new();
    for node in ctx.nodes.iter().filter(|n| n.kind == "method") {
        match node.language.as_str() {
            "objc" => {
                objc.entry(node.name.split(':').next().unwrap_or(""))
                    .or_default()
                    .push(node);
            }
            "java" | "kotlin" => jvm.entry(&node.name).or_default().push(node),
            "swift" => swift.entry(&node.name).or_default().push(node),
            _ => {}
        }
    }
    let mut add = |name: &str, module: &str, node: &'a Node| {
        if !BUILTINS.contains(&name) {
            maps.methods
                .entry(name.to_owned())
                .or_default()
                .push(Method {
                    module: module.to_owned(),
                    node,
                });
        }
    };
    for (&file, &source) in ctx.sources {
        ctx.step(source.len() / 32 + 1)?;
        if file.ends_with(".m") || file.ends_with(".mm") {
            for e in exports(source, false)? {
                if let Some(nodes) = objc.get(e.native.as_str()) {
                    ctx.step(nodes.len())?;
                    if let Some(node) = nodes
                        .iter()
                        .find(|node| node.file_path == file)
                        .or_else(|| nodes.first())
                    {
                        add(&e.name, &e.module, node);
                    }
                }
            }

            if regex(r"RCT_EXTERN_(?:REMAP_)?MODULE\b")?.is_match(source)? {
                for e in exports(source, true)? {
                    if let Some(nodes) = swift.get(e.native.as_str()) {
                        ctx.step(nodes.len())?;
                        let mut candidates: Vec<_> = nodes
                            .iter()
                            .filter(|n| {
                                n.qualified_name
                                    .split("::")
                                    .any(|q| Some(q) == e.class.as_deref())
                            })
                            .copied()
                            .collect();
                        candidates.sort_by(|a, b| {
                            a.file_path
                                .encode_utf16()
                                .cmp(b.file_path.encode_utf16())
                                .then(a.start_line.cmp(&b.start_line))
                        });
                        if let Some(node) = candidates.first() {
                            add(&e.name, &e.module, node);
                        }
                    }
                }
            }
        }
        if source.contains("NativeModules") && regex(r"\.(?:[cm]?[jt]sx?)$")?.is_match(file)? {
            alias(source, &mut maps.aliases, &mut ambiguous)?;
        }
        if file.ends_with(".java") || file.ends_with(".kt") {
            let get = regex(
                r#"\bgetName\s*\([^)]*\)\s*(?::\s*String)?\s*(?:=\s*|\{[^}]*return\s*)"([^"]+)"#,
            )?
            .captures(source)?
            .map(|m| m[1].to_owned());
            let class =
                regex(r"\bclass\s+([A-Za-z_][A-Za-z0-9_]*)\b[^{]*ReactContextBaseJavaModule")?
                    .captures(source)?
                    .or(
                        regex(r"\bclass\s+([A-Za-z_][A-Za-z0-9_]*)\b[^{]*ReactPackage")?
                            .captures(source)?,
                    )
                    .map(|m| m[1].strip_suffix("Module").unwrap_or(&m[1]).to_owned());
            if let Some(module) = get.or(class) {
                for m in regex(r"@ReactMethod\b[^{]*?(?:\bfun\s+|\bvoid\s+|\bpublic\s+\w[\w<>\[\]]*\s+)([A-Za-z_][A-Za-z0-9_]*)\s*\(")?.captures_iter(source){let m=m?;if let Some(nodes)=jvm.get(&m[1]){ctx.step(nodes.len())?;if let Some(node)=nodes.iter().find(|n|n.file_path==file).or_else(||nodes.first()){add(&m[1],&module,node);}}}
            }
        }
        if (file.ends_with(".ts")||file.ends_with(".tsx"))&&let Some(module)=regex(r#"TurboModuleRegistry\.(?:getEnforcing|get)\s*<[^>]*>\s*\(\s*['"]([^'"]+)['"]\s*\)"#)?.captures(source)?&&let Some(iface)=regex(r"export\s+interface\s+Spec\b[^{]*\{([\s\S]*?)\n\}")?.captures(source)?{for m in regex(r"(?m)^\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(")?.captures_iter(&iface[1]){let m=m?;for node in objc.get(&m[1]).into_iter().flatten().chain(jvm.get(&m[1]).into_iter().flatten()){ctx.step(1)?;add(&m[1],&module[1],node);}}}
    }
    Ok(maps)
}
pub(super) fn resolve(
    ctx: &Context<'_>,
    maps: &Maps<'_>,
    reference: &UnresolvedReference,
) -> Result<Option<Binding>> {
    if !matches!(
        reference.language.as_deref(),
        Some("javascript" | "typescript" | "tsx" | "jsx")
    ) {
        return Ok(None);
    }
    let raw = &reference.reference_name;
    let (receiver, name) = raw
        .rsplit_once('.')
        .map_or((None, raw.as_str()), |(r, n)| (Some(r), n));
    let Some(entries) = maps.methods.get(name) else {
        return Ok(None);
    };
    ctx.step(entries.len())?;
    let module = if let Some(receiver) = receiver {
        regex(r"^NativeModules\.([A-Z][\w$]*)$")?
            .captures(receiver)?
            .map(|m| m[1].to_owned())
            .or_else(|| maps.aliases.get(receiver).cloned())
    } else {
        None
    };
    let entries: Vec<_> = entries
        .iter()
        .filter(|e| module.as_ref().is_none_or(|m| m == &e.module))
        .collect();
    let picked = entries
        .iter()
        .find(|e| e.node.language == "objc")
        .or_else(|| entries.iter().find(|e| e.node.language == "swift"))
        .or_else(|| entries.first());
    Ok(picked.map(|e| {
        let mut result = binding(e.node, if module.is_some() { 0.95 } else { 0.6 });
        result.metadata = Some(json!({"bridge":"react-native","module":e.module}));
        result
    }))
}
