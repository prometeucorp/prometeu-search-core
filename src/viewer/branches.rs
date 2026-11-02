//! Query-time branch and call-site evidence from committed source.
//! SPDX-License-Identifier: MIT. See crates/prometeu-extract/UPSTREAM.md.
use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use tree_sitter::{Node, Point, Tree};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub(super) struct Guard {
    pub text: String,
    pub negated: bool,
    pub form: String,
    pub line: usize,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arm_exit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(super) struct Loop {
    pub text: String,
    pub kind: String,
    pub branch: String,
}
#[derive(Default, Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Site {
    pub guards: Vec<Guard>,
    pub loops: Vec<Loop>,
    pub when: String,
    pub call: Option<Value>,
    pub trigger: Option<Value>,
    pub receiver_type: Option<String>,
    pub available: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rules {
    boundaries: HashSet<String>,
    inline_functions: HashSet<String>,
    binding_parents: HashSet<String>,
    blocks: HashSet<String>,
}
fn rules(language: &str) -> Option<&'static Rules> {
    static RULES: OnceLock<HashMap<String, Rules>> = OnceLock::new();
    RULES
        .get_or_init(|| {
            serde_json::from_str(include_str!("branch-rules.json")).expect("embedded branch rules")
        })
        .get(language)
}
struct Cached {
    fingerprint: blake3::Hash,
    language: String,
    tree: Tree,
}
thread_local! { static TREES:RefCell<Vec<Cached>>=const {RefCell::new(Vec::new())}; }

