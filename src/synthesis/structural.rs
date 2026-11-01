use super::*;

pub(super) fn go_method_contains(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for method in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "method" && n.language == "go")
    {
        ctx.step()?;
        let Some((receiver, _)) = method.qualified_name.rsplit_once("::") else {
            continue;
        };
        if receiver.is_empty()
            || ctx
                .incoming(&method.id)
                .filter(|e| e.kind == "contains")
                .any(|e| ctx.node(&e.source).is_some_and(|n| type_kind(&n.kind)))
        {
            continue;
        }
        let directory = directory(&method.file_path);
        let owner = ctx.named(receiver).find(|n| {
            n.language == "go" && type_kind(&n.kind) && super::directory(&n.file_path) == directory
        });
        let Some(owner) = owner else {
            continue;
        };
        if seen.insert((&owner.id, &method.id)) {
            ctx.emit(
                &mut out,
                Edge {
                    source: owner.id.clone(),
                    target: method.id.clone(),
                    kind: "contains".into(),
                    line: Some(method.start_line),
                    column: None,
                    provenance: None,
                    metadata: Some(json!({"synthesizedBy":"go-method-contains"})),
                },
            )?;
        }
    }
    Ok(out)
}

pub(super) fn kotlin_expect_actual(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for actual in ctx
        .nodes
        .iter()
        .filter(|n| n.language == "kotlin" && decorated(n, "actual"))
    {
        let mut added = 0;
        for declaration in ctx.qualified(&actual.qualified_name) {
            ctx.step()?;
            if added >= CALLBACK_CAP {
                break;
            }
            if declaration.language != "kotlin"
                || declaration.id == actual.id
                || declaration.file_path == actual.file_path
                || !(declaration.kind == actual.kind
                    || type_kind(&declaration.kind) && type_kind(&actual.kind))
                || decorated(declaration, "actual")
            {
                continue;
            }
            if seen.insert((&declaration.id, &actual.id)) {
                ctx.emit(
                    &mut out,
                    call(declaration, actual, "kotlin-expect-actual", &actual.name),
                )?;
                added += 1;
            }
        }
    }
    Ok(out)
}

pub(super) fn mybatis(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut methods: HashMap<String, Vec<&Node>> = HashMap::new();
    for method in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "method" && matches!(n.language.as_str(), "java" | "kotlin"))
    {
        ctx.step()?;
        let mut parts = method.qualified_name.rsplit("::");
        let (Some(name), Some(class)) = (parts.next(), parts.next()) else {
            continue;
        };
        if !name.is_empty() && !class.is_empty() {
            methods
                .entry(format!("{class}::{name}"))
                .or_default()
                .push(method);
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for xml in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "method" && n.language == "xml")
    {
        ctx.step()?;
        let Some((namespace, name)) = xml.qualified_name.rsplit_once("::") else {
            continue;
        };
        if namespace.is_empty() || name.is_empty() {
            continue;
        }
        let class = namespace.rsplit('.').next().unwrap_or(namespace);
        let Some(candidates) = methods.get(&format!("{class}::{name}")) else {
            continue;
        };
        if candidates.len() != 1 {
            continue;
        }
        let java = candidates[0];
        if seen.insert((&java.id, &xml.id)) {
            ctx.emit(
                &mut out,
                call(java, xml, "mybatis-java-xml", &format!("{class}.{name}")),
            )?;
        }
    }
    Ok(out)
}

