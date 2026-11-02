//! Module instance values include writes from every committed source file.
use super::*;
use std::collections::HashSet;

pub(super) fn is_global(node: &Node, typed: &Typed<'_, '_>) -> bool {
    matches!(node.kind.as_str(), "variable" | "constant")
        && !typed.in_file(&node.file_path).iter().any(|owner| {
            matches!(owner.kind.as_str(), "class" | "function" | "method")
                && owner.start_line <= node.start_line
                && owner.end_line >= node.start_line
        })
}

fn constructor(value: &str) -> Result<Option<String>> {
    let mut value = value.trim();
    while value.starts_with('(')
        && statements::closing(&statements::blank_strings(value), 0) == value.len() - 1
    {
        value = value[1..value.len() - 1].trim();
    }
    let Some(capture) = regex(r"^((?:[A-Za-z_]\w*\.)*[A-Z]\w*)\s*\(")?.captures(value)? else {
        return Ok(None);
    };
    let open = capture.get(0).unwrap().end() - 1;
    let end = statements::closing(&statements::blank_strings(value), open);
    Ok((end < value.len()
        && value[end + 1..]
            .trim_matches([' ', '\t', '\r', '\n', '\\'])
            .is_empty())
    .then(|| capture[1].to_owned()))
}

fn dynamic_write(name: &str, path: &str, scopes: &scope::Scopes<'_>) -> Result<bool> {
    for (statement, module) in scopes.module_statements(path)? {
        let blank = statements::blank_strings(&statement);
        let namespace = regex(if module {
            r"\bglobals\(\s*\)|\bvars\(\s*\)|\bsys\.modules\s*\[\s*__name__\s*\]"
        } else {
            r"\bglobals\(\s*\)|\bsys\.modules\s*\[\s*__name__\s*\]"
        })?;
        let uses = namespace
            .find_iter(&blank)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .len();
        if uses == 0 {
            continue;
        }
        let assignment = scope::assignments(&statement)?;
        let mut safe = 0;
        for found in regex(r#"\bglobals\(\s*\)\s*(?:\[\s*(['"])(\w+)\1\s*\]|\.\s*get\s*\()"#)?
            .captures_iter(&statement)
        {
            let found = found?;
            if found.get(2).is_some_and(|key| key.as_str() == name)
                && assignment.as_ref().is_some_and(|assignment| {
                    assignment
                        .targets
                        .iter()
                        .any(|target| target.contains(found.get(0).unwrap().as_str()))
                })
            {
                return Ok(true);
            }
            safe += 1;
        }
        if uses > safe {
            return Ok(true);
        }
    }
    Ok(false)
}

fn module_files(dotted: &str, from: &str, environment: &Environment<'_>) -> Vec<String> {
    let dots = dotted.bytes().take_while(|byte| *byte == b'.').count();
    let mut parts = Vec::new();
    if dots > 0 {
        parts.extend(from.split('/'));
        parts.pop();
        for _ in 1..dots {
            parts.pop();
        }
    }
    parts.extend(dotted[dots..].split('.').filter(|part| !part.is_empty()));
    let path = parts.join("/");
    let wanted = [
        format!("{path}.py"),
        format!("{path}.pyi"),
        format!("{path}/__init__.py"),
    ];
    environment
        .sources
        .keys()
        .filter(|file| {
            wanted
                .iter()
                .any(|want| **file == want || dots == 0 && file.ends_with(&format!("/{want}")))
        })
        .map(|file| (*file).to_owned())
        .collect()
}

fn aliases(
    file: &str,
    target: &str,
    environment: &Environment<'_>,
) -> Result<(Vec<String>, Vec<String>)> {
    let source = environment.sources.get(file).copied().unwrap_or_default();
    let mut aliases = Vec::new();
    let mut ambiguous = Vec::new();
    for mapping in imports::mappings(source)? {
        let module = if mapping.is_namespace {
            mapping.source.clone()
        } else {
            format!(
                "{}{}{}",
                mapping.source,
                if mapping.source.ends_with('.') {
                    ""
                } else {
                    "."
                },
                mapping.exported_name
            )
        };
        let files = module_files(&module, file, environment);
        if !files.iter().any(|file| file == target) {
            continue;
        }
        let explicit = regex(&format!(
            r"\bimport\s[^\n]*(?<![\w.]){}\s+as\s+{}\b",
            regex::escape(&mapping.source),
            regex::escape(&mapping.local_name)
        ))?
        .is_match(&statements::blank_strings(source))?;
        let plain = mapping.is_namespace
            && mapping.source.contains('.')
            && mapping.source.rsplit('.').next() == Some(mapping.local_name.as_str())
            && !explicit;
        let alias = if plain {
            mapping.source
        } else {
            mapping.local_name
        };
        if files.len() == 1 {
            aliases.push(alias);
        } else {
            ambiguous.push(alias);
        }
    }
    Ok((aliases, ambiguous))
}

pub(super) fn members<'a>(
    global: &'a Node,
    member: &str,
    reference: &UnresolvedReference,
    typed: &Typed<'a, '_>,
    scopes: &scope::Scopes<'a>,
    environment: &Environment<'a>,
) -> Result<Vec<&'a Node>> {
    let file = global.file_path.as_str();
    if dynamic_write(&global.name, file, scopes)? {
        return Ok(Vec::new());
    }
    let mut writes = Vec::new();
    for binding in scopes.global_bindings(&global.name, file)? {
        let scope::LineBinding::Assign { ty, value, .. } = binding else {
            return Ok(Vec::new());
        };
        let constructed = constructor(&value)?;
        if let Some(ty) = ty {
            if constructed
                .as_ref()
                .is_some_and(|constructed| constructed.rsplit('.').next() != ty.rsplit('.').next())
            {
                return Ok(Vec::new());
            }
            writes.push((ty, file.to_owned()));
        } else if value != "None" {
            let Some(constructed) = constructed else {
                return Ok(Vec::new());
            };
            writes.push((constructed, file.to_owned()));
        }
    }
    for &other in environment.sources.keys() {
        if other == file || !other.ends_with(".py") && !other.ends_with(".pyi") {
            continue;
        }
        let test =
            crate::classify::is_test_path(other) || other.rsplit('/').next() == Some("conftest.py");
        let (aliases, ambiguous) = aliases(other, file, environment)?;
        if !test && aliases.is_empty() && ambiguous.is_empty() {
            continue;
        }
        let joined = aliases
            .iter()
            .chain(&ambiguous)
            .map(|name| regex::escape(name))
            .collect::<Vec<_>>()
            .join("|");
        let receiver = if test {
            "[\\w.]+".to_owned()
        } else {
            format!("(?:{joined})")
        };
        let name = regex::escape(&global.name);
        let target = regex(&format!(r"(?<![\w.]){receiver}\.{name}\b(?!\s*[.\[(])"))?;
        let exact = regex(&format!(
            r"^(?:{})\.{name}(?:\s*:[^=]*)?$",
            if test {
                receiver.clone()
            } else {
                aliases
                    .iter()
                    .map(|name| regex::escape(name))
                    .collect::<Vec<_>>()
                    .join("|")
            }
        ))?;
        let dynamic = regex(&format!(
            r#"\b(?:setattr|patch\.object)\s*\(\s*{receiver}\s*,\s*['"]{name}['"]|(?<![\w.]){receiver}\.__dict__\s*(?:\[|\.\s*update\s*\()"#
        ))?;
        for (statement, _) in scopes.module_statements(other)? {
            if !statement.contains(&global.name) {
                continue;
            }
            let assignment = scope::assignments(&statement)?;
            let mut assigned = false;
            if let Some(assignment) = &assignment {
                for destination in &assignment.targets {
                    assigned |= target.is_match(destination.trim())?;
                }
            }
            if !assigned && !dynamic.is_match(&statement)? {
                continue;
            }
            if test {
                if reference.file_path.as_deref() == Some(other) {
                    return Ok(Vec::new());
                }
                continue;
            }
            let value = if let Some(assignment) = &assignment
                && assignment.targets.len() == 1
                && !assignment.augmented
                && exact.is_match(assignment.targets[0].trim())?
            {
                assignment.value.trim()
            } else {
                ""
            };
            if value == "None" {
                continue;
            }
            let Some(constructed) = constructor(value)? else {
                return Ok(Vec::new());
            };
            writes.push((constructed, other.to_owned()));
        }
    }
    let mut classes = Vec::new();
    for (name, file) in writes {
        let mappings = imports::mappings(
            environment
                .sources
                .get(file.as_str())
                .copied()
                .unwrap_or_default(),
        )?;
        if mappings.iter().any(|mapping| {
            mapping.is_namespace
                && name.starts_with(&format!("{}.", mapping.source))
                && mappings
                    .iter()
                    .filter(|other| other.local_name == mapping.local_name)
                    .count()
                    != 1
        }) {
            return Ok(Vec::new());
        }
        let Some(class) = typed.class_named(&name, &file, "class")? else {
            return Ok(Vec::new());
        };
        if !classes.iter().any(|other: &&Node| other.id == class.id) {
            classes.push(class);
        }
    }
    let mut targets = Vec::new();
    for class in &classes {
        let members = value_members(class, member, typed, environment, &mut HashSet::new())?;
        if members.is_empty() {
            return Ok(Vec::new());
        }
        for node in members {
            if !targets.iter().any(|other: &&Node| other.id == node.id) {
                targets.push(node);
            }
        }
    }
    if targets.len() <= 1 {
        return Ok(targets);
    }
    let mut queue = targets.clone();
    let mut declarations = Vec::new();
    while let Some(node) = queue.pop() {
        if declarations.len() >= 32 {
            break;
        }
        let Some(owner) = typed
            .in_file(&node.file_path)
            .iter()
            .copied()
            .find(|class| {
                class.kind == "class"
                    && node.qualified_name == format!("{}::{member}", class.qualified_name)
            })
        else {
            continue;
        };
        if declarations
            .iter()
            .any(|(other, _): &(&Node, &Node)| other.id == node.id)
        {
            continue;
        }
        declarations.push((node, owner));
        for base in value_bases(owner, typed, environment)? {
            queue.extend(value_members(
                base,
                member,
                typed,
                environment,
                &mut HashSet::new(),
            )?);
        }
    }
    let mut shared = Vec::new();
    for (node, owner) in declarations {
        let mut all = true;
        for class in &classes {
            all &= class.id == owner.id
                || derives(class, owner, typed, environment, &mut HashSet::new())?;
        }
        if all {
            shared.push((node, owner));
        }
    }
    let mut nearest = Vec::new();
    for (node, owner) in &shared {
        let mut all = true;
        for (_, other) in &shared {
            all &= owner.id == other.id
                || derives(owner, other, typed, environment, &mut HashSet::new())?;
        }
        if all {
            nearest.push(*node);
        }
    }
    Ok(if nearest.len() == 1 { nearest } else { targets })
}
