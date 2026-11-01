use super::*;
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Shape {
    Get,
    Set,
    Function,
    Value,
}
fn shape(source: &str, member: &Node) -> Result<Shape> {
    let head = source
        .split('\n')
        .skip(member.start_line.saturating_sub(1) as usize)
        .take(5)
        .collect::<Vec<_>>()
        .join("\n");
    let mut units = 0;
    let start = head
        .char_indices()
        .find_map(|(at, ch)| {
            if units >= member.start_column {
                return Some(at);
            }
            units += ch.len_utf16() as u32;
            None
        })
        .unwrap_or(head.len());
    let head = &head[start..];
    let mut from = 0;
    if member.name.is_empty() {
        return Ok(Shape::Function);
    }
    loop {
        let Some(at) = head[from..].find(&member.name).map(|at| at + from) else {
            return Ok(Shape::Function);
        };
        from = at + member.name.len();
        let boundary = |ch: Option<char>| {
            ch.is_some_and(|ch| {
                ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '\'' | '"' | '`')
            })
        };
        if boundary(head[..at].chars().next_back()) || boundary(head[from..].chars().next()) {
            continue;
        }
        if let Some(accessor) = regex(r"(?<![\w$])(get|set)\s+$")?.captures(&head[..at])? {
            return Ok(if &accessor[1] == "get" {
                Shape::Get
            } else {
                Shape::Set
            });
        }
        let Some(field) =
            regex(r"^[?!]?\s*(?::[^=;]*(?:=>[^=;]*)*)?=(?![=>])\s*")?.find(&head[from..])?
        else {
            return Ok(Shape::Function);
        };
        let value = regex(r"^async\s+")?.replace(&head[from + field.end()..], "");
        if regex(r"^(?:function(?![\w$])|[A-Za-z_$][\w$]*\s*=>)")?.is_match(&value)? {
            return Ok(Shape::Function);
        }
        let close = if value.starts_with('(') {
            object::matching(&value, 0)?
        } else {
            None
        };
        return Ok(
            if let Some(close) = close.filter(|&end| end > 0)
                && regex(r"^\s*(?::[^=]*)?=>")?.is_match(&value[close + 1..])?
            {
                Shape::Function
            } else {
                Shape::Value
            },
        );
    }
}
pub(super) fn target<'a>(
    candidates: &[&'a Node],
    call: bool,
    source: &str,
    cache: &mut HashMap<String, Shape>,
) -> Result<Option<(&'a Node, &'static str, bool)>> {
    let mut methods = Vec::new();
    for &node in candidates.iter().filter(|node| node.kind == "method") {
        let shape = if let Some(shape) = cache.get(&node.id) {
            *shape
        } else {
            let value = shape(source, node)?;
            cache.insert(node.id.clone(), value);
            value
        };
        methods.push((node, shape));
    }
    if call {
        let target = methods
            .iter()
            .rev()
            .find(|(_, shape)| matches!(shape, Shape::Function | Shape::Value))
            .or_else(|| methods.first())
            .map(|(node, _)| *node)
            .or_else(|| {
                candidates
                    .iter()
                    .copied()
                    .find(|node| node.kind == "property")
            });
        return Ok(target.map(|node| (node, "calls", false)));
    }
    if let Some((getter, _)) = methods.iter().find(|(_, shape)| *shape == Shape::Get) {
        return Ok(Some((getter, "calls", false)));
    }
    Ok(methods
        .iter()
        .rev()
        .find(|(_, shape)| *shape != Shape::Set)
        .map(|(node, shape)| (*node, "references", *shape == Shape::Function)))
}
