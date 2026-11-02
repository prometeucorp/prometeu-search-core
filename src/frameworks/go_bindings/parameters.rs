use super::*;
use std::collections::HashSet;
pub(super) type Parameters = Vec<(u32, u32, HashSet<String>)>;
fn opener(ch: u8, angle: bool) -> bool {
    matches!(ch, b'[' | b'(') || (angle && ch == b'<')
}
fn closer(ch: u8, previous: Option<u8>, angle: bool) -> bool {
    matches!(ch, b']' | b')') || (angle && ch == b'>' && !matches!(previous, Some(b'-' | b'=')))
}
fn collect(text: &str, open: usize, names: &mut HashSet<String>) -> Result<()> {
    let bytes = text.as_bytes();
    let angle = bytes[open] == b'<';
    let expected = if angle {
        b'>'
    } else if bytes[open] == b'[' {
        b']'
    } else {
        b')'
    };
    let mut depth = 0i32;
    let mut end = None;
    for at in open..bytes.len() {
        let ch = bytes[at];
        if matches!(ch, b'{' | b';') {
            return Ok(());
        }
        if opener(ch, angle) {
            depth += 1
        } else if closer(ch, at.checked_sub(1).map(|p| bytes[p]), angle) {
            depth -= 1;
            if depth == 0 {
                if ch == expected {
                    end = Some(at)
                }
                break;
            }
        }
    }
    let Some(end) = end else { return Ok(()) };
    let mut start = open + 1;
    depth = 0;
    let mut parts = Vec::new();
    for at in open + 1..end {
        let ch = bytes[at];
        if opener(ch, angle) {
            depth += 1
        } else if closer(ch, at.checked_sub(1).map(|p| bytes[p]), angle) {
            depth -= 1
        } else if ch == b',' && depth == 0 {
            parts.push(&text[start..at]);
            start = at + 1
        }
    }
    parts.push(&text[start..end]);
    for part in parts {
        let bare = regex(r"^\s*(?:@[\w.]+(?:\([^()]*\))?\s*)+")?.replace(part, "");
        if let Some(m) = regex(
            r"^\s*(?:(?:in|out|reified|const|typename|class|struct)\s+|[+\-']\s*)*([A-Za-z_]\w*)",
        )?
        .captures(&bare)?
            && !matches!(&m[1], "typename" | "class")
        {
            names.insert(m[1].to_owned());
        }
    }
    Ok(())
}
fn declared(head: &str, node: &Node) -> Result<HashSet<String>> {
    let mut names = HashSet::new();
    if !node.name.is_empty() {
        let boundary = |b: Option<u8>| {
            b.is_none_or(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'_' | b'$'))
        };
        let found = head.match_indices(&node.name).find(|(at, _)| {
            boundary(at.checked_sub(1).map(|p| head.as_bytes()[p]))
                && boundary(head.as_bytes().get(at + node.name.len()).copied())
        });
        if let Some((at, _)) = found {
            let mut open = at + node.name.len();
            while matches!(head.as_bytes().get(open), Some(b' ' | b'\t')) {
                open += 1
            }
            if matches!(head.as_bytes().get(open), Some(b'<' | b'[')) {
                collect(head, open, &mut names)?
            }
            for (i, ch) in head[..at].char_indices() {
                if ch == '<'
                    && (i == 0
                        || head[..i].chars().next_back().is_some_and(|ch| {
                            crate::search::query_whitespace(ch) || matches!(ch, '(' | ',')
                        }))
                {
                    collect(head, i, &mut names)?
                }
            }
        }
    }
    if node.language == "go"
        && node.kind == "method"
        && let Some(m) =
            regex(r"^\s*func\s*\(\s*(?:[A-Za-z_]\w*\s+)?\*?\s*[A-Za-z_]\w*\s*\[")?.find(head)?
    {
        collect(head, m.end() - 1, &mut names)?
    }
    Ok(names)
}
impl Bindings<'_> {
    pub(super) fn parameter_bound(&self, r: &UnresolvedReference) -> Result<bool> {
        if !matches!(
            r.reference_kind.as_str(),
            "references" | "type_of" | "returns" | "instantiates"
        ) || !regex(r"^[A-Z][A-Za-z0-9_]*$")?.is_match(&r.reference_name)?
        {
            return Ok(false);
        }
        let file = r.file_path.as_deref().unwrap_or("");
        let Some((&path, &source)) = self.sources.get_key_value(file) else {
            return Ok(false);
        };
        let mut cache = self.parameters.borrow_mut();
        if !cache.contains_key(path) {
            let mut parameters = Vec::new();
            if source.contains(['[', '<']) {
                for &node in self.by_file.get(path).into_iter().flatten() {
                    self.step(1)?;
                    if !matches!(
                        node.kind.as_str(),
                        "class"
                            | "struct"
                            | "interface"
                            | "trait"
                            | "protocol"
                            | "enum"
                            | "type_alias"
                            | "function"
                            | "method"
                            | "union"
                            | "module"
                    ) {
                        continue;
                    }
                    let head = (node.start_line.saturating_sub(1)
                        ..node
                            .end_line
                            .min(node.start_line.saturating_sub(1).saturating_add(40)))
                        .filter_map(|line| self.line(path, line as usize))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.step(head.len() / 32 + 1)?;
                    let names = declared(&head, node)?;
                    if !names.is_empty() {
                        parameters.push((node.start_line, node.end_line, names))
                    }
                }
            }
            cache.insert(path, parameters);
        }
        Ok(cache[path].iter().any(|(start, end, names)| {
            *start <= r.line && r.line <= *end && names.contains(&r.reference_name)
        }))
    }
}
