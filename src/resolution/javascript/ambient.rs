//! Declaration-module block positions from the reference token scanner.
use super::*;
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Block {
    pub specifier: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    String,
    Template,
    Punct,
}
fn word_start(ch: u16) -> bool {
    (65..=90).contains(&ch) || (97..=122).contains(&ch) || ch == 95 || ch == 36 || ch > 127
}
fn word(ch: u16) -> bool {
    word_start(ch) || (48..=57).contains(&ch)
}
struct Scanner {
    source: Vec<u16>,
    at: usize,
    line: u32,
    line_start: usize,
    depth: usize,
    templates: Vec<usize>,
}
impl Scanner {
    fn ch(&self, at: usize) -> u16 {
        self.source.get(at).copied().unwrap_or(0)
    }
    fn newline(&mut self, at: usize) {
        self.line += 1;
        self.line_start = at + 1;
    }
    fn template(&mut self) {
        while self.at < self.source.len() {
            match self.ch(self.at) {
                92 => {
                    if self.ch(self.at + 1) == 10 {
                        self.newline(self.at + 1);
                    }
                    self.at += 2;
                }
                96 => {
                    self.at += 1;
                    return;
                }
                36 if self.ch(self.at + 1) == 123 => {
                    self.templates.push(self.depth);
                    self.at += 2;
                    return;
                }
                10 => {
                    self.newline(self.at);
                    self.at += 1;
                }
                _ => self.at += 1,
            }
        }
    }
    fn tokens(mut self, mut visit: impl FnMut(Kind, &str, u32, u32, usize)) {
        let mut regex_allowed = true;
        while self.at < self.source.len() {
            let ch = self.ch(self.at);
            if ch == 10 {
                self.newline(self.at);
                self.at += 1;
                continue;
            }
            if matches!(ch, 32 | 9 | 13 | 0xfeff | 0xa0) {
                self.at += 1;
                continue;
            }
            let column = (self.at - self.line_start) as u32;
            if ch == 47 {
                match self.ch(self.at + 1) {
                    47 => {
                        while self.at < self.source.len() && self.ch(self.at) != 10 {
                            self.at += 1;
                        }
                        continue;
                    }
                    42 => {
                        self.at += 2;
                        while self.at < self.source.len()
                            && !(self.ch(self.at) == 42 && self.ch(self.at + 1) == 47)
                        {
                            if self.ch(self.at) == 10 {
                                self.newline(self.at);
                            }
                            self.at += 1;
                        }
                        self.at += 2;
                        continue;
                    }
                    _ => {}
                }
                if regex_allowed {
                    self.at += 1;
                    let mut class = false;
                    while self.at < self.source.len() {
                        match self.ch(self.at) {
                            10 => break,
                            92 => self.at += 1,
                            91 => class = true,
                            93 => class = false,
                            47 if !class => {
                                self.at += 1;
                                break;
                            }
                            _ => {}
                        }
                        self.at += 1;
                    }
                    while self.at < self.source.len() && word(self.ch(self.at)) {
                        self.at += 1;
                    }
                    regex_allowed = false;
                    visit(Kind::Template, "", self.line, column, self.depth);
                    continue;
                }
            }
            if matches!(ch, 39 | 34) {
                let mut end = self.at + 1;
                let mut text = Vec::new();
                let mut from = end;
                while end < self.source.len() {
                    let c = self.ch(end);
                    if c == ch || c == 10 {
                        break;
                    }
                    if c == 92 {
                        text.extend_from_slice(&self.source[from..end]);
                        end += 2;
                        from = end;
                        continue;
                    }
                    end += 1;
                }
                if from <= self.source.len() {
                    text.extend_from_slice(&self.source[from..end.min(self.source.len())]);
                }
                self.at = if end < self.source.len() && self.ch(end) == ch {
                    end + 1
                } else {
                    end
                };
                regex_allowed = false;
                visit(
                    Kind::String,
                    &String::from_utf16_lossy(&text),
                    self.line,
                    column,
                    self.depth,
                );
                continue;
            }
            if ch == 96 {
                self.at += 1;
                self.template();
                regex_allowed = false;
                visit(Kind::Template, "", self.line, column, self.depth);
                continue;
            }
            if ch == 123 {
                visit(Kind::Punct, "{", self.line, column, self.depth);
                self.depth += 1;
                self.at += 1;
                regex_allowed = true;
                continue;
            }
            if ch == 125 {
                self.at += 1;
                if self.templates.last() == Some(&self.depth) {
                    self.templates.pop();
                    self.template();
                    regex_allowed = false;
                    continue;
                }
                self.depth = self.depth.saturating_sub(1);
                visit(Kind::Punct, "}", self.line, column, self.depth);
                regex_allowed = true;
                continue;
            }
            if word_start(ch) {
                let start = self.at;
                self.at += 1;
                while self.at < self.source.len() && word(self.ch(self.at)) {
                    self.at += 1;
                }
                let text = String::from_utf16_lossy(&self.source[start..self.at]);
                regex_allowed = matches!(
                    text.as_str(),
                    "return"
                        | "typeof"
                        | "case"
                        | "do"
                        | "else"
                        | "in"
                        | "of"
                        | "instanceof"
                        | "new"
                        | "delete"
                        | "void"
                        | "throw"
                        | "yield"
                        | "await"
                );
                visit(Kind::Word, &text, self.line, column, self.depth);
                continue;
            }
            if (48..=57).contains(&ch) {
                while self.at < self.source.len()
                    && (word(self.ch(self.at)) || self.ch(self.at) == 46)
                {
                    self.at += 1;
                }
                regex_allowed = false;
                continue;
            }
            self.at += 1;
            regex_allowed = !matches!(ch, 41 | 93);
            visit(
                Kind::Punct,
                &String::from_utf16_lossy(&[ch]),
                self.line,
                column,
                self.depth,
            );
        }
    }
}
pub(super) fn scan(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut state = 0;
    let mut start = (0, 0);
    let mut name = String::new();
    let mut open = None::<Block>;
    Scanner {
        source: source.encode_utf16().collect(),
        at: 0,
        line: 1,
        line_start: 0,
        depth: 0,
        templates: Vec::new(),
    }
    .tokens(|kind, text, line, column, depth| {
        if let Some(block) = &mut open {
            if kind == Kind::Punct && text == "}" && depth == 0 {
                block.end_line = line;
                block.end_column = column;
                blocks.push(open.take().unwrap());
            }
            return;
        }
        if depth != 0 {
            state = 0;
            return;
        }
        if kind == Kind::Word {
            if text == "declare" {
                state = 1;
                start = (line, column);
            } else {
                state = if state == 1 && text == "module" { 2 } else { 0 };
            }
            return;
        }
        if kind == Kind::String && state == 2 {
            state = 3;
            name = text.to_string();
            return;
        }
        if kind == Kind::Punct && text == "{" && state == 3 && !name.is_empty() {
            open = Some(Block {
                specifier: name.clone(),
                start_line: start.0,
                start_column: start.1,
                end_line: 0,
                end_column: 0,
            });
        }
        state = 0;
    });
    blocks
}

