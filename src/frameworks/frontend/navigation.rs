//! Literal navigation resolution against one immutable canonical route snapshot.
use super::{angular, href, react_router, react_tables, regex, tables, tanstack, vue_router};
use anyhow::Result;
use prometeu_extract::{Node, UnresolvedReference};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

pub(crate) struct Navigation<'a> {
    angular: Option<angular::AngularTable<'a>>,
    react: Option<react_router::Router<'a>>,
    next: tables::Table<'a>,
    expo: tables::Table<'a>,
    svelte: tables::Rooted<'a>,
    tanstack: tables::Rooted<'a>,
    vue: vue_router::Table<'a>,
}
pub(crate) struct Destination<'a> {
    pub node: &'a Node,
    pub metadata: Value,
    pub framework: &'static str,
}

impl<'a> Navigation<'a> {
    pub(crate) fn new(
        nodes: &'a [Node],
        active: &[String],
        sources: &BTreeMap<&str, &str>,
    ) -> Result<Self> {
        Ok(Self {
            angular: None,
            react: None,
            vue: if active.iter().any(|name| name == "vue-router") {
                vue_router::table(nodes, sources)?
            } else {
                vue_router::Table::new()
            },
            tanstack: if active.iter().any(|name| name == "tanstack-router") {
                tanstack::table(nodes)?
            } else {
                tables::Rooted::new()
            },
            next: if active.iter().any(|name| name == "nextjs") {
                tables::next(nodes)?
            } else {
                tables::Table::default()
            },
            expo: if active.iter().any(|name| name == "expo-router") {
                tables::expo(nodes)?
            } else {
                tables::Table::default()
            },
            svelte: if active.iter().any(|name| name == "sveltekit-router") {
                tables::svelte(nodes)?
            } else {
                tables::Rooted::new()
            },
        })
    }
    pub(crate) fn with_context(
        nodes: &'a [Node],
        active: &[String],
        sources: &'a BTreeMap<&'a str, &'a str>,
        resolve_module: &'a super::ModuleResolver<'a>,
        manifests: &'a BTreeMap<&'a str, &'a str>,
    ) -> Result<Self> {
        let mut out = Self::new(nodes, active, sources)?;
        if active.iter().any(|name| name == "react-router") {
            out.react = Some(react_router::Router::new(react_tables::Context::new(
                nodes,
                sources,
                resolve_module,
            )?)?);
        }
        if active.iter().any(|name| name == "angular-router") {
            out.angular = Some(angular::AngularTable::new(angular::AngularContext::new(
                nodes,
                sources,
                resolve_module,
                manifests,
            ))?);
        }
        Ok(out)
    }
    pub(crate) fn angular_binding(
        &self,
        reference: &UnresolvedReference,
    ) -> Result<Option<Destination<'a>>> {
        let Some(angular) = &self.angular else {
            return Ok(None);
        };
        if !reference.from_node_id.ends_with(":angular") {
            return Ok(None);
        }
        let name = &reference.reference_name;
        let layout = name.strip_prefix("layout:");
        if layout.is_none() && !regex(r"^import:([^#]+)#([\w$]+)$")?.is_match(name)? {
            return Ok(None);
        }
        Ok(angular
            .context
            .component(
                layout.unwrap_or(name),
                reference.file_path.as_deref().unwrap_or(""),
            )?
            .map(|node| Destination {
                node,
                framework: "angular-router",
                metadata: if layout.is_some() {
                    json!({"layout":true})
                } else {
                    json!({})
                },
            }))
    }
    /// Returned destinations preserve the reference's order and each arm's metadata.
    /// The graph adapter records these as framework resolutions with confidence 0.95.
    pub(crate) fn resolve(
        &self,
        reference: &UnresolvedReference,
        lines: &[&str],
        enclosing_start: Option<u32>,
    ) -> Result<Vec<Destination<'a>>> {
        if reference.reference_kind != "calls" {
            return Ok(Vec::new());
        }
        if let Some(react) = &self.react {
            let targets = react.resolve(reference, lines, enclosing_start)?;
            if !targets.is_empty() {
                return Ok(targets);
            }
        }
        let language = reference.language.as_deref().unwrap_or("");
        let file = reference.file_path.as_deref().unwrap_or("");
        let name = &reference.reference_name;
        let start = enclosing_start.unwrap_or(reference.line.saturating_sub(40).max(1));
        let callee = name.rsplit('.').next().unwrap_or(name);
        if ["typescript", "javascript", "tsx", "jsx"].contains(&language)
            && regex(r"^(?:navigate|redirect)$|^(?:router|Route)\.navigate$")?.is_match(name)?
            && let Some(routes) = tables::for_file(&self.tanstack, file)
            && !routes.exact.is_empty()
            && let Some(argument) =
                href::argument(lines, reference.line, reference.column, callee, 0)?
        {
            let mut destination = tanstack::destination(&argument)?;
            if destination.is_none() {
                destination =
                    href::via_local(lines, reference.line, reference.column, callee, start)?;
            }
            if let Some(destination) = destination {
                let targets = tables::destinations(&destination, routes)?
                    .into_iter()
                    .map(|(node, href)| Destination {
                        node,
                        framework: "tanstack-router",
                        metadata: json!({"href":href.display,"navMethod":callee}),
                    })
                    .collect::<Vec<_>>();
                if !targets.is_empty() {
                    return Ok(targets);
                }
            }
        }
        if ["typescript", "javascript", "tsx", "jsx"].contains(&language)
            && !self.next.exact.is_empty()
            && self.next.roots.iter().any(|root| file.starts_with(root))
        {
            let verb=regex(r"(?:^|\.)(push|replace|prefetch)$|^(redirect|permanentRedirect)$|^(?:NextResponse|Response)\.(redirect)$")?.captures(name)?;
            if let Some(verb) = verb {
                let method = if verb.get(3).is_some() {
                    "response.redirect"
                } else {
                    verb.get(1).or_else(|| verb.get(2)).unwrap().as_str()
                };
                if let Some(mut argument) =
                    href::argument(lines, reference.line, reference.column, callee, 0)?
                {
                    if regex(r"^\s*new\s+URL\s*\(")?.is_match(&argument)? {
                        argument =
                            href::argument(&[&argument], 1, 0, "URL", 0)?.unwrap_or_default();
                    }
                    let mut destination = href::parse(&argument)?;
                    if destination.is_none() {
                        destination = href::via_local(
                            lines,
                            reference.line,
                            reference.column,
                            callee,
                            start,
                        )?;
                    }
                    if let Some(destination) = destination {
                        let targets = tables::destinations(&destination, &self.next)?
                            .into_iter()
                            .map(|(node, href)| Destination {
                                node,
                                framework: "nextjs",
                                metadata: json!({"href":href.display,"navMethod":method}),
                            })
                            .collect::<Vec<_>>();
                        if !targets.is_empty() {
                            return Ok(targets);
                        }
                    }
                }
            }
        }
        if ["typescript", "javascript", "svelte"].contains(&language)
            && matches!(name.as_str(), "goto" | "redirect")
            && let Some(routes) = tables::for_file(&self.svelte, file)
            && !routes.exact.is_empty()
        {
            let argument_index = usize::from(name == "redirect");
            if let Some(argument) = href::argument(
                lines,
                reference.line,
                reference.column,
                name,
                argument_index,
            )? {
                let mut destination = href::parse(&argument)?;
                if destination.is_none() && argument_index == 0 {
                    destination =
                        href::via_local(lines, reference.line, reference.column, name, start)?;
                }
                if let Some(destination) = destination {
                    let targets = tables::destinations(&destination, routes)?
                        .into_iter()
                        .map(|(node, href)| Destination {
                            node,
                            framework: "sveltekit-router",
                            metadata: json!({"href":href.display,"navMethod":name}),
                        })
                        .collect::<Vec<_>>();
                    if !targets.is_empty() {
                        return Ok(targets);
                    }
                }
            }
        }
        if ["typescript", "javascript", "vue"].contains(&language)
            && regex(r"^(?:this\.)?\$?router\.(?:push|replace)$|^navigateTo$")?.is_match(name)?
            && let Some(app) = vue_router::for_file(&self.vue, file)
            && !app.routes.exact.is_empty()
            && let Some(argument) =
                href::argument(lines, reference.line, reference.column, callee, 0)?
        {
            if let Some(named) = vue_router::name(&argument)? {
                return Ok(app
                    .names
                    .get(&named)
                    .map(|&node| Destination {
                        node,
                        framework: "vue-router",
                        metadata: json!({"href":named,"navMethod":callee,"by":"name"}),
                    })
                    .into_iter()
                    .collect());
            }
            let mut destination = href::parse(&argument)?;
            if destination.is_none() {
                destination = vue_router::path_object(&argument)?;
            }
            if destination.is_none() {
                destination =
                    href::via_local(lines, reference.line, reference.column, callee, start)?;
            }
            if let Some(destination) = destination {
                let targets = tables::destinations(&destination, &app.routes)?
                    .into_iter()
                    .filter(|(_, href)| href.path.starts_with('/'))
                    .map(|(node, href)| Destination {
                        node,
                        framework: "vue-router",
                        metadata: json!({"href":href.display,"navMethod":callee}),
                    })
                    .collect::<Vec<_>>();
                if !targets.is_empty() {
                    return Ok(targets);
                }
            }
        }
        if matches!(language, "typescript" | "javascript")
            && let Some(angular) = &self.angular
        {
            let targets = angular.resolve(reference, lines)?;
            if !targets.is_empty() {
                return Ok(targets);
            }
        }
        if ["typescript","javascript","tsx","jsx"].contains(&language)&&!self.expo.exact.is_empty()
            && let Some(verb)=regex(r"(?:^|\.)(push|replace|navigate|dismissTo)$|^[a-z][A-Za-z]*(Push|Replace|Navigate)$")?.captures(name)?{
            let method=verb.get(1).map_or_else(||verb.get(2).unwrap().as_str().to_ascii_lowercase(),|m|m.as_str().to_owned());
            let mut destination=href::argument(lines,reference.line,reference.column,callee,0)?.map(|argument|href::parse(&argument)).transpose()?.flatten();
            if destination.is_none(){destination=href::via_local(lines,reference.line,reference.column,callee,start)?;}
            if let Some(destination)=destination{
                let mut targets=Vec::new();let mut seen=HashSet::new();
                for arm in destination.arms(){
                    let Some(parts)=tables::normalize(&arm.path,file)?else{continue;};let Some(node)=tables::matching(&parts,&self.expo)else{continue;};
                    if !seen.insert(&node.id){continue;}
                    let mut metadata=json!({"href":destination.display,"navMethod":method});
                    if targets.is_empty()&&callee!=method{metadata["via"]=json!(callee);}
                    targets.push(Destination{node,metadata,framework:"expo-router"});
                }
                return Ok(targets);
            }
        }
        Ok(Vec::new())
    }
}
