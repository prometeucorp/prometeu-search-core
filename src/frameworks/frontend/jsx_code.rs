//! JSX-aware literal masking. The scanner retains the reference's UTF-16 decisions
//! and preserves the reference UTF-16 widths in masked string contents.
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Code,
    Template,
    Tag,
    Children,
}
struct Frame {
    mode: Mode,
    braces: usize,
    name: Vec<u16>,
}
struct Root {
    at: usize,
    depth: usize,
    blanks: usize,
}
struct Lexer {
    source: Vec<u16>,
    blanks: Vec<(usize, usize)>,
    stack: Vec<Frame>,
    roots: Vec<Root>,
    operators: HashSet<usize>,
    i: usize,
    expression_end: bool,
    after_dot: bool,
    rereads: usize,
}

pub(crate) fn code(source: &str) -> Option<String> {
    let mut lexer = Lexer {
        source: source.encode_utf16().collect(),
        blanks: Vec::new(),
        stack: vec![Frame {
            mode: Mode::Code,
            braces: 0,
            name: Vec::new(),
        }],
        roots: Vec::new(),
        operators: HashSet::new(),
        i: 0,
        expression_end: false,
        after_dot: false,
        rereads: 0,
    };
    lexer.read()?;
    let mut blank = vec![false; lexer.source.len()];
    let mut from = 0;
    for (start, end) in lexer.blanks {
        if start < from {
            continue;
        }
        blank[start..end].fill(true);
        from = end;
    }
    let mut output = String::with_capacity(source.len());
    let mut at = 0;
    for ch in source.chars() {
        let end = at + ch.len_utf16();
        if !matches!(ch, '\r' | '\n') && blank[at..end].iter().any(|&b| b) {
            for _ in 0..ch.len_utf16() {
                output.push(' ');
            }
        } else {
            output.push(ch);
        }
        at = end;
    }
    Some(output)
}
pub(crate) fn space(c: u32) -> bool {
    matches!(c, 32 | 10 | 13 | 9 | 11 | 12 | 0xa0 | 0xfeff)
}
fn id_start(c: u32) -> bool {
    matches!(c,97..=122|65..=90|36|95) || (c > 127 && c <= 65535 && !space(c))
}
fn id_part(c: u32) -> bool {
    id_start(c) || matches!(c, 48..=57)
}
fn name_part(c: u32) -> bool {
    id_part(c) || matches!(c, 45 | 46 | 58)
}

