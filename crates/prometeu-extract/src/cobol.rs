//! Pinned fixed/free-format COBOL AST walker. Provenance: ../UPSTREAM.md.
use crate::supplemental::{children, Context};
use crate::textutil::{slice_utf16, utf16_len};
use crate::ExtractionResult;
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

static MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^([ \t]*)(IDENTIFICATION\s+DIVISION|ID\s+DIVISION|PROGRAM-ID\b|\d{2}[ \t]+[A-Za-z])",
    )
    .unwrap()
});
static SQL_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^([ \t]*(?:[0-9]{6})?[ \t]+EXEC\s+SQL\s+INCLUDE\s+[A-Za-z0-9$#@-]+\s+END-EXEC)([ \t]|$)").unwrap()
});
static SQL_PERIOD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)END-EXEC\s*\.").unwrap());
static PROGRAM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:LINK|XCTL)\b[\s\S]*?\bPROGRAM\s*\(\s*(?:['"]([A-Za-z0-9$#@-]+)['"]|([A-Za-z0-9-]+))\s*\)"#).unwrap()
});
static TRANSID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:RETURN|START)\b[\s\S]*?\bTRANSID\s*\(\s*(?:['"]([A-Za-z0-9$#@]{1,4})['"]|([A-Za-z0-9-]+))\s*\)"#).unwrap()
});
static TRANSID_VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9$#@]{1,4}$").unwrap());
static SQL_INCLUDE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bSQL\b[\s\S]*?\bINCLUDE\s+([A-Za-z0-9$#@-]+)").unwrap());
static VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bVALUE\s+['"]([A-Za-z0-9$#@-]+)['"]"#).unwrap());
static SPECIAL_REGISTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(RETURN-CODE|SQLCODE|SQLSTATE|TALLY|EIB[A-Z-]+|DFH[A-Z-]+|WHEN-COMPILED|LENGTH|ADDRESS)$").unwrap()
});

pub fn is_free_format(source: &str) -> bool {
    source
        .split('\n')
        .find_map(|line| MARKER.captures(line).map(|captures| captures[1].len() < 7))
        .unwrap_or(false)
}

pub fn prepare(source: &str) -> String {
    let free = is_free_format(source);
    source
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            let line = if free && index == 0 {
                format!("CGWIDE {line}")
            } else if free && !line.is_empty() {
                format!("       {line}")
            } else {
                line.into()
            };
            if SQL_PERIOD.is_match(&line) {
                return line;
            }
            let Some(captures) = SQL_LINE.captures(&line) else {
                return line;
            };
            let head = captures.get(1).unwrap();
            if captures[2].is_empty() {
                format!("{}.", head.as_str())
            } else {
                format!("{}.{}", head.as_str(), &line[head.end() + 1..])
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn extract(path: &str, source: &str) -> Result<ExtractionResult, String> {
    let prepared = prepare(source);
    crate::supplemental::run(path, &prepared, "cobol", walk)
}

fn child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    children(node).into_iter().find(|node| node.kind() == kind)
}

fn collapse(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if utf16_len(&flat) > 120 {
        format!("{}…", slice_utf16(&flat, 119).0)
    } else {
        flat
    }
}

fn unquote(text: &str) -> &str {
    text.strip_prefix(['\'', '"'])
        .unwrap_or(text)
        .strip_suffix(['\'', '"'])
        .unwrap_or_else(|| text.strip_prefix(['\'', '"']).unwrap_or(text))
}

fn add_ref(context: &mut Context<'_>, owner: Option<usize>, name: &str, kind: &str, at: Node<'_>) {
    if let Some(owner) = owner {
        if !name.is_empty() {
            context.reference(owner, name, kind, at);
        }
    }
}

fn copy(context: &mut Context<'_>, node: Node<'_>) {
    if let Some(book) = node.child_by_field_name("book") {
        let name = unquote(context.text(book).trim());
        if name.is_empty() {
            return;
        }
        let signature = collapse(context.text(node));
        context.create_node("import", name, node, |node| {
            node.signature = Some(signature)
        });
        add_ref(context, context.current(), name, "imports", node);
    }
}

fn data_value(context: &Context<'_>, name: &str) -> Option<String> {
    context
        .result
        .nodes
        .iter()
        .find(|node| {
            node.file_path == context.path
                && matches!(node.kind.as_str(), "variable" | "field" | "constant")
                && node.name.eq_ignore_ascii_case(name)
        })
        .and_then(|node| node.signature.as_ref())
        .and_then(|signature| VALUE.captures(signature))
        .map(|captures| captures[1].to_owned())
}

fn exec(context: &mut Context<'_>, node: Node<'_>, owner: Option<usize>) {
    let text = context.text(node);
    if let Some(captures) = PROGRAM.captures(text) {
        let name = captures
            .get(1)
            .map(|value| value.as_str().to_owned())
            .or_else(|| {
                captures
                    .get(2)
                    .and_then(|name| data_value(context, name.as_str()))
            });
        if let Some(name) = name {
            add_ref(context, owner, &name, "calls", node);
        }
    }
    if let Some(captures) = TRANSID.captures(text) {
        let name = captures
            .get(1)
            .map(|value| value.as_str().to_owned())
            .or_else(|| {
                captures
                    .get(2)
                    .and_then(|name| data_value(context, name.as_str()))
            });
        if let Some(name) = name.filter(|name| TRANSID_VALUE.is_match(name)) {
            add_ref(
                context,
                owner,
                &format!("cics-transid:{}", name.to_uppercase()),
                "calls",
                node,
            );
        }
    }
    if let Some(captures) = SQL_INCLUDE.captures(text) {
        let signature = collapse(text);
        context.create_node("import", &captures[1], node, |node| {
            node.signature = Some(signature)
        });
        add_ref(
            context,
            owner.or_else(|| context.current()),
            &captures[1],
            "imports",
            node,
        );
    }
}

fn data_entries(context: &mut Context<'_>, entries: &[Node<'_>]) {
    let items: Vec<_> = entries
        .iter()
        .map(|&node| {
            (node.kind() == "data_description").then(|| {
                let level = child(node, "level_number")
                    .and_then(|level| context.text(level).trim().parse::<u32>().ok())
                    .unwrap_or(1);
                let name = child(node, "entry_name").map(|name| context.text(name).trim());
                (level, name)
            })
        })
        .collect();
    let top = |level| matches!(level, 1 | 66 | 77);
    let mut open = Vec::new();
    for (index, &entry) in entries.iter().enumerate() {
        if entry.kind() == "copy_statement" {
            copy(context, entry);
            continue;
        }
        if entry.kind() == "exec_statement" {
            exec(context, entry, context.current());
            continue;
        }
        let Some((level, name)) = items[index] else {
            continue;
        };
        let condition = level == 88;
        if !condition {
            let until = if top(level) { 0 } else { level };
            while open.last().is_some_and(|level| *level >= until) {
                open.pop();
                context.pop_scope();
            }
        }
        let Some(name) =
            name.filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case("FILLER"))
        else {
            continue;
        };
        let kind = if condition {
            "constant"
        } else if open.is_empty() {
            "variable"
        } else {
            "field"
        };
        let mut end_line = entry.end_position().row as u32 + 1;
        if !condition {
            for (next, item) in items.iter().enumerate().skip(index + 1) {
                let Some((next_level, _)) = item else {
                    continue;
                };
                if *next_level != 88 && (*next_level <= level || top(*next_level)) {
                    break;
                }
                end_line = end_line.max(entries[next].end_position().row as u32 + 1);
            }
        }
        let signature = collapse(context.text(entry));
        if let Some(created) = context.create_node(kind, name, entry, |node| {
            node.end_line = end_line;
            node.signature = Some(signature);
        }) {
            if !condition {
                context.push_scope(created);
                open.push(level);
            }
        }
    }
    for _ in open {
        context.pop_scope();
    }
}

fn first_word(node: Node<'_>) -> Option<Node<'_>> {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if node.kind() == "WORD" {
            return Some(node);
        }
        pending.extend(children(node).into_iter().rev());
    }
    None
}

fn writes(context: &mut Context<'_>, statement: Node<'_>, fields: &[&str], owner: Option<usize>) {
    for field in fields {
        let mut cursor = statement.walk();
        for target in statement.children_by_field_name(field, &mut cursor) {
            if let Some(word) = first_word(target) {
                let name = context.text(word).trim();
                if !SPECIAL_REGISTER.is_match(name) {
                    add_ref(context, owner, name, "references", word);
                }
            }
        }
    }
}

fn collect_refs(context: &mut Context<'_>, node: Node<'_>, owner: Option<usize>) {
    stack_guard!();
    match node.kind() {
        "move_statement" => writes(context, node, &["dst"], owner),
        "add_statement" => writes(context, node, &["to", "giving"], owner),
        "compute_statement" => writes(context, node, &["left"], owner),
        "subtract_statement" => writes(
            context,
            node,
            &[if node.child_by_field_name("giving").is_some() {
                "giving"
            } else {
                "from"
            }],
            owner,
        ),
        "perform_statement_call_proc" => {
            if let Some(procedure) = node.child_by_field_name("procedure") {
                for label in children(procedure)
                    .into_iter()
                    .filter(|node| node.kind() == "label")
                {
                    add_ref(context, owner, context.text(label).trim(), "calls", label);
                }
            }
        }
        "call_statement" => {
            if let Some(target) = node
                .child_by_field_name("x")
                .filter(|node| node.kind() == "string")
            {
                add_ref(
                    context,
                    owner,
                    unquote(context.text(target)).trim(),
                    "calls",
                    target,
                );
            }
        }
        "goto_statement" => {
            if let Some(target) = node.child_by_field_name("to") {
                add_ref(context, owner, context.text(target).trim(), "calls", target);
            }
        }
        "exec_statement" => exec(context, node, owner),
        "copy_statement" => copy(context, node),
        _ => {
            for child in children(node) {
                collect_refs(context, child, owner);
            }
        }
    }
}

fn procedure(context: &mut Context<'_>, entries: &[Node<'_>], division_end: u32) {
    let mut current = context.current();
    let mut section = false;
    for (index, &entry) in entries.iter().enumerate() {
        let is_section = entry.kind() == "section_header";
        if is_section || entry.kind() == "paragraph_header" {
            if is_section && section {
                context.pop_scope();
                section = false;
            }
            let end_line = entries
                .iter()
                .skip(index + 1)
                .find(|next| {
                    next.kind() == "section_header"
                        || (!is_section && next.kind() == "paragraph_header")
                })
                .map_or(division_end, |next| {
                    (next.start_position().row as u32).max(entry.start_position().row as u32 + 1)
                });
            let header = context.text(entry).trim().trim_end_matches('.').trim();
            let name = header.split_whitespace().next().unwrap_or(header);
            if let Some(created) = context.create_node("function", name, entry, |node| {
                node.end_line = end_line;
                if is_section {
                    node.signature = Some("SECTION".into());
                }
            }) {
                current = Some(created);
                if is_section {
                    context.push_scope(created);
                    section = true;
                }
            }
        } else {
            collect_refs(context, entry, current);
        }
    }
    if section {
        context.pop_scope();
    }
}

fn walk(context: &mut Context<'_>, node: Node<'_>) {
    stack_guard!();
    match node.kind() {
        "program_definition" => {
            let name = child(node, "identification_division")
                .and_then(|division| child(division, "program_name"))
                .map(|name| unquote(context.text(name).trim()).trim_end_matches('.'));
            let owner = name.and_then(|name| context.create_node("module", name, node, |_| {}));
            if let Some(owner) = owner {
                context.push_scope(owner);
            }
            for child in children(node) {
                walk(context, child);
            }
            if owner.is_some() {
                context.pop_scope();
            }
        }
        "procedure_division" => {
            procedure(context, &children(node), node.end_position().row as u32 + 1)
        }
        "working_storage_section" | "record_description_list" => {
            data_entries(context, &children(node))
        }
        "copybook_fragment" => {
            let entries = children(node);
            if entries
                .iter()
                .any(|child| child.kind() == "record_description_list")
            {
                for child in entries {
                    walk(context, child);
                }
            } else {
                procedure(context, &entries, node.end_position().row as u32 + 1);
            }
        }
        "copy_statement" => copy(context, node),
        "exec_statement" => exec(context, node, context.current()),
        _ => {
            for child in children(node) {
                walk(context, child);
            }
        }
    }
}
