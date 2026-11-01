use super::*;
const TYPES: &[&str] = &[
    "class",
    "struct",
    "interface",
    "enum",
    "trait",
    "protocol",
    "record",
];
fn lexical(ctx: &Context<'_>, node: &Node, r: &UnresolvedReference) -> Result<bool> {
    if matches!(
        node.kind.as_str(),
        "class" | "struct" | "interface" | "enum" | "trait" | "type_alias"
    ) && !matches!(node.language.as_str(), "c" | "cpp")
    {
        let mut qn = node.qualified_name.as_str();
        while let Some((parent, _)) = qn.rsplit_once("::") {
            qn = parent;
            ctx.step(ctx.qualified.get(parent).map_or(0, Vec::len))?;
            if let Some(scope) = ctx.qualified.get(parent).into_iter().flatten().find(|p| {
                p.file_path == node.file_path
                    && matches!(p.kind.as_str(), "function" | "method")
                    && p.start_line <= node.start_line
                    && p.end_line >= node.end_line
            }) {
                if r.file_path.as_deref() != Some(node.file_path.as_str())
                    || r.line < scope.start_line
                    || r.line > scope.end_line
                {
                    return Ok(false);
                }
                break;
            }
        }
    }
    Ok(true)
}
fn reachable(ctx: &Context<'_>, node: &Node, r: &UnresolvedReference) -> Result<bool> {
    if !lexical(ctx, node, r)? {
        return Ok(false);
    }
    if r.file_path.as_deref() == Some(node.file_path.as_str()) {
        return Ok(true);
    }
    if let Some((parent, _)) = node.qualified_name.rsplit_once("::") {
        ctx.step(ctx.by_file.get(node.file_path.as_str()).map_or(0, Vec::len))?;
        if ctx
            .in_file(&node.file_path)
            .any(|n| n.qualified_name == parent && TYPES.contains(&n.kind.as_str()))
        {
            return Ok(false);
        }
    }
    if node.language == "objc"
        && matches!(
            node.kind.as_str(),
            "struct" | "union" | "enum" | "type_alias" | "variable" | "constant"
        )
        && regex(r"(?i)\.(c|cc|cpp|cxx|c\+\+|m|mm)$")?.is_match(&node.file_path)?
    {
        return Ok(false);
    }
    if crate::classify::is_test_suite_path(&node.file_path)?
        && !crate::classify::is_test_path(r.file_path.as_deref().unwrap_or(""))
    {
        return Ok(false);
    }
    if matches!(
        node.language.as_str(),
        "swift" | "kotlin" | "java" | "csharp" | "scala" | "php"
    ) && node.visibility.as_deref() == Some("private")
    {
        return Ok(false);
    }
    Ok(true)
}
pub(super) fn pick<'a>(
    ctx: &Context<'a>,
    r: &UnresolvedReference,
    kinds: &[&str],
    dirs: &[&str],
) -> Result<Option<&'a Node>> {
    let mut candidates = Vec::new();
    for node in ctx.named(&r.reference_name) {
        ctx.step(1)?;
        if kinds.contains(&node.kind.as_str())
            && !(matches!(r.reference_kind.as_str(), "extends" | "implements")
                && r.from_node_id == node.id)
            && reachable(ctx, node, r)?
        {
            candidates.push(node)
        }
    }
    let file = r.file_path.as_deref().unwrap_or("");
    let dir = file.rfind('/').map_or("", |cut| &file[..cut + 1]);
    Ok(candidates
        .iter()
        .find(|n| n.file_path == file)
        .or_else(|| {
            candidates.iter().find(|n| {
                n.file_path
                    .strip_prefix(dir)
                    .is_some_and(|tail| !tail.contains('/'))
            })
        })
        .or_else(|| {
            candidates
                .iter()
                .find(|n| dirs.iter().any(|dir| n.file_path.contains(dir)))
        })
        .or_else(|| candidates.first())
        .copied())
}
pub(super) fn resolve(
    ctx: &Context<'_>,
    framework: &str,
    r: &UnresolvedReference,
) -> Result<Option<Binding>> {
    if r.language.as_deref() != Some("swift") {
        return Ok(None);
    }
    let name = &r.reference_name;
    let patterns: &[(bool, &[&str], &[&str], f64)] = if framework == "swiftui" {
        &[
            (
                name.ends_with("View")
                    && name.as_bytes().first().is_some_and(u8::is_ascii_uppercase),
                &["struct"],
                &["/Views/", "/View/", "/Screens/", "/Components/", "/UI/"],
                0.85,
            ),
            (
                name.ends_with("ViewModel") || name.ends_with("Store") || name.ends_with("Manager"),
                &["class"],
                &[
                    "/ViewModels/",
                    "/ViewModel/",
                    "/Stores/",
                    "/Managers/",
                    "/Services/",
                ],
                0.85,
            ),
            (
                regex(r"^[A-Z][a-zA-Z]+$")?.is_match(name)?,
                &["struct", "class"],
                &["/Models/", "/Model/", "/Entities/", "/Domain/"],
                0.7,
            ),
        ]
    } else {
        &[
            (
                name.ends_with("ViewController"),
                &["class"],
                &[
                    "/ViewControllers/",
                    "/ViewController/",
                    "/Controllers/",
                    "/Screens/",
                ],
                0.85,
            ),
            (
                name.ends_with("View") && !name.ends_with("ViewController"),
                &["class"],
                &["/Views/", "/View/", "/UI/", "/Components/"],
                0.8,
            ),
            (
                name.ends_with("Cell"),
                &["class"],
                &[
                    "/Cells/",
                    "/Cell/",
                    "/Views/",
                    "/TableViewCells/",
                    "/CollectionViewCells/",
                ],
                0.85,
            ),
            (
                name.ends_with("Delegate") || name.ends_with("DataSource"),
                &["protocol"],
                &[],
                0.8,
            ),
        ]
    };
    for &(matches, kinds, dirs, confidence) in patterns {
        if matches && let Some(node) = pick(ctx, r, kinds, dirs)? {
            return Ok(Some(binding(node, confidence)));
        }
    }
    Ok(None)
}

