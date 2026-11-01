//! JSX destructuring and parameter binding, preserving UTF-16 scanner bounds.
use super::super::regex;
use anyhow::Result;

pub(super) struct Bindings {
    code: Vec<u16>,
}
impl Bindings {
    pub(super) fn new(source: &str) -> Self {
        Self {
            code: source.encode_utf16().collect(),
        }
    }
    fn ch(&self, at: isize) -> u16 {
        usize::try_from(at)
            .ok()
            .and_then(|i| self.code.get(i))
            .copied()
            .unwrap_or(0)
    }
    fn space(&self, mut at: isize, step: isize) -> isize {
        while at >= 0
            && (at as usize) < self.code.len()
            && char::from_u32(u32::from(self.ch(at))).is_some_and(crate::search::query_whitespace)
        {
            at += step;
        }
        at
    }
    fn word(&self, end: isize) -> String {
        let mut start = end;
        while start > 0 && id(self.ch(start - 1)) {
            start -= 1;
        }
        String::from_utf16_lossy(&self.code[start.max(0) as usize..(end + 1).max(0) as usize])
    }
    fn enclosing(&self, from: isize) -> isize {
        let mut depth = 0;
        for i in ((from - 10_000).max(0)..=from).rev() {
            let c = self.ch(i);
            if closing(c) {
                depth += 1;
            } else if opening(c) {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
        }
        -1
    }
    fn matching(&self, open: isize) -> isize {
        let mut depth = 0;
        for i in open..(self.code.len() as isize).min(open + 10_000) {
            let c = self.ch(i);
            if opening(c) {
                depth += 1;
            } else if closing(c) {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
        }
        -1
    }
    fn parameters(&self, paren: isize) -> Result<bool> {
        let p = self.space(paren - 1, -1);
        if id(self.ch(p))
            && matches!(
                self.word(p).as_str(),
                "if" | "while" | "for" | "switch" | "with"
            )
        {
            return Ok(false);
        }
        let close = self.matching(paren);
        if close < 0 {
            return Ok(false);
        }
        regex(r"^\s*(?::[^=;{}()\n]*)?(?:=>|\{)")?
            .is_match(&String::from_utf16_lossy(
                &self.code[(close + 1) as usize..],
            ))
            .map_err(Into::into)
    }
    fn key(&self, colon: isize) -> bool {
        let mut k = self.space(colon - 1, -1);
        let c = self.ch(k);
        if c == u16::from(b']') {
            k = self.enclosing(k - 1) - 1;
        } else if matches!(c, 34 | 39) {
            k = (0..k).rev().find(|&i| self.ch(i) == c).unwrap_or(-1) - 1;
        } else if id(c) {
            while k >= 0 && id(self.ch(k)) {
                k -= 1;
            }
        } else {
            return false;
        }
        matches!(self.ch(self.space(k, -1)), 123 | 44)
    }
    fn pattern(&self, opener: isize, depth: usize) -> Result<bool> {
        if depth > 8 {
            return Ok(false);
        }
        let p = self.space(opener - 1, -1);
        let c = self.ch(p);
        if c == 40 {
            return self.parameters(p);
        }
        if c == 91 {
            return self.pattern(p, depth + 1);
        }
        if c == 44 {
            let q = self.enclosing(p - 1);
            if self.ch(q) == 40 {
                return self.parameters(q);
            }
            return Ok(self.ch(q) == 91 && self.pattern(q, depth + 1)?);
        }
        if c == 58 {
            if !self.key(p) {
                return Ok(false);
            }
            let q = self.enclosing(p - 1);
            return Ok(self.ch(q) == 123 && self.pattern(q, depth + 1)?);
        }
        if !id(c) || !matches!(self.word(p).as_str(), "const" | "let" | "var") {
            return Ok(false);
        }
        let close = self.matching(opener);
        if close < 0 {
            return Ok(false);
        }
        Ok(
            !regex(r"^\s*(?::[^=;]*)?=\s*(?:await\s+)?(?:require|import)\s*\(")?.is_match(
                &String::from_utf16_lossy(&self.code[(close + 1) as usize..]),
            )?,
        )
    }
    fn element(&self, at: isize, length: usize) -> Result<bool> {
        if id(self.ch(at - 1)) || id(self.ch(at + length as isize)) {
            return Ok(false);
        }
        let next = self.space(at + length as isize, 1);
        let after = self.ch(next);
        if !matches!(after, 44 | 125 | 93) && (after != 61 || matches!(self.ch(next + 1), 61 | 62))
        {
            return Ok(false);
        }
        let mut prev = self.space(at - 1, -1);
        if self.ch(prev) == 46 {
            if self.ch(prev - 1) != 46 || self.ch(prev - 2) != 46 {
                return Ok(false);
            }
            prev = self.space(prev - 3, -1);
        }
        let mut opener = prev;
        if self.ch(prev) == 44 {
            opener = self.enclosing(prev - 1);
        } else if self.ch(prev) == 58 {
            if !self.key(prev) {
                return Ok(false);
            }
            opener = self.enclosing(prev - 1);
            if self.ch(opener) != 123 {
                return Ok(false);
            }
        }
        Ok(matches!(self.ch(opener), 123 | 91) && self.pattern(opener, 0)?)
    }
    pub(super) fn destructures(&self, name: &str, end: usize) -> Result<bool> {
        let name = name.encode_utf16().collect::<Vec<_>>();
        if name.is_empty() {
            return Ok(false);
        }
        for (at, part) in self.code.windows(name.len()).enumerate() {
            if at >= end {
                break;
            }
            if part == name && self.element(at as isize, name.len())? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(super) fn top_patterns(&self) -> Result<Vec<(usize, usize)>> {
        let source = String::from_utf16_lossy(&self.code);
        let mut patterns = Vec::new();
        let mut depth = 0;
        let mut at = 0;
        for found in regex(r"\b(?:const|let|var)\s*[{[]")?.find_iter(&source) {
            let found = found?;
            let index = source[..found.start()].encode_utf16().count();
            for i in at..index {
                if self.code[i] == 123 {
                    depth += 1;
                } else if self.code[i] == 125 {
                    depth -= 1;
                }
            }
            at = index;
            if depth != 0 {
                continue;
            }
            let open = source[..found.end()].encode_utf16().count() - 1;
            let close = self.matching(open as isize);
            if close > open as isize {
                patterns.push((open, close as usize));
            }
        }
        Ok(patterns)
    }
    pub(super) fn patterns_bind(&self, patterns: &[(usize, usize)], name: &str) -> Result<bool> {
        let name = name.encode_utf16().collect::<Vec<_>>();
        if name.is_empty() {
            return Ok(false);
        }
        for &(open, close) in patterns {
            for (index, part) in self.code[open..].windows(name.len()).enumerate() {
                let at = open + index;
                if at >= close {
                    break;
                }
                if part == name && self.element(at as isize, name.len())? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
fn id(c: u16) -> bool {
    u8::try_from(c).is_ok_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$'))
}
fn opening(c: u16) -> bool {
    matches!(c, 40 | 91 | 123)
}
fn closing(c: u16) -> bool {
    matches!(c, 41 | 93 | 125)
}

pub(super) fn code_binds(source: &str, name: &str) -> Result<bool> {
    let n = regex::escape(name);
    if regex(&format!(
        r"\b(?:const|let|var)\s+{n}(?![\w$])(?!\s*[,\]}}])"
    ))?
    .is_match(source)?
    {
        return Ok(true);
    }
    let paren = r"\((?:[^()]|\([^()]*\))*\)";
    let brace = r"\{(?:[^{}]|\{[^{}]*\})*\}";
    let bracket = r"\[(?:[^\[\]]|\[[^\[\]]*\])*\]";
    let angle = r"<(?!\s)(?:=>|=(?!>)|[^<>=]|<(?:=>|=(?!>)|[^<>=])*>)*>";
    let part = format!(
        r"(?:=>|=(?!>)|[^,(){{}}\[\]<>=]|<(?=\s)|>(?=[\s=])|{paren}|{brace}|{bracket}|{angle})"
    );
    let modifiers = r"(?:(?:public|private|protected|readonly|override)\s+)*";
    let return_type = format!(r"(?::(?:[^=;{{}}()\n]|{brace})*)?");
    let parameters = regex(&format!(
        r"\(\s*(?:(?:{modifiers}(?:\.\.\.)?[\w$]+|{brace}|{bracket})(?:\s*(?:\?\s*)?:{part}+|\s*={part}+|\s*),\s*)*{modifiers}{n}(?![\w$])(?:\s*\??\s*:{part}*)?(?:\s*={part}*)?(?:\s*,(?:[^()]|{paren})*)?\)\s*{return_type}(?:=>|\{{)"
    ))?;
    let control = regex(r"\b(?:if|while|for|switch|with)\s*$")?;
    for found in parameters.find_iter(source) {
        let found = found?;
        if !control.is_match(&source[..found.start()])? {
            return Ok(true);
        }
    }
    Ok(false)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_bindings_match_independent_reference() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/frontend/jsx-scope.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let name = case["name"].as_str().unwrap();
            let binding = Bindings::new(source);
            let patterns = binding.top_patterns().unwrap();
            let code = crate::synthesis::comments::strip_for_regex(source, "javascript");
            assert_eq!(
                serde_json::json!({"source":source,"name":name,"patterns":patterns,"top":binding.patterns_bind(&patterns,name).unwrap(),"destructures":binding.destructures(name,source.encode_utf16().count()).unwrap(),"binds":code_binds(&code,name).unwrap()}),
                *case,
                "{source} {name}"
            );
        }
    }
}
