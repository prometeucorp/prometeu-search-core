//! The reference's enclosing-function receiver declaration scan and typed calls.
use super::*;
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Reading {
    #[serde(rename = "type")]
    pub type_name: String,
    pub raw: String,
    pub line: u32,
}
const ARRAY_METHODS: &[&str] = &[
    "at",
    "concat",
    "copyWithin",
    "entries",
    "every",
    "fill",
    "filter",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flat",
    "flatMap",
    "forEach",
    "includes",
    "indexOf",
    "join",
    "keys",
    "lastIndexOf",
    "map",
    "pop",
    "push",
    "reduce",
    "reduceRight",
    "reverse",
    "shift",
    "slice",
    "some",
    "sort",
    "splice",
    "toReversed",
    "toSorted",
    "toSpliced",
    "unshift",
    "values",
    "with",
    "toString",
    "toLocaleString",
    "valueOf",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
];
const BUILTINS: &[&str] = &[
    "console",
    "window",
    "document",
    "global",
    "process",
    "Promise",
    "Array",
    "Object",
    "String",
    "Number",
    "Boolean",
    "Date",
    "Math",
    "JSON",
    "RegExp",
    "Error",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "fetch",
    "require",
    "module",
    "exports",
    "__dirname",
    "__filename",
    "string",
    "number",
    "boolean",
    "bigint",
    "symbol",
    "void",
    "undefined",
    "null",
    "never",
    "unknown",
    "any",
    "object",
];
fn normalize(raw: &str) -> Result<Option<String>> {
    let no_generics = regex(r"<[^>]*>")?.replace_all(raw, "");
    let cleaned = regex(r"[&*]")?.replace_all(&no_generics, "");
    let name = trim(&cleaned)
        .rsplit(['.', ':'])
        .find(|part| !part.is_empty());
    Ok(name
        .filter(|name| {
            !matches!(
                *name,
                "this"
                    | "self"
                    | "super"
                    | "new"
                    | "return"
                    | "await"
                    | "yield"
                    | "typeof"
                    | "null"
                    | "nil"
                    | "None"
                    | "true"
                    | "false"
                    | "True"
                    | "False"
                    | "undefined"
            )
        })
        .map(str::to_string))
}
impl<'a> Context<'a> {
    pub(super) fn infer_local(
        &self,
        receiver: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<Reading>> {
        let path = at.file_path.as_deref().unwrap_or("");
        let language = at.language.as_deref().unwrap_or("");
        let start = self
            .in_file(path)
            .filter(|node| {
                matches!(node.kind.as_str(), "function" | "method")
                    && node.language == language
                    && node.start_line <= at.line
                    && node.end_line >= at.line
            })
            .map(|node| node.start_line)
            .max()
            .unwrap_or(1);
        let lines = self.lines(path)?;
        let end = at.line.max(1).min(lines.starts.len() as u32);
        let key = (path.to_string(), receiver.to_string(), start);
        let previous = self
            .local_scans
            .borrow()
            .get(&key)
            .map(|state| (state.high, state.answer.clone()));
        if let Some((high, answer)) = &previous
            && *high == end
        {
            return Ok(answer.clone());
        }
        let monotonic = previous.as_ref().is_none_or(|(high, _)| *high <= end);
        let low = if monotonic {
            previous.as_ref().map_or(start, |(high, _)| high + 1)
        } else {
            start
        };
        let mut answer = None;
        let rows: Vec<_> = lines
            .enumerate()
            .skip(low.saturating_sub(1) as usize)
            .take((end.saturating_sub(low) + 1) as usize)
            .collect();
        let patterns = [
            r"^([^\x00]*)\x00[\s\S]*?\b\1\b\s*=\s*new\s+([A-Za-z_$][\w.$]*)",
            r"^([^\x00]*)\x00[\s\S]*?\b\1\b\s*=\s*(?:inject|(?:this\.)?(?:[\w$#]+\.)*createInstance)\s*<\s*([A-Z][\w.$]*)[^;=(){}]*>\s*\((?![^;]*\)\s*(?:\?\.|\.|\[))",
            r"^([^\x00]*)\x00[\s\S]*?\b\1\b\s*=\s*(?:inject|(?:this\.)?(?:[\w$#]+\.)*createInstance)\s*\(\s*([A-Z][\w.$]*)(?![^;]*\)\s*(?:\?\.|\.|\[))",
            r"^([^\x00]*)\x00[\s\S]*?\b\1\b\s*:\s*([A-Z][\w.$]*(?:(?:\s*<[^;=(){}<>]*(?:<[^;=(){}<>]*>[^;=(){}<>]*)*>)?\s*\[[^\]]*\])?)",
        ];
        for (row, line) in rows.into_iter().rev() {
            self.step()?;
            if !line.contains(receiver) || line.encode_utf16().count() > 10_000 {
                continue;
            }
            let input = format!("{receiver}\0{line}");
            for pattern in patterns {
                let expression = regex(pattern)?;
                if let Some(found) = expression.captures(&input)?
                    && let Some(type_name) = normalize(&found[2])?
                {
                    answer = Some(Reading {
                        type_name,
                        raw: found[2].to_string(),
                        line: row as u32 + 1,
                    });
                    break;
                }
            }
            if answer.is_some() {
                break;
            }
        }
        if monotonic {
            if answer.is_none() {
                answer = previous.and_then(|(_, answer)| answer);
            }
            self.local_scans.borrow_mut().insert(
                key,
                ScanState {
                    high: end,
                    answer: answer.clone(),
                },
            );
        }
        Ok(answer)
    }
    fn indexed_property(
        &self,
        indexed: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<(String, bool, String)>> {
        let pattern = regex(r#"^([A-Za-z_$][\w$]*)\s*\[\s*(['"])([\w$]+)\2\s*\]$"#)?;
        let Some(found) = pattern.captures(indexed)? else {
            return Ok(None);
        };
        let path = at.file_path.as_deref().unwrap_or("");
        let language = at.language.as_deref().unwrap_or("");
        let imported = self.imported_file(&found[1], path, language, at)?;
        let mut owners: Vec<_> = self
            .named(&found[1])
            .filter(|node| {
                matches!(
                    node.kind.as_str(),
                    "class" | "interface" | "type_alias" | "component"
                ) && family(&node.language, language)
            })
            .collect();
        owners.sort_by_key(|node| node.file_path != imported.as_deref().unwrap_or(path));
        for owner in owners {
            let Some(declaration) = self.field(owner, &found[3], at)? else {
                continue;
            };
            if declaration.value_type || self.type_parameter(owner, &declaration.type_name)? {
                return Ok(None);
            }
            let file = self
                .imported_file(
                    &declaration.type_name,
                    &owner.file_path,
                    &owner.language,
                    at,
                )?
                .unwrap_or_else(|| owner.file_path.clone());
            return Ok(Some((declaration.type_name, declaration.array, file)));
        }
        Ok(None)
    }
    pub(super) fn local_call(
        &self,
        receiver: &str,
        method: &str,
        at: &UnresolvedReference,
    ) -> Result<Option<Option<&'a Node>>> {
        let Some(reading) = self.infer_local(receiver, at)? else {
            return Ok(None);
        };
        let mut inferred = reading.type_name;
        let path = at.file_path.as_deref().unwrap_or("");
        let language = at.language.as_deref().unwrap_or("");
        if inferred.ends_with(']') {
            let array = regex(r"^(.+?)\s*\[\s*\]$")?
                .captures(&inferred)?
                .map(|capture| capture[1].to_string());
            let declared = if let Some(element) = &array {
                Some((element.clone(), true, String::new()))
            } else {
                self.indexed_property(&inferred, at)?
            };
            if let Some((kind, is_array, file)) = declared {
                if is_array && ARRAY_METHODS.contains(&method) {
                    return Ok(Some(None));
                }
                let Some(kind) = normalize(&kind)? else {
                    return Ok(Some(None));
                };
                inferred = kind;
                if array.is_none() {
                    return Ok(Some(self.fallback_method(&inferred, method, at, &file, 0)?));
                }
            } else {
                inferred = regex(r"\s*\[[^\]]*\]$")?.replace(&inferred, "").to_string();
            }
        }
        let mut imported = self.imported_file(&reading.raw, path, language, at)?;
        if imported.is_none() {
            match self.outside(&reading.raw, &inferred, path, language)? {
                Some(None) => return Ok(Some(None)),
                Some(Some(file)) => imported = Some(file),
                None => {}
            }
        }
        let file = imported.as_deref().unwrap_or(path);
        if let Some(Some(method)) =
            self.known_method(&inferred, file, method, path, language, at)?
        {
            return Ok(Some(Some(method)));
        }
        if let Some(method) = self.fallback_method(&inferred, method, at, file, 0)? {
            return Ok(Some(Some(method)));
        }
        if BUILTINS.contains(&inferred.as_str())
            || inferred.rsplit('.').next().is_some_and(|name| {
                name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                    && !self.named(name).any(|node| {
                        matches!(
                            node.kind.as_str(),
                            "class" | "struct" | "union" | "interface"
                        ) && family(&node.language, language)
                    })
            })
        {
            return Ok(Some(None));
        }
        Ok(None)
    }
}
