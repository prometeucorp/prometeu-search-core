//! General Python imports and lexical names share the typed resolver's snapshot.
use super::*;
use crate::frameworks::python_resolution::{Bindings, Environment};
use typed::{Answer, Match, Typed};
#[path = "globals.rs"]
mod globals;

pub(super) fn select<'a>(
    reference: &UnresolvedReference,
    typed: &Typed<'a, '_>,
    scopes: &scope::Scopes<'a>,
    imports: &imports::Imports<'a, '_>,
    names: &Bindings<'a>,
    environment: &Environment<'a>,
) -> Result<Match<'a>> {
    typed.charge(
        typed
            .named(
                reference
                    .reference_name
                    .rsplit('.')
                    .next()
                    .unwrap_or_default(),
            )
            .len()
            .saturating_add(1),
    )?;
    if reference.reference_kind == "function_ref" {
        return function_value(reference, typed, scopes, imports, names, environment);
    }
    let continued = std::iter::once(at_reference(reference, environment))
        .chain(
            environment
                .sources
                .get(reference.file_path.as_deref().unwrap_or_default())
                .into_iter()
                .flat_map(|source| {
                    source
                        .lines()
                        .skip(reference.line as usize)
                        .take(7)
                        .map(str::to_owned)
                }),
        )
        .collect::<Vec<_>>()
        .join("\n");
    let self_call = reference.reference_kind == "calls"
        && !reference.reference_name.contains('.')
        && regex(&format!(
            r"^self\s*\.\s*{}\s*\(",
            regex::escape(&reference.reference_name)
        ))?
        .is_match(&continued)?;
    let imported = if self_call {
        None
    } else {
        imports.resolve(reference)?
    }
    .map(|answer| Answer {
        node: answer.node,
        confidence: answer.confidence,
        resolved_by: answer.resolved_by,
        metadata: None,
    });
    if imported
        .as_ref()
        .is_some_and(|answer| answer.confidence >= 0.9)
    {
        return Ok(Match::Resolved(imported.unwrap()));
    }
    if reference.reference_name.contains('.') {
        let mut method = method_call(reference, typed, scopes, names, environment)?;
        if let Match::Resolved(answer) = &mut method
            && let Some((receiver, _)) = reference.reference_name.rsplit_once('.')
        {
            answer.metadata = typed.attribute_dependencies(receiver, reference)?;
        }
        return Ok(match (imported, method) {
            (Some(imported), Match::Resolved(method))
                if method.confidence > imported.confidence =>
            {
                Match::Resolved(method)
            }
            (Some(imported), _) => Match::Resolved(imported),
            (_, method) => method,
        });
    }
    let path = reference.file_path.as_deref().unwrap_or_default();
    let mappings = imports.scoped(reference)?;
    if reference.reference_kind == "imports"
        && mappings.iter().any(|mapping| {
            mapping.local_name == reference.reference_name
                && !mapping.is_namespace
                && !mapping.source.starts_with('.')
                && (environment.module)(&mapping.source, path).is_none()
        })
    {
        return Ok(Match::Rejected);
    }
    let exact = names
        .choose(reference, &[], environment, None)?
        .filter(|answer| answer.node.language == "python")
        .map(|answer| Answer {
            node: answer.node,
            confidence: answer.confidence,
            resolved_by: answer.resolved_by,
            metadata: None,
        });
    let exact = exact.filter(|answer| {
        if matches!(reference.reference_kind.as_str(), "extends" | "implements")
            && answer.node.id == reference.from_node_id
        {
            return false;
        }
        if reference.reference_kind == "imports"
            && at_reference(reference, environment).starts_with("from")
            && !matches!(answer.node.kind.as_str(), "file" | "module")
        {
            return false;
        }
        if !matches!(reference.reference_kind.as_str(), "extends" | "implements")
            || answer.node.file_path == path
        {
            return true;
        }
        let bindings = mappings
            .iter()
            .filter(|mapping| {
                mapping.local_name == reference.reference_name
                    || mapping.is_namespace
                        && mapping.source.split('.').next()
                            == Some(reference.reference_name.as_str())
            })
            .collect::<Vec<_>>();
        bindings.is_empty()
            || !bindings.iter().all(|mapping| {
                let module_path = format!("/{}", mapping.source.replace('.', "/"));
                let target = format!("/{}", answer.node.file_path);
                !mapping.source.starts_with('.')
                    && (environment.module)(&mapping.source, path).is_none()
                    && !target.contains(&format!("{module_path}/"))
                    && !target.ends_with(&format!("{module_path}.py"))
                    && !target.ends_with(&format!("{module_path}.pyi"))
            })
    });
    Ok(match (imported, exact) {
        (Some(imported), Some(exact)) if exact.confidence > imported.confidence => {
            Match::Resolved(exact)
        }
        (Some(imported), _) => Match::Resolved(imported),
        (_, Some(exact)) => Match::Resolved(exact),
        _ => Match::Rejected,
    })
}