pub(super) fn script(source: &str) -> bool {
    let mut is_script = true;
    let mut import = false;
    Scanner {
        source: source.encode_utf16().collect(),
        at: 0,
        line: 1,
        line_start: 0,
        depth: 0,
        templates: Vec::new(),
    }
    .tokens(|kind, text, _, _, depth| {
        if depth != 0 {
            return;
        }
        if import {
            if !matches!(text, "(" | ".") {
                is_script = false;
            }
            import = false;
        }
        if kind == Kind::Word {
            if text == "export" {
                is_script = false;
            }
            import = text == "import";
        }
    });
    is_script
}

pub(super) fn exports(source: &str, block: &Block) -> HashMap<String, String> {
    let mut exports = HashMap::new();
    let mut state = 0;
    let mut item = Vec::new();
    Scanner {
        source: source.encode_utf16().collect(),
        at: 0,
        line: 1,
        line_start: 0,
        depth: 0,
        templates: Vec::new(),
    }
    .tokens(|kind, text, line, column, depth| {
        if (line, column) < (block.start_line, block.start_column)
            || (line, column) > (block.end_line, block.end_column)
        {
            return;
        }
        if state == 2 && (depth == 2 || text == "}" && depth == 1) {
            if text == "," || text == "}" {
                let words: Vec<_> = item.iter().map(String::as_str).collect();
                let words = if words.first() == Some(&"type") {
                    &words[1..]
                } else {
                    &words[..]
                };
                match words {
                    [local] => {
                        exports.insert((*local).to_owned(), (*local).to_owned());
                    }
                    [local, "as", exported] => {
                        exports.insert((*exported).to_owned(), (*local).to_owned());
                    }
                    _ => {}
                }
                item.clear();
                if text == "}" {
                    state = 0;
                }
            } else if matches!(kind, Kind::Word | Kind::String) {
                item.push(text.to_string());
            }
            return;
        }
        if depth != 1 {
            return;
        }
        if state == 3 && kind == Kind::Word {
            if !matches!(text, "abstract" | "class" | "function" | "async") {
                exports.insert("default".into(), text.into());
                state = 0;
            }
            return;
        }
        if state == 1 {
            match text {
                "{" => {
                    state = 2;
                    return;
                }
                "default" | "=" => {
                    state = 3;
                    return;
                }
                "type" => return,
                _ => state = 0,
            }
        }
        if kind == Kind::Word && text == "export" {
            state = 1;
        }
    });
    exports
}
impl Context<'_> {
    pub(super) fn declaring_module(&self, node: &Node) -> Result<Option<String>> {
        let path = node.file_path.as_str();
        if !self.ambient.borrow().contains_key(path) {
            let source = self.source(node);
            self.steps(source.len() / 64 + 1)?;
            self.ambient.borrow_mut().insert(
                node.file_path.clone(),
                if test(r#"\bdeclare\s+module\s*['"]"#, source)? && script(source) {
                    scan(source)
                } else {
                    Vec::new()
                },
            );
        }
        Ok(self
            .ambient
            .borrow()
            .get(path)
            .into_iter()
            .flatten()
            .find(|block| {
                (node.start_line, node.start_column) >= (block.start_line, block.start_column)
                    && (node.start_line, node.start_column) <= (block.end_line, block.end_column)
            })
            .map(|block| block.specifier.clone()))
    }
}
