//! Offset-preserving Delphi grammar repairs. Positions are UTF-16, like the reference parser.
use std::collections::HashMap;

const ROUTINES: &[&str] = &[
    "procedure",
    "function",
    "constructor",
    "destructor",
    "operator",
];
const MODIFIERS: &[&str] = &["const", "var", "out", "constref"];
const LABEL_ENDS: &[&str] = &["end", "until", "except", "finally"];
const BLOCK_OPENERS: &[&str] = &["begin", "asm", "try", "case", "record"];
const NOT_FIELDS: &[&str] = &[
    "begin",
    "class",
    "object",
    "interface",
    "implementation",
    "procedure",
    "function",
    "constructor",
    "destructor",
    "property",
    "type",
    "var",
    "const",
    "private",
    "protected",
    "public",
    "published",
    "strict",
];
const STATEMENTS: &[&str] = &[
    "begin",
    "if",
    "while",
    "for",
    "repeat",
    "case",
    "try",
    "with",
    "raise",
    "inherited",
    "goto",
    "asm",
];
const TYPES: &[&str] = &[
    "procedure",
    "function",
    "reference",
    "class",
    "record",
    "object",
    "interface",
    "array",
    "set",
    "file",
    "packed",
    "string",
    "type",
    "const",
    "var",
    "out",
];
const CONTINUES: &[&str] = &[
    "then", "do", "else", "of", "and", "or", "xor", "not", "div", "mod", "shl", "shr", "in", "is",
    "as", "to", "downto", "until", "goto", "on",
];
const HINTS: &[&str] = &[
    "static",
    "virtual",
    "dynamic",
    "abstract",
    "override",
    "overload",
    "reintroduce",
    "inline",
    "stdcall",
    "cdecl",
    "pascal",
    "register",
    "safecall",
    "assembler",
    "noreturn",
    "local",
    "far",
    "near",
    "default",
    "nodefault",
    "deprecated",
    "experimental",
    "message",
    "platform",
    "unimplemented",
    "cppdecl",
    "cvar",
    "mwpascal",
    "nostackframe",
    "interrupt",
    "iocheck",
    "hardfloat",
    "softfloat",
    "ms_abi_default",
    "ms_abi_cdecl",
    "saveregisters",
    "sysv_abi_default",
    "sysv_abi_cdecl",
    "vectorcall",
    "varargs",
    "winapi",
    "public",
    "export",
    "alias",
    "dispid",
];
type Range = (usize, usize);
type Edit = (usize, usize, &'static str);

fn at(s: &[u16], i: usize) -> u16 {
    s.get(i).copied().unwrap_or(0)
}
fn ident(c: u16) -> bool {
    part(c) && !(48..=57).contains(&c) || c == 38
}
fn part(c: u16) -> bool {
    (65..=90).contains(&c)
        || (97..=122).contains(&c)
        || c == 95
        || (48..=57).contains(&c)
        || c >= 128
}
fn space(c: u16) -> bool {
    matches!(c, 9 | 10 | 13 | 32)
}
fn word_end(s: &[u16], mut i: usize) -> usize {
    while part(at(s, i)) {
        i += 1;
    }
    i
}
fn lower(s: &[u16], a: usize, b: usize) -> String {
    String::from_utf16_lossy(&s[a..b]).to_lowercase()
}
fn word(s: &[u16], i: usize) -> String {
    if ident(at(s, i)) {
        lower(s, i, word_end(s, i + 1))
    } else {
        String::new()
    }
}
fn token(c: u16) -> String {
    char::from_u32(c as u32).unwrap_or('\u{FFFD}').to_string()
}

fn comment(s: &[u16], i: usize) -> Option<usize> {
    match at(s, i) {
        123 => Some(
            (i + 1..s.len())
                .find(|&j| s[j] == 125)
                .map_or(s.len(), |j| j + 1),
        ),
        40 if at(s, i + 1) == 42 => Some(
            (i + 2..s.len())
                .find(|&j| s[j] == 42 && at(s, j + 1) == 41)
                .map_or(s.len(), |j| j + 2),
        ),
        47 if at(s, i + 1) == 47 => Some((i + 2..s.len()).find(|&j| s[j] == 10).unwrap_or(s.len())),
        39 => {
            let mut j = i + 1;
            while j < s.len() {
                if s[j] == 39 {
                    if at(s, j + 1) != 39 {
                        return Some(j + 1);
                    }
                    j += 2;
                } else if s[j] == 10 {
                    return Some(j);
                } else {
                    j += 1;
                }
            }
            Some(j)
        }
        _ => None,
    }
}
fn comment_start(s: &[u16], i: usize) -> bool {
    at(s, i) == 123 || at(s, i) == 40 && at(s, i + 1) == 42 || at(s, i) == 47 && at(s, i + 1) == 47
}
fn trivia(s: &[u16], mut i: usize) -> usize {
    while i < s.len() {
        if space(s[i]) {
            i += 1;
        } else if comment_start(s, i) {
            i = comment(s, i).unwrap();
        } else {
            break;
        }
    }
    i
}
fn before(s: &[u16], mut i: usize) -> usize {
    loop {
        while i > 0 && space(s[i - 1]) {
            i -= 1;
        }
        if i > 0 && s[i - 1] == 125 {
            if let Some(j) = (0..i - 1).rev().find(|&j| s[j] == 123) {
                i = j;
            } else {
                return i;
            }
        } else if i > 1 && s[i - 1] == 41 && s[i - 2] == 42 {
            if let Some(j) = (0..i - 2).rev().find(|&j| s[j] == 40 && at(s, j + 1) == 42) {
                i = j;
            } else {
                return i;
            }
        } else {
            return i;
        }
    }
}
fn word_before(s: &[u16], i: usize) -> Option<usize> {
    let mut k = i;
    while k > 0 && part(s[k - 1]) {
        k -= 1;
    }
    if k == i {
        return None;
    }
    if k > 0 && s[k - 1] == 38 {
        k -= 1;
    }
    ident(at(s, k)).then_some(k)
}
fn semicolon(s: &[u16], mut i: usize, max: usize) -> Option<usize> {
    let limit = s.len().min(i + max);
    let mut depth = 0;
    while i < limit {
        if let Some(k) = comment(s, i) {
            i = k;
            continue;
        }
        match s[i] {
            40 | 91 => depth += 1,
            41 | 93 => depth -= 1,
            59 if depth <= 0 => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}
fn attribute(s: &[u16], i: usize) -> Option<usize> {
    let limit = s.len().min(i + 2000);
    let mut depth = 0;
    let mut j = i;
    while j < limit {
        if let Some(k) = comment(s, j) {
            j = k;
            continue;
        }
        match s[j] {
            40 | 91 => depth += 1,
            41 | 93 => {
                depth -= 1;
                if depth == 0 {
                    return (s[j] == 93).then_some(j + 1);
                }
            }
            59 => return None,
            _ => {}
        }
        j += 1;
    }
    None
}
fn routine_name(s: &[u16], mut i: usize) -> usize {
    loop {
        i = trivia(s, i);
        if !ident(at(s, i)) {
            return i;
        }
        i = word_end(s, i + 1);
        let mut j = trivia(s, i);
        if at(s, j) == 60 {
            let mut depth = 0;
            while j < s.len() {
                match s[j] {
                    60 => depth += 1,
                    62 => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    59 | 40 | 41 | 10 => return i,
                    _ => {}
                }
                j += 1;
            }
            i = j;
            j = trivia(s, i);
        }
        if at(s, j) != 46 {
            return i;
        }
        i = j + 1;
    }
}
fn parameters(s: &[u16], mut i: usize, ranges: &mut Vec<Range>) -> usize {
    let mut depth = 0;
    let mut start = false;
    while i < s.len() {
        if start {
            start = false;
            loop {
                i = trivia(s, i);
                if at(s, i) == 91 {
                    if let Some(end) = attribute(s, i) {
                        ranges.push((i, end));
                        i = end;
                    } else {
                        break;
                    }
                } else if ident(at(s, i)) {
                    let e = word_end(s, i + 1);
                    if !MODIFIERS.contains(&lower(s, i, e).as_str()) {
                        break;
                    }
                    i = e;
                } else {
                    break;
                }
            }
            continue;
        }
        if let Some(k) = comment(s, i) {
            i = k;
            continue;
        }
        match s[i] {
            40 | 91 => {
                depth += 1;
                if depth == 1 {
                    start = true;
                }
            }
            41 | 93 => {
                depth -= 1;
                if depth <= 0 {
                    return i + 1;
                }
            }
            59 if depth == 1 => start = true,
            _ => {}
        }
        i += 1;
    }
    i
}
fn for_variable(s: &[u16], end: usize, ranges: &mut Vec<Range>) -> usize {
    let var = trivia(s, end);
    if word(s, var) != "var" {
        return end;
    }
    let name = trivia(s, var + 3);
    if !ident(at(s, name)) {
        return end;
    }
    let mut i = trivia(s, word_end(s, name + 1));
    let mut ty = None;
    if at(s, i) == 58 && at(s, i + 1) != 61 {
        ty = Some(i);
        let limit = s.len().min(i + 500);
        let mut depth = 0;
        i += 1;
        loop {
            if i >= limit {
                return end;
            }
            if let Some(k) = comment(s, i) {
                i = k;
                continue;
            }
            match s[i] {
                60 | 40 | 91 => depth += 1,
                62 | 41 | 93 => {
                    depth -= 1;
                    if depth < 0 {
                        return end;
                    }
                }
                59 | 61 => return end,
                c if ident(c) => {
                    let w = word(s, i);
                    if depth == 0 && w == "in" {
                        break;
                    }
                    if ["do", "to", "downto", "begin", "end"].contains(&w.as_str()) {
                        return end;
                    }
                    i = word_end(s, i + 1);
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }
    if word(s, i) != "in" {
        return end;
    }
    ranges.push((var, var + 3));
    if let Some(start) = ty {
        ranges.push((start, i));
    }
    i
}
fn class_before(s: &[u16], mut i: usize) -> Option<usize> {
    while i > 0 && space(s[i - 1]) {
        i -= 1;
    }
    let w = i.checked_sub(5)?;
    (lower(s, w, i) == "class" && (w == 0 || !part(s[w - 1]))).then_some(w)
}
fn property(s: &[u16], start: usize, end: usize, ranges: &mut Vec<Range>) -> usize {
    let name = trivia(s, end);
    if !ident(at(s, name)) {
        return end;
    }
    let i = trivia(s, word_end(s, name + 1));
    if matches!(at(s, i), 58 | 91) {
        return end;
    }
    let Some(close) = semicolon(s, i, 500) else {
        return end;
    };
    ranges.push((class_before(s, start).unwrap_or(start), close));
    close
}

struct Group {
    bounds: Vec<usize>,
    nested: Vec<bool>,
}
fn conditional(s: &[u16], i: usize) -> u8 {
    let mut j = i + 2;
    while (97..=122).contains(&(at(s, j) | 32)) {
        j += 1;
    }
    match lower(s, i + 2, j).as_str() {
        "if" | "ifdef" | "ifndef" | "ifopt" => 1,
        "else" | "elseif" => 2,
        "endif" | "ifend" => 3,
        _ => 0,
    }
}
fn groups(s: &[u16]) -> Vec<Group> {
    let mut groups = vec![];
    let mut open: Vec<Group> = vec![];
    let mut i = 0;
    while i < s.len() {
        let Some(k) = comment(s, i) else {
            i += 1;
            continue;
        };
        if s[i] == 123 && at(s, i + 1) == 36 {
            match conditional(s, i) {
                1 => {
                    if let Some(top) = open.last_mut() {
                        *top.nested.last_mut().unwrap() = true;
                    }
                    open.push(Group {
                        bounds: vec![i, k],
                        nested: vec![false],
                    });
                }
                2 => {
                    if let Some(top) = open.last_mut() {
                        top.bounds.extend([i, k]);
                        top.nested.push(false);
                    }
                }
                3 => {
                    if let Some(mut top) = open.pop() {
                        top.bounds.extend([i, k]);
                        groups.push(top);
                    }
                }
                _ => {}
            }
        }
        i = k;
    }
    groups
}
fn body(s: &[u16], mut i: usize, to: usize) -> bool {
    let mut blocks = vec![];
    let mut pending = 0;
    let mut own = false;
    let mut last = String::new();
    let mut prev = String::new();
    while i < to {
        if let Some(k) = comment(s, i) {
            if s[i] == 39 {
                prev = last;
                last = "'".into();
            }
            i = k;
            continue;
        }
        if ident(s[i]) {
            let e = word_end(s, i + 1);
            let w = if e - i <= 11 {
                lower(s, i, e)
            } else {
                "word".into()
            };
            if blocks.is_empty() {
                if ROUTINES.contains(&w.as_str()) && (last.is_empty() || last == ";") {
                    pending += 1;
                } else if (w == "forward" || w == "external") && pending > 0 {
                    pending -= 1;
                }
            }
            if w == "end" {
                if blocks.pop().is_none() {
                    return true;
                }
            } else if w == "begin" || w == "asm" {
                if blocks.is_empty() {
                    own = pending == 0;
                    if !own {
                        pending -= 1;
                    }
                }
                blocks.push(w.clone());
            } else if w == "try"
                || w == "record"
                || w == "case" && blocks.last().map(String::as_str) != Some("record")
            {
                blocks.push(w.clone());
            }
            prev = last;
            last = w;
            i = e;
            continue;
        }
        if !space(s[i]) {
            prev = last;
            last = token(s[i]);
        }
        i += 1;
    }
    blocks.is_empty() && own && prev == "end" && last == ";"
}
fn block(s: &[u16], mut i: usize, to: usize) -> bool {
    while i < to {
        if let Some(k) = comment(s, i) {
            i = k;
            continue;
        }
        if ident(s[i]) {
            let e = word_end(s, i + 1);
            if matches!(e - i, 3 | 5) && ["begin", "asm"].contains(&lower(s, i, e).as_str()) {
                return true;
            }
            i = e;
        } else {
            i += 1;
        }
    }
    false
}
fn after_header(
    s: &[u16],
    mut i: usize,
    groups: &[Group],
    map: &HashMap<usize, usize>,
    ranges: &mut Vec<Range>,
) {
    loop {
        while i < s.len() {
            if space(s[i]) {
                i += 1;
            } else if comment_start(s, i) && !map.contains_key(&i) {
                i = comment(s, i).unwrap();
            } else {
                break;
            }
        }
        if let Some(&g) = map.get(&i) {
            let b = &groups[g].bounds;
            if (1..b.len() - 1).step_by(2).all(|j| body(s, b[j], b[j + 1])) {
                return;
            }
            ranges.extend(b.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])));
            i = if block(s, b[1], b[b.len() - 2]) {
                b[b.len() - 1]
            } else {
                b[1]
            };
            continue;
        }
        if !HINTS.contains(&word(s, i).as_str()) {
            return;
        }
        let Some(end) = semicolon(s, i, 500) else {
            return;
        };
        i = end;
    }
}
fn open_blocks(s: &[u16], mut i: usize, to: usize) -> Option<Vec<(usize, u8)>> {
    let mut open = vec![];
    while i < to {
        if let Some(k) = comment(s, i) {
            i = k;
            continue;
        }
        if ident(s[i]) {
            let e = word_end(s, i + 1);
            if (3..=6).contains(&(e - i)) {
                let w = lower(s, i, e);
                if w == "end" {
                    open.pop()?;
                } else if BLOCK_OPENERS.contains(&w.as_str()) {
                    open.push((
                        i,
                        match w.as_str() {
                            "begin" => 0,
                            "asm" => 1,
                            _ => 2,
                        },
                    ));
                }
            }
            i = e;
        } else {
            i += 1;
        }
    }
    Some(open)
}
fn duplicate_begins(s: &[u16], groups: &[Group], ranges: &mut Vec<Range>) {
    for g in groups {
        if g.nested.len() < 2 || g.nested.contains(&true) {
            continue;
        }
        let open: Option<Vec<_>> = (0..g.nested.len())
            .map(|b| {
                open_blocks(s, g.bounds[2 * b + 1], g.bounds[2 * b + 2]).filter(|v| !v.is_empty())
            })
            .collect();
        let Some(open) = open else {
            continue;
        };
        let kinds = |k| open.iter().filter(|o| o.iter().all(|v| v.1 == k)).count();
        if kinds(0) == open.len() && open.iter().all(|o| o.len() == open[0].len()) {
            for o in &open[1..] {
                for &(at, _) in o {
                    ranges.push((at, at + 5));
                }
            }
        } else if open.iter().all(|o| o.len() == 1)
            && kinds(1) > 0
            && kinds(0) > 0
            && kinds(0) + kinds(1) == open.len()
        {
            let mut kept = false;
            for (b, o) in open.iter().enumerate() {
                let (at, k) = o[0];
                if k == 1 {
                    ranges.push((g.bounds[2 * b + 1], g.bounds[2 * b + 2]));
                } else if kept {
                    ranges.push((at, at + 5));
                } else {
                    kept = true;
                }
            }
        }
    }
}
fn statement_shape(s: &[u16], mut i: usize, to: usize, bare: bool) -> i8 {
    let mut blocks = vec![];
    let (mut depth, mut heads, mut tokens, mut start, mut name) = (0, 0, 0, 0, 0);
    let mut last = String::new();
    while i < to {
        if let Some(k) = comment(s, i) {
            if s[i] == 39 {
                tokens += 1;
                name = 0;
                last = "'".into();
            }
            i = k;
            continue;
        }
        let c = s[i];
        if space(c) {
            i += 1;
            continue;
        }
        tokens += 1;
        if ident(c) {
            let e = word_end(s, i + 1);
            let w = if e - i <= 9 {
                lower(s, i, e)
            } else {
                "word".into()
            };
            match w.as_str() {
                "begin" | "try" | "case" | "asm" => blocks.push(false),
                "repeat" => blocks.push(true),
                "end" => {
                    if blocks.pop() != Some(false) {
                        return -1;
                    }
                }
                "until" => {
                    if blocks.pop() != Some(true) {
                        return -1;
                    }
                }
                "if" | "while" | "for" | "with" | "on" => heads += 1,
                "then" | "do" => heads -= 1,
                _ => {}
            }
            if tokens == 1 {
                if STATEMENTS.contains(&w.as_str()) {
                    start = 1;
                } else if !TYPES.contains(&w.as_str())
                    && !CONTINUES.contains(&w.as_str())
                    && w != "end"
                {
                    name = 1;
                }
            } else {
                name = if name == 2 { 1 } else { 0 };
            }
            last = w;
            i = e;
            continue;
        }
        if (48..=57).contains(&c) {
            name = 0;
            last = "0".into();
            i = word_end(s, i + 1);
            continue;
        }
        if c == 58 && at(s, i + 1) == 61 {
            if depth == 0 && start != 2 {
                start = 1;
            }
            name = 0;
            last = ":=".into();
            i += 2;
            continue;
        }
        if matches!(c, 40 | 91) {
            if c == 40 && name == 1 && start == 0 {
                start = 2;
            }
            depth += 1;
        } else if matches!(c, 41 | 93) {
            depth -= 1;
            if depth < 0 {
                return -1;
            }
        } else if c == 59 && name == 1 && start == 0 {
            start = 3;
        }
        name = if c == 46 && name == 1 { 2 } else { 0 };
        last = token(c);
        i += 1;
    }
    if tokens == 0 {
        return 2;
    }
    if depth != 0 || !blocks.is_empty() || heads != 0 {
        return -1;
    }
    if name == 1 && start == 0 {
        start = 3;
    }
    if start == 0 || start == 3 && !bare {
        return -1;
    }
    let ends = [";", ")", "]", "^", "'", "0"].contains(&last.as_str())
        || ident(last.encode_utf16().next().unwrap_or(0)) && !CONTINUES.contains(&last.as_str());
    if !ends {
        -1
    } else if last == ";" {
        1
    } else {
        0
    }
}
fn alternatives(
    s: &[u16],
    groups: &[Group],
    labels: &HashMap<usize, bool>,
    ranges: &[Range],
    words: &mut Vec<Edit>,
) {
    for g in groups {
        let b = &g.bounds;
        let Some(&label) = labels.get(&b[0]) else {
            continue;
        };
        if g.nested.len() < 2 || g.nested.contains(&true) {
            continue;
        }
        let (mut filled, mut semi, mut breaks, mut ok) = (0, false, label, true);
        for n in 0..g.nested.len() {
            let shape = statement_shape(s, b[2 * n + 1], b[2 * n + 2], !label);
            if shape == 2 {
                continue;
            }
            if shape == -1 {
                ok = false;
                break;
            }
            if filled > 0 && !semi {
                breaks = true;
            }
            filled += 1;
            semi = shape == 1;
        }
        if !ok || filled < 2 || !breaks || s[b[0]..b[0] + 5].iter().any(|c| matches!(c, 10 | 13)) {
            continue;
        }
        if b.as_chunks::<2>()
            .0
            .iter()
            .any(|p| ranges.iter().any(|&(f, t)| f < p[1] && t > p[0]))
        {
            continue;
        }
        words.push((b[0], b[1], "begin"));
        for n in (2..b.len() - 2).step_by(2) {
            words.push((b[n], b[n + 1], ";"));
        }
        words.push((
            b[b.len() - 2],
            b[b.len() - 1],
            if semi { "end;" } else { "end" },
        ));
    }
}
fn statement_position(s: &[u16], a: usize, b: usize) -> Option<bool> {
    if b - a == 1 {
        return (at(s, a) == 58).then_some(true);
    }
    if !(2..=4).contains(&(b - a)) {
        return None;
    }
    ["then", "else", "do"]
        .contains(&lower(s, a, b).as_str())
        .then_some(false)
}
fn anonymous_start(s: &[u16], i: usize) -> Option<usize> {
    let mut j = before(s, i);
    let mut w = word_before(s, j);
    if w.is_some_and(|w| word(s, w) == "packed") {
        j = before(s, w?);
        w = word_before(s, j);
    }
    let mut array = false;
    if w.is_some_and(|w| word(s, w) == "of") {
        j = before(s, w?);
        if j > 0 && s[j - 1] == 93 {
            let open = (0..j).rev().find(|&k| s[k] == 91)?;
            j = before(s, open);
        }
        w = word_before(s, j);
        if !w.is_some_and(|w| word(s, w) == "array") {
            return None;
        }
        j = before(s, w?);
        w = word_before(s, j);
        if w.is_some_and(|w| word(s, w) == "packed") {
            j = before(s, w?);
        }
        array = true;
    }
    let c = *s.get(j.checked_sub(1)?)?;
    if c != 58 && !(c == 61 && array) {
        return None;
    }
    let mut start = word_before(s, before(s, j - 1))?;
    if c == 61 {
        return Some(start);
    }
    loop {
        let k = before(s, start);
        if k == 0 || s[k - 1] != 44 {
            return Some(start);
        }
        start = word_before(s, before(s, k - 1))?;
    }
}
fn record_end(s: &[u16], mut i: usize) -> Option<usize> {
    let limit = s.len().min(i + 20000);
    let (mut depth, mut brackets) = (1, 0);
    let mut prev = String::new();
    while i < limit {
        if let Some(k) = comment(s, i) {
            if s[i] == 39 {
                prev = "'".into();
            }
            i = k;
            continue;
        }
        let c = s[i];
        if ident(c) {
            let e = word_end(s, i + 1);
            let w = lower(s, i, e);
            if brackets == 0 {
                if w == "end" {
                    depth -= 1;
                    if depth == 0 {
                        return Some(e);
                    }
                } else if w == "record" {
                    depth += 1;
                } else if NOT_FIELDS.contains(&w.as_str())
                    && ![":", "of", "to"].contains(&prev.as_str())
                {
                    return None;
                }
            }
            prev = w;
            i = e;
            continue;
        }
        if matches!(c, 40 | 91) {
            brackets += 1;
        } else if matches!(c, 41 | 93) {
            brackets -= 1;
        } else if c == 61 && brackets == 0 {
            return None;
        }
        if !space(c) {
            prev = token(c);
        }
        i += 1;
    }
    None
}
fn anonymous_record(
    s: &[u16],
    start: usize,
    end: usize,
    ranges: &mut Vec<Range>,
    words: &mut Vec<Edit>,
) -> usize {
    if anonymous_start(s, start).is_none() {
        return end;
    }
    let first = word(s, trivia(s, end));
    if first.is_empty() || NOT_FIELDS.contains(&first.as_str()) {
        return end;
    }
    let Some(close) = record_end(s, end) else {
        return end;
    };
    let next = trivia(s, close);
    let last = word(s, next) == "end";
    if !matches!(at(s, next), 59 | 61 | 41) && !last {
        return end;
    }
    let packed = word_before(s, before(s, start));
    let pos = packed.filter(|&w| word(s, w) == "packed").unwrap_or(start);
    words.push((pos, pos + 6, "string"));
    if last {
        ranges.push((pos + 6, close - 3));
        words.push((close - 3, close, ";"));
    } else {
        ranges.push((pos + 6, close));
    }
    close
}

pub(super) fn prepare(source: &str) -> String {
    let s: Vec<u16> = source.encode_utf16().collect();
    let mut ranges = vec![];
    let mut words = vec![];
    let groups = if source.contains("{$") {
        groups(&s)
    } else {
        vec![]
    };
    let map: HashMap<_, _> = groups
        .iter()
        .enumerate()
        .map(|(i, g)| (g.bounds[0], i))
        .collect();
    let mut labels = HashMap::new();
    let mut last = None;
    let mut i = 0;
    while i < s.len() {
        if let Some(k) = comment(&s, i) {
            if s[i] == 39 {
                last = Some((i, k));
            } else if at(&s, i + 1) == 36 && map.contains_key(&i) {
                if let Some((a, b)) = last {
                    if let Some(p) = statement_position(&s, a, b) {
                        labels.insert(i, p);
                    }
                }
            }
            i = k;
            continue;
        }
        let c = s[i];
        if ident(c) {
            let e = word_end(&s, i + 1);
            let w = if (6..=11).contains(&(e - i)) {
                lower(&s, i, e)
            } else {
                String::new()
            };
            if ROUTINES.contains(&w.as_str()) && (w != "operator" || class_before(&s, i).is_some())
            {
                let open = trivia(&s, routine_name(&s, e));
                let next = if at(&s, open) == 40 {
                    parameters(&s, open, &mut ranges)
                } else {
                    e
                };
                if !map.is_empty() {
                    if let Some(end) = semicolon(&s, next, 500) {
                        after_header(&s, end, &groups, &map, &mut ranges);
                    }
                }
                i = next;
                last = None;
                continue;
            } else if w == "property" {
                i = property(&s, i, e, &mut ranges);
                last = None;
                continue;
            } else if w == "record" {
                i = anonymous_record(&s, i, e, &mut ranges, &mut words);
                last = None;
                continue;
            } else if e - i == 3 && lower(&s, i, e) == "for" {
                i = for_variable(&s, e, &mut ranges);
                last = None;
                continue;
            } else if e - i == 5 && lower(&s, i, e) == "raise" {
                let next = trivia(&s, e);
                let after = word(&s, next);
                if at(&s, next) == 59 || after == "else" {
                    ranges.push((i, e));
                } else if LABEL_ENDS.contains(&after.as_str()) {
                    words.push((i, e, ";"));
                }
            } else {
                let mut colon = e;
                while matches!(at(&s, colon), 32 | 9) {
                    colon += 1;
                }
                if at(&s, colon) == 58
                    && at(&s, colon + 1) != 61
                    && (i == 0 || s[i - 1] != 64)
                    && LABEL_ENDS.contains(&word(&s, trivia(&s, colon + 1)).as_str())
                {
                    ranges.push((i, colon + 1));
                }
            }
            last = Some((i, e));
            i = e;
        } else if (48..=57).contains(&c) {
            let end = word_end(&s, i + 1);
            last = Some((i, end));
            i = end;
        } else if space(c) {
            i += 1;
        } else {
            let end = i + if c == 58 && at(&s, i + 1) == 61 { 2 } else { 1 };
            last = Some((i, end));
            i = end;
        }
    }
    duplicate_begins(&s, &groups, &mut ranges);
    if !labels.is_empty() {
        alternatives(&s, &groups, &labels, &ranges, &mut words);
    }
    if ranges.is_empty() && words.is_empty() {
        return source.into();
    }
    words.extend(ranges.into_iter().map(|(a, b)| (a, b, "")));
    words.sort_by_key(|e| e.0);
    let mut out = s.clone();
    let mut last = 0;
    for (start, to, w) in words {
        let from = start.max(last);
        if to <= from {
            continue;
        }
        let written = if from == start && w.len() <= to - from {
            w
        } else {
            ""
        };
        for (i, c) in out[from..to].iter_mut().enumerate() {
            if i < written.len() {
                *c = written.as_bytes()[i] as u16;
            } else if !matches!(*c, 10 | 13) {
                *c = 32;
            }
        }
        last = to;
    }
    String::from_utf16(&out).expect("repairs preserve UTF-16 boundaries")
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_independent_preprocessing_corpus() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/upstream/pascal/cases.json"
        ))
        .unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            assert_eq!(
                super::prepare(case["source"].as_str().unwrap()),
                case["prepared"].as_str().unwrap(),
                "{}",
                case["id"]
            );
        }
    }
}