fn method_call<'a>(
    reference: &UnresolvedReference,
    typed: &Typed<'a, '_>,
    scopes: &scope::Scopes<'a>,
    names: &Bindings<'a>,
    environment: &Environment<'a>,
) -> Result<Match<'a>> {
    if reference.reference_kind != "calls" {
        return Ok(Match::Rejected);
    }
    let Some((receiver, name)) = reference.reference_name.rsplit_once('.') else {
        return Ok(Match::Unhandled);
    };
    if receiver.ends_with(']') {
        if !regex(r"^\w+$")?.is_match(name)?
            || regex(r"^__\w+__$")?.is_match(name)?
            || standard_method(name)
        {
            return Ok(Match::Rejected);
        }
        let methods = typed
            .named(name)
            .iter()
            .copied()
            .filter(|node| node.kind == "method")
            .collect::<Vec<_>>();
        let Some(node) = methods.first().copied().filter(|_| methods.len() == 1) else {
            return Ok(Match::Rejected);
        };
        let file = reference.file_path.as_deref().unwrap_or_default();
        let in_hand = node.file_path == file
            || imports::mappings(environment.sources.get(file).copied().unwrap_or_default())?
                .iter()
                .any(|mapping| {
                    (environment.module)(&mapping.source, file).as_deref()
                        == Some(node.file_path.as_str())
                });
        return Ok(if in_hand && !test_double(node, "", file, environment)? {
            Match::Resolved(Answer {
                node,
                confidence: 0.7,
                resolved_by: "instance-method",
                metadata: None,
            })
        } else {
            Match::Rejected
        });
    }
    if !regex(r"^[\w.]+$")?.is_match(receiver)? || !regex(r"^\w+$")?.is_match(name)? {
        return Ok(Match::Rejected);
    }
    let path = reference.file_path.as_deref().unwrap_or_default();
    if typed.outside_name(receiver.split('.').next().unwrap_or(receiver), path)?
        && !scopes.locally_bound(receiver.split('.').next().unwrap_or(receiver), reference)?
    {
        return Ok(Match::Rejected);
    }
    if !receiver.contains('.') {
        let inferred = typed.infer_local(receiver, reference)?.or(fixture_type(
            receiver,
            reference,
            typed,
            names,
            environment,
        )?);
        if let Some(inferred) = inferred {
            let classes = typed
                .named(&inferred)
                .iter()
                .copied()
                .filter(|node| node.kind == "class")
                .collect::<Vec<_>>();
            if classes.is_empty() {
                return Ok(Match::Rejected);
            }
            for class in classes {
                if let Some(node) = method_on(
                    class,
                    name,
                    typed,
                    environment,
                    &mut std::collections::HashSet::new(),
                )? {
                    return Ok(Match::Resolved(Answer {
                        node,
                        confidence: 0.9,
                        resolved_by: "instance-method",
                        metadata: None,
                    }));
                }
            }
        }
        if let Some(node) =
            global_method(receiver, name, reference, typed, scopes, names, environment)?
        {
            return Ok(Match::Resolved(Answer {
                node,
                confidence: 0.9,
                resolved_by: "instance-method",
                metadata: None,
            }));
        }
        for &class in typed
            .named(receiver)
            .iter()
            .filter(|node| node.kind == "class")
        {
            if !names.lexically_reachable(class, reference)? {
                continue;
            }
            if let Some(node) = method_on(
                class,
                name,
                typed,
                environment,
                &mut std::collections::HashSet::new(),
            )? {
                return Ok(Match::Resolved(Answer {
                    node,
                    confidence: 0.85,
                    resolved_by: "qualified-name",
                    metadata: None,
                }));
            }
        }
        if let Some(first) = receiver.chars().next() {
            let capitalized = format!("{}{}", first.to_uppercase(), &receiver[first.len_utf8()..]);
            if capitalized != receiver {
                for &class in typed
                    .named(&capitalized)
                    .iter()
                    .filter(|node| node.kind == "class")
                {
                    if !names.lexically_reachable(class, reference)? {
                        continue;
                    }
                    if let Some(node) = method_on(
                        class,
                        name,
                        typed,
                        environment,
                        &mut std::collections::HashSet::new(),
                    )? {
                        return Ok(Match::Resolved(Answer {
                            node,
                            confidence: 0.8,
                            resolved_by: "instance-method",
                            metadata: None,
                        }));
                    }
                }
            }
        }
    }
    let mut candidates = Vec::new();
    let mut scoped_out = false;
    for &node in typed.named(name) {
        if node.kind != "method"
            || (!crate::classify::is_test_path(path)
                && crate::classify::is_test_path(&node.file_path))
        {
            continue;
        }
        if dispatched_action(node)? || property(node, environment)? {
            scoped_out = true;
            continue;
        }
        if names.lexically_reachable(node, reference)? {
            candidates.push(node);
        } else {
            scoped_out = true;
        }
    }
    let chained = receiver.contains('.');
    let link = receiver.rsplit('.').next().unwrap_or(receiver);
    if candidates.len() == 1
        && !chained
        && !scoped_out
        && candidates[0].id != reference.from_node_id
        && !test_double(candidates[0], link, path, environment)?
    {
        return Ok(Match::Resolved(Answer {
            node: candidates[0],
            confidence: 0.7,
            resolved_by: "instance-method",
            metadata: None,
        }));
    }
    let receiver_words = words(link)?;
    let head = receiver_words.last();
    let whole = link.replace('_', "").to_lowercase();
    candidates.sort_by_key(|node| node.file_path != path);
    let mut best = None;
    let mut score = 0;
    for node in candidates {
        let owner_words = words(owner(node))?;
        if chained
            && head.is_some_and(|head| {
                !owner_words
                    .iter()
                    .any(|word| word.trim_end_matches('s') == head.trim_end_matches('s'))
            })
            && whole != owner_words.join("")
        {
            continue;
        }
        if test_double(node, link, path, environment)? {
            continue;
        }
        let mut value = receiver_words
            .iter()
            .filter(|word| owner_words.contains(word))
            .count()
            + 1;
        if head.is_some() && head == owner_words.last() {
            value += 1;
        }
        if !owner_words.is_empty() && whole == owner_words.join("") {
            value += 2;
        }
        if value > score {
            score = value;
            best = Some(node);
        }
    }
    Ok(
        if let Some(node) = best.filter(|node| score >= 2 && node.id != reference.from_node_id) {
            Match::Resolved(Answer {
                node,
                confidence: 0.65,
                resolved_by: "instance-method",
                metadata: None,
            })
        } else {
            Match::Rejected
        },
    )
}

