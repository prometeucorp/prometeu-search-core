use super::{regex, string, string_arg};
use anyhow::{Result, ensure};
use std::cell::Cell;
pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(crate::search::query_whitespace)
}
#[derive(Default)]
pub(super) struct Scan {
    work: Cell<usize>,
}
pub(super) struct Hit<'a> {
    pub name: String,
    pub args: &'a str,
    pub start: usize,
    pub end: usize,
}
pub(super) struct Scope {
    pub kind: &'static str,
    pub prefix: String,
    pub start: usize,
    pub end: usize,
}
impl Scan {
    pub fn step(&self, n: usize) -> Result<()> {
        let work = self.work.get().saturating_add(n);
        ensure!(work <= 10_000_000, "decorator scan work limit exceeded");
        self.work.set(work);
        Ok(())
    }
    fn string_end(&self, source: &str, start: usize) -> Result<usize> {
        let bytes = source.as_bytes();
        let quote = bytes[start];
        let mut at = start + 1;
        while at < bytes.len() {
            self.step(1)?;
            if bytes[at] == quote {
                return Ok(at + 1);
            }
            if bytes[at] == b'\\' {
                at += 1
            }
            at += 1;
        }
        Ok(bytes.len())
    }
    pub fn close(&self, source: &str, open: usize, any: bool) -> Result<Option<usize>> {
        let bytes = source.as_bytes();
        if !bytes
            .get(open)
            .is_some_and(|byte| matches!(byte, b'(' | b'[' | b'{'))
        {
            return Ok(None);
        }
        let mut at = open;
        let mut depth = 0i32;
        while at < bytes.len() {
            self.step(1)?;
            let byte = bytes[at];
            if matches!(byte, b'\'' | b'"' | b'`') {
                at = self.string_end(source, at)?;
                continue;
            }
            if byte == b'(' || any && matches!(byte, b'[' | b'{') {
                depth += 1
            } else if byte == b')' || any && matches!(byte, b']' | b'}') {
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(at));
                }
            }
            at += 1;
        }
        Ok(None)
    }
    pub fn split<'a>(&self, source: &'a str, objects: bool) -> Result<Vec<&'a str>> {
        let bytes = source.as_bytes();
        let mut at = 0;
        let mut start = 0;
        let mut depth = 0i32;
        let mut object_start = None;
        let mut out = Vec::new();
        while at < bytes.len() {
            self.step(1)?;
            let byte = bytes[at];
            if matches!(byte, b'\'' | b'"' | b'`') {
                at = self.string_end(source, at)?;
                continue;
            }
            if objects && depth == 0 && byte == b'{' {
                object_start = Some(at);
            }
            if matches!(byte, b'(' | b'[' | b'{') {
                depth += 1
            } else if matches!(byte, b')' | b']' | b'}') {
                depth -= 1;
                if objects
                    && depth == 0
                    && byte == b'}'
                    && let Some(begin) = object_start.take()
                {
                    out.push(&source[begin + 1..at]);
                }
            } else if !objects && depth == 0 && byte == b',' {
                out.push(&source[start..at]);
                start = at + 1;
            }
            at += 1;
        }
        if !objects {
            out.push(&source[start..]);
            out.retain(|text| !trim(text).is_empty());
        }
        Ok(out)
    }
    pub fn decorators<'a>(&self, source: &'a str, names: &str) -> Result<Vec<Hit<'a>>> {
        let pattern = regex(&format!(r"@({names})\s*\("))?;
        let mut end = 0;
        let mut hits = Vec::new();
        for found in pattern.captures_iter(source) {
            let found = found?;
            let full = found.get(0).unwrap();
            if full.start() < end {
                continue;
            }
            let Some(close) = self.close(source, full.end() - 1, false)? else {
                continue;
            };
            end = close + 1;
            hits.push(Hit {
                name: found[1].into(),
                args: &source[full.end()..close],
                start: full.start(),
                end,
            });
        }
        Ok(hits)
    }
    pub fn name_after(&self, source: &str, start: usize, class: bool) -> Result<Option<String>> {
        let mut at = start;
        loop {
            at += source[at..].len()
                - source[at..]
                    .trim_start_matches(crate::search::query_whitespace)
                    .len();
            if source.as_bytes().get(at) != Some(&b'@') {
                break;
            }
            let Some(name) = regex(r"^@[\w.]+")?.find(&source[at..])? else {
                break;
            };
            at += name.end();
            at += source[at..].len()
                - source[at..]
                    .trim_start_matches(crate::search::query_whitespace)
                    .len();
            if source.as_bytes().get(at) == Some(&b'(') {
                let Some(end) = self.close(source, at, false)? else {
                    return Ok(None);
                };
                at = end + 1;
            }
        }
        let pattern = if class {
            r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)"
        } else {
            r"^\s*(?:(?:public|private|protected|async|static)\b\s*)*([A-Za-z_$][\w$]*)\s*\("
        };
        Ok(regex(pattern)?
            .captures(&source[at..])?
            .map(|found| found[1].into()))
    }
    pub fn scopes(&self, source: &str) -> Result<Vec<Scope>> {
        let mut scopes = Vec::new();
        for (name, kind) in [
            ("Controller", "controller"),
            ("Resolver", "resolver"),
            ("WebSocketGateway", "gateway"),
            ("Injectable", "other"),
            ("Module", "other"),
            ("Catch", "other"),
        ] {
            for hit in self.decorators(source, name)? {
                let prefix = if kind == "controller" {
                    let found = regex(r#"path\s*:\s*['"`]([^'"`]*)['"`]"#)?.captures(hit.args)?;
                    if let Some(found) = found {
                        found[1].into()
                    } else {
                        string_arg(hit.args)?
                    }
                } else if kind == "gateway" {
                    string(r#"namespace\s*:\s*['"`]([^'"`]*)['"`]"#, hit.args)?
                } else {
                    String::new()
                };
                scopes.push(Scope {
                    kind,
                    prefix,
                    start: hit.start,
                    end: source.len(),
                });
            }
        }
        scopes.sort_by_key(|scope| scope.start);
        for i in 1..scopes.len() {
            scopes[i - 1].end = scopes[i].start;
        }
        Ok(scopes)
    }
    pub fn array<'a>(&self, source: &'a str, name: &str) -> Result<Option<&'a str>> {
        let Some(found) =
            regex(&format!(r"(?:^|[,{{\s]){}\s*:\s*\[", regex::escape(name)))?.find(source)?
        else {
            return Ok(None);
        };
        Ok(self
            .close(source, found.end() - 1, true)?
            .map(|end| &source[found.end()..end]))
    }
}
