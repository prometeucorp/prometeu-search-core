use super::*;

const INTERFACE_LANGUAGES: &[&str] = &[
    "java",
    "kotlin",
    "csharp",
    "typescript",
    "tsx",
    "javascript",
    "jsx",
    "swift",
    "scala",
    "go",
    "rust",
    "arkts",
];

pub(super) fn cpp(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["cpp"]) {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for class in ctx.nodes.iter().filter(|n| n.kind == "class") {
        ctx.step()?;
        let methods: Vec<_> = ctx
            .methods(&class.id)
            .into_iter()
            .filter(|n| n.language == "cpp")
            .collect();
        if methods.is_empty() {
            continue;
        }
        for edge in ctx.outgoing(&class.id).filter(|e| e.kind == "extends") {
            let Some(base) = ctx
                .node(&edge.target)
                .filter(|n| n.language == "cpp" && n.id != class.id)
            else {
                continue;
            };
            let base_methods: HashMap<_, _> = ctx
                .methods(&base.id)
                .into_iter()
                .filter(|n| !specialization(n, ctx))
                .map(|n| (n.name.as_str(), n))
                .collect();
            let mut added = 0;
            for method in &methods {
                ctx.step()?;
                if added >= CALLBACK_CAP {
                    break;
                }
                let Some(base_method) = base_methods.get(method.name.as_str()) else {
                    continue;
                };
                if method.id == base_method.id || specialization(method, ctx) {
                    continue;
                }
                if seen.insert((&base_method.id, &method.id)) {
                    ctx.emit(
                        &mut out,
                        call(base_method, method, "cpp-override", &method.name),
                    )?;
                    added += 1;
                }
            }
        }
    }
    Ok(out)
}