fn method_on<'a>(
    class: &'a Node,
    name: &str,
    typed: &Typed<'a, '_>,
    environment: &Environment<'a>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<Option<&'a Node>> {
    if seen.len() >= 5 || !seen.insert(class.id.clone()) {
        return Ok(None);
    }
    let qualified = format!("{}::{name}", class.qualified_name);
    for &node in typed.named(name) {
        if node.file_path == class.file_path
            && node.qualified_name == qualified
            && node.kind == "method"
            && !property(node, environment)?
        {
            return Ok(Some(node));
        }
    }
    let mut bases = (environment.supertypes)(&class.id);
    if bases.is_empty() {
        bases.extend(typed.class_bases(class)?.into_iter().flatten());
    }
    for base in bases {
        if let Some(node) = method_on(base, name, typed, environment, seen)? {
            return Ok(Some(node));
        }
    }
    Ok(None)
}

fn property(node: &Node, environment: &Environment<'_>) -> Result<bool> {
    let source = environment
        .sources
        .get(node.file_path.as_str())
        .copied()
        .unwrap_or_default();
    let lines = source
        .lines()
        .take(node.start_line.saturating_sub(1) as usize)
        .collect::<Vec<_>>();
    for line in lines.into_iter().rev() {
        let line = trim(line);
        if !line.starts_with('@') {
            break;
        }
        if regex(r"^@(?:property|[\w.]+\.setter|[\w.]+\.getter)\b")?.is_match(line)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn owner(node: &Node) -> &str {
    node.qualified_name
        .rsplit_once("::")
        .map_or("", |(owner, _)| {
            owner.rsplit([':', '.']).next().unwrap_or("")
        })
}
fn words(name: &str) -> Result<Vec<String>> {
    let name = regex(r"([a-z])([A-Z])")?.replace_all(name, "$1 $2");
    let name = regex(r"([A-Z]+)([A-Z][a-z])")?.replace_all(&name, "$1 $2");
    Ok(regex(r"[\s._:/\\]+")?
        .split(&name)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|word| word.encode_utf16().count() > 1)
        .map(str::to_lowercase)
        .collect())
}
fn test_double(
    node: &Node,
    receiver: &str,
    file: &str,
    environment: &Environment<'_>,
) -> Result<bool> {
    let pattern = regex(r"(?i)\b(?:mock|mocked|fake|dummy|stub|spy)\b")?;
    Ok(pattern.is_match(&words(owner(node))?.join(" "))?
        && !pattern.is_match(&words(receiver)?.join(" "))?
        && !environment
            .sources
            .get(file)
            .is_some_and(|source| source.contains(owner(node))))
}

fn function_value<'a>(
    reference: &UnresolvedReference,
    typed: &Typed<'a, '_>,
    scopes: &scope::Scopes<'a>,
    imports: &imports::Imports<'a, '_>,
    names: &Bindings<'a>,
    environment: &Environment<'a>,
) -> Result<Match<'a>> {
    if reference.reference_name.contains('.') {
        return member_value(reference, typed, scopes, imports, names, environment);
    }
    if let Some(imported) = imports.resolve(reference)?
        && matches!(imported.node.kind.as_str(), "function" | "method" | "class")
    {
        return Ok(Match::Resolved(Answer {
            node: imported.node,
            confidence: imported.confidence,
            resolved_by: imported.resolved_by,
            metadata: None,
        }));
    }
    let named = typed
        .named(&reference.reference_name)
        .iter()
        .copied()
        .filter(|node| {
            matches!(node.kind.as_str(), "function" | "class") && node.id != reference.from_node_id
        })
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for &node in &named {
        if names.lexically_reachable(node, reference)? {
            candidates.push(node);
        }
    }
    let mut fixture = false;
    for &candidate in &candidates {
        fixture |= names.fixture_in_reach(
            candidate,
            reference.file_path.as_deref().unwrap_or_default(),
            environment,
        )?;
    }
    if candidates.is_empty() || !fixture && names.local_binding(reference, environment)? {
        return Ok(Match::Rejected);
    }
    let file = reference.file_path.as_deref().unwrap_or_default();
    let own = candidates
        .iter()
        .copied()
        .filter(|node| node.file_path == file)
        .collect::<Vec<_>>();
    let answer = if let Some(node) = own.iter().copied().min_by_key(|node| node.start_line) {
        Some((node, if own.len() == 1 { 0.95 } else { 0.9 }))
    } else if candidates.len() == 1
        && (named.len() == 1
            || !scopes
                .module_import_keys(&reference.reference_name, file)?
                .is_empty())
    {
        Some((candidates[0], 0.8))
    } else {
        None
    };
    Ok(answer.map_or(Match::Rejected, |(node, confidence)| {
        Match::Resolved(Answer {
            node,
            confidence,
            resolved_by: "function-ref",
            metadata: None,
        })
    }))
}

