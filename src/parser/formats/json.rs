//! A flat JSON/JSONC syntax tree retaining property order and source offsets.
//! Shopify section references require the first duplicate property, as jsonc-parser does.
use regex::Regex;

pub(super) enum Kind {
    Object,
    Array,
    String(String),
    Scalar(String),
}

pub(super) struct Node {
    pub kind: Kind,
    pub offset: usize,
    pub children: Vec<(Option<String>, usize)>,
}

impl Node {
    pub fn string(&self) -> Option<&str> {
        match &self.kind {
            Kind::String(value) => Some(value),
            _ => None,
        }
    }
}

pub(super) struct Tree {
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn property(&self, parent: usize, name: &str, last: bool) -> Option<usize> {
        let node = &self.nodes[parent];
        if !matches!(node.kind, Kind::Object) {
            return None;
        }
        if last {
            node.children
                .iter()
                .rev()
                .find_map(|(key, child)| (key.as_deref() == Some(name)).then_some(*child))
        } else {
            node.children
                .iter()
                .find_map(|(key, child)| (key.as_deref() == Some(name)).then_some(*child))
        }
    }

    fn truthy(&self, index: usize) -> bool {
        match &self.nodes[index].kind {
            Kind::String(value) => !value.is_empty(),
            Kind::Scalar(value) => {
                value == "true" || value.parse::<f64>().is_ok_and(|number| number != 0.0)
            }
            _ => true,
        }
    }

    fn display(&self, index: usize) -> String {
        let mut result = String::new();
        let mut pending = vec![Some(index)];
        while let Some(next) = pending.pop() {
            let Some(next) = next else {
                result.push(',');
                continue;
            };
            match &self.nodes[next].kind {
                Kind::String(value) => result.push_str(value),
                Kind::Scalar(value) if value != "null" => result.push_str(value),
                Kind::Scalar(_) => {}
                Kind::Object => result.push_str("[object Object]"),
                Kind::Array => {
                    for (position, (_, child)) in self.nodes[next].children.iter().enumerate().rev()
                    {
                        pending.push(Some(*child));
                        if position != 0 {
                            pending.push(None);
                        }
                    }
                }
            }
        }
        result
    }
}

enum Token {
    Punctuation(u8),
    Value(Kind),
}

fn next(
    source: &str,
    cursor: &mut usize,
    jsonc: bool,
    number: &Regex,
) -> Result<Option<(Token, usize)>, ()> {
    let bytes = source.as_bytes();
    loop {
        while bytes
            .get(*cursor)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            *cursor += 1;
        }
        if jsonc && source[*cursor..].starts_with("//") {
            *cursor += 2;
            while bytes
                .get(*cursor)
                .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
            {
                *cursor += 1;
            }
        } else if jsonc && source[*cursor..].starts_with("/*") {
            *cursor += 2;
            *cursor += source[*cursor..].find("*/").ok_or(())? + 2;
        } else {
            break;
        }
    }
    let start = *cursor;
    let Some(&byte) = bytes.get(start) else {
        return Ok(None);
    };
    match byte {
        byte @ (b'{' | b'}' | b'[' | b']' | b',' | b':') => {
            *cursor += 1;
            Ok(Some((Token::Punctuation(byte), start)))
        }
        b'"' => {
            *cursor += 1;
            while let Some(byte) = bytes.get(*cursor) {
                *cursor += 1;
                if *byte == b'\\' {
                    *cursor += 1;
                } else if *byte == b'"' {
                    let value: String = serde_json::from_str(source.get(start..*cursor).ok_or(())?)
                        .map_err(|_| ())?;
                    return Ok(Some((Token::Value(Kind::String(value)), start)));
                }
            }
            Err(())
        }
        b'-' | b'0'..=b'9' => {
            let found = number.find(&source[start..]).ok_or(())?;
            *cursor += found.end();
            Ok(Some((
                Token::Value(Kind::Scalar(found.as_str().into())),
                start,
            )))
        }
        _ => {
            for literal in ["true", "false", "null"] {
                if source[start..].starts_with(literal) {
                    *cursor += literal.len();
                    return Ok(Some((Token::Value(Kind::Scalar(literal.into())), start)));
                }
            }
            Err(())
        }
    }
}