impl Lexer {
    fn c(&self, at: usize) -> u32 {
        self.source.get(at).map_or(u32::MAX, |&c| c as u32)
    }
    fn starts(&self, at: usize, text: &str) -> bool {
        self.source.get(at..).is_some_and(|source| {
            source
                .iter()
                .copied()
                .take(text.len())
                .eq(text.bytes().map(u16::from))
        })
    }
    fn space_end(&self, mut at: usize) -> usize {
        while at < self.source.len() && space(self.c(at)) {
            at += 1;
        }
        at
    }
    fn name_end(&self, mut at: usize) -> usize {
        while at < self.source.len() && name_part(self.c(at)) {
            at += 1;
        }
        at
    }
    fn line_end(&self, mut at: usize) -> usize {
        while at < self.source.len() && !matches!(self.c(at), 10 | 13) {
            at += 1;
        }
        at
    }
    fn regex_end(&self, start: usize) -> Option<usize> {
        let mut class = false;
        let mut i = start + 1;
        while i < self.source.len() {
            match self.c(i) {
                10 | 13 => return None,
                92 => {
                    i += 1;
                    if matches!(self.c(i), 10 | 13) {
                        return None;
                    }
                }
                91 => class = true,
                93 => class = false,
                47 if !class => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }
    fn string_end(&self, start: usize) -> Option<usize> {
        let quote = self.c(start);
        let mut i = start + 1;
        while i < self.source.len() {
            let c = self.c(i);
            if c == quote {
                return Some(i);
            }
            if c == 92 {
                i += if self.c(i + 1) == 13 && self.c(i + 2) == 10 {
                    2
                } else {
                    1
                };
            } else if matches!(c, 10 | 13) {
                return None;
            }
            i += 1;
        }
        None
    }
    fn find(&self, start: usize, text: &str) -> Option<usize> {
        (start..self.source.len()).find(|&at| self.starts(at, text))
    }
    fn read(&mut self) -> Option<()> {
        let budget = 4 * self.source.len() + 65536;
        let mut steps = 0;
        loop {
            while self.i < self.source.len() {
                steps += 1;
                if steps > budget {
                    return None;
                }
                let c = self.c(self.i);
                let valid = match self.stack.last()?.mode {
                    Mode::Code => self.read_code(c),
                    Mode::Template => self.template(c),
                    Mode::Tag => self.tag(c),
                    Mode::Children => self.children(c),
                };
                if !valid {
                    return None;
                }
            }
            if self.stack.len() == 1 && self.stack[0].braces == 0 {
                break;
            }
            if !self.reread() {
                return None;
            }
        }
        Some(())
    }
    fn blank(&mut self, from: usize, to: usize) {
        if to > from {
            self.blanks.push((from, to.min(self.source.len())));
        }
    }
    fn code_frame(&mut self) {
        self.stack.push(Frame {
            mode: Mode::Code,
            braces: 0,
            name: Vec::new(),
        });
        self.expression_end = false;
        self.after_dot = false;
    }
    fn reread(&mut self) -> bool {
        let Some(root) = self.roots.pop() else {
            return false;
        };
        self.rereads += 1;
        if self.rereads > 64 {
            return false;
        }
        self.stack.truncate(root.depth);
        self.blanks.truncate(root.blanks);
        self.operators.retain(|&k| k <= root.at);
        self.operators.insert(root.at);
        self.i = root.at;
        self.expression_end = false;
        self.after_dot = false;
        true
    }
    fn open_element(&mut self, lt: usize) -> bool {
        if self.c(lt + 1) == 62 {
            self.stack.push(Frame {
                mode: Mode::Children,
                braces: 0,
                name: Vec::new(),
            });
            self.i = lt + 2;
            return true;
        }
        let mut k = self.name_end(lt + 1);
        let name = self.source[lt + 1..k].to_vec();
        let next = self.space_end(k);
        if next > k && self.starts(next, "extends") && space(self.c(next + 7)) {
            return false;
        }
        if self.c(k) == 60 {
            let mut depth = 0;
            let limit = self.source.len().min(k + 1000);
            while k < limit {
                if self.c(k) == 60 {
                    depth += 1;
                } else if self.c(k) == 62 {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                k += 1;
            }
            if depth != 0 {
                return false;
            }
            k += 1;
        }
        self.stack.push(Frame {
            mode: Mode::Tag,
            braces: 0,
            name,
        });
        self.i = k;
        true
    }
    fn close_element(&mut self) {
        self.stack.pop();
        if self.stack.last().is_some_and(|top| top.mode == Mode::Code) {
            self.roots.pop();
            self.expression_end = true;
            self.after_dot = false;
        }
    }
    fn read_code(&mut self, c: u32) -> bool {
        let i = self.i;
        if space(c) {
            self.i = i + 1;
            return true;
        }
        if c == 47 {
            let d = self.c(i + 1);
            if d == 47 {
                let end = self.line_end(i);
                self.blank(i, end);
                self.i = end;
                return true;
            }
            if d == 42 {
                let Some(end) = self.find(i + 2, "*/") else {
                    return self.reread();
                };
                self.blank(i, end + 2);
                self.i = end + 2;
                return true;
            }
            if self.expression_end {
                if d == 62 {
                    return self.reread();
                }
                self.i = i + 1;
                self.expression_end = false;
                self.after_dot = false;
                return true;
            }
            let Some(end) = self.regex_end(i) else {
                self.i = i + 1;
                return true;
            };
            self.blank(i + 1, end);
            let mut k = end + 1;
            while k < self.source.len() && id_part(self.c(k)) {
                k += 1;
            }
            self.i = k;
            self.expression_end = true;
            self.after_dot = false;
            return true;
        }
        if matches!(c, 34 | 39) {
            let Some(end) = self.string_end(i) else {
                return self.reread();
            };
            self.blank(i + 1, end);
            self.i = end + 1;
            self.expression_end = true;
            self.after_dot = false;
            return true;
        }
        if c == 96 {
            self.stack.push(Frame {
                mode: Mode::Template,
                braces: 0,
                name: Vec::new(),
            });
            self.i = i + 1;
            return true;
        }
        if c == 60 {
            let d = self.c(i + 1);
            if !self.expression_end && (d == 62 || id_start(d)) && !self.operators.contains(&i) {
                self.roots.push(Root {
                    at: i,
                    depth: self.stack.len(),
                    blanks: self.blanks.len(),
                });
                return self.open_element(i) || self.reread();
            }
            if d == 47 {
                return self.reread();
            }
            self.i = i + if matches!(d, 60 | 61) { 2 } else { 1 };
            self.expression_end = false;
            self.after_dot = false;
            return true;
        }
        if c == 123 {
            self.stack.last_mut().unwrap().braces += 1;
            self.i = i + 1;
            self.expression_end = false;
            self.after_dot = false;
            return true;
        }
        if c == 125 {
            if self.stack.last().unwrap().braces > 0 {
                self.stack.last_mut().unwrap().braces -= 1;
                self.i = i + 1;
                self.expression_end = false;
                self.after_dot = false;
                return true;
            }
            if self.stack.len() == 1 {
                return self.reread();
            }
            self.stack.pop();
            self.i = i + 1;
            return true;
        }
        if id_start(c) {
            let mut end = i + 1;
            while end < self.source.len() && id_part(self.c(end)) {
                end += 1;
            }
            let word = String::from_utf16_lossy(&self.source[i..end]);
            self.expression_end = self.after_dot
                || !matches!(
                    word.as_str(),
                    "return"
                        | "typeof"
                        | "instanceof"
                        | "in"
                        | "of"
                        | "new"
                        | "delete"
                        | "void"
                        | "throw"
                        | "case"
                        | "do"
                        | "else"
                        | "yield"
                        | "await"
                        | "default"
                );
            self.after_dot = false;
            self.i = end;
            return true;
        }
        let d = self.c(i + 1);
        if matches!(c, 48..=57) || (c == 46 && matches!(d, 48..=57)) {
            let mut end = i + 1;
            while end < self.source.len() && (id_part(self.c(end)) || self.c(end) == 46) {
                end += 1;
            }
            self.i = end;
            self.expression_end = true;
            self.after_dot = false;
            return true;
        }
        if c == 46 {
            if d == 46 && self.c(i + 2) == 46 {
                self.i = i + 3;
                self.expression_end = false;
                self.after_dot = false;
            } else {
                self.i = i + 1;
                self.expression_end = false;
                self.after_dot = true;
            }
            return true;
        }
        if matches!(c, 41 | 93) {
            self.i = i + 1;
            self.expression_end = true;
            self.after_dot = false;
            return true;
        }
        if matches!(c, 43 | 45) && d == c {
            self.i = i + 2;
            return true;
        }
        if c == 33 && self.expression_end && d != 61 {
            self.i = i + 1;
            return true;
        }
        if c == 35 && i == 0 && d == 33 {
            self.i = self.line_end(0);
            return true;
        }
        self.i = i + 1;
        self.expression_end = false;
        self.after_dot = false;
        true
    }
    fn template(&mut self, c: u32) -> bool {
        let i = self.i;
        if c == 96 {
            self.stack.pop();
            self.i = i + 1;
            self.expression_end = true;
            self.after_dot = false;
            return true;
        }
        if c == 92 {
            self.blank(i, i + 2);
            self.i = i + 2;
            return true;
        }
        if c == 36 && self.c(i + 1) == 123 {
            self.code_frame();
            self.i = i + 2;
            return true;
        }
        let mut end = i + 1;
        while end < self.source.len() && !matches!(self.c(end), 96 | 92 | 36) {
            end += 1;
        }
        self.blank(i, end);
        self.i = end;
        true
    }
    fn tag(&mut self, c: u32) -> bool {
        let i = self.i;
        if space(c) {
            self.i = i + 1;
            return true;
        }
        if c == 62 {
            self.stack.last_mut().unwrap().mode = Mode::Children;
            self.i = i + 1;
            return true;
        }
        if c == 47 {
            let d = self.c(i + 1);
            if d == 62 {
                self.i = i + 2;
                self.close_element();
                return true;
            }
            if d == 47 {
                let end = self.line_end(i);
                self.blank(i, end);
                self.i = end;
                return true;
            }
            if d == 42
                && let Some(end) = self.find(i + 2, "*/")
            {
                self.blank(i, end + 2);
                self.i = end + 2;
                return true;
            }
            return self.reread();
        }
        if c == 123 {
            self.code_frame();
            self.i = i + 1;
            return true;
        }
        if id_start(c) {
            let end = self.name_end(i + 1);
            let eq = self.space_end(end);
            if self.c(eq) != 61 {
                self.i = end;
                return true;
            }
            let value = self.space_end(eq + 1);
            let q = self.c(value);
            if matches!(q, 34 | 39) {
                let Some(close) = (value + 1..self.source.len()).find(|&at| self.c(at) == q) else {
                    return self.reread();
                };
                self.blank(value + 1, close);
                self.i = close + 1;
                return true;
            }
            if q == 123 {
                self.code_frame();
                self.i = value + 1;
                return true;
            }
            let next = self.c(value + 1);
            if q == 60 && (next == 62 || id_start(next)) && self.open_element(value) {
                return true;
            }
        }
        self.reread()
    }
    fn children(&mut self, c: u32) -> bool {
        let i = self.i;
        if c == 123 {
            self.code_frame();
            self.i = i + 1;
            return true;
        }
        if c == 60 {
            let d = self.c(i + 1);
            if d == 47 {
                let start = self.space_end(i + 2);
                let end = self.name_end(start);
                let gt = self.space_end(end);
                if self.c(gt) != 62 || self.source[start..end] != self.stack.last().unwrap().name {
                    return self.reread();
                }
                self.i = gt + 1;
                self.close_element();
                return true;
            }
            if (d == 62 || id_start(d)) && self.open_element(i) {
                return true;
            }
            return self.reread();
        }
        if matches!(c, 62 | 125) {
            return self.reread();
        }
        let mut end = i + 1;
        while end < self.source.len() && !matches!(self.c(end), 123 | 60 | 62 | 125) {
            end += 1;
        }
        self.i = end;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jsx_mask_matches_original_scanner() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/frontend/jsx-code.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let actual = code(case["source"].as_str().unwrap());
            assert_eq!(serde_json::json!(actual), case["expected"], "{case}");
        }
    }
}