fn member_value<'a>(
    reference: &UnresolvedReference,
    typed: &Typed<'a, '_>,
    scopes: &scope::Scopes<'a>,
    imports: &imports::Imports<'a, '_>,
    names: &Bindings<'a>,
    environment: &Environment<'a>,
) -> Result<Match<'a>> {
    let (receiver, member) = reference.reference_name.rsplit_once('.').unwrap();
    let path = reference.file_path.as_deref().unwrap_or_default();
    let root = receiver.split('.').next().unwrap();
    let result = |nodes: Vec<&'a Node>, confidence: f64| -> Result<Match<'a>> {
        let Some(node) = nodes.first().copied().filter(|_| nodes.len() == 1) else {
            return Ok(Match::Rejected);
        };
        Ok(
            if node.id != reference.from_node_id
                && matches!(node.kind.as_str(), "function" | "method")
                && !property(node, environment)?
            {
                Match::Resolved(Answer {
                    node,
                    confidence,
                    resolved_by: "function-ref",
                    metadata: None,
                })
            } else {
                Match::Rejected
            },
        )
    };
    let imported = imports
        .scoped(reference)?
        .iter()
        .any(|mapping| mapping.local_name == root);
    if imported {
        if let Some(class) = typed.class_named(receiver, path, "class")? {
            return result(
                value_members(
                    class,
                    member,
                    typed,
                    environment,
                    &mut std::collections::HashSet::new(),
                )?,
                0.9,
            );
        }
        if receiver.split('.').count() <= 2 {
            let mut probe = reference.clone();
            probe.reference_name = receiver.to_owned();
            probe.reference_kind = "references".into();
            if let Some(answer) = imports.resolve(&probe)?
                && globals::is_global(answer.node, typed)
                && answer.node.name == receiver.rsplit('.').next().unwrap()
                && scopes.all_import_keys(root, path)?.len() == 1
                && !scopes.global_rebound(root, path)?
                && !scopes.locally_bound(root, reference)?
            {
                return result(
                    globals::members(answer.node, member, reference, typed, scopes, environment)?,
                    0.9,
                );
            }
        }
        if let Some(answer) = imports.resolve(reference)? {
            return result(
                typed
                    .named(&answer.node.name)
                    .iter()
                    .copied()
                    .filter(|node| {
                        node.file_path == answer.node.file_path
                            && node.qualified_name == answer.node.qualified_name
                    })
                    .collect(),
                0.9,
            );
        }
        return Ok(Match::Rejected);
    }
    if matches!(receiver, "self" | "cls") {
        let owner = typed
            .in_file(path)
            .iter()
            .copied()
            .filter(|node| {
                node.kind == "class"
                    && node.start_line <= reference.line
                    && node.end_line >= reference.line
            })
            .max_by_key(|node| node.start_line);
        return match owner {
            Some(owner) => result(
                value_members(
                    owner,
                    member,
                    typed,
                    environment,
                    &mut std::collections::HashSet::new(),
                )?,
                0.9,
            ),
            None => Ok(Match::Rejected),
        };
    }
    if regex(r"^(?:self|cls)\.\w+$")?.is_match(receiver)? {
        match typed.attribute(receiver, member, reference)? {
            Match::Resolved(answer) => {
                return result(
                    vec![answer.node],
                    if answer.confidence == 0.85 {
                        0.9
                    } else {
                        answer.confidence
                    },
                );
            }
            Match::Rejected => return Ok(Match::Rejected),
            Match::Unhandled => {}
        }
    }
    let inferred = value_local(receiver, reference, typed, environment)?;
    if inferred.is_none()
        && !scopes.locally_bound(receiver, reference)?
        && typed
            .in_file(path)
            .iter()
            .any(|node| node.name == receiver && globals::is_global(node, typed))
    {
        return result(
            global_method(
                receiver,
                member,
                reference,
                typed,
                scopes,
                names,
                environment,
            )?
            .into_iter()
            .collect(),
            0.9,
        );
    }
    let inferred = inferred.or_else(|| {
        receiver
            .starts_with(|ch: char| ch.is_ascii_uppercase())
            .then(|| receiver.to_owned())
    });
    if let Some(inferred) = inferred.filter(|name| !matches!(name.as_str(), "object" | "Any")) {
        let Some(class) = typed.class_named(&inferred, path, "class")? else {
            return Ok(Match::Rejected);
        };
        let members = value_members(
            class,
            member,
            typed,
            environment,
            &mut std::collections::HashSet::new(),
        )?;
        if !members.is_empty() {
            return result(members, 0.9);
        }
        let mut descendants = Vec::new();
        for &node in typed
            .named(member)
            .iter()
            .filter(|node| node.kind == "method")
        {
            if let Some(owner) = typed.in_file(&node.file_path).iter().find(|owner| {
                owner.kind == "class"
                    && node.qualified_name == format!("{}::{member}", owner.qualified_name)
            }) && derives(
                owner,
                class,
                typed,
                environment,
                &mut std::collections::HashSet::new(),
            )? {
                descendants.push(node);
            }
        }
        return result(descendants, 0.8);
    }
    let candidate = result(typed.named(member).to_vec(), 0.8)?;
    if let Match::Resolved(answer) = &candidate
        && answer.node.kind == "method"
    {
        let receiver_words = words(receiver.rsplit('.').next().unwrap())?;
        let owner_words = words(owner(answer.node))?;
        if !receiver_words.iter().any(|word| owner_words.contains(word)) {
            return Ok(Match::Rejected);
        }
    }
    Ok(candidate)
}