fn parse_tree(source: &str, grammar: &tree_sitter::Language) -> Option<Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(grammar).ok()?;
    let started = std::time::Instant::now();
    let mut cancel =
        |_: &tree_sitter::ParseState| started.elapsed() > std::time::Duration::from_millis(100);
    parser.parse_with_options(
        &mut |offset, _| source.as_bytes().get(offset..).unwrap_or_default(),
        None,
        Some(tree_sitter::ParseOptions::new().progress_callback(&mut cancel)),
    )
}
pub(super) fn for_edge(graph: &Graph, index: usize) -> Site {
    let edge = &graph.edges[index];
    let file = &graph.files[&graph.nodes[edge.from].path];
    let column = graph.canonical_edges[index]
        .as_ref()
        .and_then(|edge| edge.column)
        .or(edge.column)
        .map(|column| column as usize);
    in_source(
        &file.source,
        &file.analysis.language,
        edge.line,
        column,
        Some(&edge.target),
    )
}
pub(super) fn in_source(
    source: &str,
    language: &str,
    line: usize,
    column: Option<usize>,
    callee: Option<&str>,
) -> Site {
    if source.len() > 256 * 1024 || line == 0 {
        return Site::default();
    }
    let Some(grammar) = prometeu_extract::grammar_for(language) else {
        return Site::default();
    };
    let fingerprint = blake3::hash(source.as_bytes());
    TREES.with(|cache| {
        let mut cache = cache.borrow_mut();
        let position = if let Some(position) = cache
            .iter()
            .position(|entry| entry.language == language && entry.fingerprint == fingerprint)
        {
            position
        } else {
            let Some(tree) = parse_tree(source, &grammar) else {
                return Site::default();
            };
            if cache.len() == 8 {
                cache.remove(0);
            }
            cache.push(Cached {
                fingerprint,
                language: language.into(),
                tree,
            });
            cache.len() - 1
        };
        let root = cache[position].tree.root_node();
        let point = point(source, line, column);
        let Some(start) = root.descendant_for_point_range(point, point) else {
            return Site::default();
        };
        let mut site = Site {
            call: call_site(root, source, line, column, callee),
            trigger: trigger(start, source),
            ..Site::default()
        };
        site.receiver_type = site
            .call
            .as_ref()
            .and_then(|call| call["callee"].as_str())
            .and_then(|callee| member_type(start, source, callee));
        if let Some(rules) = rules(language) {
            site.available = true;
            let mut current = start;
            let mut work = 0;
            while let Some(parent) = current.parent() {
                work += 1;
                if work > 4096 || rules.boundaries.contains(parent.kind()) {
                    break;
                }
                if rules.inline_functions.contains(parent.kind()) {
                    if parent
                        .parent()
                        .is_some_and(|p| rules.binding_parents.contains(p.kind()))
                    {
                        break;
                    }
                    current = parent;
                    continue;
                }
                let before = site.guards.len();
                enclosing(parent, current, source, language, &mut site.guards);
                for guard in &mut site.guards[before..] {
                    if guard.branch.is_empty() {
                        guard.branch = branch(parent, source);
                    }
                    if guard.arm_exit.is_none() {
                        guard.arm_exit = exit_kind(current, source).map(str::to_owned);
                    }
                }
                if rules.blocks.contains(parent.kind()) || parent.kind() == "statement_list" {
                    early_exits(parent, current, source, language, &mut site.guards);
                }
                if let Some(kind) = loop_kind(parent.kind())
                    && !is_field(parent, "condition", current)
                    && !is_field(parent, "value", current)
                {
                    let body = parent
                        .child_by_field_name("body")
                        .or_else(|| children(parent).find(|node| blockish(node.kind())));
                    let raw = body
                        .map(|body| &source[parent.start_byte()..body.start_byte()])
                        .unwrap_or_else(|| text(parent, source));
                    let mut header = collapse(raw);
                    for keyword in ["foreach", "for", "while", "do", "repeat"] {
                        if let Some(rest) = header.strip_prefix(keyword) {
                            header = rest.trim().into();
                            break;
                        }
                    }
                    header = header.trim_end_matches(['{', ':']).trim().into();
                    if header.starts_with('(') && header.ends_with(')') {
                        header = header[1..header.len() - 1].trim().into();
                    }
                    for prefix in ["const ", "let ", "var ", "val ", "final "] {
                        if let Some(rest) = header.strip_prefix(prefix) {
                            header = rest.into();
                            break;
                        }
                    }
                    if !header.is_empty() {
                        site.loops.push(Loop {
                            text: cut(&header, 60),
                            kind: kind.into(),
                            branch: branch(parent, source),
                        });
                    }
                }
                current = parent;
            }
            site.guards.reverse();
            site.loops.reverse();
            site.when = label(&site.guards);
        }
        site
    })
}
fn point(source: &str, line: usize, column: Option<usize>) -> Point {
    let text = source.lines().nth(line.saturating_sub(1)).unwrap_or("");
    let column = column.unwrap_or_else(|| {
        text.chars()
            .take_while(|ch| ch.is_whitespace())
            .map(char::len_utf16)
            .sum()
    });
    let mut units = 0;
    let mut bytes = 0;
    for ch in text.chars() {
        if units + ch.len_utf16() > column {
            break;
        }
        units += ch.len_utf16();
        bytes += ch.len_utf8();
    }
    Point::new(line.saturating_sub(1), bytes)
}
fn branch(node: Node<'_>, source: &str) -> String {
    let point = node.start_position();
    let row = source.lines().nth(point.row).unwrap_or("");
    format!(
        "{}:{}",
        point.row + 1,
        row.get(..point.column).unwrap_or("").encode_utf16().count()
    )
}
fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn cut(value: &str, max: usize) -> String {
    if value.encode_utf16().count() <= max {
        return value.into();
    }
    let mut units = 0;
    let mut out = String::new();
    for ch in value.chars() {
        if units + ch.len_utf16() > max.saturating_sub(1) {
            break;
        }
        units += ch.len_utf16();
        out.push(ch);
    }
    out.push('…');
    out
}
fn children(node: Node<'_>) -> impl DoubleEndedIterator<Item = Node<'_>> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index))
}
fn condition(mut node: Node<'_>, source: &str) -> String {
    while node.kind() == "parenthesized_expression" && node.named_child_count() == 1 {
        node = node.named_child(0).unwrap();
    }
    let value = collapse(text(node, source));
    if matches!(
        value.as_str(),
        "let" | "var" | "case" | "try" | "await" | "guard" | "if" | "else" | "some" | "any"
    ) {
        String::new()
    } else {
        cut(&value, 80)
    }
}
fn is_field(parent: Node<'_>, field: &str, child: Node<'_>) -> bool {
    parent
        .child_by_field_name(field)
        .is_some_and(|node| node.id() == child.id())
}
fn blockish(kind: &str) -> bool {
    matches!(
        kind,
        "statement_block"
            | "block"
            | "statements"
            | "statement_list"
            | "function_body"
            | "compound_statement"
            | "control_structure_body"
            | "else_clause"
            | "else_statement"
            | "catch_clause"
            | "catch_block"
            | "except_clause"
            | "finally_clause"
    )
}
fn exit_kind(mut node: Node<'_>, source: &str) -> Option<&'static str> {
    for _ in 0..256 {
        match node.kind() {
            "throw_statement" | "raise_statement" | "throw_expression" => return Some("throw"),
            "return_statement" | "break_statement" | "continue_statement" | "goto_statement" => {
                return Some("return");
            }
            "yield_statement" => {
                return (0..node.child_count())
                    .filter_map(|index| node.child(index))
                    .any(|child| child.kind() == "break")
                    .then_some("return");
            }
            "control_transfer_statement" | "jump_expression" => {
                return Some(if text(node, source).trim_start().starts_with("throw") {
                    "throw"
                } else {
                    "return"
                });
            }
            kind if blockish(kind) => node = children(node).next_back()?,
            _ => return None,
        }
    }
    None
}
fn loop_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "for_statement"
        | "for_in_statement"
        | "for_of_statement"
        | "for_each_statement"
        | "enhanced_for_statement"
        | "foreach_statement"
        | "for_range_loop"
        | "for_expression" => Some("each"),
        "while_statement"
        | "while_expression"
        | "do_statement"
        | "do_while_statement"
        | "repeat_while_statement" => Some("while"),
        _ => None,
    }
}
fn push(
    out: &mut Vec<Guard>,
    form: &str,
    cond: Option<Node<'_>>,
    negated: bool,
    override_text: Option<String>,
    branch_node: Option<Node<'_>>,
    source: &str,
) {
    let value = override_text
        .unwrap_or_else(|| cond.map(|node| condition(node, source)).unwrap_or_default());
    if !value.is_empty() {
        out.push(Guard {
            text: value,
            negated,
            form: form.into(),
            line: cond.map_or(0, |node| node.start_position().row + 1),
            branch: branch_node
                .map(|node| branch(node, source))
                .unwrap_or_default(),
            arm_exit: None,
            exit: None,
        });
    }
}
fn swift_conditions<'a>(node: Node<'a>, source: &str) -> (Option<Node<'a>>, String) {
    let parts = (0..node.child_count())
        .filter(|&i| node.field_name_for_child(i as u32) == Some("condition"))
        .filter_map(|i| node.child(i))
        .collect::<Vec<_>>();
    match (parts.first(), parts.last()) {
        (Some(first), Some(last)) => (
            Some(*first),
            cut(&collapse(&source[first.start_byte()..last.end_byte()]), 80),
        ),
        _ => (None, String::new()),
    }
}
fn after_else(parent: Node<'_>, child: Node<'_>) -> bool {
    let mut yes = false;
    for i in 0..parent.child_count() {
        let Some(node) = parent.child(i) else {
            continue;
        };
        if node.id() == child.id() {
            return yes;
        }
        if node.kind() == "else" {
            yes = true;
        }
    }
    false
}
fn enclosing(
    parent: Node<'_>,
    child: Node<'_>,
    source: &str,
    language: &str,
    out: &mut Vec<Guard>,
) {
    let kind = parent.kind();
    if kind == "if_statement" || kind == "elif_clause" {
        if language == "swift" {
            if (0..parent.child_count()).any(|i| {
                parent.child(i).is_some_and(|n| n.id() == child.id())
                    && parent.field_name_for_child(i as u32) == Some("condition")
            }) {
                return;
            }
            let (cond, value) = swift_conditions(parent, source);
            let negated = after_else(parent, child);
            push(
                out,
                if negated { "else" } else { "if" },
                cond,
                negated,
                Some(value),
                None,
                source,
            );
            return;
        }
        let cond = parent.child_by_field_name("condition");
        if is_field(parent, "consequence", child) {
            push(out, "if", cond, false, None, None, source);
        } else if is_field(parent, "alternative", child)
            || matches!(child.kind(), "elif_clause" | "else_clause")
        {
            push(out, "else", cond, true, None, None, source);
            if language == "python" {
                for previous in children(parent)
                    .take_while(|node| node.id() != child.id())
                    .filter(|node| node.kind() == "elif_clause")
                {
                    push(
                        out,
                        "else",
                        previous.child_by_field_name("condition"),
                        true,
                        None,
                        None,
                        source,
                    );
                }
            }
        }
        return;
    }
    if kind == "guard_statement" && language == "swift" {
        let (cond, value) = swift_conditions(parent, source);
        if !is_field(parent, "condition", child) {
            push(out, "else", cond, true, Some(value), None, source);
        }
        return;
    }
    if kind == "if_expression" && language == "kotlin" {
        let cond = children(parent).next();
        let arms = children(parent)
            .filter(|node| node.kind() == "control_structure_body")
            .collect::<Vec<_>>();
        for (index, arm) in arms.iter().enumerate() {
            if arm.id() == child.id() {
                push(
                    out,
                    if index == 0 { "if" } else { "else" },
                    cond,
                    index != 0,
                    None,
                    None,
                    source,
                );
            }
        }
        return;
    }
    if matches!(kind, "ternary_expression" | "conditional_expression") {
        if language == "python" {
            let kids = children(parent).collect::<Vec<_>>();
            if kids.len() >= 3 {
                if child.id() == kids[0].id() {
                    push(out, "ternary", Some(kids[1]), false, None, None, source);
                } else if child.id() == kids[2].id() {
                    push(out, "ternary", Some(kids[1]), true, None, None, source);
                }
            }
        } else {
            let cond = parent.child_by_field_name("condition");
            if is_field(parent, "consequence", child) || is_field(parent, "if_true", child) {
                push(out, "ternary", cond, false, None, None, source);
            } else if is_field(parent, "alternative", child) || is_field(parent, "if_false", child)
            {
                push(out, "ternary", cond, true, None, None, source);
            }
        }
        return;
    }
    if matches!(
        kind,
        "binary_expression"
            | "boolean_operator"
            | "conjunction_expression"
            | "disjunction_expression"
    ) {
        let left = parent
            .child_by_field_name("left")
            .or_else(|| children(parent).next());
        let right = parent
            .child_by_field_name("right")
            .or_else(|| children(parent).next_back());
        if right.is_none_or(|node| node.id() != child.id()) {
            return;
        }
        let operator = parent
            .child_by_field_name("operator")
            .map(|node| text(node, source))
            .unwrap_or_else(|| {
                if kind == "conjunction_expression" {
                    "&&"
                } else if kind == "disjunction_expression" {
                    "||"
                } else {
                    (0..parent.child_count())
                        .filter_map(|i| parent.child(i))
                        .find_map(|node| {
                            matches!(node.kind(), "and" | "or").then_some(text(node, source))
                        })
                        .unwrap_or("")
                }
            });
        if matches!(operator, "&&" | "and") {
            push(out, "and", left, false, None, None, source);
        } else if matches!(operator, "||" | "or") {
            push(out, "or", left, true, None, None, source);
        }
        return;
    }
    if matches!(
        kind,
        "catch_clause" | "catch_block" | "except_clause" | "except_group_clause"
    ) {
        if !is_field(parent, "parameter", child)
            && (matches!(
                language,
                "typescript" | "tsx" | "javascript" | "jsx" | "swift"
            ) || blockish(child.kind())
                || is_field(parent, "body", child))
        {
            push(
                out,
                "catch",
                None,
                false,
                Some("on error".into()),
                None,
                source,
            );
        }
        return;
    }
    if matches!(
        kind,
        "switch_case"
            | "switch_default"
            | "switch_entry"
            | "case_clause"
            | "switch_block_statement_group"
            | "switch_rule"
            | "when_entry"
            | "switch_section"
            | "switch_expression_arm"
            | "expression_case"
            | "type_case"
            | "communication_case"
            | "default_case"
            | "case_statement"
    ) {
        let mut statement = parent.parent();
        for _ in 0..3 {
            if statement.is_some_and(|node| {
                matches!(
                    node.kind(),
                    "switch_body" | "switch_block" | "block" | "compound_statement"
                )
            }) {
                statement = statement.and_then(|node| node.parent());
            } else {
                break;
            }
        }
        let condition_field = match language {
            "swift" => "expr",
            "python" => "subject",
            "java" | "c" | "cpp" => "condition",
            _ => "value",
        };
        let subject_node = statement.and_then(|node| {
            node.child_by_field_name(condition_field).or_else(|| {
                (language == "kotlin")
                    .then(|| children(node).find(|child| child.kind() == "when_subject"))
                    .flatten()
            })
        });
        let subject = subject_node
            .map(|node| condition(node, source))
            .unwrap_or_default()
            .trim_matches(['(', ')'])
            .to_owned();
        let patterns = children(parent)
            .filter(|node| {
                matches!(
                    node.kind(),
                    "switch_pattern" | "case_pattern" | "switch_label" | "when_condition"
                ) || (language == "csharp"
                    && (node.kind().ends_with("pattern") || node.kind().ends_with("switch_label")))
            })
            .collect::<Vec<_>>();
        let value_node = parent
            .child_by_field_name("value")
            .or_else(|| parent.child_by_field_name("type"))
            .or_else(|| parent.child_by_field_name("communication"))
            .or_else(|| parent.child_by_field_name("pattern"))
            .or_else(|| patterns.first().copied());
        if value_node.is_some_and(|node| node.id() == child.id())
            || patterns.iter().any(|node| node.id() == child.id())
        {
            return;
        }
        if matches!(kind, "case_clause" | "when_entry" | "switch_expression_arm")
            && !(is_field(parent, "consequence", child)
                || is_field(parent, "expression", child)
                || child.kind() == "control_structure_body")
        {
            return;
        }
        let value = if language == "java" {
            patterns
                .iter()
                .flat_map(|&node| children(node))
                .map(|node| condition(node, source))
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        } else if patterns.len() > 1 {
            patterns
                .iter()
                .map(|&node| condition(node, source))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            value_node
                .map(|node| condition(node, source))
                .unwrap_or_default()
        };
        let default = matches!(kind, "switch_default" | "default_case")
            || value.is_empty()
            || value == "_"
            || (0..parent.child_count())
                .filter_map(|i| parent.child(i))
                .any(|node| node.kind() == "default_keyword");
        let value = if kind == "communication_case" {
            value
        } else if default {
            let label = if language == "kotlin" {
                "else"
            } else {
                "default"
            };
            if subject.is_empty() {
                label.into()
            } else {
                format!("{subject}: {label}")
            }
        } else if subject.is_empty() {
            value
        } else {
            let operator = if matches!(language, "typescript" | "tsx" | "javascript" | "jsx") {
                "==="
            } else {
                "=="
            };
            format!("{subject} {operator} {value}")
        };
        let cond = if default
            && matches!(
                kind,
                "switch_default" | "default_case" | "case_statement" | "switch_entry"
            ) {
            subject_node
        } else {
            value_node
        };
        push(out, "case", cond, false, Some(value), statement, source);
    }
}
fn early_exits(
    parent: Node<'_>,
    child: Node<'_>,
    source: &str,
    language: &str,
    out: &mut Vec<Guard>,
) {
    let previous = children(parent)
        .take_while(|node| node.id() != child.id())
        .collect::<Vec<_>>();
    for node in previous.into_iter().rev() {
        if !matches!(
            node.kind(),
            "if_statement" | "if_expression" | "guard_statement"
        ) {
            continue;
        }
        if node.child_by_field_name("alternative").is_some()
            || children(node).any(|node| matches!(node.kind(), "elif_clause" | "else_clause"))
            || (0..node.child_count())
                .filter_map(|i| node.child(i))
                .any(|node| node.kind() == "else")
                && node.kind() != "guard_statement"
        {
            continue;
        }
        let arms = children(node)
            .filter(|node| node.kind() == "control_structure_body")
            .collect::<Vec<_>>();
        if language == "kotlin" && arms.len() > 1 {
            continue;
        }
        let body = node.child_by_field_name("consequence").or_else(|| {
            children(node)
                .find(|node| matches!(node.kind(), "statements" | "control_structure_body"))
        });
        let exit = body.and_then(|body| exit_kind(body, source)).or_else(|| {
            if !matches!(language, "go" | "c" | "cpp") {
                return None;
            }
            let mut body = body?;
            for _ in 0..20 {
                if blockish(body.kind()) {
                    body = children(body).next_back()?;
                } else {
                    break;
                }
            }
            let call = body.named_child(0)?;
            let callee = call.child_by_field_name("function")?;
            matches!(
                text(callee, source),
                "panic"
                    | "os.Exit"
                    | "log.Fatal"
                    | "log.Fatalf"
                    | "log.Fatalln"
                    | "exit"
                    | "_exit"
                    | "abort"
                    | "longjmp"
            )
            .then_some("exit")
        });
        if exit.is_none() && node.kind() != "guard_statement" {
            continue;
        }
        let (cond, value) = if language == "swift" {
            swift_conditions(node, source)
        } else {
            let cond = node.child_by_field_name("condition").or_else(|| {
                (language == "kotlin")
                    .then(|| children(node).next())
                    .flatten()
            });
            (
                cond,
                cond.map(|node| condition(node, source)).unwrap_or_default(),
            )
        };
        let before = out.len();
        push(
            out,
            "guard",
            cond,
            node.kind() != "guard_statement",
            Some(value),
            Some(node),
            source,
        );
        if out.len() > before {
            out.last_mut().unwrap().exit = Some(exit.unwrap_or("exit").into());
        }
    }
}
pub(super) fn label(guards: &[Guard]) -> String {
    guards
        .iter()
        .map(|guard| {
            if guard.form == "catch" {
                return guard.text.clone();
            }
            let text = &guard.text;
            if !guard.negated {
                return if top_or(text) {
                    format!("({text})")
                } else {
                    text.clone()
                };
            }
            if let Some(rest) = text.strip_prefix('!').filter(|rest| simple(rest)) {
                return rest.into();
            }
            if let Some(rest) = text.strip_prefix("not ").filter(|rest| simple(rest.trim())) {
                return rest.trim().into();
            }
            if !text.contains(['&', '|', '?']) && !text.contains(" and ") && !text.contains(" or ")
            {
                for (operator, opposite) in [
                    ("!==", "==="),
                    ("===", "!=="),
                    ("!=", "=="),
                    ("==", "!="),
                    (" is not ", "is"),
                    (" is ", "is not"),
                ] {
                    if let Some((left, right)) = text.split_once(operator)
                        && !left.contains(['=', '!', '<', '>'])
                        && !right.contains(['=', '!', '<', '>'])
                    {
                        return format!("{} {opposite} {}", left.trim(), right.trim());
                    }
                }
            }
            if simple(text) {
                format!("!{text}")
            } else {
                format!("!({text})")
            }
        })
        .collect::<Vec<_>>()
        .join(" && ")
}
fn top_or(text: &str) -> bool {
    let mut depth = 0;
    let mut quote = None;
    let mut escaped = false;
    let chars = text.chars().collect::<Vec<_>>();
    for (i, &ch) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if let Some(current) = quote {
            if ch == '\\' {
                escaped = true;
            } else if ch == current {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            '|' if depth == 0 && chars.get(i + 1) == Some(&'|') => return true,
            _ => {}
        }
    }
    false
}
fn simple(text: &str) -> bool {
    if text.contains(['=', '<', '>'])
        || text.contains(" && ")
        || text.contains(" || ")
        || text.contains(" and ")
        || text.contains(" or ")
    {
        return false;
    }
    let head = text.split('(').next().unwrap_or("");
    if head.is_empty()
        || !head
            .chars()
            .all(|ch| ch.is_alphanumeric() || "_$ .?!".contains(ch) && ch != ' ')
    {
        return false;
    }
    let tail = &text[head.len()..];
    if tail.is_empty() {
        return true;
    }
    let mut depth = 0;
    for (index, ch) in tail.char_indices() {
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 && index + 1 != tail.len() {
                return false;
            }
        }
    }
    depth == 0
}
fn call_kind(kind: &str) -> bool {
    matches!(
        kind,
        "call_expression"
            | "new_expression"
            | "call"
            | "method_invocation"
            | "object_creation_expression"
            | "invocation_expression"
            | "call_expression_suffix"
            | "function_call"
            | "method_call_expression"
            | "scoped_call_expression"
    )
}
fn arguments(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("arguments")
        .or_else(|| {
            children(node).find(|node| {
                matches!(
                    node.kind(),
                    "arguments" | "argument_list" | "value_arguments"
                )
            })
        })
        .or_else(|| {
            children(node)
                .find(|node| node.kind() == "call_suffix")
                .and_then(|node| {
                    children(node)
                        .find(|node| matches!(node.kind(), "value_arguments" | "lambda_literal"))
                })
        })
}
fn callee_text(call: Node<'_>, args: Node<'_>, source: &str) -> String {
    let end = if args
        .parent()
        .is_some_and(|node| node.kind() == "call_suffix")
    {
        args.parent().unwrap().start_byte()
    } else {
        args.start_byte()
    };
    let mut value = collapse(&source[call.start_byte()..end]);
    for _ in 0..2 {
        for prefix in ["await ", "new ", "yield ", "return "] {
            if let Some(rest) = value.strip_prefix(prefix) {
                value = rest.into();
                break;
            }
        }
    }
    cut(
        &value
            .replace("?.", ".")
            .replace("!.", ".")
            .split_whitespace()
            .collect::<String>(),
        96,
    )
}
fn abbreviate(node: Node<'_>, source: &str) -> String {
    match node.kind() {
        "array" | "array_creation_expression" | "list" | "list_literal" => "[…]".into(),
        "arrow_function" | "function_expression" | "lambda" | "lambda_expression" => {
            "() => …".into()
        }
        "lambda_literal" | "trailing_closure" | "lambda_argument" => "{ … }".into(),
        "object" | "dictionary" => {
            let keys = children(node)
                .filter_map(|node| {
                    node.child_by_field_name("key").or_else(|| {
                        matches!(
                            node.kind(),
                            "shorthand_property_identifier" | "spread_element"
                        )
                        .then_some(node)
                    })
                })
                .map(|node| cut(text(node, source), 24))
                .collect::<Vec<_>>();
            if keys.is_empty() {
                "{…}".into()
            } else {
                format!(
                    "{{ {}{} }}",
                    keys.iter().take(4).cloned().collect::<Vec<_>>().join(", "),
                    if keys.len() > 4 { ", …" } else { "" }
                )
            }
        }
        kind if call_kind(kind) => {
            let callee = node
                .child_by_field_name("function")
                .or_else(|| node.child_by_field_name("constructor"))
                .or_else(|| node.named_child(0));
            format!(
                "{}{}(…)",
                if kind == "new_expression" { "new " } else { "" },
                callee
                    .map(|node| cut(&collapse(text(node, source)), 28))
                    .unwrap_or_default()
            )
        }
        _ => cut(&collapse(text(node, source)), 48),
    }
}
fn call_site(
    root: Node<'_>,
    source: &str,
    line: usize,
    column: Option<usize>,
    want: Option<&str>,
) -> Option<Value> {
    let want = want.map(|name| name.rsplit(['.', ':']).next().unwrap_or(name));
    let columns = column
        .map(|column| {
            vec![
                Some(column),
                Some(column + 1),
                Some(column.saturating_sub(1)),
                None,
            ]
        })
        .unwrap_or_else(|| vec![None]);
    for column in columns {
        let point = point(source, line, column);
        let mut current = root.descendant_for_point_range(point, point);
        let mut first = None;
        let mut chosen = None;
        for _ in 0..10 {
            let Some(node) = current else {
                break;
            };
            if call_kind(node.kind()) {
                first.get_or_insert(node);
                if let Some(args) = arguments(node) {
                    let callee = callee_text(node, args, source);
                    let bare = callee.rsplit(['.', ':']).next().unwrap_or(&callee);
                    if want.is_none_or(|want| want == bare) {
                        chosen = Some(node);
                        break;
                    }
                }
            }
            current = node.parent();
        }
        let Some(call) = chosen.or(first) else {
            continue;
        };
        let Some(args) = arguments(call) else {
            continue;
        };
        let parts = children(args)
            .filter(|node| node.kind() != "comment")
            .map(|node| abbreviate(node, source))
            .collect::<Vec<_>>();
        let callee = callee_text(call, args, source);
        let mut result = json!({"callee":callee,"args":cut(&parts.join(", "),120),"argList":parts,"span":{"start":position(call.start_position(),source),"end":position(call.end_position(),source)}});
        if let Some(within) = enclosing_call(call, source) {
            result["within"] = json!(within);
        }
        if let Some(status) = children(args)
            .find_map(|arg| status_property(arg, source, 0))
            .or_else(|| status_before(call, &callee, source))
        {
            result["status"] = json!(status);
        }
        return Some(result);
    }
    None
}
fn position(point: Point, source: &str) -> Value {
    let row = source.lines().nth(point.row).unwrap_or("");
    json!({"line":point.row+1,"column":row.get(..point.column).unwrap_or("").encode_utf16().count()})
}
fn enclosing_call(call: Node<'_>, source: &str) -> Option<String> {
    let mut parent = call.parent();
    for _ in 0..12 {
        let node = parent?;
        if [
            "function",
            "lambda",
            "closure",
            "block",
            "statement",
            "body",
            "declaration",
        ]
        .iter()
        .any(|kind| node.kind().contains(kind))
        {
            return None;
        }
        if call_kind(node.kind()) {
            let args = arguments(node)?;
            return (args.start_byte() <= call.start_byte() && call.end_byte() <= args.end_byte())
                .then(|| callee_text(node, args, source));
        }
        parent = node.parent();
    }
    None
}
fn status_property(node: Node<'_>, source: &str, depth: usize) -> Option<u16> {
    if depth > 2 {
        return None;
    }
    for child in children(node) {
        if matches!(
            child.kind(),
            "pair"
                | "keyword_argument"
                | "named_argument"
                | "property_assignment"
                | "object_property"
        ) {
            let key = child
                .child_by_field_name("key")
                .or_else(|| child.child_by_field_name("name"))
                .or_else(|| child.named_child(0));
            let value = child
                .child_by_field_name("value")
                .or_else(|| children(child).next_back());
            if let (Some(key), Some(value)) = (key, value)
                && matches!(
                    text(key, source).trim_matches(['\'', '"']),
                    "status" | "statusCode" | "status_code" | "code"
                )
                && let Ok(status) = text(value, source).parse::<u16>()
                && (100..600).contains(&status)
            {
                return Some(status);
            }
        }
        if let Some(status) = status_property(child, source, depth + 1) {
            return Some(status);
        }
    }
    None
}
fn status_before(mut call: Node<'_>, callee: &str, source: &str) -> Option<u16> {
    let (receiver, method) = callee.split_once('.')?;
    if !matches!(
        receiver,
        "res" | "response" | "reply" | "rep" | "ctx" | "c" | "context"
    ) || !matches!(
        method,
        "json"
            | "jsonp"
            | "send"
            | "render"
            | "sendFile"
            | "download"
            | "end"
            | "text"
            | "html"
            | "body"
    ) {
        return None;
    }
    while let Some(parent) = call.parent() {
        if matches!(
            parent.kind(),
            "statement_block" | "program" | "block" | "class_body" | "module"
        ) {
            break;
        }
        call = parent;
    }
    let mut previous = call.prev_named_sibling();
    for _ in 0..6 {
        let node = previous?;
        previous = node.prev_named_sibling();
        let compact = text(node, source).split_whitespace().collect::<String>();
        let compact = compact
            .strip_prefix("await")
            .unwrap_or(&compact)
            .trim_end_matches(';');
        for method in ["status", "code"] {
            if let Some(value) = compact
                .strip_prefix(&format!("{receiver}.{method}("))
                .and_then(|value| value.strip_suffix(')'))
                && let Ok(status) = value.parse::<u16>()
                && (100..600).contains(&status)
            {
                return Some(status);
            }
        }
        if let Some(value) = compact.strip_prefix(&format!("{receiver}.statusCode="))
            && let Ok(status) = value.parse::<u16>()
            && (100..600).contains(&status)
        {
            return Some(status);
        }
    }
    None
}
fn member_type(mut node: Node<'_>, source: &str, callee: &str) -> Option<String> {
    let receiver = callee
        .strip_prefix("this.")
        .or_else(|| callee.strip_prefix("self."))
        .unwrap_or(callee)
        .split(['.', ':', '('])
        .next()?;
    if receiver.is_empty() || receiver.chars().next()?.is_uppercase() {
        return None;
    }
    for _ in 0..16 {
        if matches!(
            node.kind(),
            "class_declaration"
                | "class_definition"
                | "class"
                | "object_declaration"
                | "struct_declaration"
                | "record_declaration"
        ) {
            break;
        }
        node = node.parent()?;
    }
    if !matches!(
        node.kind(),
        "class_declaration"
            | "class_definition"
            | "class"
            | "object_declaration"
            | "struct_declaration"
            | "record_declaration"
    ) {
        return None;
    }
    let mut types = BTreeMap::new();
    let mut put = |name: Option<Node<'_>>, kind: Option<Node<'_>>| {
        if let (Some(name), Some(kind)) = (name, kind) {
            let kind = collapse(text(kind, source))
                .trim_start_matches(':')
                .trim()
                .to_owned();
            if !kind.is_empty() {
                types.entry(text(name, source).to_owned()).or_insert(kind);
            }
        }
    };
    let mut visit_params = |params: Node<'_>| {
        for param in children(params) {
            match param.kind() {
                "required_parameter" | "optional_parameter"
                    if children(param).any(|c| {
                        matches!(c.kind(), "accessibility_modifier" | "override_modifier")
                    }) || ["public ", "private ", "protected ", "readonly "]
                        .iter()
                        .any(|prefix| text(param, source).trim_start().starts_with(prefix)) =>
                {
                    put(
                        param.child_by_field_name("pattern"),
                        param.child_by_field_name("type"),
                    )
                }
                "parameter" | "formal_parameter" => put(
                    param.child_by_field_name("name"),
                    param.child_by_field_name("type"),
                ),
                "class_parameter"
                    if children(param).any(|c| c.kind() == "binding_pattern_kind") =>
                {
                    put(
                        children(param).find(|c| c.kind() == "simple_identifier"),
                        children(param).find(|c| matches!(c.kind(), "user_type" | "nullable_type")),
                    )
                }
                _ => {}
            }
        }
    };
    for ctor in children(node).filter(|c| c.kind() == "primary_constructor") {
        visit_params(
            children(ctor)
                .find(|c| c.kind() == "class_parameters")
                .unwrap_or(ctor),
        );
    }
    let body = children(node)
        .find(|c| {
            matches!(
                c.kind(),
                "class_body" | "declaration_list" | "field_declaration_list"
            )
        })
        .or_else(|| node.child_by_field_name("body"))?;
    for member in children(body) {
        if (member.kind() == "constructor_declaration"
            || member.kind() == "method_definition"
                && member
                    .child_by_field_name("name")
                    .is_some_and(|n| text(n, source) == "constructor"))
            && let Some(params) = member.child_by_field_name("parameters")
        {
            visit_params(params);
        }
    }
    for member in children(body) {
        match member.kind() {
            "public_field_definition" | "field_definition" => put(
                member.child_by_field_name("name"),
                member.child_by_field_name("type"),
            ),
            "field_declaration" => {
                let decl = if member.child_by_field_name("type").is_some() {
                    Some(member)
                } else {
                    children(member).find(|c| c.kind() == "variable_declaration")
                };
                if let Some(decl) = decl {
                    for item in children(decl).filter(|c| c.kind() == "variable_declarator") {
                        put(
                            item.child_by_field_name("name"),
                            decl.child_by_field_name("type"),
                        );
                    }
                }
            }
            "property_declaration" => {
                if member.child_by_field_name("type").is_some() {
                    put(
                        member.child_by_field_name("name"),
                        member.child_by_field_name("type"),
                    );
                } else if let Some(decl) =
                    children(member).find(|c| c.kind() == "variable_declaration")
                {
                    put(
                        children(decl).find(|c| c.kind() == "simple_identifier"),
                        children(decl).find(|c| matches!(c.kind(), "user_type" | "nullable_type")),
                    );
                }
            }
            _ => {}
        }
    }
    types
        .get(receiver)
        .or_else(|| types.get(receiver.trim_start_matches('_')))
        .cloned()
}
fn trigger(mut node: Node<'_>, source: &str) -> Option<Value> {
    let mut deferred = false;
    for _ in 0..32 {
        if matches!(
            node.kind(),
            "function_declaration" | "method_definition" | "class_declaration" | "program"
        ) {
            return None;
        }
        if matches!(node.kind(), "arrow_function" | "function_expression") {
            if node
                .parent()
                .is_some_and(|node| node.kind() == "variable_declarator")
            {
                return None;
            }
            deferred = true;
        }
        if node.kind() == "jsx_attribute" {
            let name = node
                .named_child(0)
                .map(|node| text(node, source))
                .unwrap_or("prop");
            if !deferred
                && !name
                    .strip_prefix("on")
                    .is_some_and(|name| name.starts_with(char::is_uppercase))
            {
                return None;
            }
            let owner = node
                .parent()
                .and_then(|node| node.child_by_field_name("name"))
                .map(|node| collapse(text(node, source)));
            return Some(json!({"kind":"prop","name":name,"of":owner}));
        }
        if node.kind() == "pair"
            && let Some(key) = node.child_by_field_name("key")
        {
            let name = text(key, source).trim_matches(['\'', '"', '`']);
            if name
                .strip_prefix("on")
                .is_some_and(|rest| rest.starts_with(char::is_uppercase))
            {
                return Some(json!({"kind":"option","name":name,"of":null}));
            }
        }
        if node.kind() == "arguments"
            && let Some(call) = node.parent().filter(|node| call_kind(node.kind()))
        {
            let callee = callee_text(call, node, source);
            let name = callee.rsplit('.').next().unwrap_or(&callee);
            if matches!(
                name,
                "useEffect"
                    | "useLayoutEffect"
                    | "setTimeout"
                    | "setInterval"
                    | "requestAnimationFrame"
                    | "addListener"
                    | "addEventListener"
                    | "on"
                    | "once"
                    | "then"
                    | "catch"
                    | "finally"
                    | "subscribe"
            ) {
                let of = node
                    .named_child(0)
                    .filter(|node| matches!(node.kind(), "string" | "string_literal"))
                    .map(|node| cut(text(node, source), 48));
                return Some(json!({"kind":"callback","name":name,"of":of}));
            }
        }
        node = node.parent()?;
    }
    None
}

