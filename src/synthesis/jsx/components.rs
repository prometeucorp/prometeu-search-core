//! React component declarations, wrappers, and explicitly forwarded exports.
use super::super::*;
use crate::frameworks::frontend::{
    react_exports,
    react_loader::{self, Reader},
};

pub(super) struct Components<'a, 'c> {
    ctx: &'c Context<'a>,
    exports: HashMap<(String, String), Option<&'a Node>>,
    own: HashMap<(String, String), Option<Own<'a>>>,
}
#[derive(Clone, Copy)]
pub(super) struct Own<'a> {
    pub declaration: &'a Node,
    pub component: Option<&'a Node>,
}
impl<'a, 'c> Components<'a, 'c> {
    pub(super) fn new(ctx: &'c Context<'a>) -> Self {
        Self {
            ctx,
            exports: HashMap::new(),
            own: HashMap::new(),
        }
    }
    pub(super) fn declared(&mut self, name: &str, file: &str) -> Result<Option<Own<'a>>> {
        let key = (file.to_owned(), name.to_owned());
        if let Some(own) = self.own.get(&key) {
            return Ok(*own);
        }
        let own = if let Some(declaration) = self.ctx.in_file(file).find(|n| {
            n.name == name
                && matches!(n.kind.as_str(), "constant" | "variable")
                && n.qualified_name == name
        }) {
            let component = if let Some((spec, member)) = self.value_loader(declaration)? {
                self.lazy_export(&spec, &member, file)?
            } else {
                self.wrapped(declaration)?
            };
            Some(Own {
                declaration,
                component,
            })
        } else {
            None
        };
        self.own.insert(key, own);
        Ok(own)
    }
    fn module(&self, spec: &str, file: &str) -> Option<String> {
        let language = language(file);
        (self.ctx.resolve_module)(spec, file, language).or_else(|| {
            (language != "typescript")
                .then(|| (self.ctx.resolve_module)(spec, file, "typescript"))
                .flatten()
        })
    }
    fn lazy_export(&mut self, spec: &str, member: &str, file: &str) -> Result<Option<&'a Node>> {
        let Some(module) = self.module(spec, file) else {
            return Ok(None);
        };
        let exported = self.exported(&module, member)?;
        if exported.is_none() && member == "default" {
            self.exported(&module, "Component")
        } else {
            Ok(exported)
        }
    }
    pub(super) fn exported(&mut self, file: &str, name: &str) -> Result<Option<&'a Node>> {
        let key = (file.into(), name.into());
        if let Some(value) = self.exports.get(&key) {
            return Ok(*value);
        }
        let value = self.forwarded(file, name, &HashSet::new())?;
        self.exports.insert(key, value);
        Ok(value)
    }
    fn forwarded(
        &self,
        file: &str,
        name: &str,
        visited: &HashSet<String>,
    ) -> Result<Option<&'a Node>> {
        self.ctx.step()?;
        if visited.contains(file) || visited.len() > 4 {
            return Ok(None);
        }
        let mut through = visited.clone();
        through.insert(file.into());
        let code = comments::strip_for_regex(
            self.ctx.sources.get(file).copied().unwrap_or(""),
            "typescript",
        );
        if let Some(local) = own_export(&code, name)? {
            let declared_as = |binding: &str| {
                let named = self
                    .ctx
                    .in_file(file)
                    .filter(|n| {
                        n.name == binding
                            && matches!(
                                n.kind.as_str(),
                                "function" | "component" | "class" | "constant" | "variable"
                            )
                    })
                    .collect::<Vec<_>>();
                named
                    .iter()
                    .copied()
                    .find(|n| n.qualified_name == binding)
                    .or_else(|| named.first().copied())
            };
            let exported = declared_as(&local);
            let mut binding = local.clone();
            let mut declared = exported;
            for _ in 0..4 {
                let Some(node) = declared else { break };
                let held = if node.kind == "constant" {
                    self.held(node)?
                } else {
                    None
                };
                let Some(held) = held else { break };
                binding = held;
                declared = declared_as(&binding);
            }
            if let Some(declared) = declared {
                let wrapped = if matches!(declared.kind.as_str(), "constant" | "variable") {
                    self.wrapped(declared)?
                } else if declared.kind == "component" {
                    self.handed(declared)?
                } else {
                    None
                };
                return Ok(Some(wrapped.unwrap_or(declared)));
            }
            if binding == local
                && name == "default"
                && react_exports::default_binding(&code)?.as_deref() != Some(local.as_str())
            {
                return Ok(None);
            }
            if let Some(import) = self.ctx.imports.get(&(file, binding.as_str()))
                && !import.is_namespace
            {
                let target = (self.ctx.resolve_module)(&import.source, file, language(file));
                if let Some(target) = target {
                    let id = self.forwarded(
                        &target,
                        if import.is_default {
                            "default"
                        } else {
                            &import.imported_name
                        },
                        &through,
                    )?;
                    if id.is_some() {
                        return Ok(id);
                    }
                }
            }
            return Ok(exported);
        }
        let exports = react_exports::reexports(&code)?;
        for export in &exports {
            if export.kind == "named" && export.exported_name.as_deref() == Some(name) {
                return match (self.ctx.resolve_module)(&export.source, file, language(file)) {
                    Some(target) => {
                        self.forwarded(&target, export.original_name.as_deref().unwrap(), &through)
                    }
                    None => Ok(None),
                };
            }
        }
        if name == "default" {
            return Ok(None);
        }
        let mut found: Option<&Node> = None;
        for export in exports
            .into_iter()
            .filter(|export| export.kind == "wildcard")
        {
            let Some(target) = (self.ctx.resolve_module)(&export.source, file, language(file))
            else {
                continue;
            };
            if let Some(id) = self.forwarded(&target, name, &through)? {
                if found.is_some_and(|previous| previous.id != id.id) {
                    return Ok(None);
                }
                found = Some(id);
            }
        }
        Ok(found)
    }
    fn value_loader(&self, node: &Node) -> Result<Option<(String, String)>> {
        let code = comments::strip_for_regex(&declaration(node, self.ctx), "typescript");
        let Some(init) = react_loader::initializer_at(&code, node.name.len())? else {
            return Ok(None);
        };
        let reader = Reader::default();
        let Some(loader) = reader.loader_of(&code[init..])? else {
            return Ok(None);
        };
        let page = reader.loaded_page(reader.value(&loader)?)?;
        if page.is_some() {
            return Ok(page);
        }
        if let Some(function) = react_loader::function(&loader)?
            && !function.block
            && let Some(found) =
                regex(r#"\bimport\s*\(\s*["']([^"']+)["']\s*\)"#)?.captures(function.body)?
        {
            return Ok(Some((found[1].into(), "default".into())));
        }
        Ok(None)
    }
    fn wrapped(&self, value: &Node) -> Result<Option<&'a Node>> {
        let inner = self
            .ctx
            .in_file(&value.file_path)
            .filter(|n| {
                kind(n)
                    && n.qualified_name == format!("{}::{}", value.qualified_name, n.name)
                    && n.start_line >= value.start_line
                    && n.end_line <= value.end_line
            })
            .collect::<Vec<_>>();
        let held = inner
            .iter()
            .copied()
            .find(|n| n.name == value.name)
            .or_else(|| {
                (inner.len() == 1 && inner[0].end_line == value.end_line).then(|| inner[0])
            });
        if held.is_some() || !inner.is_empty() {
            return Ok(held);
        }
        self.handed(value)
    }
    fn held(&self, value: &Node) -> Result<Option<String>> {
        let code = comments::strip_for_regex(&declaration(value, self.ctx), "typescript");
        let Some(init) = react_loader::initializer_at(&code, value.name.len())? else {
            return Ok(None);
        };
        Ok(regex(r"^\s*([A-Za-z_$][\w$]*)\s*(?:$|(?:as|satisfies)\s)")?
            .captures(&code[init..react_loader::statement_end(&code, init, false)?])?
            .filter(|m| m[1] != value.name)
            .map(|m| m[1].into()))
    }
    fn handed(&self, value: &Node) -> Result<Option<&'a Node>> {
        let code = comments::strip_for_regex(&declaration(value, self.ctx), "typescript");
        let Some(init) = react_loader::initializer_at(&code, value.name.len())? else {
            return Ok(None);
        };
        let handed = Reader::default()
            .wrapped_name(&code[init..react_loader::statement_end(&code, init, false)?])?;
        Ok(handed
            .filter(|name| pascal(name) && name != &value.name)
            .and_then(|handed| {
                self.ctx
                    .in_file(&value.file_path)
                    .find(|n| n.name == handed && n.qualified_name == handed && kind(n))
            }))
    }
    pub(super) fn wrapped_name(&self, node: &Node) -> Result<Option<String>> {
        let code = comments::strip_for_regex(&declaration(node, self.ctx), "typescript");
        let Some(init) = react_loader::initializer_at(&code, node.name.len())? else {
            return Ok(None);
        };
        let expression = code[init..react_loader::statement_end(&code, init, false)?].trim();
        let name = if let Some(styled) =
            regex(r"^styled\s*\(\s*([A-Za-z_$][\w$]*)\s*[,)]")?.captures(expression)?
        {
            Some(styled[1].into())
        } else if regex(r"^(?:React\s*\.\s*)?(?:memo|forwardRef)\b")?.is_match(expression)? {
            Reader::default().wrapped_name(expression)?
        } else {
            None
        };
        Ok(name.filter(|name| pascal(name) && name != &node.name))
    }
}
pub(super) fn kind(node: &Node) -> bool {
    matches!(node.kind.as_str(), "component" | "function" | "class")
}
fn pascal(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && name.bytes().all(|b| b.is_ascii_alphanumeric())
}
pub(super) fn language(file: &str) -> &'static str {
    if file.ends_with(".tsx") {
        "tsx"
    } else if [".ts", ".mts", ".cts"]
        .iter()
        .any(|suffix| file.ends_with(suffix))
    {
        "typescript"
    } else if file.ends_with(".jsx") {
        "jsx"
    } else {
        "javascript"
    }
}
pub(super) fn byte(source: &str, units: u32) -> usize {
    let mut read = 0;
    for (at, ch) in source.char_indices() {
        if read >= units as usize {
            return at;
        }
        read += ch.len_utf16();
    }
    source.len()
}
fn declaration(node: &Node, ctx: &Context<'_>) -> String {
    let source = ctx
        .sources
        .get(node.file_path.as_str())
        .copied()
        .unwrap_or("");
    let mut lines = source
        .split('\n')
        .skip(node.start_line.saturating_sub(1) as usize)
        .take(node.end_line.saturating_sub(node.start_line) as usize + 1)
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    if let Some(last) = lines.last_mut()
        && node.end_column > 0
    {
        *last = &last[..byte(last, node.end_column)];
    }
    if let Some(first) = lines.first_mut() {
        *first = &first[byte(first, node.start_column)..];
    }
    lines.join("\n")
}
fn own_export(source: &str, name: &str) -> Result<Option<String>> {
    if name == "default" {
        if let Some(at) = regex(r"\bexport\s+default\s+")?.find(source)? {
            let rest = &source[at.end()..];
            if let Some(named)=regex(r"^(?:async\s+)?(?:function\s*\*?\s*|class\s+)([A-Za-z_$][\w$]*)|^([A-Za-z_$][\w$]*)(?:[ \t]*(?:;|\r?\n|$)|\s+(?:as|satisfies)\s)")?.captures(rest)?{return Ok(Some(named.get(1).or_else(||named.get(2)).unwrap().as_str().into()))}
            if let Some(name) = Reader::default()
                .wrapped_name(&rest[..react_loader::statement_end(rest, 0, false)?])?
            {
                return Ok(Some(name));
            }
        }
    } else if regex(&format!(
        r"\bexport\s+(?:async\s+)?(?:const|let|var|function|class)\s+{}(?![\w$])",
        regex::escape(name)
    ))?
    .is_match(source)?
    {
        return Ok(Some(name.into()));
    }
    Ok(react_exports::local(source)?
        .into_iter()
        .find(|alias| alias.exported_name == name)
        .map(|alias| alias.local_name))
}