fn value_local(
    receiver: &str,
    reference: &UnresolvedReference,
    typed: &Typed<'_, '_>,
    environment: &Environment<'_>,
) -> Result<Option<String>> {
    if !regex(r"^\w+$")?.is_match(receiver)? {
        return Ok(None);
    }
    let path = reference.file_path.as_deref().unwrap_or_default();
    let caller = typed
        .in_file(path)
        .iter()
        .find(|node| node.id == reference.from_node_id);
    let start = caller.map_or(1, |node| node.start_line);
    let source = crate::synthesis::comments::strip_for_regex(
        environment.sources.get(path).copied().unwrap_or_default(),
        "python",
    );
    let lines = source.lines().collect::<Vec<_>>();
    let assigned = regex(&format!(
        r#"^\s*{}\s*(?::\s*["']?([\w.]+)["']?)?\s*=\s*(.*)$"#,
        regex::escape(receiver)
    ))?;
    let declared = regex(&format!(
        r#"^\s*{}\s*:\s*["']?([\w.]+)"#,
        regex::escape(receiver)
    ))?;
    for at in (start.saturating_sub(1)..reference.line).rev() {
        let line = lines.get(at as usize).copied().unwrap_or_default();
        if let Some(capture) = assigned.captures(line)? {
            return Ok(Some(if let Some(ty) = capture.get(1) {
                ty.as_str().to_owned()
            } else {
                regex(r"^([A-Z][\w.]*)\s*\(")?
                    .captures(&capture[2])?
                    .map_or_else(|| "<unknown>".into(), |capture| capture[1].into())
            }));
        }
        if let Some(capture) = declared.captures(line)? {
            return Ok(Some(capture[1].into()));
        }
    }
    Ok(regex(&format!(
        r#"\b{}\s*:\s*["']?([\w.]+)"#,
        regex::escape(receiver)
    ))?
    .captures(
        caller
            .and_then(|node| node.signature.as_deref())
            .unwrap_or_default(),
    )?
    .map(|capture| capture[1].into()))
}