pub(super) fn react_render(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    render(ctx, false)
}
pub(super) fn flutter_build(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    render(ctx, true)
}
fn render(ctx: &Context<'_>, flutter: bool) -> Result<Vec<Edge>> {
    if flutter && !ctx.has(&["dart"]) {
        return Ok(Vec::new());
    }
    let pattern = regex(if flutter {
        r"\bsetState\s*\("
    } else {
        r"this\.setState\s*\("
    })?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for class in ctx.nodes.iter().filter(|n| n.kind == "class") {
        ctx.step()?;
        let methods = ctx.methods(&class.id);
        let Some(&target) = methods
            .iter()
            .find(|n| n.name == if flutter { "build" } else { "render" })
        else {
            continue;
        };
        if flutter && !target.file_path.ends_with(".dart") {
            continue;
        }
        let mut added = 0;
        for method in methods {
            ctx.step()?;
            if added >= CALLBACK_CAP {
                break;
            }
            if method.id == target.id {
                continue;
            }
            let Some(source) = ctx.slice(method) else {
                continue;
            };
            if pattern.is_match(source)? && seen.insert((&method.id, &target.id)) {
                ctx.emit(
                    &mut out,
                    call(
                        method,
                        target,
                        if flutter {
                            "flutter-build"
                        } else {
                            "react-render"
                        },
                        "setState",
                    ),
                )?;
                added += 1;
            }
        }
    }
    Ok(out)
}

pub(super) fn arkui_state(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    const REACTIVE: &[&str] = &[
        "State",
        "Prop",
        "Link",
        "Provide",
        "Consume",
        "StorageLink",
        "StorageProp",
        "LocalStorageLink",
        "LocalStorageProp",
        "ObjectLink",
        "Local",
        "Provider",
        "Consumer",
    ];
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for structure in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "struct" && n.language == "arkts")
    {
        ctx.step()?;
        let children = ctx.children(&structure.id);
        let Some(&build) = children
            .iter()
            .find(|n| n.kind == "method" && n.name == "build")
        else {
            continue;
        };
        let properties: Vec<_> = children
            .iter()
            .filter(|n| n.kind == "property" && REACTIVE.iter().any(|d| decorated(n, d)))
            .map(|n| ::regex::escape(&n.name))
            .collect();
        if properties.is_empty() {
            continue;
        }
        let mutation = regex(&format!(
            r"this\.(?:{})\s*(?:=(?!=)|\+\+|--|[+\-*/%&|^]=|\.(?:push|pop|shift|unshift|splice|sort|reverse|fill)\s*\()",
            properties.join("|")
        ))?;
        let mut added = 0;
        for method in children {
            ctx.step()?;
            if added >= CALLBACK_CAP {
                break;
            }
            if method.kind != "method" || method.id == build.id {
                continue;
            }
            let Some(source) = ctx.slice(method) else {
                continue;
            };
            if mutation.is_match(&strip_comments(source))? && seen.insert((&method.id, &build.id)) {
                ctx.emit(
                    &mut out,
                    call(method, build, "arkui-state", "state assignment"),
                )?;
                added += 1;
            }
        }
    }
    Ok(out)
}

pub(super) fn go_grpc(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut structures = Vec::new();
    for structure in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "struct" && n.language == "go")
    {
        ctx.step()?;
        let methods = ctx.methods(&structure.id);
        let names: HashSet<_> = methods.iter().map(|n| n.name.as_str()).collect();
        structures.push((
            structure,
            methods,
            names,
            prometeu_extract::is_generated_file(&structure.file_path),
        ));
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (stub, methods, _, is_generated) in &structures {
        if !is_generated
            || !stub.name.starts_with("Unimplemented")
            || !stub.name.ends_with("Server")
        {
            continue;
        }
        let rpc: Vec<_> = methods
            .iter()
            .filter(|m| !m.name.starts_with("mustEmbed") && m.name != "testEmbeddedByValue")
            .collect();
        if rpc.is_empty() {
            continue;
        }
        for (candidate, implementations, names, is_generated) in &structures {
            ctx.step()?;
            if candidate.id == stub.id
                || *is_generated
                || !rpc.iter().all(|m| names.contains(m.name.as_str()))
            {
                continue;
            }
            let mut added = 0;
            for method in &rpc {
                for implementation in implementations {
                    ctx.step()?;
                    if added >= CALLBACK_CAP {
                        break;
                    }
                    if method.name == implementation.name
                        && seen.insert((&method.id, &implementation.id))
                    {
                        ctx.emit(
                            &mut out,
                            call(
                                method,
                                implementation,
                                "go-grpc-stub-impl",
                                &implementation.name,
                            ),
                        )?;
                        added += 1;
                    }
                }
                if added >= CALLBACK_CAP {
                    break;
                }
            }
        }
    }
    Ok(out)
}

