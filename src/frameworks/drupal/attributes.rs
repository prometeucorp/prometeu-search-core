use super::*;

struct Attribute<'a> {
    name: &'a str,
    args: &'a str,
}
pub(super) struct Declaration<'a> {
    attributes: Vec<Attribute<'a>>,
    line: u32,
    offset: usize,
    method: Option<&'a str>,
    class: Option<&'a str>,
}
fn trivia(source: &str, mut at: usize) -> usize {
    loop {
        at += source[at..].len()
            - source[at..]
                .trim_start_matches(crate::search::query_whitespace)
                .len();
        let tail = &source[at..];
        if tail.starts_with("//") || (tail.starts_with('#') && !tail.starts_with("#[")) {
            let Some(end) = tail.find('\n') else {
                return source.len();
            };
            at += end;
        } else if tail.starts_with("/*") {
            let Some(end) = tail.find("*/") else {
                return source.len();
            };
            at += end + 2;
        } else {
            return at;
        }
    }
}
pub(super) fn declarations(source: &str) -> Result<Vec<Declaration<'_>>> {
    let ctx = Context::new("", source, "php");
    let mut out = Vec::new();
    let pattern = regex(r"(?m)^[ \t]*#\[")?;
    let mut from = 0;
    while let Some(found) = pattern.find_from_pos(source, from)? {
        let start = found.end() - 2;
        let mut attributes = Vec::new();
        let mut at = start;
        while source[at..].starts_with("#[") {
            let Some(close) = object::matching(source, at + 1)? else {
                break;
            };
            if let Some(body) =
                regex(r"^\s*\\?([\w\\]+)\s*(\()?")?.captures(&source[at + 2..close])?
            {
                let open = at + 2 + body.get(0).unwrap().end() - 1;
                let end = if body.get(2).is_some() {
                    object::matching(source, open)?
                } else {
                    None
                };
                attributes.push(Attribute {
                    name: body.get(1).unwrap().as_str().rsplit('\\').next().unwrap(),
                    args: end.map_or("", |end| &source[open + 1..end]),
                });
            }
            at = trivia(source, close + 1);
        }
        from = at.max(found.end());
        if let Some(declaration) = regex(r"^(?:(?:abstract|final|readonly|public|protected|private|static)\s+)*(?:function\s+&?\s*([A-Za-z_]\w*)\s*\(|class\s+([A-Za-z_]\w*))")?.captures(&source[at..])? {
            out.push(Declaration {attributes, line: ctx.position(start).0, offset: at, method: declaration.get(1).map(|value| value.as_str()), class: declaration.get(2).map(|value| value.as_str())});
        }
    }
    Ok(out)
}
fn first_argument<'a>(args: &'a str, pattern: &'static str) -> Result<Option<&'a str>> {
    Ok(regex(pattern)?
        .captures(args)?
        .and_then(|capture| capture.get(1).map(|value| value.as_str())))
}
pub(super) fn hooks(
    ctx: &Context<'_>,
    declarations: &[Declaration<'_>],
) -> Result<Vec<UnresolvedReference>> {
    let mut out = Vec::new();
    for declaration in declarations {
        for attribute in &declaration.attributes {
            if attribute.name != "Hook" {
                continue;
            }
            let Some(hook) =
                first_argument(attribute.args, r#"^\s*(?:hook\s*:\s*)?['"]([^'"]+)['"]"#)?
            else {
                continue;
            };
            let hook = format!("hook_{hook}");
            if let Some(method) = declaration.method {
                out.push(hook_reference(
                    ctx.path,
                    "method",
                    method,
                    &hook,
                    declaration.line,
                ));
                continue;
            }
            let method = first_argument(attribute.args, r#"\bmethod\s*:\s*['"](\w+)['"]"#)?
                .unwrap_or("__invoke");
            let pattern = format!(r"\bfunction\s+&?\s*{}\s*\(", regex::escape(method));
            let Some(found) = super::super::extractors::regex(&pattern)?
                .find(&ctx.source[declaration.offset..])?
            else {
                continue;
            };
            let offset = declaration.offset + found.start();
            let marked = declarations.iter().find(|other| {
                other.method == Some(method)
                    && other.offset <= offset
                    && ctx.source[other.offset..offset].encode_utf16().count() < 200
            });
            let line = marked.map_or_else(|| ctx.position(offset).0, |other| other.line);
            out.push(hook_reference(ctx.path, "method", method, &hook, line));
        }
    }
    Ok(out)
}
pub(super) fn plugins(
    ctx: &Context<'_>,
    declarations: &[Declaration<'_>],
    out: &mut ExtractionResult,
) -> Result<()> {
    if !regex(r"(?:^|/)src/(?:Plugin|Entity|Element)/")?.is_match(ctx.path)?
        || regex(r"(?:^|/)tests?/")?.is_match(ctx.path)?
    {
        return Ok(());
    }
    let mut seen = HashSet::new();
    let mut add = |kind: &str, id: &str, class: &str, line: u32| {
        if !seen.insert(format!("{kind}:{id}:{class}")) {
            return;
        }
        let name = format!("{kind}:{id}");
        let qualified = format!("plugin:{name}");
        let component = node(
            ctx,
            "component",
            &name,
            &qualified,
            format!("component:{}:{line}:{qualified}", ctx.path),
            line,
        );
        out.unresolved_references.push(reference(
            &component,
            &format!("drupal:class:{}#{class}", ctx.path),
            "references",
        ));
        out.nodes.push(component);
    };
    for declaration in declarations {
        let Some(class) = declaration.class else {
            continue;
        };
        for attribute in &declaration.attributes {
            if [
                "Attribute",
                "AllowDynamicProperties",
                "Deprecated",
                "Group",
                "Hook",
            ]
            .contains(&attribute.name)
            {
                continue;
            }
            let first = first_argument(attribute.args, r#"^\s*(?:id\s*:\s*)?['"]([^'"]+)['"]"#)?;
            let id = if first.is_some() {
                first
            } else {
                first_argument(attribute.args, r#"\bid\s*:\s*['"]([^'"]+)['"]"#)?
            };
            if let Some(id) = id {
                add(attribute.name, id, class, declaration.line);
            }
        }
    }
    for doc in regex(r"/\*\*((?:(?!\*/)[\s\S])*)\*/\s*(?:#\[[^\n]*\n\s*)*(?:(?:abstract|final|readonly)\s+)*class\s+([A-Za-z_]\w*)")?.captures_iter(ctx.source) {
        let doc = doc?;
        let body = doc.get(1).unwrap();
        let text = body.as_str();
        let pattern = regex(r"(?m)^[ \t]*\*[ \t]*@([A-Z]\w*)\s*\(")?;
        let mut from = 0;
        while let Some(annotation) = pattern.captures_from_pos(text, from)? {
            let whole = annotation.get(0).unwrap();
            let open = whole.end()-1;
            from = whole.end();
            let Some(close) = object::matching(text, open)? else { continue; };
            let args = &text[open..close];
            let first = first_argument(args, r#"(?:^|[\s,(])id\s*=\s*"([^"]+)""#)?;
            let id = if first.is_some() { first } else { first_argument(args, r#"^\(\s*"([^"]+)""#)? };
            if let Some(id) = id {
                let offset = body.start() + whole.start() + whole.as_str().find('@').unwrap();
                add(&annotation[1], id, &doc[2], ctx.position(offset).0);
            }
            from = close;
        }
    }
    Ok(())
}