fn value_members<'a>(
    class: &'a Node,
    member: &str,
    typed: &Typed<'a, '_>,
    environment: &Environment<'a>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<Vec<&'a Node>> {
    if seen.len() >= 16 || !seen.insert(class.id.clone()) {
        return Ok(Vec::new());
    }
    let body = environment
        .sources
        .get(class.file_path.as_str())
        .copied()
        .unwrap_or_default()
        .lines()
        .skip(class.start_line.saturating_sub(1) as usize)
        .take(class.end_line.saturating_sub(class.start_line) as usize + 1)
        .collect::<Vec<_>>()
        .join("\n");
    if regex(&format!(
        r"(?m)^\s*(?:(?:self|cls)\.)?{}\s*(?:=|:)",
        regex::escape(member)
    ))?
    .is_match(&body)?
    {
        return Ok(vec![class]);
    }
    let qualified = format!("{}::{member}", class.qualified_name);
    let own = typed
        .named(member)
        .iter()
        .copied()
        .filter(|node| node.file_path == class.file_path && node.qualified_name == qualified)
        .collect::<Vec<_>>();
    if !own.is_empty() {
        return Ok(own);
    }
    let mut inherited = Vec::new();
    for base in value_bases(class, typed, environment)? {
        for node in value_members(base, member, typed, environment, seen)? {
            if !inherited.iter().any(|other: &&Node| other.id == node.id) {
                inherited.push(node);
            }
        }
    }
    Ok(inherited)
}