pub(super) fn highlights(source: &str, language: &str, from: usize, to: usize) -> Vec<Value> {
    if source.len() > 256 * 1024 {
        return vec![];
    }
    let Some(grammar) = prometeu_extract::grammar_for(language) else {
        return vec![];
    };
    let fingerprint = blake3::hash(source.as_bytes());
    TREES.with(|cache| {
        let mut cache=cache.borrow_mut();
        let index=if let Some(index)=cache.iter().position(|entry|entry.language==language && entry.fingerprint==fingerprint){index}else{
            let mut parser=tree_sitter::Parser::new();if parser.set_language(&grammar).is_err(){return vec![];}
            let Some(tree)=parser.parse(source,None)else{return vec![];};
            if cache.len()==8{cache.remove(0);}cache.push(Cached{fingerprint,language:language.into(),tree});cache.len()-1
        };
        let lines=source.lines().collect::<Vec<_>>();
        let mut stack=vec![cache[index].tree.root_node()];let mut tokens=vec![];
        while let Some(node)=stack.pop(){
            if tokens.len()==20_000{break;}
            let first=node.start_position();let end=node.end_position();
            if first.row+1>to || end.row+1<from {continue;}
            let kind=node.kind();
            let category=if kind.contains("comment"){Some("comment")}
                else if matches!(kind,"string"|"string_literal"|"raw_string_literal"|"interpreted_string_literal"|"char_literal"|"character_literal"|"template_string"){Some("string")}
                else if kind.contains("integer")||kind.contains("float")||matches!(kind,"number"|"decimal_integer_literal"|"decimal_floating_point_literal"){Some("number")}
                else if kind.contains("type_identifier")||kind=="primitive_type"{Some("type")}
                else if node.child_count()==0 && !node.is_named() && text(node,source).chars().all(|ch|ch.is_alphabetic() || ch=='_') && !text(node,source).is_empty(){Some("keyword")}
                else if kind=="identifier" && node.parent().is_some_and(|parent|is_field(parent,"name",node) && (parent.kind().contains("function") || parent.kind().contains("method"))){Some("function")}
                else{None};
            if let Some(category)=category{
                for row in first.row.max(from.saturating_sub(1))..=end.row.min(to.saturating_sub(1)){
                    let line=lines.get(row).copied().unwrap_or("");let start=if row==first.row{first.column}else{0};let finish=if row==end.row{end.column}else{line.len()};
                    if start>=finish{continue;}
                    tokens.push(json!({"line":row+1,"from":line.get(..start).unwrap_or("").encode_utf16().count(),"to":line.get(..finish).unwrap_or(line).encode_utf16().count(),"kind":category}));
                }
            }else{stack.extend((0..node.child_count()).rev().filter_map(|index|node.child(index)));}
        }
        tokens
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn yielding_a_value_does_not_mark_later_calls_as_unreachable() {
        let source = "class Iterator {\nSystem.Collections.Generic.IEnumerable<int> Read(bool flag) {\nif (flag) { yield return Next(); }\nLog();\nyield break;\n}\n}";
        let later = in_source(source, "csharp", 4, None, Some("Log"));
        assert!(later.available);
        assert!(later.guards.is_empty(), "{:?}", later.guards);
        let yielded = in_source(source, "csharp", 3, Some(25), Some("Next"));
        assert!(!yielded.guards.is_empty());
        assert!(yielded.guards.iter().all(|guard| guard.arm_exit.is_none()));

        let stopped = source.replace("yield return Next();", "yield break;");
        let later = in_source(&stopped, "csharp", 4, None, Some("Log"));
        assert!(
            later.guards.iter().any(|guard| {
                guard.text == "flag" && guard.negated && guard.exit.as_deref() == Some("return")
            }),
            "{:?}",
            later.guards
        );
    }

    #[test]
    fn syntax_highlights_include_keyword_tokens_without_coloring_string_contents() {
        let source = "function demo(value) {\n  if (value) return 'return';\n  return 'if';\n}";
        let spans = highlights(source, "javascript", 1, 4);
        let keywords = spans
            .iter()
            .filter(|span| span["kind"] == "keyword")
            .map(|span| {
                let line = source
                    .lines()
                    .nth(span["line"].as_u64().unwrap() as usize - 1)
                    .unwrap();
                &line
                    [span["from"].as_u64().unwrap() as usize..span["to"].as_u64().unwrap() as usize]
            })
            .collect::<Vec<_>>();
        assert_eq!(keywords, ["function", "if", "return", "return"]);
        assert_eq!(
            spans.iter().filter(|span| span["kind"] == "string").count(),
            2
        );
    }

    #[test]
    fn branch_guards_and_loops_match_independent_language_fixtures() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/viewer/branches.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            for site in case["sites"].as_array().unwrap() {
                let actual = in_source(
                    case["source"].as_str().unwrap(),
                    case["language"].as_str().unwrap(),
                    site["line"].as_u64().unwrap() as usize,
                    Some(site["column"].as_u64().unwrap() as usize),
                    Some("hit"),
                );
                assert_eq!(
                    serde_json::to_value(&actual.guards).unwrap(),
                    site["guards"],
                    "{} at {}:{}",
                    case["language"],
                    site["line"],
                    site["column"]
                );
                assert_eq!(
                    serde_json::to_value(&actual.loops).unwrap(),
                    site["loops"],
                    "{} loop at {}",
                    case["language"],
                    site["line"]
                );
            }
        }
    }
    #[test]
    fn call_evidence_and_injected_receiver_types_match_independent_fixtures() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/viewer/calls.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            for expected in case["sites"].as_array().unwrap() {
                let actual = in_source(
                    case["source"].as_str().unwrap(),
                    case["language"].as_str().unwrap(),
                    expected["line"].as_u64().unwrap() as usize,
                    Some(expected["column"].as_u64().unwrap() as usize),
                    None,
                );
                assert_eq!(
                    actual.call,
                    Some(expected["call"].clone()),
                    "{} {}",
                    case["language"],
                    expected
                );
                let callee = expected["call"]["callee"].as_str().unwrap();
                let receiver = callee
                    .strip_prefix("this.")
                    .unwrap_or(callee)
                    .split('.')
                    .next()
                    .unwrap();
                assert_eq!(
                    actual.receiver_type.as_deref(),
                    expected["members"][receiver].as_str(),
                    "{} receiver {callee}",
                    case["language"]
                );
            }
        }
    }
}
