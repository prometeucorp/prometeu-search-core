//! Behavior callback dispatch with arity, export, and ambiguity gates.
use super::comments::{Language, strip};
use super::*;

pub(super) fn behaviour(ctx: &Context<'_>) -> Result<Vec<Edge>> {
    let mut modules = HashMap::new();
    for node in ctx.nodes {
        ctx.step()?;
        if node.kind == "namespace" && node.language == "erlang" {
            modules.entry(node.file_path.as_str()).or_insert(node);
        }
    }
    if modules.is_empty() {
        return Ok(Vec::new());
    }
    let declaration = regex(r"(^|\n)\s*-callback\s+('[^'\n]+'|[a-z][A-Za-z0-9_@]*)\s*\(")?;
    let mut declarations: HashMap<(String, usize), Vec<&Node>> = HashMap::new();
    let mut names = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !extension(file) || !source.contains("-callback") {
            continue;
        }
        let Some(&module) = modules.get(file) else {
            continue;
        };
        let safe = strip(source, Language::Erlang);
        for found in declaration.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let name = found.get(2).unwrap().as_str().trim_matches('\'');
            let Some(arity) = arity_at(&safe, found.get(0).unwrap().end() - 1) else {
                continue;
            };
            let entries = declarations.entry((name.to_owned(), arity)).or_default();
            if !entries.iter().any(|node| node.id == module.id) {
                entries.push(module);
            }
            names.insert(name.to_owned());
        }
    }
    if declarations.is_empty() {
        return Ok(Vec::new());
    }
    let dispatch = regex(r"(^|[^?\w@'])([A-Z][A-Za-z0-9_@]*):([a-z][A-Za-z0-9_@]*)\(")?;
    let gate = regex(r"[A-Z][A-Za-z0-9_@]*:[a-z]")?;
    let arity_suffix = regex(r"/(\d{1,3})$")?;
    let mut cached_targets: HashMap<(&str, String, usize), Vec<&Node>> = HashMap::new();
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for (&file, &source) in ctx.sources {
        ctx.step()?;
        if !extension(file) || !gate.is_match(source)? {
            continue;
        }
        let safe = strip(source, Language::Erlang);
        for found in dispatch.captures_iter(&safe) {
            ctx.step()?;
            let found = found?;
            let name = found.get(3).unwrap().as_str();
            if !names.contains(name) {
                continue;
            }
            let Some(arity) = arity_at(&safe, found.get(0).unwrap().end() - 1) else {
                continue;
            };
            let Some(behaviours) = declarations.get(&(name.to_owned(), arity)) else {
                continue;
            };
            if behaviours.len() != 1 {
                continue;
            }
            let behaviour = behaviours[0];
            let key = (behaviour.id.as_str(), name.to_owned(), arity);
            if !cached_targets.contains_key(&key) {
                let mut targets = Vec::new();
                for edge in ctx
                    .incoming(&behaviour.id)
                    .filter(|edge| edge.kind == "implements")
                {
                    ctx.step()?;
                    let Some(module) = ctx.node(&edge.source) else {
                        continue;
                    };
                    if module.kind != "namespace" || module.language != "erlang" {
                        continue;
                    }
                    for node in ctx.in_file(&module.file_path) {
                        ctx.step()?;
                        if node.kind != "function"
                            || node.name != name
                            || node.is_exported == Some(false)
                        {
                            continue;
                        }
                        if arity_suffix
                            .captures(&node.qualified_name)?
                            .is_some_and(|found| {
                                found.get(1).unwrap().as_str().parse::<usize>().ok() == Some(arity)
                            })
                        {
                            targets.push(node);
                            break;
                        }
                    }
                }
                cached_targets.insert(key.clone(), targets);
            }
            let targets = &cached_targets[&key];
            if targets.is_empty() || targets.len() > 24 {
                continue;
            }
            let line = ctx.line_at(file, found.get(0).unwrap().start());
            let Some(from) = ctx.enclosing(file, line)? else {
                continue;
            };
            for &target in targets {
                ctx.step()?;
                if from.id != target.id && seen.insert((&from.id, &target.id)) {
                    ctx.emit(
                        &mut output,
                        jobs::dispatched(
                            from,
                            target,
                            "erlang-behaviour",
                            &format!("{}:{name}/{arity}", behaviour.name),
                            file,
                            line,
                        ),
                    )?;
                }
            }
        }
    }
    Ok(output)
}

fn extension(file: &str) -> bool {
    file.ends_with(".erl") || file.ends_with(".hrl")
}

/// The scan ceiling is measured in UTF-16 code units, matching the reference.
fn arity_at(source: &str, open: usize) -> Option<usize> {
    let source: Vec<_> = source[open..].encode_utf16().take(4000).collect();
    let mut i = 1;
    let mut depth = 1usize;
    let mut binary_depth = 0usize;
    let mut commas = 0;
    let mut argument = false;
    while i < source.len() {
        let c = source[i];
        if c == b'"' as u16 || c == b'\'' as u16 {
            i += 1;
            while i < source.len() && source[i] != c {
                if source[i] == b'\\' as u16 {
                    i += 1;
                }
                i += 1;
            }
            argument = true;
        } else if c == b'$' as u16 {
            i += 1;
            if source.get(i) == Some(&(b'\\' as u16)) {
                i += 1;
            }
            argument = true;
        } else if c == b'<' as u16 && source.get(i + 1) == Some(&c) {
            binary_depth += 1;
            i += 1;
            argument = true;
        } else if c == b'>' as u16 && source.get(i + 1) == Some(&c) && binary_depth > 0 {
            binary_depth -= 1;
            i += 1;
        } else if (*b"([{").map(u16::from).contains(&c) {
            depth += 1;
            argument = true;
        } else if (*b")]}").map(u16::from).contains(&c) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(if argument { commas + 1 } else { 0 });
            }
        } else if c == b',' as u16 && depth == 1 && binary_depth == 0 {
            commas += 1;
        } else if !matches!(c, 0x0009..=0x000d | 0x0020 | 0x00a0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff)
        {
            argument = true;
        }
        i += 1;
    }
    None
}
