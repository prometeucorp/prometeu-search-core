use super::{AngularContext, Node, object, regex, static_string, template_path};
use anyhow::Result;
use std::ops::Range;
pub(crate) struct Definition {
    pub name: String,
    pub selectors: Vec<String>,
    pub url: Option<String>,
    pub inline: Option<Range<usize>>,
}
pub(crate) struct Template<'a> {
    pub file: String,
    pub text: &'a str,
    pub first_line: u32,
    pub inline: bool,
}
pub(crate) struct Component<'a> {
    pub node: &'a Node,
    pub file: &'a str,
    pub selectors: Vec<String>,
    pub template: Option<Template<'a>>,
}
pub(crate) fn eligible(file: &str, source: &str) -> Result<bool> {
    Ok(regex(r"\.[cm]?ts$")?.is_match(file)?
        && !file.ends_with(".d.ts")
        && !crate::classify::is_test_path(file)
        && source.contains("@Component"))
}
pub(crate) fn definitions(source: &str) -> Result<Vec<Definition>> {
    let safe = crate::synthesis::comments::strip_for_regex(source, "typescript");
    let mut out = Vec::new();
    let mut at = 0;
    let decorator = regex(r"@Component\s*\(")?;
    while let Some(found) = decorator.find_from_pos(&safe, at)? {
        let paren = found.end() - 1;
        at = found.end();
        let Some(close) = object::matching(&safe, paren)? else {
            continue;
        };
        at = close;
        let Some(open) = safe[paren..]
            .find('{')
            .map(|offset| paren + offset)
            .filter(|&open| open <= close)
        else {
            continue;
        };
        let Some(end) = object::matching(&safe, open)? else {
            continue;
        };
        let Some(class) =
            regex(r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)")?
                .captures(object::prefix(&safe[close + 1..], 399))?
        else {
            continue;
        };
        let fields = object::fields(&safe, open, end)?;
        let selector = fields
            .get("selector")
            .map(|field| static_string(field.text))
            .transpose()?
            .flatten()
            .unwrap_or_default();
        let mut selectors = Vec::new();
        for selector in selector.split(',').map(super::trim) {
            if regex(r"^[a-zA-Z][\w-]*$")?.is_match(selector)? {
                selectors.push(selector.into());
            }
        }
        let url = fields
            .get("templateUrl")
            .map(|field| static_string(field.text))
            .transpose()?
            .flatten()
            .filter(|url| !url.is_empty());
        let mut inline = None;
        if url.is_none()
            && let Some(field) = fields.get("template")
        {
            let quote = super::trim(field.text).chars().next().unwrap_or('`');
            let from = field.at + "template".len();
            if let Some(quote_at) = source[from..].find(quote).map(|offset| from + offset)
                && let Some(end) =
                    object::skip_string(source, quote_at)?.filter(|&end| end > quote_at)
            {
                inline = Some(quote_at + 1..end);
            }
        }
        out.push(Definition {
            name: class[1].into(),
            selectors,
            url,
            inline,
        });
    }
    Ok(out)
}
pub(crate) fn components<'a>(context: &AngularContext<'a>) -> Result<Vec<Component<'a>>> {
    let mut out = Vec::new();
    for (&file, &source) in context.sources {
        if !eligible(file, source)? {
            continue;
        }
        let lines = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(at, _)| at + 1))
            .collect::<Vec<_>>();
        for definition in definitions(source)? {
            let Some(node) = context
                .nodes_in(file)
                .iter()
                .copied()
                .find(|node| node.kind == "class" && node.name == definition.name)
            else {
                continue;
            };
            let template = if let Some(url) = definition.url {
                let path = template_path(file, &url);
                context
                    .read(&path)
                    .filter(|text| !text.is_empty())
                    .map(|text| Template {
                        file: path,
                        text,
                        first_line: 1,
                        inline: false,
                    })
            } else {
                definition.inline.map(|range| Template {
                    file: file.into(),
                    text: &source[range.clone()],
                    first_line: lines.partition_point(|&at| at <= range.start) as u32,
                    inline: true,
                })
            };
            out.push(Component {
                node,
                file,
                selectors: definition.selectors,
                template,
            });
        }
    }
    Ok(out)
}