// The .9 exact-name outcomes that can outrank Swift's .7–.85 framework
// conventions. Other kinds/languages require the general source/type matcher.
pub(super) fn exact_type(ctx: &Context<'_>, r: &UnresolvedReference) -> Result<Option<Binding>> {
    if r.language.as_deref() != Some("swift")
        || !regex(r"^[A-Za-z_]\w*$")?.is_match(&r.reference_name)?
    {
        return Ok(None);
    }
    let named: Vec<_> = ctx
        .named(&r.reference_name)
        .filter(|node| node.kind != "import")
        .collect();
    ctx.step(named.len())?;
    if named
        .iter()
        .any(|node| node.language != "swift" || !TYPES.contains(&node.kind.as_str()))
    {
        return Ok(None);
    }
    let file = r.file_path.as_deref().unwrap_or("");
    let line = ctx.line(file, r.line.saturating_sub(1) as usize);
    ctx.step(line.len() / 32 + 1)?;
    let call = r.reference_kind == "calls"
        && regex(&format!(
            r"(?<![\w$]){}\s*[({{<[]",
            regex::escape(&r.reference_name)
        ))?
        .is_match(line)?;
    let mut candidates = Vec::new();
    for node in named {
        if !lexical(ctx, node, r)? {
            continue;
        }
        if call {
            let segments: Vec<_> = node.file_path.split('/').collect();
            if let Some(index) = segments[..segments.len().saturating_sub(1)]
                .iter()
                .position(|segment| segment.ends_with("Tests") || segment.ends_with(".playground"))
                && !file.starts_with(&format!("{}/", segments[..=index].join("/")))
            {
                continue;
            }
        }
        candidates.push(node);
    }
    let selected = if candidates.len() == 1 {
        candidates.first().copied()
    } else if candidates.len() <= 500 {
        let own: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|node| node.file_path == file)
            .collect();
        if own.len() == 1 && own[0].id != r.from_node_id {
            let node = own[0];
            let decisive = if let Some((owner, _)) = node.qualified_name.rsplit_once("::") {
                let declared: Vec<_> = ctx
                    .in_file(file)
                    .filter(|node| node.qualified_name == owner)
                    .collect();
                ctx.step(ctx.by_file.get(file).map_or(0, Vec::len))?;
                declared
                    .iter()
                    .all(|node| matches!(node.kind.as_str(), "function" | "method"))
                    || ctx.by_id.get(r.from_node_id.as_str()).is_some_and(|from| {
                        from.qualified_name == owner
                            || from.qualified_name.starts_with(&format!("{owner}::"))
                    })
            } else {
                true
            };
            decisive.then_some(node)
        } else {
            None
        }
    } else {
        None
    };
    let Some(node) = selected else {
        return Ok(None);
    };
    if node.file_path != file
        && (node.visibility.as_deref() == Some("private")
            || crate::classify::is_test_suite_path(&node.file_path)?
                && !crate::classify::is_test_path(file))
    {
        return Ok(None);
    }
    Ok(Some(binding(node, 0.9)))
}
