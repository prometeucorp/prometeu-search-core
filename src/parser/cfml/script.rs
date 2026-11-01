//! CFScript/CFQuery branches of the upstream generic AST walker.
use super::super::preprocess::{captures, replace, test};
use anyhow::{Context as _, Result};
use prometeu_extract::ExtractionResult;
use prometeu_extract::supplemental::{Context, children, docstring};
use tree_sitter::Node as Syntax;

const FUNCTION: &[&str] = &[
    "function_declaration",
    "function_expression",
    "arrow_function",
];
const LITERALS: &[&str] = &[
    "string",
    "string_literal",
    "interpreted_string_literal",
    "raw_string_literal",
    "template_string",
    "concatenated_string",
    "formatted_string",
    "f_string",
    "line_string_literal",
    "string_content",
    "heredoc_body",
    "number",
    "number_literal",
    "integer",
    "integer_literal",
    "float",
    "float_literal",
    "int_literal",
    "decimal_integer_literal",
    "real_literal",
    "char_literal",
    "character_literal",
    "rune_literal",
    "regex",
    "regex_literal",
    "true",
    "false",
    "boolean_literal",
    "bool_literal",
    "none",
    "null",
    "nil",
    "null_literal",
    "undefined",
    "list",
    "list_literal",
    "array",
    "array_literal",
    "array_creation_expression",
    "dictionary",
    "dict_literal",
    "object",
    "tuple",
    "set",
];
const COMPONENT_PATH: &str = r"^[A-Za-z_][\w$-]*(?:\.[A-Za-z_][\w$-]*)*$";

enum Task<'a> {
    Visit(Syntax<'a>, bool),
    Variable(Syntax<'a>, Option<String>),
    Leave,
}

pub(super) fn extract(path: &str, source: &str, language: &str) -> Result<ExtractionResult> {
    let grammar = prometeu_extract::grammar_for(language).context("CF grammar not registered")?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&grammar)?;
    let tree = parser
        .parse(source, None)
        .context("Failed to parse CF source")?;
    let mut context = Context::new(path, source, language);
    let mut tasks = vec![Task::Visit(tree.root_node(), false)];
    while let Some(task) = tasks.pop() {
        let (node, body) = match task {
            Task::Leave => {
                context.pop_scope();
                continue;
            }
            Task::Variable(node, docs) => {
                let Some(name) = node.child_by_field_name("name") else {
                    continue;
                };
                let value = node.child_by_field_name("value");
                if matches!(name.kind(), "object_pattern" | "array_pattern") {
                    tasks.push(Task::Visit(node, true));
                    continue;
                }
                if let Some(value) = value.filter(|value| FUNCTION.contains(&value.kind())) {
                    tasks.push(Task::Visit(value, false));
                    continue;
                }
                let signature = value
                    .map(|value| context.text(value))
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        let units = value.encode_utf16().take(100).collect::<Vec<_>>();
                        format!(
                            "= {}{}",
                            String::from_utf16_lossy(&units),
                            if units.len() >= 100 { "..." } else { "" }
                        )
                    });
                let name = context.text(name).to_owned();
                if let Some(index) = context.create_node("variable", &name, node, |node| {
                    node.docstring = docs;
                    node.signature = signature;
                    node.is_exported = Some(false);
                }) && let Some(value) = value
                {
                    context.push_scope(index);
                    tasks.push(Task::Leave);
                    tasks.push(Task::Visit(value, true));
                }
                continue;
            }
            Task::Visit(node, body) => (node, body),
        };
        let kind = node.kind();
        let current = context.current().unwrap();
        let in_class = matches!(
            context.result.nodes[current].kind.as_str(),
            "class" | "interface"
        );
        if language == "cfscript" && kind == "component" {
            let interface = node
                .child(0)
                .is_some_and(|first| first.kind() == "interface");
            let docs = docstring(node, source);
            let name = name_of(node, &context);
            if let Some(index) = context.create_node(
                if interface { "interface" } else { "class" },
                &name,
                node,
                |node| node.docstring = docs,
            ) {
                inheritance(node, index, &mut context)?;
                context.push_scope(index);
                tasks.push(Task::Leave);
                let scope = node.child_by_field_name("body").unwrap_or(node);
                tasks.extend(
                    children(scope)
                        .into_iter()
                        .rev()
                        .map(|child| Task::Visit(child, false)),
                );
            }
            continue;
        }
        if language == "cfscript" && FUNCTION.contains(&kind) {
            let mut name = name_of(node, &context);
            if body && name == "<anonymous>" {
                tasks.extend(
                    children(node)
                        .into_iter()
                        .rev()
                        .map(|child| Task::Visit(child, true)),
                );
                continue;
            }
            if name == "<anonymous>"
                && let Some(parent) = node
                    .parent()
                    .filter(|parent| parent.kind() == "variable_declarator")
                && let Some(binding) = parent.child_by_field_name("name")
            {
                name = context.text(binding).into();
            }
            begin_function(
                node,
                &name,
                !body && in_class && kind == "function_declaration",
                &mut context,
                &mut tasks,
            );
            continue;
        }
        if language == "cfscript" && !body && kind == "method_definition" {
            if !in_class
                && node
                    .parent()
                    .is_some_and(|parent| matches!(parent.kind(), "object" | "object_expression"))
            {
                if let Some(body) = node.child_by_field_name("body") {
                    tasks.push(Task::Visit(body, true));
                }
            } else {
                let name = name_of(node, &context);
                begin_function(node, &name, in_class, &mut context, &mut tasks);
            }
            continue;
        }
        if language == "cfscript" && !body && in_class && kind == "property_declaration" {
            property(node, &mut context);
            continue;
        }
        if language == "cfscript" && !body && !in_class && kind == "variable_declaration" {
            let docs = docstring(node, source);
            tasks.extend(
                children(node)
                    .into_iter()
                    .rev()
                    .filter(|child| child.kind() == "variable_declarator")
                    .map(|child| Task::Variable(child, docs.clone())),
            );
            continue;
        }
        if language == "cfscript"
            && (kind == "include_statement" || !body && kind == "import_statement")
        {
            import(node, &mut context)?;
        }
        if kind == "call_expression" {
            call(node, &mut context)?;
        } else if kind == "new_expression" {
            instantiate(node, &mut context);
        }
        tasks.extend(
            children(node)
                .into_iter()
                .rev()
                .map(|child| Task::Visit(child, body)),
        );
    }
    Ok(context.finish(tree.root_node().has_error()))
}