#[derive(Clone, Copy)]
enum State {
    Key(bool),
    Colon,
    Value,
    Array(bool),
    Comma,
}

struct Frame {
    node: usize,
    state: State,
    key: Option<String>,
}

/// Iterative parsing keeps malformed or deeply nested source off the call stack.
pub(super) fn parse(source: &str, jsonc: bool) -> Option<Tree> {
    let number = Regex::new(r"^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?").ok()?;
    let mut tree = Tree { nodes: Vec::new() };
    let mut stack: Vec<Frame> = Vec::new();
    let mut cursor = 0;
    loop {
        let Some((token, offset)) = next(source, &mut cursor, jsonc, &number).ok()? else {
            return (stack.is_empty() && !tree.nodes.is_empty()).then_some(tree);
        };
        if let Some(frame) = stack.last_mut() {
            match frame.state {
                State::Key(close) => match token {
                    Token::Punctuation(b'}') if close => {
                        stack.pop();
                        continue;
                    }
                    Token::Value(Kind::String(key)) => {
                        frame.key = Some(key);
                        frame.state = State::Colon;
                        continue;
                    }
                    _ => return None,
                },
                State::Colon => {
                    if !matches!(token, Token::Punctuation(b':')) {
                        return None;
                    }
                    frame.state = State::Value;
                    continue;
                }
                State::Comma => match token {
                    Token::Punctuation(b',') => {
                        frame.state = if matches!(tree.nodes[frame.node].kind, Kind::Object) {
                            State::Key(jsonc)
                        } else {
                            State::Array(jsonc)
                        };
                        continue;
                    }
                    Token::Punctuation(b'}')
                        if matches!(tree.nodes[frame.node].kind, Kind::Object) =>
                    {
                        stack.pop();
                        continue;
                    }
                    Token::Punctuation(b']')
                        if matches!(tree.nodes[frame.node].kind, Kind::Array) =>
                    {
                        stack.pop();
                        continue;
                    }
                    _ => return None,
                },
                State::Array(true) if matches!(token, Token::Punctuation(b']')) => {
                    stack.pop();
                    continue;
                }
                State::Value | State::Array(_) => {}
            }
        } else if !tree.nodes.is_empty() {
            return None;
        }
        let (kind, state) = match token {
            Token::Punctuation(b'{') => (Kind::Object, Some(State::Key(true))),
            Token::Punctuation(b'[') => (Kind::Array, Some(State::Array(true))),
            Token::Value(kind) => (kind, None),
            _ => return None,
        };
        let index = tree.nodes.len();
        tree.nodes.push(Node {
            kind,
            offset,
            children: Vec::new(),
        });
        if let Some(frame) = stack.last_mut() {
            tree.nodes[frame.node]
                .children
                .push((frame.key.take(), index));
            frame.state = State::Comma;
        }
        if let Some(state) = state {
            stack.push(Frame {
                node: index,
                state,
                key: None,
            });
        }
    }
}

pub(super) fn schema_name(source: &str) -> String {
    let Some(tree) = parse(source, false) else {
        return "schema".into();
    };
    let Some(name) = tree
        .property(0, "name", true)
        .filter(|&index| tree.truthy(index))
    else {
        return "schema".into();
    };
    if let Some(name) = tree.nodes[name].string() {
        return name.into();
    }
    if let Some(english) = tree
        .property(name, "en", true)
        .filter(|&index| tree.truthy(index))
    {
        return tree.display(english);
    }
    // Object.values puts integer-index keys first, then preserves insertion order.
    // Duplicate keys replace their values without changing their first position.
    let children = &tree.nodes[name].children;
    let numeric = children
        .iter()
        .filter_map(|child| {
            let key = child.0.as_deref()?;
            let index = key.parse::<u32>().ok()?;
            (index != u32::MAX && index.to_string() == key).then_some((index, child))
        })
        .min_by_key(|(index, _)| *index)
        .map(|(_, child)| child);
    let first = numeric
        .or_else(|| children.first())
        .and_then(|(key, child)| {
            key.as_deref()
                .and_then(|key| tree.property(name, key, true))
                .or(Some(*child))
        });
    first
        .filter(|&index| tree.truthy(index))
        .map_or_else(|| "schema".into(), |index| tree.display(index))
}
