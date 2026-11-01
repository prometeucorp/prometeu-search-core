//! Declared C/C++ receivers, qualified calls and construction sites.
use super::*;

pub(in crate::graph::resolution) struct Answer {
    pub reference: UnresolvedReference,
    pub targets: Vec<String>,
    pub confidence: f64,
    pub method: &'static str,
}
fn leading_type(text: &str) -> Result<Option<String>> {
    let stripped = regex(r"^(?:(?:new|const|volatile|static|thread_local|constexpr|constinit|mutable|struct|class|union|typename)\s+)*")?.replace(text, "");
    let mut end = 0;
    loop {
        let pattern = if end == 0 {
            r"^(?:::\s*)?[A-Za-z_]\w*"
        } else {
            r"^\s*::\s*[A-Za-z_]\w*"
        };
        let Some(name) = regex(pattern)?.find(&stripped[end..])? else {
            break;
        };
        end += name.end();
        if let Some(open) = regex(r"^\s*<")?.find(&stripped[end..])? {
            end += open.end();
            let mut depth = 1usize;
            let mut parens = 0usize;
            for (at, c) in stripped[end..].char_indices() {
                if c == '(' {
                    parens += 1;
                } else if c == ')' {
                    parens = parens.saturating_sub(1);
                } else if parens == 0 {
                    if c == '<' {
                        depth += 1;
                    } else if c == '>' {
                        depth -= 1;
                    }
                }
                if depth == 0 {
                    end += at + 1;
                    break;
                }
            }
            if depth != 0 {
                return Ok(None);
            }
        }
    }
    Ok((end > 0).then(|| stripped[..end].to_string()))
}
struct ConstructorShape {
    key: String,
    minimum: usize,
    maximum: usize,
}
fn constructor_shape(signature: Option<&str>) -> Result<Option<ConstructorShape>> {
    let Some(signature) = signature.map(|value| value.trim_end_matches(';')) else {
        return Ok(None);
    };
    let Some(text) = signature
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .map(str::trim)
    else {
        return Ok(None);
    };
    if matches!(text, "" | "void") {
        return Ok(Some(ConstructorShape {
            key: String::new(),
            minimum: 0,
            maximum: 0,
        }));
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote = None;
    let mut escape = false;
    for (at, c) in text.char_indices() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        if "(<[{".contains(c) {
            depth += 1;
        } else if ")>]}".contains(c) {
            depth -= 1;
        }
        if c == ',' && depth == 0 {
            parts.push(&text[start..at]);
            start = at + 1;
        }
    }
    if depth != 0 || quote.is_some() {
        return Ok(None);
    }
    parts.push(&text[start..]);
    let mut types = Vec::new();
    for part in &parts {
        if regex(r"[<>]=|==|!=")?.is_match(part)? {
            return Ok(None);
        }
        let ty = part.split('=').next().unwrap_or("").trim();
        let named = regex(r"(.*[\s*&>])([A-Za-z_]\w*)$")?.captures(ty)?;
        let ty = if let Some(named) = named {
            if regex(
                r"^(?:void|bool|char|short|int|long|float|double|signed|unsigned|const|volatile)$",
            )?
            .is_match(&named[2])?
                || regex(r"^(?:const|volatile|struct|class|enum)\s*$")?.is_match(&named[1])?
            {
                ty.to_string()
            } else {
                named[1].to_string()
            }
        } else {
            ty.to_string()
        };
        types.push(regex(r"\s+")?.replace_all(&ty, "").into_owned());
    }
    Ok(Some(ConstructorShape {
        key: types.join(","),
        minimum: parts
            .iter()
            .filter(|part| !part.contains('=') && !part.contains("..."))
            .count(),
        maximum: if parts.iter().any(|part| part.contains("...")) {
            usize::MAX
        } else {
            parts.len()
        },
    }))
}
impl<'a> Types<'a> {
    fn site_text(&self, at: &UnresolvedReference) -> &str {
        self.lines
            .get(at.file_path.as_deref().unwrap_or(""))
            .and_then(|lines| lines.get(at.line.saturating_sub(1) as usize))
            .map_or("", |line| &line[byte(line, at.column)..])
    }
    fn visible_at(&self, at: &UnresolvedReference) -> Result<()> {
        let mut visible = HashSet::new();
        for node in self.nodes {
            self.step(1)?;
            if Some(node.file_path.as_str()) != at.file_path.as_deref()
                && regex(r"(?i)\.(?:c|cc|cpp|cxx|c\+\+|cu|metal)$")?.is_match(&node.file_path)?
            {
                continue;
            }
            let local = self.qualified(parent(&node.qualified_name)).find(|owner| {
                matches!(owner.kind.as_str(), "function" | "method")
                    && owner.file_path == node.file_path
                    && owner.start_line <= node.start_line
                    && owner.end_line >= node.start_line
            });
            if local.is_some_and(|owner| {
                Some(owner.file_path.as_str()) != at.file_path.as_deref()
                    || at.line < owner.start_line
                    || at.line > owner.end_line
            }) {
                continue;
            }
            visible.insert(node.id.clone());
        }
        *self.visible.borrow_mut() = Some(visible);
        Ok(())
    }
    fn choose(&self, nodes: Vec<&'a Node>, at: &UnresolvedReference) -> Option<&'a Node> {
        if nodes.len() <= 1 {
            return nodes.first().copied();
        }
        let own: Vec<_> = nodes
            .iter()
            .copied()
            .filter(|node| Some(node.file_path.as_str()) == at.file_path.as_deref())
            .collect();
        if own.len() == 1 {
            return Some(own[0]);
        }
        if !own.is_empty() {
            let above = own
                .iter()
                .copied()
                .filter(|node| node.start_line <= at.line)
                .max_by_key(|node| node.start_line);
            if above.is_some() {
                return above;
            }
        }
        // Repeated declarations with the same qualified type are one type.
        // Distinct overload signatures remain separate choices.
        if nodes.iter().all(|node| {
            node.qualified_name == nodes[0].qualified_name && node.signature == nodes[0].signature
        }) {
            return nodes.first().copied();
        }
        None
    }
    fn declared_node(
        &self,
        found: Option<Option<Declared<'a>>>,
        at: &UnresolvedReference,
        exclude: Option<&str>,
    ) -> Option<Option<&'a Node>> {
        found.map(|found| match found {
            Some(Declared::Alias(alias)) => Some(alias),
            Some(Declared::Enums(nodes)) => self.choose(nodes, at),
            Some(Declared::Class(class)) => self.choose(
                self.qualified(&class)
                    .filter(|node| {
                        family(node)
                            && CLASSES.contains(&node.kind.as_str())
                            && Some(node.id.as_str()) != exclude
                            && self
                                .visible
                                .borrow()
                                .as_ref()
                                .is_none_or(|visible| visible.contains(&node.id))
                    })
                    .collect(),
                at,
            ),
            _ => None,
        })
    }
    fn named_type(
        &self,
        written: &str,
        at: &UnresolvedReference,
        declared: bool,
        base: bool,
    ) -> Result<Option<Option<&'a Node>>> {
        let Some(name) = read_type(written)? else {
            return Ok(Some(None));
        };
        let caller = self.ids.get(at.from_node_id.as_str()).copied();
        if !name.global
            && caller.is_some_and(|node| {
                self.parameters(node)
                    .is_ok_and(|names| names.contains(&name.names[0]))
            })
        {
            return Ok(Some(None));
        }
        let scope = if base {
            caller.map_or("", |node| parent(&node.qualified_name))
        } else {
            self.caller_scope(at)
        };
        let in_scopes = if name.global {
            vec![String::new()]
        } else {
            scopes(scope)
        };
        let exclude = base.then(|| caller.map(|node| node.id.as_str())).flatten();
        let lexical = if name.global {
            in_scopes.as_slice()
        } else {
            &in_scopes[..in_scopes.len() - 1]
        };
        let found = self.declared_node(self.class(written, lexical, at, declared)?, at, exclude);
        if found.is_some() {
            return Ok(found);
        }
        let lines = self
            .lines
            .get(at.file_path.as_deref().unwrap_or(""))
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .take(at.line.saturating_sub(1) as usize)
            .rev();
        for line in lines {
            let alias = regex(r"\bnamespace\s+(\w+)\s*=\s*([:\w]+)\s*;")?.captures(line)?;
            let using = regex(r"\busing\s+(namespace\s+)?([:\w]+)\s*;")?.captures(line)?;
            let rewritten = if let Some(alias) = alias
                && alias[1] == name.names[0]
            {
                Some(format!(
                    "{}{}",
                    &alias[2],
                    written.find("::").map_or("", |i| &written[i..])
                ))
            } else if !name.global
                && let Some(using) = using
            {
                if using.get(1).is_some() {
                    Some(format!("{}::{written}", &using[2]))
                } else if using[2].rsplit("::").next() == Some(name.names[0].as_str()) {
                    Some(format!(
                        "{}{}",
                        &using[2],
                        written.find("::").map_or("", |i| &written[i..])
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(rewritten) = rewritten {
                let scopes = if rewritten.starts_with("::") {
                    &[String::new()]
                } else {
                    in_scopes.as_slice()
                };
                if let Some(Some(node)) =
                    self.declared_node(self.class(&rewritten, scopes, at, declared)?, at, exclude)
                {
                    return Ok(Some(Some(node)));
                }
            }
        }
        if name.names.len() > 1
            && let Some(paths) = self.namespaces.aliases.get(&name.names[0])
        {
            if paths.len() != 1 {
                return Ok(Some(None));
            }
            let rewritten = format!("{}::{}", paths[0], name.names[1..].join("::"));
            if let Some(found) = self.declared_node(
                self.class(&rewritten, &in_scopes, at, declared)?,
                at,
                exclude,
            ) {
                return Ok(Some(found));
            }
        }
        let global = self.declared_node(
            self.class(written, &[String::new()], at, declared)?,
            at,
            exclude,
        );
        if global.is_some() {
            return Ok(global);
        }
        let path = name.names.join("::");
        let macros = self
            .named(name.names.last().unwrap())
            .filter(|node| {
                family(node)
                    && (CLASSES.contains(&node.kind.as_str()) || node.kind == "type_alias")
                    && self.namespaces.qualified(node) == path
            })
            .collect::<Vec<_>>();
        Ok((!macros.is_empty()).then(|| self.choose(macros, at)))
    }
    fn method(
        &self,
        class: &str,
        name: &str,
        arity: Option<usize>,
        depth: usize,
    ) -> Result<Vec<&'a Node>> {
        if depth >= 8 {
            return Ok(Vec::new());
        }
        let qualified = format!("{class}::{name}");
        let mut methods = self
            .qualified(&qualified)
            .filter(|node| family(node) && matches!(node.kind.as_str(), "method" | "function"))
            .collect::<Vec<_>>();
        if let Some(arity) = arity {
            let mut groups: BTreeMap<String, (Vec<&Node>, usize, usize)> = BTreeMap::new();
            for node in methods {
                if node
                    .signature
                    .as_deref()
                    .is_some_and(|signature| signature.contains("initializer_list"))
                {
                    return Ok(Vec::new());
                }
                let Some(shape) = constructor_shape(node.signature.as_deref())? else {
                    return Ok(Vec::new());
                };
                let group =
                    groups
                        .entry(shape.key)
                        .or_insert((Vec::new(), shape.minimum, shape.maximum));
                group.0.push(node);
                group.1 = group.1.min(shape.minimum);
            }
            let mut viable = groups
                .into_values()
                .filter(|(_, minimum, maximum)| *minimum <= arity && arity <= *maximum)
                .collect::<Vec<_>>();
            if viable.len() != 1 {
                return Ok(Vec::new());
            }
            let group = viable.pop().unwrap().0;
            let definitions = group
                .iter()
                .copied()
                .filter(|node| {
                    node.signature
                        .as_deref()
                        .is_some_and(|signature| !signature.ends_with(';'))
                })
                .collect::<Vec<_>>();
            let targets = if definitions.is_empty() {
                group
            } else {
                definitions
            };
            return Ok(if targets.len() == 1 {
                targets
            } else {
                Vec::new()
            });
        }
        if !methods.is_empty() {
            methods.sort_by_key(|node| {
                node.signature
                    .as_deref()
                    .is_some_and(|signature| signature.trim_end().ends_with(';'))
            });
            let signatures = methods
                .iter()
                .map(|node| self.signature_key(node))
                .collect::<Result<HashSet<_>>>()?;
            return Ok(if signatures.len() == 1 {
                vec![methods[0]]
            } else {
                Vec::new()
            });
        }
        for base in self.base_classes(class)? {
            let inherited = self.method(&base, name, arity, depth + 1)?;
            if !inherited.is_empty() {
                return Ok(inherited);
            }
        }
        Ok(Vec::new())
    }
    fn signature_key(&self, node: &Node) -> Result<String> {
        let signature = node.signature.as_deref().unwrap_or("");
        let signature = signature
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(';')
            .trim_end_matches(')');
        let mut names = Vec::new();
        for parameter in split(signature) {
            let parameter = parameter.split('=').next().unwrap_or("").trim();
            let without_name = regex(r"^(.*[\s*&])([A-Za-z_]\w*)\s*$")?.captures(parameter)?;
            names.push(without_name.map_or(parameter.to_string(), |capture| {
                capture[1].trim().to_string()
            }));
        }
        Ok(names.join(","))
    }
}
pub(in crate::graph::resolution) fn bindings(
    nodes: &[Node],
    sources: &BTreeMap<&str, &str>,
    references: &[UnresolvedReference],
    imports: &super::super::cpp_imports::CppImports,
) -> Result<Vec<Answer>> {
    let types = Types::new(nodes, sources)?;
    let receivers = receivers::read(nodes, sources)?;
    let resolve_include = |specifier: &str, from: &str| {
        imports.resolve(specifier, from).or_else(|| {
            let suffix = format!("/{specifier}");
            let matches = sources
                .keys()
                .filter(|file| **file == specifier || file.ends_with(&suffix))
                .collect::<Vec<_>>();
            (matches.len() == 1).then(|| matches[0].to_string())
        })
    };
    let macros = macros::Macros::new(sources, &resolve_include)?;
    let mut answers = Vec::new();
    for at in references
        .iter()
        .filter(|at| matches!(at.language.as_deref(), Some("c" | "cpp")))
    {
        if at.reference_kind == "imports" || at.reference_kind == "function_ref" {
            continue;
        }
        types.visible_at(at)?;
        if at.reference_kind == "calls" && regex(r"^\w+$")?.is_match(&at.reference_name)? {
            let is_macro = |node: &Node| {
                node.kind == "constant"
                    && node
                        .signature
                        .as_deref()
                        .is_some_and(|signature| signature.trim_start().starts_with("#define"))
            };
            let named = types.named(&at.reference_name).collect::<Vec<_>>();
            if named.iter().any(|node| is_macro(node))
                && (named.iter().all(|node| is_macro(node))
                    || macros.visible(at)?
                    || !named
                        .iter()
                        .any(|node| matches!(node.kind.as_str(), "function" | "method"))
                        && macros.possible(at)?)
            {
                answers.push(Answer {
                    reference: at.clone(),
                    targets: Vec::new(),
                    confidence: 0.0,
                    method: "exact-match",
                });
                continue;
            }
        }
        let mut confidence = 0.9;
        let mut method = "qualified-name";
        let result: Option<Vec<&Node>> =
            if matches!(at.reference_kind.as_str(), "extends" | "implements") {
                Some(
                    types
                        .named_type(&at.reference_name, at, false, true)?
                        .flatten()
                        .into_iter()
                        .collect(),
                )
            } else if at.reference_kind == "references"
                && types.ids.get(at.from_node_id.as_str()).is_some_and(|node| {
                    CLASSES.contains(&node.kind.as_str()) || node.kind == "type_alias"
                })
            {
                Some(
                    types
                        .named_type(&at.reference_name, at, true, false)?
                        .flatten()
                        .into_iter()
                        .collect(),
                )
            } else if at.reference_kind == "instantiates" {
                if types
                    .named(at.reference_name.rsplit("::").next().unwrap_or(""))
                    .next()
                    .is_none()
                {
                    answers.push(Answer {
                        reference: at.clone(),
                        targets: Vec::new(),
                        confidence: 0.0,
                        method: "qualified-name",
                    });
                    continue;
                }
                let written =
                    leading_type(types.site_text(at))?.unwrap_or_else(|| at.reference_name.clone());
                match types.alias(&written, at)? {
                    Some(Some(alias)) => {
                        let names: Vec<_> =
                            alias.target.iter().chain(&alias.rest).cloned().collect();
                        let class = types.reached(&names, &alias.scope, at)?;
                        Some(
                            class
                                .and_then(|class| {
                                    types.choose(
                                        types
                                            .qualified(&class)
                                            .filter(|node| CLASSES.contains(&node.kind.as_str()))
                                            .collect(),
                                        at,
                                    )
                                })
                                .or(types.named_type(&written, at, false, false)?.flatten())
                                .into_iter()
                                .collect(),
                        )
                    }
                    Some(None) => Some(Vec::new()),
                    None => types
                        .named_type(&written, at, false, false)?
                        .map(|node| node.into_iter().collect()),
                }
            } else if at.reference_kind == "calls" {
                let name = at.reference_name.as_str();
                if let Some((constructor, count)) = name.rsplit_once('/')
                    && let Ok(count) = count.parse::<usize>()
                    && let Some((written, _)) = constructor.rsplit_once("::")
                {
                    let class = match types.alias(written, at)? {
                        Some(Some(alias)) => {
                            let names: Vec<_> =
                                alias.target.iter().chain(&alias.rest).cloned().collect();
                            types.class_named(&names, &scopes(&alias.scope))
                        }
                        Some(None) => None,
                        None => types
                            .named_type(written, at, false, false)?
                            .flatten()
                            .filter(|node| CLASSES.contains(&node.kind.as_str()))
                            .map(|node| node.qualified_name.clone()),
                    };
                    Some(if let Some(class) = class {
                        types.method(
                            &class,
                            class.rsplit("::").next().unwrap_or(""),
                            Some(count),
                            0,
                        )?
                    } else {
                        Vec::new()
                    })
                } else if let Some((receiver, member)) = name.rsplit_once('.') {
                    method = "instance-method";
                    let class = types.caller_scope(at);
                    let local = receivers
                        .iter()
                        .filter(|binding| {
                            binding.name == receiver
                                && binding.owner.is_none()
                                && Some(binding.file.as_str()) == at.file_path.as_deref()
                                && binding.first <= at.line
                                && binding.last >= at.line
                                && binding.declared <= at.line
                        })
                        .max_by_key(|binding| binding.declared);
                    let field = receivers.iter().find(|binding| {
                        binding.name == receiver && binding.owner.as_deref() == Some(class)
                    });
                    if receiver == "this" {
                        Some(types.method(class, member, None, 0)?)
                    } else if let Some(binding) = local.or(field) {
                        let mut known = true;
                        let class = match types.alias(&binding.raw, at)? {
                            Some(Some(alias)) => {
                                known = alias.pointer
                                    || !alias.class_template && !types.site_text(at).contains("->");
                                let names: Vec<_> =
                                    alias.target.iter().chain(&alias.rest).cloned().collect();
                                types.reached(&names, &alias.scope, at)?
                            }
                            Some(None) => None,
                            None => {
                                let found =
                                    types.named_type(&binding.raw, at, false, false)?.flatten();
                                if let Some(node) =
                                    found.filter(|node| CLASSES.contains(&node.kind.as_str()))
                                {
                                    Some(node.qualified_name.clone())
                                } else {
                                    let factory = types
                                        .named(&binding.raw)
                                        .filter(|node| family(node) && node.kind == "function")
                                        .collect::<Vec<_>>();
                                    if factory.len() == 1 {
                                        let returns = leading_type(&types.text(factory[0]))?;
                                        if let Some(returns) = returns {
                                            types
                                                .named_type(&returns, at, false, false)?
                                                .flatten()
                                                .filter(|node| {
                                                    CLASSES.contains(&node.kind.as_str())
                                                })
                                                .map(|node| node.qualified_name.clone())
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }
                            }
                        };
                        let methods = if let Some(class) = class {
                            types.method(&class, member, None, 0)?
                        } else {
                            Vec::new()
                        };
                        if methods.is_empty() && !known {
                            let candidates = types
                                .named(member)
                                .filter(|node| {
                                    family(node)
                                        && node.kind == "method"
                                        && node.id != at.from_node_id
                                })
                                .collect::<Vec<_>>();
                            if candidates.len() == 1 {
                                confidence = 0.7;
                                Some(candidates)
                            } else {
                                None
                            }
                        } else {
                            Some(methods)
                        }
                    } else {
                        None
                    }
                } else if let Some((qualifier, member)) = name.rsplit_once("::") {
                    let class = match types.alias(qualifier, at)? {
                        Some(Some(alias)) => {
                            confidence = if types
                                .named(member)
                                .filter(|node| node.kind == "method")
                                .count()
                                > 1
                            {
                                0.65
                            } else {
                                0.7
                            };
                            method = "instance-method";
                            let names: Vec<_> =
                                alias.target.iter().chain(&alias.rest).cloned().collect();
                            types.reached(&names, &alias.scope, at)?
                        }
                        Some(None) => return_blocked(&mut answers, at),
                        None => types
                            .named_type(qualifier, at, false, false)?
                            .flatten()
                            .filter(|node| CLASSES.contains(&node.kind.as_str()))
                            .map(|node| node.qualified_name.clone()),
                    };
                    let class = if class.is_none() {
                        if let Some(caller) = types.ids.get(at.from_node_id.as_str()) {
                            let parameters = super::super::type_parameters::declared(
                                caller,
                                &types.text(caller),
                                &types.before(caller, 8),
                            )?;
                            if let Some(Some(default)) = parameters.get(qualifier) {
                                confidence = 0.7;
                                method = "instance-method";
                                types
                                    .named_type(default, at, false, false)?
                                    .flatten()
                                    .map(|node| node.qualified_name.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        class
                    };
                    if let Some(class) = class {
                        if method == "qualified-name" {
                            confidence = 0.85;
                        }
                        Some(types.method(&class, member, None, 0)?)
                    } else if let Some(Some(node)) = types.named_type(name, at, false, false)? {
                        let constructors = if CLASSES.contains(&node.kind.as_str()) {
                            types.method(&node.qualified_name, &node.name, None, 0)?
                        } else {
                            Vec::new()
                        };
                        if constructors.is_empty() {
                            confidence = if types.namespaces.qualified(node) == name
                                && node.qualified_name != name
                            {
                                0.8
                            } else {
                                0.95
                            };
                            Some(vec![node])
                        } else {
                            confidence = 0.7;
                            method = "instance-method";
                            Some(constructors)
                        }
                    } else {
                        let mut found = Vec::new();
                        for scope in scopes(types.caller_scope(at)) {
                            let qualified = if scope.is_empty() {
                                name.to_string()
                            } else {
                                format!("{scope}::{name}")
                            };
                            found = types
                                .qualified(&qualified)
                                .filter(|node| family(node) && node.kind == "function")
                                .collect();
                            if !found.is_empty() {
                                break;
                            }
                        }
                        if found.is_empty() {
                            found = types
                                .named(member)
                                .filter(|node| {
                                    family(node)
                                        && matches!(node.kind.as_str(), "function" | "type_alias")
                                        && types.namespaces.qualified(node) == name
                                })
                                .collect();
                            if !found.is_empty() {
                                confidence = 0.8;
                            }
                        }
                        if found.is_empty()
                            && qualifier
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
                        {
                            let namespaces: HashSet<_> = scopes(types.caller_scope(at))
                                .into_iter()
                                .map(|scope| {
                                    if scope.is_empty() {
                                        qualifier.to_string()
                                    } else {
                                        format!("{scope}::{qualifier}")
                                    }
                                })
                                .collect();
                            for node in types
                                .named(member)
                                .filter(|node| family(node) && node.kind == "method")
                            {
                                let owner = parent(&node.qualified_name);
                                if !owner.ends_with("::<anonymous>")
                                    || !namespaces.contains(parent(owner))
                                {
                                    continue;
                                }
                                for class in types.qualified(owner) {
                                    if regex(&format!(
                                        r"\b(?:class|struct|union)\s+{}\s*<",
                                        ::regex::escape(member)
                                    ))?
                                    .is_match(&types.text(class))?
                                    {
                                        found.push(node);
                                        break;
                                    }
                                }
                            }
                            if found.len() == 1 {
                                confidence = 0.7;
                                method = "instance-method";
                            }
                        }
                        if found.is_empty()
                            && qualifier
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
                        {
                            Some(Vec::new())
                        } else if found.is_empty() {
                            None
                        } else {
                            Some(types.choose(found, at).into_iter().collect())
                        }
                    }
                } else {
                    let functions = types
                        .named(name)
                        .filter(|node| family(node) && node.kind == "function")
                        .collect::<Vec<_>>();
                    if functions.len() == 1 {
                        method = "exact-match";
                        Some(functions)
                    } else if functions.is_empty() {
                        if let Some(Some(node)) = types.named_type(name, at, false, false)? {
                            if CLASSES.contains(&node.kind.as_str()) {
                                let constructors = types
                                    .qualified(&format!("{}::{}", node.qualified_name, node.name))
                                    .filter(|node| {
                                        node.kind == "method"
                                            && node.signature.as_deref().is_some_and(|signature| {
                                                !signature.trim_end().ends_with(';')
                                            })
                                    })
                                    .collect::<Vec<_>>();
                                method = "exact-match";
                                if constructors.is_empty() {
                                    Some(vec![node])
                                } else {
                                    confidence = 0.4;
                                    Some(constructors)
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            } else {
                None
            };
        if let Some(targets) = result {
            answers.push(Answer {
                reference: at.clone(),
                targets: targets.iter().map(|node| node.id.clone()).collect(),
                confidence,
                method,
            });
        }
    }
    Ok(answers)
}
fn return_blocked(answers: &mut Vec<Answer>, at: &UnresolvedReference) -> Option<String> {
    answers.push(Answer {
        reference: at.clone(),
        targets: Vec::new(),
        confidence: 0.9,
        method: "qualified-name",
    });
    None
}