pub(super) fn fabric_native(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for component in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "component" && n.id.starts_with("fabric-component:"))
    {
        for suffix in ["", "View", "ViewManager", "ComponentView", "Manager"] {
            for native in ctx
                .named(&format!("{}{suffix}", component.name))
                .filter(|n| {
                    n.kind == "class"
                        && matches!(n.language.as_str(), "objc" | "kotlin" | "java" | "cpp")
                })
            {
                ctx.step()?;
                if seen.insert((&component.id, &native.id)) {
                    let mut edge = call(component, native, "fabric-native-impl", "");
                    edge.line = None;
                    edge.metadata = Some(
                        json!({"synthesizedBy":"fabric-native-impl","viaSuffix":if suffix.is_empty(){"(exact)"}else{suffix},"componentName":component.name}),
                    );
                    ctx.emit(&mut out, edge)?;
                }
            }
        }
    }
    Ok(out)
}

pub(super) fn expo_cross_platform(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(&["swift"]) || !ctx.has(&["kotlin"]) {
        return Ok(Vec::new());
    }
    let mut groups: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
    for method in ctx
        .nodes
        .iter()
        .filter(|n| n.kind == "method" && n.id.starts_with("expo-module:"))
    {
        if let Some(key) = method
            .qualified_name
            .rsplit("::")
            .next()
            .filter(|k| !k.is_empty())
        {
            groups.entry(key).or_default().push(method);
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for group in groups.values() {
        for a in group {
            for b in group {
                ctx.step()?;
                if a.id != b.id && a.language != b.language && seen.insert((&a.id, &b.id)) {
                    let mut edge = call(a, b, "expo-cross-platform", &a.name);
                    edge.metadata
                        .as_mut()
                        .and_then(Value::as_object_mut)
                        .unwrap()
                        .remove("registeredAt");
                    ctx.emit(&mut out, edge)?;
                }
            }
        }
    }
    Ok(out)
}

pub(super) fn rn_cross_platform(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    if !ctx.has(JS) {
        return Ok(Vec::new());
    }
    const INFRA: &[&str] = &[
        "addListener",
        "removeListeners",
        "getConstants",
        "constantsToExport",
        "getName",
        "invalidate",
        "initialize",
        "getDefaultEventTypes",
        "supportedEvents",
        "requiresMainQueueSetup",
        "methodQueue",
    ];
    let mut groups: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
    for method in ctx.nodes.iter().filter(|n| {
        n.kind == "method" && matches!(n.language.as_str(), "java" | "kotlin" | "objc" | "cpp")
    }) {
        groups
            .entry(method.name.split(':').next().unwrap_or(""))
            .or_default()
            .push(method);
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (name, group) in groups {
        if INFRA.contains(&name)
            || group
                .iter()
                .map(|n| &n.language)
                .collect::<HashSet<_>>()
                .len()
                < 2
        {
            continue;
        }
        for method in &group {
            ctx.step()?;
            if !ctx.incoming(&method.id).any(|e| {
                e.kind == "calls"
                    && ctx
                        .node(&e.source)
                        .is_some_and(|n| JS.contains(&n.language.as_str()))
            }) {
                continue;
            }
            for sibling in &group {
                ctx.step()?;
                if sibling.id == method.id || sibling.language == method.language {
                    continue;
                }
                for (a, b) in [(method, sibling), (sibling, method)] {
                    if seen.insert((&a.id, &b.id)) {
                        let mut edge = call(a, b, "rn-cross-platform", name);
                        edge.metadata
                            .as_mut()
                            .and_then(Value::as_object_mut)
                            .unwrap()
                            .remove("registeredAt");
                        ctx.emit(&mut out, edge)?;
                    }
                }
            }
        }
    }
    Ok(out)
}

fn nix_segments(name: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut i = 0;
    let bytes = name.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            i += 1;
            let token = &name[start..i];
            if token.contains("${") {
                break;
            }
            segments.push(token.to_owned());
            if i >= bytes.len() || bytes[i] != b'.' {
                break;
            }
            i += 1;
        } else {
            let start = i;
            while i < bytes.len() && bytes[i] != b'.' {
                if bytes[i] == b'"' || bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
                    return segments;
                }
                i += 1;
            }
            let token = &name[start..i];
            if token.is_empty()
                || !token.as_bytes()[0].is_ascii_alphabetic() && token.as_bytes()[0] != b'_'
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'\'' | b'-'))
            {
                break;
            }
            segments.push(token.to_owned());
            i += 1;
        }
    }
    segments
}

