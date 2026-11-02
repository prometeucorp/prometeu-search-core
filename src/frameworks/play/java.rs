//! Java overload parameter and argument scoring from the reference matcher.
use super::{Context, arguments, regex};
use anyhow::Result;
use prometeu_extract::{Node, UnresolvedReference};

const PRIMITIVES: &[&str] = &[
    "boolean", "byte", "char", "short", "int", "long", "float", "double",
];
const BOXED: &[(&str, &str)] = &[
    ("boolean", "Boolean"),
    ("byte", "Byte"),
    ("char", "Character"),
    ("short", "Short"),
    ("int", "Integer"),
    ("long", "Long"),
    ("float", "Float"),
    ("double", "Double"),
];
const FUNCTIONAL: &str = r"^(?:Runnable|Callable|Comparator|(?:Bi|Boolean|Double|Int|Long|Obj(?:Double|Int|Long)|To(?:Double|Int|Long)(?:Bi)?|(?:Double|Int|Long)To(?:Double|Int|Long))?(?:Consumer|Function|Predicate|Supplier)|(?:Double|Int|Long)?(?:Binary|Unary)Operator)$";
const STATEMENTS: &[&str] = &[
    "return",
    "throw",
    "new",
    "else",
    "case",
    "yield",
    "assert",
    "instanceof",
    "final",
];
#[derive(Clone)]
pub(super) struct Parameter {
    pub kind: String,
    pub varargs: bool,
}
pub(super) fn without_generics(text: &str) -> Result<String> {
    let generic = regex(r"<[^<>]*>")?;
    let mut text = text.to_owned();
    loop {
        let next = generic.replace_all(&text, "").into_owned();
        if next == text {
            return Ok(text);
        }
        text = next;
    }
}
pub(super) fn bare(text: &str) -> Result<String> {
    let text = without_generics(text)?;
    let text = regex(r"\s+")?.replace_all(&text, "");
    let base = text.trim_end_matches("[]");
    let dimensions = &text[base.len()..];
    Ok(format!(
        "{}{dimensions}",
        base.rsplit('.').next().unwrap_or(base)
    ))
}
pub(super) fn parameters(signature: Option<&str>) -> Result<Option<Vec<Parameter>>> {
    let Some(signature) = signature else {
        return Ok(None);
    };
    let Some(open) = signature.find('(') else {
        return Ok(None);
    };
    let Some(close) = signature.rfind(')').filter(|&close| close >= open) else {
        return Ok(None);
    };
    let list = regex(r"/\*[\s\S]*?\*/")?.replace_all(&signature[open + 1..close], " ");
    let list = list.trim_matches(crate::search::query_whitespace);
    let mut params = Vec::new();
    if list.is_empty() {
        return Ok(Some(params));
    }
    let annotation = regex(r"@[\w.]+(?:\([^()]*\))?")?;
    let final_word = regex(r"\bfinal\b")?;
    let primary = regex(r"^([\s\S]+?)(\.\.\.)?\s+[\w$]+((?:\s*\[\s*\])*)$")?;
    let variadic = regex(r"^([\s\S]+?)(\.\.\.)\s*[\w$]+()$")?;
    for written in arguments::split(list) {
        let plain = annotation.replace_all(written, " ");
        let plain = final_word.replace_all(&plain, " ");
        let plain = plain.trim_matches(crate::search::query_whitespace);
        let Some(m) = primary.captures(plain)?.or(variadic.captures(plain)?) else {
            return Ok(None);
        };
        params.push(Parameter {
            kind: bare(&format!("{}{}", &m[1], &m[3]))?,
            varargs: m.get(2).is_some(),
        });
    }
    Ok(Some(params))
}
fn widened(from: &str, to: &str) -> bool {
    match from {
        "byte" => ["short", "int", "long", "float", "double"].contains(&to),
        "short" | "char" => ["int", "long", "float", "double"].contains(&to),
        "int" => ["long", "float", "double"].contains(&to),
        "long" => ["float", "double"].contains(&to),
        "float" => to == "double",
        _ => false,
    }
}
fn function_fit(ctx: &Context<'_>, arg: &str, param: &str) -> Result<f64> {
    if regex(FUNCTIONAL)?.is_match(param)? {
        let takes = if regex(r"Supplier$|^Runnable$|^Callable$")?.is_match(param)? {
            0
        } else if regex(r"^Bi|BiFunction$|Binary|^Obj|^Comparator$")?.is_match(param)? {
            2
        } else {
            1
        };
        return Ok(if arg == "->" || arg == format!("->{takes}") {
            1.0
        } else {
            -1.0
        });
    }
    if ["Object", "String"].contains(&param) || param.ends_with("[]") || PRIMITIVES.contains(&param)
    {
        return Ok(-1.0);
    }
    let decls = ctx.jvm_types(param);
    Ok(
        if !decls.is_empty() && decls.iter().all(|node| node.kind != "interface") {
            -1.0
        } else {
            0.0
        },
    )
}
fn fit(ctx: &Context<'_>, arg: &str, param: &str) -> Result<f64> {
    if arg == param {
        return Ok(2.0);
    }
    let variable = regex(r"^[A-Z][A-Z0-9]{0,2}(?:\[\])*$")?;
    if variable.is_match(arg)? || variable.is_match(param)? {
        return Ok(0.0);
    }
    if arg.starts_with("->") {
        return function_fit(ctx, arg, param);
    }
    let project = |name: &str| ctx.jvm_types(name.trim_end_matches("[]"));
    let refused = |name: &str| {
        if name == "String" || !project(name).is_empty() {
            -1.0
        } else {
            0.0
        }
    };
    let value_refused = |name: &str| -> Result<f64> {
        Ok(if regex(FUNCTIONAL)?.is_match(name)? {
            -1.0
        } else {
            refused(name)
        })
    };
    let arg_primitive = PRIMITIVES.contains(&arg);
    let param_primitive = PRIMITIVES.contains(&param);
    if arg_primitive && param_primitive {
        return Ok(if widened(arg, param) { 1.5 } else { -1.0 });
    }
    if arg_primitive {
        return if BOXED.iter().any(|&(a, b)| a == arg && b == param)
            || ["Object", "Number", "Comparable", "Serializable"].contains(&param)
        {
            Ok(1.0)
        } else {
            value_refused(param)
        };
    }
    if param_primitive {
        return Ok(
            if BOXED.iter().any(|&(primitive, boxed)| {
                boxed == arg && (primitive == param || widened(primitive, param))
            }) {
                1.0
            } else {
                -1.0
            },
        );
    }
    if param == "Object" {
        return Ok(1.0);
    }
    if arg.ends_with("[]") != param.ends_with("[]") {
        return Ok(if ["Serializable", "Cloneable"].contains(&param) {
            1.0
        } else if param.ends_with("[]") {
            -1.0
        } else {
            refused(param)
        });
    }
    if arg == "String" {
        return if [
            "CharSequence",
            "Comparable",
            "Serializable",
            "Constable",
            "ConstantDesc",
        ]
        .contains(&param)
        {
            Ok(1.0)
        } else {
            value_refused(param)
        };
    }
    let bare = param.trim_end_matches("[]");
    for decl in project(arg) {
        if ctx.ancestors(&decl.qualified_name)?.contains(bare) {
            return Ok(1.0);
        }
    }
    Ok(refused(param))
}
fn closing(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (at, ch) in text[open..].char_indices() {
        if ch == '(' {
            depth += 1
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(open + at);
            }
        }
    }
    None
}
fn local_type(
    ctx: &Context<'_>,
    name: &str,
    reference: &UnresolvedReference,
) -> Result<Option<String>> {
    let file = reference.file_path.as_deref().unwrap_or("");
    let lines = ctx.lines(file);
    let start = ctx
        .in_file(file)
        .iter()
        .filter(|node| {
            matches!(node.kind.as_str(), "function" | "method")
                && Some(&node.language) == reference.language.as_ref()
                && node.start_line <= reference.line
                && node.end_line >= reference.line
        })
        .map(|n| n.start_line)
        .max()
        .unwrap_or(1);
    let declared = regex(&format!(
        r"((?:[A-Za-z_$][\w$]*\.)*[A-Za-z_$][\w$]*(?:\s*<[^;=(){{}}]*>)?(?:\s*\[\s*\])*)(\s*\.\.\.)?\s+{}((?:\s*\[\s*\])*)\s*(?:[=;,):]|$)",
        regex::escape(name)
    ))?;
    for line in lines
        .iter()
        .take((reference.line as usize).min(lines.len()))
        .skip(start.saturating_sub(1) as usize)
        .rev()
    {
        ctx.step(line.len())?;
        if !line.contains(name) || regex(r"^\s*(?://|\*|/\*)")?.is_match(line)? {
            continue;
        }
        let Some(m) = declared.captures(line)? else {
            continue;
        };
        if &m[1] == "var" {
            return Ok(Some("var".into()));
        }
        if STATEMENTS.contains(&&m[1]) {
            continue;
        }
        let kind = bare(&format!(
            "{}{}{}",
            &m[1],
            if m.get(2).is_some() { "[]" } else { "" },
            &m[3]
        ))?;
        if kind.starts_with(|ch: char| ch.is_ascii_uppercase())
            || PRIMITIVES.contains(&kind.trim_end_matches("[]"))
        {
            return Ok(Some(kind));
        }
    }
    Ok(None)
}
pub(super) fn field_type(field: &Node) -> Result<Option<String>> {
    if field.kind == "enum_member" {
        return Ok(field.qualified_name.rsplit("::").nth(1).map(str::to_owned));
    }
    let kind = field
        .signature
        .as_deref()
        .map(|signature| bare(&signature[..signature.rfind(&field.name).unwrap_or(0)]))
        .transpose()?
        .unwrap_or_default();
    Ok((!kind.is_empty()).then_some(kind))
}
fn declared_field(
    ctx: &Context<'_>,
    name: &str,
    reference: &UnresolvedReference,
) -> Result<Option<String>> {
    let mut around = ctx
        .in_file(reference.file_path.as_deref().unwrap_or(""))
        .into_iter()
        .filter(|node| {
            super::types::visible_kind(&node.kind)
                && node.start_line <= reference.line
                && node.end_line >= reference.line
        })
        .collect::<Vec<_>>();
    around.sort_by_key(|node| std::cmp::Reverse(node.start_line));
    for owner in around {
        if let Some(field) = ctx.static_field(owner, name)? {
            return field_type(field);
        }
    }
    Ok(None)
}
fn named_type(
    ctx: &Context<'_>,
    name: &str,
    reference: &UnresolvedReference,
) -> Result<Option<String>> {
    match local_type(ctx, name, reference)? {
        Some(local) if local == "var" => Ok(None),
        Some(local) => Ok(Some(local)),
        None => declared_field(ctx, name, reference),
    }
}
fn receiver_type(
    ctx: &Context<'_>,
    name: &str,
    reference: &UnresolvedReference,
) -> Result<Option<String>> {
    if regex(r"^[A-Z].*[a-z]")?.is_match(name)? {
        Ok(Some(name.into()))
    } else {
        named_type(ctx, name, reference)
    }
}
pub(super) fn argument_type(
    ctx: &Context<'_>,
    arg: &str,
    reference: &UnresolvedReference,
) -> Result<Option<String>> {
    if let Some(m) = regex(r"^(?:\(([^()]*)\)|[A-Za-z_$][\w$]*)\s*->")?.captures(arg)? {
        return Ok(Some(format!(
            "->{}",
            m.get(1).map_or(1, |m| arguments::split(m.as_str()).len())
        )));
    }
    for (pattern, kind) in [
        (
            r"^[\w$.]+(?:<[^()]*>)?(?:\[\])*\s*::\s*[A-Za-z_$][\w$]*$",
            "->",
        ),
        (r#"^"(?:[^"\\]|\\.)*"(?:\s*\+[\s\S]*)?$"#, "String"),
        (r"^'(?:[^'\\]|\\.)+'$", "char"),
        (r"^(?:true|false)$", "boolean"),
        (r"^-?(?:0[xX][\da-fA-F_]+|0[bB][01_]+|\d[\d_]*)$", "int"),
        (
            r"^-?(?:0[xX][\da-fA-F_]+|0[bB][01_]+|\d[\d_]*)[lL]$",
            "long",
        ),
        (
            r"^-?(?:\d[\d_]*(?:\.[\d_]*)?|\.\d[\d_]*)(?:[eE][-+]?\d+)?[fF]$",
            "float",
        ),
        (
            r"^-?(?:(?:\d[\d_]*\.[\d_]*|\.\d[\d_]*)(?:[eE][-+]?\d+)?[dD]?|\d[\d_]*(?:[eE][-+]?\d+[dD]?|[dD]))$",
            "double",
        ),
    ] {
        if regex(pattern)?.is_match(arg)? {
            return Ok(Some(kind.into()));
        }
    }
    if let Some(m) = regex(r"^new\s+([\w$.]+)\s*(?:<[^()]*>)?\s*(\(|\[)")?.captures(arg)? {
        if &m[2] == "[" {
            return Ok(Some(format!("{}[]", bare(&m[1])?)));
        }
        return Ok(
            if let Some(end) = closing(arg, m.get(0).unwrap().end() - 1)
                && regex(r"^\s*(?:\{[\s\S]*\})?$")?.is_match(&arg[end + 1..])?
            {
                Some(bare(&m[1])?)
            } else {
                None
            },
        );
    }
    if let Some(m) =
        regex(r#"^\(\s*([\w$.]+(?:\s*<[^()]*>)?(?:\s*\[\s*\])*)\s*\)\s*[\w$("']"#)?.captures(arg)?
    {
        return Ok(Some(bare(&m[1])?));
    }
    if arg == "this" {
        let mut around = ctx
            .in_file(reference.file_path.as_deref().unwrap_or(""))
            .into_iter()
            .filter(|node| {
                super::types::visible_kind(&node.kind)
                    && node.start_line <= reference.line
                    && node.end_line >= reference.line
            })
            .collect::<Vec<_>>();
        around.sort_by_key(|node| std::cmp::Reverse(node.start_line));
        return Ok(around
            .first()
            .filter(|node| !(node.name.starts_with('<') && node.name.contains("$anon@")))
            .map(|node| node.name.clone()));
    }
    if arg == "null" {
        return Ok(None);
    }
    if let Some(m) = regex(r"^this\s*\.\s*([A-Za-z_$][\w$]*)$")?.captures(arg)? {
        return declared_field(ctx, &m[1], reference);
    }
    if let Some(m) = regex(r"^([A-Za-z_$][\w$]*)\s*\[[^[\]]*\]$")?.captures(arg)? {
        return Ok(named_type(ctx, &m[1], reference)?
            .and_then(|kind| kind.strip_suffix("[]").map(str::to_owned)));
    }
    if let Some(m) = regex(r"^([A-Za-z_$][\w$]*)\s*\.\s*([A-Za-z_$][\w$]*)$")?.captures(arg)? {
        let Some(kind) = receiver_type(ctx, &m[1], reference)? else {
            return Ok(None);
        };
        if kind.ends_with("[]") {
            return Ok((&m[2] == "length").then(|| "int".into()));
        }
        let Some(owner) = ctx.type_ref(&kind, reference.file_path.as_deref().unwrap_or(""))? else {
            return Ok(None);
        };
        let owners = ctx.type_owners(&owner, reference.file_path.as_deref().unwrap_or(""))?;
        return if owners.len() == 1 {
            ctx.static_field(owners[0], &m[2])?
                .map(field_type)
                .transpose()
                .map(Option::flatten)
        } else {
            Ok(None)
        };
    }
    if regex(r"^[A-Za-z_$][\w$]*$")?.is_match(arg)? {
        return named_type(ctx, arg, reference);
    }
    let Some(m) = regex(r"^(?:this\s*\.\s*)?([A-Za-z_$][\w$]*)\s*\.\s*([A-Za-z_$][\w$]*)\s*\(")?
        .captures(arg)?
    else {
        return Ok(None);
    };
    let Some(close) =
        closing(arg, m.get(0).unwrap().end() - 1).filter(|&close| close == arg.len() - 1)
    else {
        return Ok(None);
    };
    let Some(kind) = receiver_type(ctx, &m[1], reference)? else {
        return Ok(None);
    };
    let Some(owner) = ctx.type_ref(&kind, reference.file_path.as_deref().unwrap_or(""))? else {
        return Ok(None);
    };
    let Some(method) = ctx.type_method(&owner, &m[2], reference, 0)? else {
        return Ok(None);
    };
    let count = arguments::split(&arg[m.get(0).unwrap().end()..close]).len();
    let mut overloads = Vec::new();
    for node in ctx.in_file(&method.file_path) {
        if node.kind == "method"
            && node.qualified_name == method.qualified_name
            && ctx
                .parameters(node)?
                .is_some_and(|params| params.len() == count)
        {
            overloads.push(node)
        }
    }
    let returned = overloads
        .first()
        .and_then(|node| node.return_type.as_deref());
    Ok(
        if let Some(returned) = returned.filter(|returned| !returned.is_empty())
            && overloads
                .iter()
                .all(|node| node.return_type.as_deref() == Some(returned))
        {
            Some(bare(returned)?)
        } else {
            None
        },
    )
}
pub(super) fn fitting<'a>(
    ctx: &Context<'a>,
    overloads: &[&'a Node],
    written: &str,
    reference: &UnresolvedReference,
) -> Result<Option<Vec<&'a Node>>> {
    let args = arguments::split(written);
    let mut counted = Vec::new();
    for &node in overloads {
        if node.language != "java" {
            return Ok(None);
        }
        let Some(params) = ctx.parameters(node)? else {
            return Ok(None);
        };
        if node.visibility.as_deref() == Some("private")
            && Some(&node.file_path) != reference.file_path.as_ref()
        {
            continue;
        }
        if if params.last().is_some_and(|p| p.varargs) {
            args.len() >= params.len() - 1
        } else {
            args.len() == params.len()
        } {
            counted.push((node, params))
        }
    }
    if counted.len() < 2 {
        return Ok((counted.len() == 1).then(|| vec![counted[0].0]));
    }
    let types = args
        .iter()
        .map(|arg| argument_type(ctx, arg, reference))
        .collect::<Result<Vec<_>>>()?;
    let mut best = -1.0;
    let mut fitting = Vec::new();
    for (node, params) in counted {
        let mut score = 0.0;
        for (i, kind) in types.iter().enumerate() {
            let Some(kind) = kind else { continue };
            let param = &params[i.min(params.len() - 1)];
            let value = if i < params.len() - 1 || !param.varargs {
                fit(ctx, kind, &param.kind)?
            } else {
                fit(ctx, kind, &format!("{}[]", param.kind))?
                    .max(fit(ctx, kind, &param.kind)?.min(1.0))
            };
            if value < 0.0 {
                score = -1.0;
                break;
            }
            score += value;
        }
        if score < 0.0 {
            continue;
        }
        let rank = score
            - if params.last().is_some_and(|p| p.varargs) && types.iter().all(Option::is_some) {
                0.5
            } else {
                0.0
            };
        if rank > best {
            best = rank;
            fitting = vec![node]
        } else if rank == best {
            fitting.push(node)
        }
    }
    Ok((!fitting.is_empty()).then_some(fitting))
}