fn name_of(node: Syntax<'_>, context: &Context<'_>) -> String {
    node.child_by_field_name("name")
        .or_else(|| {
            children(node).into_iter().find(|child| {
                matches!(
                    child.kind(),
                    "identifier" | "property_identifier" | "type_identifier"
                )
            })
        })
        .map_or_else(|| "<anonymous>".into(), |name| context.text(name).into())
}
fn begin_function<'a>(
    node: Syntax<'a>,
    name: &str,
    method: bool,
    context: &mut Context<'_>,
    tasks: &mut Vec<Task<'a>>,
) {
    let body = node.child_by_field_name("body");
    if name == "<anonymous>" {
        if let Some(body) = body {
            tasks.push(Task::Visit(body, true));
        }
        return;
    }
    let signature = node
        .child_by_field_name("parameters")
        .map(|parameters| context.text(parameters).into());
    let visibility = children(node)
        .into_iter()
        .find(|child| child.kind() == "access_type")
        .and_then(|access| match context.text(access) {
            "public" | "remote" => Some("public"),
            "private" => Some("private"),
            "package" => Some("internal"),
            _ => None,
        })
        .map(str::to_owned);
    let docs = docstring(node, context.source);
    if let Some(index) = context.create_node(
        if method { "method" } else { "function" },
        name,
        node,
        |node| {
            node.signature = signature;
            node.visibility = visibility;
            node.docstring = docs;
        },
    ) && let Some(body) = body
    {
        context.push_scope(index);
        tasks.push(Task::Leave);
        tasks.push(Task::Visit(body, true));
    }
}
fn property(node: Syntax<'_>, context: &mut Context<'_>) {
    let Some(name) = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("property"))
        .or_else(|| {
            children(node)
                .into_iter()
                .find(|child| child.kind() == "identifier")
        })
    else {
        return;
    };
    let name = context.text(name).to_owned();
    let ty = children(node).into_iter().find(|child| {
        !matches!(
            child.kind(),
            "modifier"
                | "modifiers"
                | "identifier"
                | "accessor_list"
                | "accessors"
                | "equals_value_clause"
        )
    });
    let signature = ty.map_or_else(
        || name.clone(),
        |ty| {
            format!(
                "{} {name}",
                context.text(ty).trim_start_matches(':').trim_start()
            )
        },
    );
    let docs = docstring(node, context.source);
    context.create_node("property", &name, node, |node| {
        node.signature = Some(signature);
        node.docstring = docs;
        node.is_static = Some(false);
    });
}
fn inheritance(node: Syntax<'_>, owner: usize, context: &mut Context<'_>) -> Result<()> {
    for attribute in children(node)
        .into_iter()
        .filter(|child| child.kind() == "component_attribute")
    {
        let parts = children(attribute);
        let Some(label) = parts.iter().find(|child| child.kind() == "identifier") else {
            continue;
        };
        let Some(value) = parts.iter().find(|child| child.kind() != "identifier") else {
            continue;
        };
        let label = context.text(*label).to_lowercase();
        if !["extends", "implements"].contains(&label.as_str()) {
            continue;
        }
        let text = replace(context.text(*value), r#"^["']|["']$"#, |_| String::new())?;
        for name in if label == "implements" {
            text.split(',').collect::<Vec<_>>()
        } else {
            vec![text.as_str()]
        } {
            let name = name.trim();
            if !name.is_empty() {
                context.reference(owner, name, &label, *value);
            }
        }
    }
    Ok(())
}
fn literal(node: Option<Syntax<'_>>, context: &Context<'_>) -> Result<Option<String>> {
    let Some(node) = node.filter(|node| node.kind() == "string") else {
        return Ok(None);
    };
    if children(node)
        .iter()
        .any(|child| child.kind() != "string_fragment")
    {
        return Ok(None);
    }
    let text = replace(context.text(node), r#"^["']|["']$"#, |_| String::new())?;
    Ok((!text.trim().is_empty()).then(|| text.trim().to_owned()))
}
fn template_argument(node: Syntax<'_>, context: &Context<'_>) -> Result<Option<String>> {
    if node.kind() != "assignment_expression" {
        return Ok(None);
    }
    let Some(left) = node.child_by_field_name("left").filter(|left| {
        left.kind() == "identifier" && context.text(*left).eq_ignore_ascii_case("template")
    }) else {
        return Ok(None);
    };
    let _ = left;
    literal(node.child_by_field_name("right"), context)
}
fn include_call(node: Syntax<'_>, context: &Context<'_>) -> bool {
    node.child_by_field_name("function")
        .is_some_and(|function| {
            function.kind() == "identifier"
                && context.text(function).eq_ignore_ascii_case("cfinclude")
        })
}
fn import(node: Syntax<'_>, context: &mut Context<'_>) -> Result<()> {
    let module = match node.kind() {
        "include_statement" => {
            let parts = children(node);
            if let Some(value) = parts.iter().find(|child| child.kind() == "string") {
                literal(Some(*value), context)?
            } else if let Some(attribute) = parts
                .iter()
                .find(|child| child.kind() == "assignment_expression")
            {
                template_argument(*attribute, context)?
            } else {
                None
            }
        }
        "call_expression" => {
            let args = node
                .child_by_field_name("arguments")
                .map(children)
                .unwrap_or_default();
            let mut module = None;
            for argument in &args {
                if let Some(value) = template_argument(*argument, context)? {
                    module = Some(value);
                    break;
                }
            }
            if module.is_none() {
                module = literal(args.first().copied(), context)?;
            }
            module
        }
        _ => node
            .child_by_field_name("source")
            .map(|source| {
                if source.kind() == "import_path" {
                    Ok(children(source)
                        .iter()
                        .map(|child| context.text(*child))
                        .collect::<Vec<_>>()
                        .join("."))
                } else {
                    replace(context.text(source), r#"^["']|["']$"#, |_| String::new())
                }
            })
            .transpose()?,
    };
    if let Some(module) = module.filter(|module| !module.is_empty()) {
        let signature = context.text(node).trim().to_owned();
        let owner = context.current().unwrap();
        context.create_node("import", &module, node, |node| {
            node.signature = Some(signature)
        });
        context.reference(owner, &module, "imports", node);
    }
    Ok(())
}
fn created_component(node: Syntax<'_>, context: &Context<'_>) -> Result<Option<String>> {
    if node.kind() != "call_expression"
        || !node
            .child_by_field_name("function")
            .is_some_and(|function| {
                function.kind() == "identifier"
                    && context.text(function).eq_ignore_ascii_case("createobject")
            })
    {
        return Ok(None);
    }
    let args = node
        .child_by_field_name("arguments")
        .map(children)
        .unwrap_or_default();
    let mut kind = None;
    let mut name = None;
    if !args.is_empty() && args.iter().all(|arg| arg.kind() == "assignment_expression") {
        for arg in args {
            let key = context
                .text(arg.child_by_field_name("left").unwrap_or(arg))
                .to_lowercase();
            if key == "type" {
                kind = Some(literal(arg.child_by_field_name("right"), context)?);
            } else if key == "component" {
                name = literal(arg.child_by_field_name("right"), context)?;
            }
        }
    } else if args.len() == 1 {
        name = literal(args.first().copied(), context)?;
    } else if args.len() >= 2 {
        kind = Some(literal(args.first().copied(), context)?);
        name = literal(args.get(1).copied(), context)?;
    }
    if kind.is_some_and(|kind| !kind.is_some_and(|kind| kind.eq_ignore_ascii_case("component"))) {
        return Ok(None);
    }
    match name {
        Some(name) if test(COMPONENT_PATH, &name)? => Ok(Some(name)),
        _ => Ok(None),
    }
}
fn constructed_component(mut node: Syntax<'_>, context: &Context<'_>) -> Result<Option<String>> {
    while node.kind() == "parenthesized_expression" {
        let Some(inner) = node.named_child(0) else {
            return Ok(None);
        };
        node = inner;
    }
    if node.kind() == "new_expression" {
        let name = node
            .child_by_field_name("constructor")
            .map(|constructor| replace(context.text(constructor), r"\s+", |_| String::new()))
            .transpose()?
            .unwrap_or_default();
        return Ok(test(COMPONENT_PATH, &name)?.then_some(name));
    }
    created_component(node, context)
}
fn call(node: Syntax<'_>, context: &mut Context<'_>) -> Result<()> {
    let owner = context.current().unwrap();
    if context.language == "cfscript" {
        if include_call(node, context) {
            return import(node, context);
        }
        if let Some(name) = created_component(node, context)? {
            context.reference(owner, &name, "instantiates", node);
            return Ok(());
        }
    }
    let Some(function) = node.child_by_field_name("function") else {
        return Ok(());
    };
    let mut name = if function.kind() == "member_expression" {
        let Some(property) = function
            .child_by_field_name("property")
            .or_else(|| function.named_child(1))
        else {
            return Ok(());
        };
        let method = context.text(property);
        let receiver = function
            .child_by_field_name("object")
            .or_else(|| function.named_child(0));
        if receiver.is_some_and(|receiver| LITERALS.contains(&receiver.kind())) {
            return Ok(());
        }
        if let Some(receiver) = receiver.filter(|receiver| receiver.kind() == "identifier") {
            let name = context.text(receiver);
            if ["self", "this", "cls", "super"].contains(&name) {
                method.into()
            } else {
                format!("{name}.{method}")
            }
        } else if context.language == "cfscript" {
            if let Some(name) = receiver
                .map(|receiver| constructed_component(receiver, context))
                .transpose()?
                .flatten()
            {
                format!("{name}().{method}")
            } else if let Some(receiver) =
                receiver.filter(|receiver| receiver.kind() == "member_expression")
                && test(
                    r"(?i)^(variables|this|local|arguments)\.[A-Za-z_][\w]*$",
                    context.text(receiver),
                )?
            {
                format!("{}.{method}", context.text(receiver))
            } else {
                method.into()
            }
        } else {
            method.into()
        }
    } else {
        context.text(function).to_owned()
    };
    if let Some(m) = captures(r"^\(\s*\*?\s*([A-Za-z_][\w.]*)\s*\)$", &name)?.first() {
        name = m[1].to_owned();
    }
    if !name.is_empty() {
        context.reference(owner, &name, "calls", node);
    }
    Ok(())
}
fn instantiate(node: Syntax<'_>, context: &mut Context<'_>) {
    let Some(constructor) = node
        .child_by_field_name("constructor")
        .or_else(|| node.child_by_field_name("type"))
        .or_else(|| node.child_by_field_name("name"))
        .or_else(|| node.named_child(0))
    else {
        return;
    };
    let mut name = context.text(constructor);
    if let Some(at) = name.find('<').filter(|at| *at > 0) {
        name = &name[..at];
    }
    name = name.rsplit(['.', ':']).next().unwrap_or(name).trim();
    if !name.is_empty() {
        context.reference(context.current().unwrap(), name, "instantiates", node);
    }
}