fn specialization(node: &Node, ctx: &Context<'_>) -> bool {
    if node.language != "cpp"
        || !matches!(node.kind.as_str(), "function" | "method")
        || node.signature.as_ref().is_some_and(|s| s.starts_with('('))
    {
        return false;
    }
    let head = node
        .signature
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| parameters::head(node, ctx, 12, true))
        .unwrap_or_default();
    for (at, _) in head.match_indices(&node.name) {
        let open = at + node.name.len();
        let word = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$');
        if at.checked_sub(1).is_some_and(|i| word(head.as_bytes()[i]))
            || head.as_bytes().get(open).is_some_and(|&b| word(b))
        {
            continue;
        }
        if head.as_bytes().get(open) != Some(&b'<') {
            return false;
        }
        return template_end(&head, open).is_some_and(|end| !head[open + 1..end].trim().is_empty());
    }
    false
}
fn template_end(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if open == 0
        || !(bytes[open - 1].is_ascii_alphanumeric() || bytes[open - 1] == b'_')
        || matches!(bytes.get(open + 1), Some(b'<' | b'='))
    {
        return None;
    }
    let mut angles = 0i32;
    let mut others = 0i32;
    let mut i = open;
    while i < bytes.len() {
        let c = bytes[i];
        if matches!(c, b'"' | b'\'') {
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
        } else if matches!(c, b'(' | b'[' | b'{') {
            others += 1;
        } else if matches!(c, b')' | b']' | b'}') {
            others -= 1;
            if others < 0 {
                return None;
            }
        } else if others == 0 {
            if c == b'<' {
                angles += 1;
            } else if c == b'>' && i > 0 && bytes[i - 1] != b'-' {
                angles -= 1;
                if angles == 0 {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

pub(super) fn interfaces(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut filter = parameters::Filter::new(ctx);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut methods_memo: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut base_memo: HashMap<&str, Vec<&Node>> = HashMap::new();
    let mut promotions = go::Promotions::new(ctx);
    let mut promoted = Vec::new();
    for kind in ["class", "struct", "union", "type_alias"] {
        for class in ctx
            .nodes
            .iter()
            .filter(|n| n.kind == kind && (kind != "type_alias" || n.language == "go"))
        {
            ctx.step()?;
            let supers: Vec<_> = ctx
                .outgoing(&class.id)
                .filter(|e| matches!(e.kind.as_str(), "implements" | "extends"))
                .collect();
            if supers.is_empty() {
                continue;
            }
            let methods = methods_memo
                .entry(&class.id)
                .or_insert_with(|| ctx.methods(&class.id));
            let implementations: Vec<_> = methods
                .iter()
                .copied()
                .filter(|n| INTERFACE_LANGUAGES.contains(&n.language.as_str()))
                .collect();
            let go_struct = class.language == "go" && class.kind == "struct";
            if implementations.is_empty()
                && !(go_struct
                    && supers
                        .iter()
                        .any(|edge| edge.provenance.as_deref() == Some("heuristic")))
            {
                continue;
            }
            for supertype in supers {
                let Some(base) = ctx.node(&supertype.target).filter(|n| {
                    INTERFACE_LANGUAGES.contains(&n.language.as_str()) && n.id != class.id
                }) else {
                    continue;
                };
                if class.language == "go" && base.kind != "interface" {
                    continue;
                }
                let promotes = go_struct
                    && supertype.provenance.as_deref() == Some("heuristic")
                    && base.kind == "interface";
                let base_methods = base_memo.entry(&base.id).or_insert_with(|| {
                    let mut methods = ctx.methods(&base.id);
                    if base.language == "swift" && base.kind == "interface" {
                        for extension in ctx
                            .named(&base.name)
                            .filter(|n| n.language == "swift" && n.kind == "class")
                        {
                            methods.extend(ctx.methods(&extension.id));
                        }
                    }
                    methods
                });
                let key = |n: &Node| {
                    if base.language == "go" && base.kind == "interface" {
                        go::method_key(n)
                    } else {
                        n.name.clone()
                    }
                };
                let mut by_key: HashMap<String, Vec<&Node>> = HashMap::new();
                for method in &implementations {
                    by_key.entry(key(method)).or_default().push(method);
                }
                let mut counts: HashMap<&str, usize> = HashMap::new();
                for method in base_methods.iter() {
                    *counts.entry(&method.name).or_default() += 1;
                }
                let mut added = 0;
                let mut unmatched = Vec::new();
                for base_method in base_methods.iter() {
                    ctx.step()?;
                    if added >= CALLBACK_CAP {
                        break;
                    }
                    let Some(implementations) = by_key.get(&key(base_method)) else {
                        if promotes {
                            unmatched.push(*base_method);
                        }
                        continue;
                    };
                    let implementations =
                        if implementations.len() > 1 || counts[base_method.name.as_str()] > 1 {
                            filter.matching(base_method, implementations)?
                        } else {
                            implementations.clone()
                        };
                    for method in implementations {
                        ctx.step()?;
                        if added >= CALLBACK_CAP {
                            break;
                        }
                        if method.id == base_method.id
                            || JS.contains(&method.language.as_str())
                                && (base_method.is_static == Some(true))
                                    != (method.is_static == Some(true))
                        {
                            continue;
                        }
                        if seen.insert((&base_method.id, &method.id)) {
                            ctx.emit(
                                &mut out,
                                call(base_method, method, "interface-impl", &method.name),
                            )?;
                            added += 1;
                        }
                    }
                }
                if !unmatched.is_empty() && added < CALLBACK_CAP {
                    promoted.push((class, unmatched, CALLBACK_CAP - added));
                }
            }
        }
    }
    for (class, methods, mut remaining) in promoted {
        ctx.step()?;
        for base_method in methods {
            if remaining == 0 {
                break;
            }
            let Some(promotion) = promotions.get(&class.id, &go::method_key(base_method))? else {
                continue;
            };
            for method in promotion.methods {
                if remaining == 0 {
                    break;
                }
                if !seen.insert((&base_method.id, &method.id)) {
                    continue;
                }
                let mut edge = call(base_method, method, "interface-impl", &method.name);
                edge.metadata = Some(
                    json!({"synthesizedBy":"interface-impl","via":method.name,"promotedInto":class.name,"registeredAt":format!("{}:{}",class.file_path,if promotion.line==0 {class.start_line}else{promotion.line})}),
                );
                ctx.emit(&mut out, edge)?;
                remaining -= 1;
            }
        }
    }
    Ok(out)
}
