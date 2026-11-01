use super::*;

pub(super) fn builtin_method(name: &str) -> bool {
    matches!(
        name,
        "append"
            | "extend"
            | "insert"
            | "remove"
            | "pop"
            | "clear"
            | "sort"
            | "reverse"
            | "copy"
            | "update"
            | "keys"
            | "values"
            | "items"
            | "get"
            | "add"
            | "discard"
            | "union"
            | "intersection"
            | "difference"
            | "split"
            | "join"
            | "strip"
            | "lstrip"
            | "rstrip"
            | "replace"
            | "lower"
            | "upper"
            | "startswith"
            | "endswith"
            | "find"
            | "index"
            | "count"
            | "encode"
            | "decode"
            | "format"
            | "isdigit"
            | "isalpha"
            | "isalnum"
            | "read"
            | "write"
            | "readline"
            | "readlines"
            | "close"
            | "flush"
            | "seek"
    )
}
fn builtin_type(name: &str) -> bool {
    matches!(
        name,
        "list"
            | "dict"
            | "set"
            | "tuple"
            | "str"
            | "int"
            | "float"
            | "bool"
            | "bytes"
            | "bytearray"
            | "frozenset"
            | "object"
            | "super"
    )
}
impl<'a> Typed<'a, '_> {
    pub(super) fn builtin(&self, reference: &UnresolvedReference) -> Result<bool> {
        let name = &reference.reference_name;
        if matches!(
            name.as_str(),
            "print"
                | "len"
                | "range"
                | "str"
                | "int"
                | "float"
                | "list"
                | "dict"
                | "set"
                | "tuple"
                | "open"
                | "input"
                | "type"
                | "isinstance"
                | "hasattr"
                | "getattr"
                | "setattr"
                | "super"
                | "self"
                | "cls"
                | "None"
                | "True"
                | "False"
        ) {
            return Ok(true);
        }
        if let Some((receiver, method)) = name.split_once('.') {
            if builtin_type(receiver) {
                return Ok(true);
            }
            if builtin_method(method) {
                let path = reference.file_path.as_deref().unwrap_or_default();
                for node in self.named(receiver) {
                    if node.file_path==path&&matches!(node.kind.as_str(),"variable"|"constant")&&node.qualified_name==receiver&&regex(r"^=\s*(?:[\[{]|(?:dict|list|set|tuple|frozenset)\s*\(|\(\s*\)|\([^()]*,)")?.is_match(node.signature.as_deref().unwrap_or_default())? {return Ok(true)}
                }
                let mut capitalized = receiver.to_owned();
                if let Some(first) = receiver.chars().next() {
                    capitalized =
                        format!("{}{}", first.to_uppercase(), &receiver[first.len_utf8()..]);
                }
                let class = self
                    .named(&capitalized)
                    .iter()
                    .any(|node| matches!(node.kind.as_str(), "class" | "struct" | "interface"));
                if !class && !self.project_module(receiver, reference)? {
                    return Ok(true);
                }
            }
        }
        Ok(builtin_method(name) && !self.known.contains(name.as_str()))
    }
    fn project_module(&self, receiver: &str, reference: &UnresolvedReference) -> Result<bool> {
        let path = reference.file_path.as_deref().unwrap_or_default();
        if !self
            .mappings
            .get(path)
            .into_iter()
            .flatten()
            .any(|mapping| mapping.local_name == receiver)
        {
            return Ok(false);
        }
        if let Some(local) = self.scopes.function_module_alias(receiver, reference)? {
            return Ok((self.module)(&local, path).is_some());
        }
        for mapping in self.imports.scoped(reference)? {
            if mapping.local_name != receiver {
                continue;
            }
            let source = if mapping.is_namespace {
                mapping.source.clone()
            } else {
                format!(
                    "{}{}{}",
                    mapping.source,
                    if mapping.source.ends_with('.') {
                        ""
                    } else {
                        "."
                    },
                    mapping.exported_name
                )
            };
            if (self.module)(&source, path).is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(super) fn chain(&self, reference: &UnresolvedReference) -> Result<Option<Answer<'a>>> {
        let Some(capture) =
            regex(r"^([A-Za-z_][\w.]*)\(\)\.(\w+)$")?.captures(&reference.reference_name)?
        else {
            return Ok(None);
        };
        let callee = &capture[1];
        let path = reference.file_path.as_deref().unwrap_or_default();
        let lines = self.line_text(path);
        let function = self
            .in_file(path)
            .iter()
            .copied()
            .filter(|node| {
                matches!(node.kind.as_str(), "function" | "method")
                    && node.start_line <= reference.line
                    && node.end_line >= reference.line
            })
            .max_by_key(|node| node.start_line);
        if let Some(function) = function
            && !lines.is_empty()
            && self.bound(
                lines,
                function.start_line as usize,
                reference.line as usize,
                callee.split('.').next().unwrap_or_default(),
                true,
            )?
        {
            return Ok(None);
        }
        let mut read = Read::new();
        let made = if capital(callee.rsplit('.').next().unwrap_or_default()) {
            self.class_named(callee, path, "class")?
                .map_or(Type::Unknown, Type::Class)
        } else {
            self.factory(callee, false, path, &mut read, 0)?
        };
        let Type::Class(class) = made else {
            return Ok(None);
        };
        Ok(self
            .method(class, &capture[2], &mut read)?
            .map(|answer| with_read(answer, read, path)))
    }
    pub(in super::super) fn select(&self, reference: &UnresolvedReference) -> Result<Match<'a>> {
        if self.builtin(reference)? {
            let local =
                reference.reference_kind == "calls"
                    && reference.reference_name.split_once('.').is_some_and(
                        |(receiver, method)| builtin_method(method) && !builtin_type(receiver),
                    );
            return Ok(if local {
                self.local(reference)?
                    .map_or(Match::Rejected, Match::Resolved)
            } else {
                Match::Rejected
            });
        }
        if reference.reference_kind != "calls" {
            return Ok(Match::Unhandled);
        }
        if regex(r"^(.+)\(\)\.(\w+)$")?.is_match(&reference.reference_name)? {
            return Ok(self
                .chain(reference)?
                .map_or(Match::Rejected, Match::Resolved));
        }
        let Some((receiver, method)) = reference
            .reference_name
            .rsplit_once('.')
            .filter(|(receiver, _)| receiver.contains('.'))
        else {
            return Ok(Match::Unhandled);
        };
        let imported = self.imports.resolve(reference)?;
        if let Some(imported) = imported
            && imported.confidence >= 0.9
        {
            return Ok(Match::Resolved(Answer {
                node: imported.node,
                confidence: imported.confidence,
                resolved_by: imported.resolved_by,
                metadata: None,
            }));
        }
        let typed = self.attribute(receiver, method, reference)?;
        Ok(match (typed, imported) {
            (Match::Unhandled, _) => Match::Unhandled,
            (Match::Resolved(typed), Some(imported)) if imported.confidence >= typed.confidence => {
                Match::Resolved(Answer {
                    node: imported.node,
                    confidence: imported.confidence,
                    resolved_by: imported.resolved_by,
                    metadata: None,
                })
            }
            (Match::Rejected, Some(imported)) => Match::Resolved(Answer {
                node: imported.node,
                confidence: imported.confidence,
                resolved_by: imported.resolved_by,
                metadata: None,
            }),
            (typed, _) => typed,
        })
    }
}