fn value_bases<'a>(
    class: &'a Node,
    typed: &Typed<'a, '_>,
    environment: &Environment<'a>,
) -> Result<Vec<&'a Node>> {
    let source = environment
        .sources
        .get(class.file_path.as_str())
        .copied()
        .unwrap_or_default();
    let mappings = imports::mappings(source)?;
    let declaration = source
        .lines()
        .skip(class.start_line.saturating_sub(1) as usize)
        .take(class.end_line.saturating_sub(class.start_line) as usize + 1)
        .collect::<Vec<_>>()
        .join("\n");
    if mappings.iter().any(|mapping| {
        mapping.is_namespace
            && declaration.contains(&format!("{}.", mapping.source))
            && mappings
                .iter()
                .filter(|other| other.local_name == mapping.local_name)
                .count()
                != 1
    }) {
        return Ok(Vec::new());
    }
    let mut bases = (environment.supertypes)(&class.id);
    if bases.is_empty() {
        bases.extend(typed.class_bases(class)?.into_iter().flatten());
    }
    Ok(bases)
}

fn derives<'a>(
    class: &'a Node,
    base: &Node,
    typed: &Typed<'a, '_>,
    environment: &Environment<'a>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<bool> {
    if seen.len() >= 16 || !seen.insert(class.id.clone()) {
        return Ok(false);
    }
    for parent in value_bases(class, typed, environment)? {
        if parent.id == base.id || derives(parent, base, typed, environment, seen)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn at_reference(reference: &UnresolvedReference, environment: &Environment<'_>) -> String {
    let line = environment
        .sources
        .get(reference.file_path.as_deref().unwrap_or_default())
        .and_then(|source| {
            source
                .lines()
                .nth(reference.line.saturating_sub(1) as usize)
        })
        .unwrap_or_default();
    let units = line
        .encode_utf16()
        .skip(reference.column as usize)
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

fn fixture_type(
    receiver: &str,
    reference: &UnresolvedReference,
    typed: &Typed<'_, '_>,
    names: &Bindings<'_>,
    environment: &Environment<'_>,
) -> Result<Option<String>> {
    if !regex(r"^[a-z_]\w*$")?.is_match(receiver)? || matches!(receiver, "self" | "cls") {
        return Ok(None);
    }
    let path = reference.file_path.as_deref().unwrap_or_default();
    let Some(caller) = typed.in_file(path).iter().find(|node| {
        node.id == reference.from_node_id && matches!(node.kind.as_str(), "function" | "method")
    }) else {
        return Ok(None);
    };
    if !regex(&format!(
        r"[(,]\s*{}\s*(?:[:=,)]|$)",
        regex::escape(receiver)
    ))?
    .is_match(caller.signature.as_deref().unwrap_or_default())?
    {
        return Ok(None);
    }
    let mut fixtures = Vec::new();
    for &node in typed.named(receiver) {
        if node.kind == "function" && names.fixture_in_reach(node, path, environment)? {
            fixtures.push(node);
        }
    }
    fixtures.sort_by_key(|node| {
        (
            node.file_path != path,
            std::cmp::Reverse(node.file_path.len()),
        )
    });
    let Some(fixture) = fixtures.first() else {
        return Ok(None);
    };
    let body = environment
        .sources
        .get(fixture.file_path.as_str())
        .copied()
        .unwrap_or_default()
        .lines()
        .skip(fixture.start_line as usize)
        .take(fixture.end_line.saturating_sub(fixture.start_line) as usize)
        .collect::<Vec<_>>()
        .join("\n");
    let Some(returned) =
        regex(r"(?m)^\s*(?:return|yield)\s+([A-Za-z_][\w.]*)\s*(\()?")?.captures(&body)?
    else {
        return Ok(None);
    };
    let direct = if returned.get(2).is_some() {
        Some(returned[1].to_owned())
    } else {
        regex(&format!(
            r"(?m)^\s*{}\s*=\s*([A-Za-z_][\w.]*)\s*\(",
            regex::escape(&returned[1])
        ))?
        .captures(&body)?
        .map(|capture| capture[1].to_owned())
    };
    Ok(direct
        .and_then(|name| name.rsplit('.').next().map(str::to_owned))
        .filter(|name| name.starts_with(|ch: char| ch.is_ascii_uppercase())))
}

fn global_method<'a>(
    receiver: &str,
    member: &str,
    reference: &UnresolvedReference,
    typed: &Typed<'a, '_>,
    scopes: &scope::Scopes<'a>,
    names: &Bindings<'a>,
    environment: &Environment<'a>,
) -> Result<Option<&'a Node>> {
    let path = reference.file_path.as_deref().unwrap_or_default();
    if scopes.locally_bound(receiver, reference)? {
        return Ok(None);
    }
    let nodes = typed.in_file(path);
    if !nodes.iter().any(|node| {
        node.name == receiver
            && matches!(node.kind.as_str(), "variable" | "constant")
            && !nodes.iter().any(|owner| {
                matches!(owner.kind.as_str(), "class" | "function" | "method")
                    && owner.start_line <= node.start_line
                    && owner.end_line >= node.start_line
            })
    }) {
        return Ok(None);
    }
    for &node in typed.named(receiver) {
        if node.file_path == path
            && matches!(node.kind.as_str(), "class" | "function")
            && node.qualified_name.contains("::")
            && names.lexically_reachable(node, reference)?
        {
            return Ok(None);
        }
    }
    let global = nodes
        .iter()
        .find(|node| node.name == receiver && globals::is_global(node, typed))
        .unwrap();
    let targets = globals::members(global, member, reference, typed, scopes, environment)?;
    let Some(node) = targets.first().copied().filter(|_| targets.len() == 1) else {
        return Ok(None);
    };
    Ok(
        (matches!(node.kind.as_str(), "function" | "method") && !property(node, environment)?)
            .then_some(node),
    )
}

