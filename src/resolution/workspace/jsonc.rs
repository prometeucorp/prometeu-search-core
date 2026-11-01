//! Tolerant manifest values following the reference JSONC scanner and parser.
//! SPDX-License-Identifier: MIT. Attribution: THIRD_PARTY_NOTICES.md.
use anyhow::{Result, ensure};
use serde_json::{Map, Value};

#[derive(Debug)]
enum Token {
    Punctuation(u16),
    Value(Value),
    End,
}
struct Parser {
    text: Vec<u16>,
    cursor: usize,
    token: Token,
}
impl Parser {
    fn peek(&self) -> Option<u16> {
        self.text.get(self.cursor).copied()
    }
    fn next(&mut self) -> Result<()> {
        self.token = loop {
            let Some(ch) = self.peek() else {
                break Token::End;
            };
            self.cursor += 1;
            match ch {
                9 | 10 | 13 | 32 => continue,
                123 | 125 | 91 | 93 | 58 | 44 => break Token::Punctuation(ch),
                34 => {
                    let string = self.string()?;
                    break Token::Value(Value::String(string));
                }
                47 => match self.peek() {
                    Some(47) => {
                        self.cursor += 1;
                        while self.peek().is_some_and(|ch| ch != 10 && ch != 13) {
                            self.cursor += 1;
                        }
                    }
                    Some(42) => {
                        self.cursor += 1;
                        while self.cursor < self.text.len() {
                            if self.peek() == Some(42)
                                && self.text.get(self.cursor + 1) == Some(&47)
                            {
                                self.cursor += 2;
                                break;
                            }
                            self.cursor += 1;
                        }
                    }
                    _ => {}
                },
                45 | 48..=57 => {
                    if ch == 45 && !self.peek().is_some_and(digit) {
                        continue;
                    }
                    let start = self.cursor - 1;
                    if ch == 45 {
                        self.cursor += 1;
                    }
                    if self.text[self.cursor - 1] != 48 {
                        while self.peek().is_some_and(digit) {
                            self.cursor += 1;
                        }
                    }
                    if self.peek() == Some(46) {
                        self.cursor += 1;
                        while self.peek().is_some_and(digit) {
                            self.cursor += 1;
                        }
                    }
                    let mut end = self.cursor;
                    if matches!(self.peek(), Some(69 | 101)) {
                        self.cursor += 1;
                        if matches!(self.peek(), Some(43 | 45)) {
                            self.cursor += 1;
                        }
                        if self.peek().is_some_and(digit) {
                            while self.peek().is_some_and(digit) {
                                self.cursor += 1;
                            }
                            end = self.cursor;
                        }
                    }
                    let text = String::from_utf16(&self.text[start..end])?;
                    let value = text.parse::<f64>().unwrap_or(0.0);
                    break Token::Value(
                        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number),
                    );
                }
                _ => {
                    let start = self.cursor - 1;
                    while self.peek().is_some_and(|ch| {
                        !matches!(
                            ch,
                            9 | 10 | 13 | 32 | 125 | 93 | 123 | 91 | 34 | 58 | 44 | 47
                        )
                    }) {
                        self.cursor += 1;
                    }
                    match &self.text[start..self.cursor] {
                        [116, 114, 117, 101] => break Token::Value(Value::Bool(true)),
                        [102, 97, 108, 115, 101] => break Token::Value(Value::Bool(false)),
                        [110, 117, 108, 108] => break Token::Value(Value::Null),
                        _ => {}
                    }
                }
            }
        };
        Ok(())
    }
    fn string(&mut self) -> Result<String> {
        let mut out = Vec::new();
        while let Some(ch) = self.peek() {
            if matches!(ch, 10 | 13) {
                break;
            }
            self.cursor += 1;
            if ch == 34 {
                break;
            }
            if ch != 92 {
                out.push(ch);
                continue;
            }
            let Some(escape) = self.peek() else {
                break;
            };
            self.cursor += 1;
            match escape {
                34 | 47 | 92 => out.push(escape),
                98 => out.push(8),
                102 => out.push(12),
                110 => out.push(10),
                114 => out.push(13),
                116 => out.push(9),
                117 => {
                    let mut value = 0;
                    let mut digits = 0;
                    while digits < 4 {
                        let Some(hex) = self
                            .peek()
                            .and_then(|ch| char::from_u32(ch as u32))
                            .and_then(|ch| ch.to_digit(16))
                        else {
                            break;
                        };
                        self.cursor += 1;
                        value = value * 16 + hex;
                        digits += 1;
                    }
                    if digits == 4 {
                        out.push(value as u16);
                    }
                }
                _ => {}
            }
        }
        Ok(String::from_utf16(&out)?)
    }
    fn punctuation(&self, ch: u16) -> bool {
        matches!(self.token,Token::Punctuation(value)if value==ch)
    }
    fn skip(&mut self, close: u16) -> Result<()> {
        while !matches!(self.token, Token::End) && !self.punctuation(close) && !self.punctuation(44)
        {
            self.next()?;
        }
        Ok(())
    }
    fn value(&mut self, depth: usize) -> Result<Option<Value>> {
        ensure!(
            depth <= 128,
            "workspace manifest nesting exceeds 128 levels"
        );
        match std::mem::replace(&mut self.token, Token::End) {
            Token::Value(value) => {
                self.next()?;
                Ok(Some(value))
            }
            Token::Punctuation(123) => {
                self.next()?;
                let mut object = Map::new();
                while !self.punctuation(125) && !matches!(self.token, Token::End) {
                    if self.punctuation(44) {
                        self.next()?;
                    }
                    if let Token::Value(Value::String(key)) = &self.token {
                        let key = key.clone();
                        self.next()?;
                        if self.punctuation(58) {
                            self.next()?;
                            if let Some(value) = self.value(depth + 1)? {
                                object.insert(key, value);
                            } else {
                                self.skip(125)?;
                            }
                        } else {
                            self.skip(125)?;
                        }
                    } else {
                        self.skip(125)?;
                    }
                }
                if self.punctuation(125) {
                    self.next()?;
                }
                Ok(Some(Value::Object(object)))
            }
            Token::Punctuation(91) => {
                self.next()?;
                let mut array = Vec::new();
                while !self.punctuation(93) && !matches!(self.token, Token::End) {
                    if self.punctuation(44) {
                        self.next()?;
                    }
                    if let Some(value) = self.value(depth + 1)? {
                        array.push(value);
                    } else {
                        self.skip(93)?;
                    }
                }
                if self.punctuation(93) {
                    self.next()?;
                }
                Ok(Some(Value::Array(array)))
            }
            other => {
                self.token = other;
                Ok(None)
            }
        }
    }
}
fn digit(ch: u16) -> bool {
    (48..=57).contains(&ch)
}

pub(crate) fn parse(source: &str) -> Result<Option<Value>> {
    let mut parser = Parser {
        text: source.encode_utf16().collect(),
        cursor: 0,
        token: Token::End,
    };
    parser.next()?;
    parser.value(0)
}