pub(super) fn nix_options(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    struct Record<'a> {
        node: &'a Node,
        segments: Vec<String>,
    }
    let mut by_file: BTreeMap<&str, Vec<Record<'_>>> = BTreeMap::new();
    for kind in ["variable", "function"] {
        for node in ctx
            .nodes
            .iter()
            .filter(|n| n.language == "nix" && n.kind == kind)
        {
            ctx.step()?;
            let segments = nix_segments(&node.name);
            if !segments.is_empty() {
                by_file
                    .entry(&node.file_path)
                    .or_default()
                    .push(Record { node, segments });
            }
        }
    }
    let mut declarations: HashMap<String, Vec<&Node>> = HashMap::new();
    let mut writes = Vec::new();
    for records in by_file.values_mut() {
        records.sort_by_key(|r| (r.node.start_line, std::cmp::Reverse(r.node.end_line)));
        let mut stack: Vec<(u32, u32, Vec<String>)> = Vec::new();
        for record in records {
            ctx.step()?;
            let node = record.node;
            while stack
                .last()
                .is_some_and(|(_, end, _)| *end < node.start_line)
            {
                stack.pop();
            }
            let enclosing = stack.last().filter(|(start, end, _)| {
                node.start_line >= *start
                    && node.end_line <= *end
                    && (node.start_line != *start || node.end_line != *end)
            });
            let prefix = if record.segments[0] == "options" {
                Some(if enclosing.is_some() {
                    vec!["\0submodule".into()]
                } else {
                    record.segments[1..].to_vec()
                })
            } else {
                enclosing
                    .map(|(_, _, prefix)| prefix.iter().chain(&record.segments).cloned().collect())
            };
            if let Some(prefix) = prefix {
                if prefix.len() >= 2 && !prefix.iter().any(|s| s == "\0submodule") {
                    declarations.entry(prefix.join(".")).or_default().push(node);
                }
                stack.push((node.start_line, node.end_line, prefix));
            } else if record.segments.len() >= 2 {
                writes.push((node, record.segments.clone()));
            }
        }
    }
    let mut out = Vec::new();
    for (write, segments) in writes {
        let segments = if segments[0] == "config" {
            &segments[1..]
        } else {
            &segments[..]
        };
        for length in (2..=segments.len().min(6)).rev() {
            ctx.step()?;
            let path = segments[..length].join(".");
            let Some(candidates) = declarations.get(&path) else {
                continue;
            };
            if candidates
                .iter()
                .all(|n| n.file_path == candidates[0].file_path)
            {
                let target = candidates[0];
                if target.id != write.id {
                    let mut edge = call(write, target, "nix-option-path", "");
                    edge.kind = "references".into();
                    edge.metadata = Some(
                        json!({"synthesizedBy":"nix-option-path","optionPath":path,"registeredAt":format!("{}:{}",target.file_path,target.start_line)}),
                    );
                    ctx.emit(&mut out, edge)?;
                }
            }
            break;
        }
    }
    Ok(out)
}