fn dispatched_action(node: &Node) -> Result<bool> {
    Ok(matches!(
        node.name.as_str(),
        "get"
            | "post"
            | "put"
            | "patch"
            | "delete"
            | "head"
            | "options"
            | "index"
            | "show"
            | "store"
            | "update"
            | "destroy"
            | "create"
            | "edit"
            | "list"
            | "retrieve"
            | "partial_update"
    ) && regex(r"(?:View|ViewSet|APIView|Controller|Endpoint|ViewMixin)$")?
        .is_match(owner(node))?)
}

fn standard_method(name: &str) -> bool {
    matches!(
        name,
        "get"
            | "setdefault"
            | "pop"
            | "popitem"
            | "update"
            | "items"
            | "keys"
            | "values"
            | "copy"
            | "clear"
            | "append"
            | "extend"
            | "insert"
            | "remove"
            | "index"
            | "count"
            | "sort"
            | "reverse"
            | "add"
            | "discard"
            | "union"
            | "intersection"
            | "difference"
            | "split"
            | "rsplit"
            | "join"
            | "strip"
            | "lstrip"
            | "rstrip"
            | "replace"
            | "startswith"
            | "endswith"
            | "lower"
            | "upper"
            | "format"
            | "encode"
            | "decode"
            | "find"
            | "rfind"
            | "splitlines"
            | "read"
            | "readline"
            | "readlines"
            | "write"
            | "writelines"
            | "close"
            | "seek"
            | "flush"
            | "group"
            | "groups"
            | "match"
            | "search"
            | "sub"
            | "findall"
            | "fullmatch"
    )
}
