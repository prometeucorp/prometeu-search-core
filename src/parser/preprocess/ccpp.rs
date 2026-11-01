//! Offset-preserving C/C++ passes from upstream languages/c-cpp.ts.
use super::patterns::*;
use super::*;

pub(super) fn prepare_cpp(path: &Path, source: &str) -> Result<String> {
    let raw = RawStrings::mask(source)?;
    let source = flatten_conditionals(&rewrite_access(&rewrite_foundation(&raw.masked)?)?)?;
    let mut out = normalize_interfaces(&source)?;
    out = export_macros(&out)?;
    out = blank_pattern(&out, CPP_INLINE_MACRO_RE)?;
    out = blank_pattern(&out, CPP_API_PREFIX_RE)?;
    out = inline_annotations(&out)?;
    out = pointer_annotations(&out)?;
    out = trailing_attributes(&out)?;
    out = declarator_attributes(&out)?;
    out = annotation_calls(&out, false)?;
    out = leading_c_attributes(&out)?;
    out = leading_cpp_attributes(&out)?;
    out = lone_macro_lines(&out, false)?;
    let lower = path.to_string_lossy().to_lowercase();
    if lower.ends_with(".metal") {
        out = blank_pattern(&out, METAL_ATTRIBUTE_RE)?;
    } else if lower.ends_with(".cu") || lower.ends_with(".cuh") || looks_like_cuda(&source) {
        out = cuda(&out)?;
    }
    raw.restore(&restore_directives(&source, &out)?)
}

pub(super) fn prepare_c(source: &str) -> Result<String> {
    let raw = RawStrings::mask(source)?;
    let source = rewrite_foundation(&raw.masked)?;
    let mut out = cplusplus_guards(&source)?;
    out = kernel_annotations(&out)?;
    out = designated_arguments(&out)?;
    out = replace(&out, C_SANDWICH_RE, |m| {
        format!("{}{}{}", &m[1], &m[2], blank(&m[3], true))
    })?;
    out = blank_pattern(&out, C_AUTO_INFER_RE)?;
    out = parameterized_annotations(&out)?;
    out = type_keyword_arguments(&out)?;
    out = va_arguments(&out)?;
    out = prefixed_declarations(&out)?;
    out = rewrite_prefixed_initializers(&out)?;
    out = annotation_calls(&out, true)?;
    out = replace(&out, C_TRAILING_PARAM_ATTR_RE, |m| {
        format!("{}{}{}", &m[1], &m[2], blank(&m[3], true))
    })?;
    out = statement_macros(&out)?;
    out = lone_macro_lines(&out, false)?;
    out = leading_c_attributes(&out)?;
    if looks_like_cuda(&out) {
        out = cuda(&out)?;
    }
    out = restore_directives(&source, &out)?;
    out = out
        .split('\n')
        .map(|line| {
            replace(line, C_NAMED_VARIADIC_DEFINE_RE, |m| {
                format!("{}   ", &m[1])
            })
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    raw.restore(&unbalanced_conditionals(&out)?)
}

fn blank_pattern(source: &str, pattern: &'static str) -> Result<String> {
    replace(source, pattern, |m| blank(&m[0], true))
}

struct RawStrings<'a> {
    original: &'a str,
    masked: String,
    spans: Vec<Range<usize>>,
}
impl<'a> RawStrings<'a> {
    fn mask(source: &'a str) -> Result<Self> {
        let mut out = Self {
            original: source,
            masked: source.to_owned(),
            spans: Vec::new(),
        };
        if !source.contains("R\"") {
            return Ok(out);
        }
        let re = regex(
            r#"//[^\r\n]*|/\*[\s\S]*?(?:\*/|$)|\b(?:u8|[LuU])?R"([^ \t\x0b\x0c\r\n()\\]{0,16})\(|"(?:\\[\s\S]|[^"\\])*(?:"|$)|(?<!\w)(?:u8|[LuU])?'(?:\\[\s\S]|[^'\\])*(?:'|$)"#,
        )?;
        let mut at = 0;
        let mut spans = Vec::new();
        while let Some(m) = re.captures_from_pos(source, at)? {
            let whole = m.get(0).unwrap();
            at = whole.end();
            if let Some(delimiter) = m.get(1) {
                let closer = format!("){}\"", delimiter.as_str());
                let end = source[at..]
                    .find(&closer)
                    .map_or(source.len(), |end| at + end + closer.len());
                spans.push(whole.start()..end);
                at = end;
            }
        }
        let mut units: Vec<_> = source.encode_utf16().collect();
        for span in spans {
            let start = utf16_len(&source[..span.start]);
            let end = start + utf16_len(&source[span]);
            for ch in &mut units[start..end] {
                if *ch != 10 && *ch != 13 {
                    *ch = 0;
                }
            }
            out.spans.push(start..end);
        }
        out.masked = String::from_utf16(&units)?;
        Ok(out)
    }
    fn restore(&self, blanked: &str) -> Result<String> {
        if self.spans.is_empty() {
            return Ok(blanked.to_owned());
        }
        if blanked == self.masked {
            return Ok(self.original.to_owned());
        }
        let original: Vec<_> = self.original.encode_utf16().collect();
        let mut out: Vec<_> = blanked.encode_utf16().collect();
        // Upstream restores the original file if a rewrite changes UTF-16 length.
        if out.len() != original.len() {
            return Ok(self.original.to_owned());
        }
        for span in &self.spans {
            for i in span.clone() {
                if out[i] == 0 {
                    out[i] = original[i];
                }
            }
        }
        Ok(String::from_utf16(&out)?)
    }
}

// The mask keeps byte positions for regex spans; output blanks keep UTF-16 positions.
fn mask_literals(source: &str, tokens: bool) -> Result<String> {
    replace(source, CPP_COMMENT_OR_LITERAL_RE, |m| {
        let marker = if tokens && !m[0].starts_with('/') {
            '\u{1}'
        } else {
            ' '
        };
        m[0].bytes()
            .map(|ch| {
                if ch == b'\n' || ch == b'\r' {
                    ch as char
                } else {
                    marker
                }
            })
            .collect()
    })
}
fn mask_code(source: &str) -> Result<String> {
    replace(
        &mask_literals(source, true)?,
        r"(?m)^[ \t]*#(?:\\\r?\n|[^\r\n])*",
        |m| blank(&m[0], true),
    )
}
fn run_tokens(spans: &mut Vec<Range<usize>>, run: &str, start: usize) -> Result<()> {
    for m in captures(CPP_RUN_TOKEN_RE, run)? {
        let m = m.get(0).unwrap();
        spans.push(start + m.start()..start + m.end());
    }
    Ok(())
}
fn skip_angles(code: &str, open: usize) -> Option<usize> {
    let mut angles = 0;
    let mut parens = 0;
    for i in open..code.len().min(open + 4000) {
        match byte(code, i) {
            b'(' => parens += 1,
            b')' => {
                parens -= 1;
                if parens < 0 {
                    return None;
                }
            }
            b';' | b'{' | b'}' => return None,
            b'<' if parens == 0 => angles += 1,
            b'>' if parens == 0 && i > 0 && byte(code, i - 1) != b'-' => {
                angles -= 1;
                if angles == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}
fn export_macros(source: &str) -> Result<String> {
    if !source.contains("class") && !source.contains("struct") {
        return Ok(source.into());
    }
    let code = mask_code(source)?;
    let mut spans = Vec::new();
    for m in captures(CPP_CLASS_HEAD_MACROS_RE, &code)? {
        let mut i = skip_space(&code, m.get(0).unwrap().end());
        if byte(&code, i) == b'<' {
            let Some(end) = skip_angles(&code, i) else {
                continue;
            };
            i = skip_space(&code, end);
        }
        if test(r"^final\b", &code[i..])? {
            i = skip_space(&code, i + 5);
        }
        if byte(&code, i) != b'{' && !(byte(&code, i) == b':' && byte(&code, i + 1) != b':') {
            continue;
        }
        let run = m.get(1).unwrap();
        run_tokens(&mut spans, run.as_str(), run.start())?;
    }
    Ok(blank_spans(source, &spans, true))
}
fn pointer_annotations(source: &str) -> Result<String> {
    let code = mask_code(source)?;
    let mut spans = Vec::new();
    for m in captures(CPP_POINTER_ANNOTATION_RE, &code)? {
        let run = m.get(2).unwrap();
        let whole = m.get(0).unwrap();
        if &m[1] == "&" && byte(&code, run.end()) == b'*' {
            continue;
        }
        if test(
            r"(?:[)\]]|\b\d[\w.]*)\s*$",
            &code[prefix_start(&code, whole.start(), 40)..whole.start()],
        )? {
            continue;
        }
        run_tokens(&mut spans, run.as_str(), run.start())?;
    }
    Ok(blank_spans(source, &spans, true))
}
fn closes_parameters(code: &str, close: usize) -> Result<bool> {
    let mut depth = 0;
    let mut open = None;
    for i in (close.saturating_sub(3999)..=close).rev() {
        match byte(code, i) {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    open = Some(i);
                    break;
                }
            }
            b';' | b'{' | b'}' => return Ok(false),
            _ => {}
        }
    }
    let Some(open) = open else {
        return Ok(false);
    };
    let before = &code[prefix_start(code, open, 200)..open];
    if test(
        r"\boperator\s*(?:\(\s*\)|\[\s*\]|[^\s\w()\[\]]{1,3}|\s+[A-Za-z_][\w\s:<>*&]*)\s*$",
        before,
    )? {
        return Ok(true);
    }
    let names = captures(r"([A-Za-z_]\w*)\s*$", before)?;
    let Some(name) = names.first().map(|m| &m[1]) else {
        return Ok(false);
    };
    Ok(!CPP_PAREN_KEYWORDS.contains(&name) && !test(r"^[A-Z][A-Z0-9_]*$", name)?)
}
fn trailing_attributes(source: &str) -> Result<String> {
    let code = mask_code(source)?;
    let mut spans = Vec::new();
    let mut resume = 0;
    for m in captures(CPP_TRAILING_ATTR_CANDIDATE_RE, &code)? {
        let whole = m.get(0).unwrap();
        if whole.start() < resume || test(r"\n[ \t]*\r?\n", whole.as_str())? {
            continue;
        }
        let start = whole.end();
        let Some(run) = first_at(CPP_TRAILING_ATTR_RUN_RE, &code, start)? else {
            continue;
        };
        let run = run.get(0).unwrap();
        if first_at(CPP_TRAILING_ATTR_FOLLOW_RE, &code, run.end())?.is_none()
            || !closes_parameters(&code, whole.start())?
        {
            continue;
        }
        run_tokens(&mut spans, run.as_str(), start)?;
        resume = run.end();
    }
    Ok(blank_spans(source, &spans, true))
}
fn previous_nonspace(code: &str, before: usize) -> Option<usize> {
    code[..before]
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| i)
}
fn follows_declared_name(code: &str, last: usize) -> bool {
    let mut start = last;
    while start > 0 && word(byte(code, start - 1)) {
        start -= 1;
    }
    let name = &code[start..last + 1];
    if !name
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        || !name.bytes().any(|b| b.is_ascii_lowercase())
        || CPP_KEYWORDS.contains(&name)
    {
        return false;
    }
    let Some(i) = previous_nonspace(code, start) else {
        return false;
    };
    match byte(code, i) {
        b'*' | b'&' => return true,
        b'>' => return i == 0 || byte(code, i - 1) != b'-',
        c if !word(c) => return false,
        _ => {}
    }
    let mut j = i;
    while j > 0 && word(byte(code, j - 1)) {
        j -= 1;
    }
    let before = &code[j..i + 1];
    if matches!(before, "const" | "volatile") {
        return previous_nonspace(code, j).is_some_and(|k| matches!(byte(code, k), b'*' | b'&'));
    }
    CPP_TYPE_KEYWORDS.contains(&before)
        || (before
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
            && !CPP_KEYWORDS.contains(&before))
}
fn declarator_attributes(source: &str) -> Result<String> {
    let code = mask_code(source)?;
    let mut spans = Vec::new();
    let mut resume = 0;
    for m in captures(CPP_DECLARATOR_ATTR_CANDIDATE_RE, &code)? {
        let start = m.get(0).unwrap().start();
        if start < resume {
            continue;
        }
        let Some(last) = previous_nonspace(&code, start) else {
            continue;
        };
        if last + 1 == start || !(word(byte(&code, last)) || byte(&code, last) == b']') {
            continue;
        }
        let Some(run) = first_at(CPP_DECLARATOR_ATTR_RUN_RE, &code, start)? else {
            continue;
        };
        let run = run.get(0).unwrap();
        if !test(r"^[;,)]|^=(?!=)", &code[run.end()..])?
            || (byte(&code, last) != b']' && !follows_declared_name(&code, last))
        {
            continue;
        }
        run_tokens(&mut spans, run.as_str(), start)?;
        resume = run.end();
    }
    Ok(blank_spans(source, &spans, true))
}
fn leading_cpp_attributes(source: &str) -> Result<String> {
    let code = mask_code(source)?;
    let mut spans = Vec::new();
    for m in captures(CPP_LEADING_ATTR_RE, &code)? {
        let run = m.get(2).unwrap();
        run_tokens(&mut spans, run.as_str(), run.start())?;
    }
    Ok(blank_spans(source, &spans, true))
}
fn leading_c_attributes(source: &str) -> Result<String> {
    replace(source, C_LEADING_ATTR_MACRO_RE, |m| {
        format!("{}{}", &m[1], blank(&m[2], true))
    })
}
fn inline_annotations(source: &str) -> Result<String> {
    let raw = RawStrings::mask(source)?;
    let mut spans = Vec::new();
    let mut resume = 0;
    for m in captures(CPP_INLINE_ANNOTATION_RE, &raw.masked)? {
        let whole = m.get(0).unwrap();
        if whole.start() < resume {
            continue;
        }
        if let Some(end) = balanced_parens(&raw.masked, whole.end() - 1, false) {
            spans.push(whole.start()..end);
            resume = end;
        }
    }
    raw.restore(&blank_spans(&raw.masked, &spans, true))
}
fn annotation_calls(source: &str, keep_c_return_types: bool) -> Result<String> {
    let raw = RawStrings::mask(source)?;
    let source = &raw.masked;
    let mut spans = Vec::new();
    let mut resume = 0;
    for m in captures(r"(?m)^([ \t]*)([A-Z][A-Z0-9_]{2,})(\s*)\(", source)? {
        let whole = m.get(0).unwrap();
        if whole.start() < resume {
            continue;
        }
        let start = m.get(2).unwrap().start();
        let open = whole.end() - 1;
        let Some(end) = balanced_parens(source, open, false) else {
            continue;
        };
        let next = skip_space(source, end);
        let after = byte(source, next);
        if !after.is_ascii_alphabetic() && !matches!(after, b'_' | b'~' | b'#') {
            continue;
        }
        if keep_c_return_types
            && start == whole.start()
            && is_c_return_type(source, start, open, end, next)?
        {
            continue;
        }
        spans.push(start..end);
        resume = end;
    }
    raw.restore(&blank_spans(source, &spans, true))
}
fn is_c_return_type(
    source: &str,
    start: usize,
    open: usize,
    end: usize,
    next: usize,
) -> Result<bool> {
    if !test(C_RETURN_TYPE_ARG_RE, &source[open + 1..end - 1])? {
        return Ok(false);
    }
    let Some(head) = first_at(C_DECLARATOR_HEAD_RE, source, next)? else {
        return Ok(false);
    };
    let Some(close) = balanced_parens(source, head.get(0).unwrap().end() - 1, true) else {
        return Ok(false);
    };
    if first_at(C_DECLARATOR_TAIL_RE, source, close)?.is_none() {
        return Ok(false);
    }
    if start == 0 {
        return Ok(true);
    }
    let from = source[..start.saturating_sub(1)]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let line = source[from..start - 1]
        .split("//")
        .next()
        .unwrap_or("")
        .trim();
    Ok(line.is_empty() || line.starts_with('#') || !test(r"[\w*]$", line)?)
}
pub(super) fn prepare_objc(source: &str) -> Result<String> {
    lone_macro_lines(&rewrite_foundation(source)?, true)
}

fn lone_macro_lines(source: &str, objc: bool) -> Result<String> {
    if !test(r"(?m)^[ \t]*[A-Z][A-Z0-9_]{3,}[ \t]*\r?$", source)? {
        return Ok(source.to_owned());
    }
    let mut lines: Vec<_> = source.split('\n').map(str::to_owned).collect();
    for i in 0..lines.len() {
        let matches = captures(LONE_MACRO_LINE_RE, &lines[i])?;
        let Some(m) = matches.first() else {
            continue;
        };
        if !m[1].contains('_') {
            continue;
        }
        let prev = lines[..i]
            .iter()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map_or("", |line| line.trim());
        let previous = if test(r"^(?://|/\*|\*)", prev)? {
            String::new()
        } else {
            replace(prev, r"/\*.*?\*/|//.*$", |_| String::new())?
                .trim()
                .to_owned()
        };
        let directive_ends = previous.starts_with('#') && !previous.ends_with('\\');
        if !previous.is_empty()
            && !directive_ends
            && !test(r"^template\s*<", &previous)?
            && test(LONE_MACRO_CONTINUATION_END_RE, &previous)?
        {
            continue;
        }
        let first = first_code_character(&lines[i + 1..]);
        if first.is_some_and(|c| {
            !c.is_ascii_alphabetic()
                && !matches!(c, b'_' | b'#' | b'{' | b'}' | b'~')
                && !(objc && c == b'@')
        }) {
            continue;
        }
        if objc && previous.ends_with('{') && first == Some(b'}') {
            continue;
        }
        let span = m.get(1).unwrap();
        lines[i] = format!(
            "{}{}{}",
            &lines[i][..span.start()],
            " ".repeat(span.end() - span.start()),
            &lines[i][span.end()..]
        );
    }
    Ok(lines.join("\n"))
}
fn first_code_character(lines: &[String]) -> Option<u8> {
    let mut in_block = false;
    for line in lines {
        let mut text = line.trim();
        loop {
            if in_block {
                let Some(end) = text.find("*/") else {
                    text = "";
                    break;
                };
                text = text[end + 2..].trim();
                in_block = false;
            }
            if text.starts_with("//") {
                text = "";
            } else if let Some(rest) = text.strip_prefix("/*") {
                in_block = true;
                text = rest;
                continue;
            }
            break;
        }
        if !text.is_empty() {
            return Some(text.as_bytes()[0]);
        }
    }
    None
}
fn cuda(source: &str) -> Result<String> {
    let mut out = blank_pattern(source, CUDA_LAUNCH_BOUNDS_RE)?;
    out = blank_pattern(&out, CUDA_SPECIFIER_RE)?;
    replace(&out, CUDA_LAUNCH_CONFIG_RE, |m| {
        let mut depth = 0;
        for ch in m[0].chars() {
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth < 0 {
                    return m[0].to_owned();
                }
            }
        }
        if depth == 0 {
            blank(&m[0], false)
        } else {
            m[0].to_owned()
        }
    })
}
fn looks_like_cuda(source: &str) -> bool {
    ["__global__", "__device__", "__constant__", "cudaStream_t"]
        .iter()
        .any(|marker| source.contains(marker))
}
fn restore_directives(original: &str, blanked: &str) -> Result<String> {
    if original == blanked || !original.contains('#') {
        return Ok(blanked.to_owned());
    }
    let mut lines: Vec<_> = blanked.split('\n').map(str::to_owned).collect();
    let mut continuation = false;
    for (i, line) in original.split('\n').enumerate() {
        let directive = continuation || test(r"^[ \t]*#", line)?;
        if directive && let Some(out) = lines.get_mut(i) {
            *out = line.to_owned();
        }
        continuation = directive && test(r"\\\s*$", line.trim_end_matches('\r'))?;
    }
    Ok(lines.join("\n"))
}
fn normalize_interfaces(source: &str) -> Result<String> {
    if !source.contains("interface") {
        return Ok(source.to_owned());
    }
    let code = mask_literals(source, false)?;
    let alias = test(
        r"(?m)^[ \t]*#[ \t]*define[ \t]+interface[ \t]+struct\b",
        &code,
    )?;
    let code = replace(&code, r"(?m)^[ \t]*#(?:\\\r?\n|[^\r\n])*", |m| {
        m[0].bytes()
            .map(|c| {
                if c == b'\n' || c == b'\r' {
                    c as char
                } else {
                    '\0'
                }
            })
            .collect()
    })?;
    let re = regex(r"(?m)(^|[;{}])\s*\binterface\s+[A-Za-z_]\w*\s*(:[^;{}]+)?\{")?;
    let mut out = source.to_owned();
    let mut at = 0;
    while let Some(m) = re.captures_from_pos(&code, at)? {
        let whole = m.get(0).unwrap();
        let end = code[whole.end()..]
            .find('}')
            .map_or(code.len(), |end| whole.end() + end);
        at = whole.end() - 1;
        if !alias && m.get(2).is_none() && !test(r"\bvirtual\b", &code[whole.end()..end])? {
            continue;
        }
        let start = whole.start() + whole.as_str().find("interface").unwrap();
        out.replace_range(start..start + 9, "struct   ");
    }
    Ok(out)
}
fn rewrite_access(source: &str) -> Result<String> {
    replace(source, ACCESS_MACRO_LINE, |m| {
        let keyword = if m[2].contains("PRIVATE") {
            "private"
        } else if m[2].contains("PROTECTED") {
            "protected"
        } else if m[2].contains("PUBLIC") || matches!(&m[2], "Q_SIGNALS" | "Q_SLOTS") {
            "public"
        } else {
            return m[0].to_owned();
        };
        let available = m[2].len() + m[3].len();
        if keyword.len() > available {
            return m[0].to_owned();
        }
        format!(
            "{}{}{}:",
            &m[1],
            keyword,
            " ".repeat(available - keyword.len())
        )
    })
}
fn flatten_conditionals(source: &str) -> Result<String> {
    if !source.contains('#') {
        return Ok(source.to_owned());
    }
    let mut lines: Vec<_> = source.split('\n').map(str::to_owned).collect();
    let mut last_code = String::new();
    let mut in_block = false;
    let mut region = 0;
    let mut skip_from = 0;
    let mut continued = false;
    for line in &mut lines {
        if continued {
            continued = test(r"\\\s*$", line)?;
            continue;
        }
        let directives = captures(r"^[ \t]*#[ \t]*(\w+)", line)?;
        if let Some(directive) = directives
            .first()
            .filter(|_| !in_block)
            .map(|m| m[1].to_owned())
        {
            if test(r"\\\s*$", line)? {
                continued = true;
                if region == 0 {
                    continue;
                }
            }
            let opens = directive.starts_with("if");
            if region == 0 {
                let mid = opens
                    && !last_code.is_empty()
                    && !test(r"[;{}]$|^[\w\s]*:$|\*/$", &last_code)?
                    && !test(r"^(?:public|private|protected)\s*:$", &last_code)?
                    && !test(r"^[A-Z][A-Z0-9_]*(?:\s*\(.*\))?$", &last_code)?;
                if !mid {
                    continue;
                }
                region = 1;
                *line = blank(line, true);
                continue;
            }
            if opens {
                region += 1;
            } else if matches!(directive.as_str(), "else" | "elif") && skip_from == 0 {
                skip_from = region;
            } else if directive == "endif" {
                if skip_from == region {
                    skip_from = 0;
                }
                region -= 1;
            }
            if matches!(
                directive.as_str(),
                "if" | "ifdef" | "ifndef" | "else" | "elif" | "elifdef" | "elifndef" | "endif"
            ) {
                *line = blank(line, true);
            }
            continue;
        }
        if region > 0 && skip_from > 0 {
            if !line.trim().is_empty() {
                *line = blank(line, true);
            }
            continue;
        }
        let no_line_comment = line.split("//").next().unwrap_or("");
        let trimmed = replace(no_line_comment, r"/\*.*?\*/", |_| String::new())?
            .trim()
            .to_owned();
        if in_block {
            if line.contains("*/") {
                in_block = false;
            }
            continue;
        }
        if test(r"/\*(?!.*\*/)", no_line_comment)? {
            in_block = true;
            let before = trimmed.split("/*").next().unwrap_or("").trim();
            if !before.is_empty() {
                last_code = before.to_owned();
            }
            continue;
        }
        if !trimmed.is_empty() {
            last_code = trimmed;
        }
    }
    Ok(lines.join("\n"))
}
fn rewrite_foundation(source: &str) -> Result<String> {
    if !source.contains("_ENUM") && !source.contains("_OPTIONS") {
        return Ok(source.to_owned());
    }
    let code = mask_code(source)?;
    let mut spans = Vec::new();
    let mut replacements = Vec::new();
    for m in captures(FOUNDATION_ENUM_MACRO_RE, &code)? {
        if let Some(typedef) = m.get(1) {
            spans.push(typedef.range());
        }
        let name = m.get(3).unwrap();
        replacements.push(name.start());
        spans.push(name.start() + 4..name.end());
        let first = m.get(5).unwrap();
        spans.push(first.start() - 1..first.end() + 1);
        let end = m.get(0).unwrap().end();
        spans.push(end - 1..end);
    }
    let mut changed = source.to_owned();
    for start in replacements {
        changed.replace_range(start..start + 4, "enum");
    }
    Ok(blank_spans(&changed, &spans, true))
}

fn cplusplus_guards(source: &str) -> Result<String> {
    if !source.contains("__cplusplus") {
        return Ok(source.to_owned());
    }
    let mut lines: Vec<_> = source.split('\n').map(str::to_owned).collect();
    let mut i = 0;
    while i < lines.len() {
        if !test(C_CPLUSPLUS_GUARD_OPEN_RE, lines[i].trim_end_matches('\r'))? {
            i += 1;
            continue;
        }
        let mut end = None;
        for (j, line) in lines
            .iter()
            .enumerate()
            .take((i + 42).min(lines.len()))
            .skip(i + 1)
        {
            if test(C_PREPROC_ENDIF_RE, line.trim_end_matches('\r'))? {
                end = Some(j);
                break;
            }
            if test(C_PREPROC_DIRECTIVE_RE, line.trim_end_matches('\r'))? {
                break;
            }
        }
        if let Some(end) = end {
            for line in &mut lines[i + 1..end] {
                *line = blank(line, true);
            }
            i = end;
        }
        i += 1;
    }
    Ok(lines.join("\n"))
}
fn kernel_annotations(source: &str) -> Result<String> {
    if !source.contains("__") || !test(C_KERNEL_ANNOTATION_RE, source)? {
        return Ok(source.to_owned());
    }
    let out = blank_pattern(source, C_KERNEL_ANNOTATION_RE)?;
    replace(
        &out,
        r"(\bcontainer_of\s*\([^;()]*?,\s*)(struct|union)(\s+)",
        |m| format!("{}{}{}", &m[1], blank(&m[2], true), &m[3]),
    )
}
fn designated_arguments(source: &str) -> Result<String> {
    if !source.contains('=') {
        return Ok(source.to_owned());
    }
    let raw = RawStrings::mask(source)?;
    let source = &raw.masked;
    let mut spans = Vec::new();
    let mut resume = 0;
    for m in captures(r"(?m)^[ \t]*([A-Z_][A-Z0-9_]*)\s*\(", source)? {
        let whole = m.get(0).unwrap();
        if whole.start() < resume {
            continue;
        }
        let open = whole.end() - 1;
        let mut depth = 1;
        let mut i = open + 1;
        while i < source.len() && depth > 0 {
            match byte(source, i) {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        if depth != 0 {
            continue;
        }
        let close = i - 1;
        if test(
            r"(^|[,{(\s])(\.[A-Za-z_]\w*|\[[^\]]+\])\s*=[^=]",
            &source[open + 1..close],
        )? && test(r"^\s*;", &source[close + 1..])?
        {
            spans.push(open + 1..close);
            resume = close;
        }
    }
    raw.restore(&blank_spans(source, &spans, false))
}
fn parameterized_annotations(source: &str) -> Result<String> {
    if !source.contains("__") {
        return Ok(source.to_owned());
    }
    let raw = RawStrings::mask(source)?;
    let source = &raw.masked;
    let mut spans = Vec::new();
    for m in captures(C_PARAM_ANNOTATION_RE, source)? {
        let whole = m.get(0).unwrap();
        let start = whole.start();
        let mut end = whole.end();
        let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
        if test(r"^[ \t]*$", &source[line_start..start])?
            && let Some(after) = first_at(r"[ \t]*;", source, end)?
        {
            end = after.get(0).unwrap().end();
        }
        spans.push(start..end);
    }
    raw.restore(&blank_spans(source, &spans, true))
}
fn type_keyword_arguments(source: &str) -> Result<String> {
    if !test(r"\b(?:struct|union|enum)[ \t\r\n]", source)? {
        return Ok(source.to_owned());
    }
    let raw = RawStrings::mask(source)?;
    let source = &raw.masked;
    let mut spans = Vec::new();
    for m in captures(C_TYPE_ARG_HEAD_RE, source)? {
        if C_TYPE_ARG_HEAD_EXCLUSIONS.contains(&&m[1]) {
            continue;
        }
        let whole = m.get(0).unwrap();
        let mut j = whole.start();
        while j > 0 && matches!(byte(source, j - 1), b' ' | b'\t') {
            j -= 1;
        }
        if j > 0 && (word(byte(source, j - 1)) || byte(source, j - 1) == b'*') {
            let mut w = j;
            while w > 0 && word(byte(source, w - 1)) {
                w -= 1;
            }
            if &source[w..j] != "return" {
                continue;
            }
        }
        let open = whole.end() - 1;
        let mut depth = 1;
        let mut argument_start = true;
        let mut k = open + 1;
        while k < source.len() && k - open <= 600 {
            let ch = byte(source, k);
            if ch == b'\'' || ch == b'"' {
                k += 1;
                while k < source.len() && byte(source, k) != ch {
                    if byte(source, k) == b'\\' {
                        k += 1;
                    }
                    k += 1;
                }
                argument_start = false;
                k += 1;
                continue;
            }
            match ch {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    k += 1;
                    continue;
                }
                b'(' => {
                    depth += 1;
                    argument_start = false;
                    k += 1;
                    continue;
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    argument_start = false;
                    k += 1;
                    continue;
                }
                b';' | b'{' | b'}' => break,
                b',' => {
                    if depth == 1 {
                        argument_start = true;
                    }
                    k += 1;
                    continue;
                }
                _ => {}
            }
            if depth == 1 && argument_start && source.is_char_boundary(k) {
                let mut end = (k + 160).min(source.len());
                while !source.is_char_boundary(end) {
                    end -= 1;
                }
                let found = captures(C_TYPE_ARG_OPENER_RE, &source[k..end])?;
                if let Some(opener) = found.first() {
                    let keyword = opener.get(1).unwrap();
                    spans.push(k + keyword.start()..k + keyword.end());
                    if let Some(tail) = opener.get(4) {
                        let stars = tail
                            .as_str()
                            .bytes()
                            .rev()
                            .take_while(|ch| *ch == b'*')
                            .count();
                        spans.push(k + tail.end() - stars..k + tail.end());
                    }
                    k += opener.get(0).unwrap().end();
                    argument_start = false;
                    continue;
                }
            }
            argument_start = false;
            k += 1;
        }
    }
    raw.restore(&blank_spans(source, &spans, true))
}
fn va_arguments(source: &str) -> Result<String> {
    let mut out = String::new();
    let mut at = 0;
    for m in captures(C_VA_ARG_RE, source)? {
        let whole = m.get(0).unwrap();
        out.push_str(&source[at..whole.start()]);
        if test(r"^,[ \t]*[A-Za-z_]\w*[ \t]*$", &m[2])? {
            out.push_str(whole.as_str());
        } else {
            out.push_str(&format!("va_arg({}{})", &m[1], blank(&m[2], true)));
        }
        at = whole.end();
    }
    out.push_str(&source[at..]);
    Ok(out)
}
fn prefixed_declarations(source: &str) -> Result<String> {
    let raw = RawStrings::mask(source)?;
    let mut out = Vec::new();
    for line in raw.masked.split('\n') {
        let matches = captures(C_PREFIXED_DECL_MACRO_RE, line)?;
        let should_blank = matches
            .first()
            .and_then(|m| balanced_parens(line, m.get(0).unwrap().end() - 1, false))
            .is_some_and(|end| line[end..].trim() == ";");
        out.push(if should_blank {
            blank(line, true)
        } else {
            line.to_owned()
        });
    }
    raw.restore(&out.join("\n"))
}
fn rewrite_prefixed_initializers(source: &str) -> Result<String> {
    let mut out = Vec::new();
    for line in source.split('\n') {
        let matches = captures(C_PREFIXED_DECL_MACRO_INIT_RE, line)?;
        let Some(m) = matches.first() else {
            out.push(line.to_owned());
            continue;
        };
        let argument = replace(m[2].trim(), r"[ \t]+", |_| " ".into())?;
        if !test(r"^[A-Za-z_][\w \t*]*$", &argument)? {
            out.push(line.to_owned());
            continue;
        }
        let name = m.get(3).unwrap();
        let tail = m.get(4).unwrap();
        let prefix = format!("{}static {argument}", &m[1]);
        if prefix.len() + 1 > name.start() {
            out.push(line.to_owned());
            continue;
        }
        out.push(format!(
            "{}{}{}{}{}",
            prefix,
            " ".repeat(name.start() - prefix.len()),
            name.as_str(),
            " ".repeat(tail.start() - name.end()),
            &line[tail.start()..]
        ));
    }
    Ok(out.join("\n"))
}
fn statement_follows(ch: u8) -> bool {
    matches!(ch, b';' | b'{' | b'}' | b':' | b')')
}
fn statement_lines(code: &str) -> Result<Vec<bool>> {
    const BLOCK: isize = -1;
    const OTHER_BRACES: isize = -2;
    let mut statements = vec![false];
    let mut open = Vec::<isize>::new();
    let mut line = 0;
    let mut line_start = true;
    let mut previous = None;
    let mut after_control = false;
    for (i, ch) in code.bytes().enumerate() {
        if ch == b'\n' {
            line += 1;
            statements.push(false);
            line_start = true;
            continue;
        }
        if matches!(ch, b' ' | b'\t' | b'\r') {
            continue;
        }
        let top = open.last().copied();
        let prev = previous.map_or(0, |at| byte(code, at));
        if line_start {
            line_start = false;
            statements[line] = top == Some(BLOCK) && !after_control && statement_follows(prev);
        }
        after_control = false;
        match ch {
            b'(' | b'[' => open.push(i as isize),
            b')' | b']' => {
                if let Some(top) = top.filter(|top| *top >= 0) {
                    open.pop();
                    let top = top as usize;
                    after_control = test(
                        r"\b(?:if|for|while|switch)\s*$",
                        &code[prefix_start(code, top, 10)..top],
                    )?;
                }
            }
            b'{' => {
                let after_else = if let Some(previous) = previous {
                    test(
                        r"(?:^|\W)(?:else|do)$",
                        &code[prefix_start(code, previous, 4)..previous + 1],
                    )?
                } else {
                    false
                };
                let block = prev == b'('
                    || prev == b')' && top.is_none()
                    || top == Some(BLOCK) && (statement_follows(prev) || after_else);
                open.push(if block { BLOCK } else { OTHER_BRACES });
            }
            b'}' => while open.pop().is_some_and(|entry| entry >= 0) {},
            _ => {}
        }
        previous = Some(i);
    }
    Ok(statements)
}
fn statement_macros(source: &str) -> Result<String> {
    let raw = RawStrings::mask(source)?;
    let source = &raw.masked;
    let mut lines: Vec<_> = source.split('\n').map(str::to_owned).collect();
    let mut statement_lines_cache = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].clone();
        let matches = captures(r"^[ \t]+([a-z_][a-z0-9_]*|[A-Z_][A-Z0-9_]*)[ \t]*\(", &line)?;
        let Some(m) = matches
            .first()
            .filter(|m| !C_STMT_MACRO_KEYWORDS.contains(&&m[1]))
        else {
            i += 1;
            continue;
        };
        let open = m.get(0).unwrap().end() - 1;
        let mut depth = 0;
        let mut close = None;
        let mut k = open;
        while k < line.len() {
            let ch = byte(&line, k);
            if matches!(ch, b'\'' | b'"') {
                k += 1;
                while k < line.len() && byte(&line, k) != ch {
                    if byte(&line, k) == b'\\' {
                        k += 1;
                    }
                    k += 1;
                }
            } else if ch == b'(' {
                depth += 1;
            } else if ch == b')' {
                depth -= 1;
                if depth == 0 {
                    close = Some(k);
                    break;
                }
            }
            k += 1;
        }
        let mut end_line = i;
        if close.is_none() {
            if line.contains(';') {
                i += 1;
                continue;
            }
            for (j, continuation) in lines
                .iter()
                .enumerate()
                .take((i + 6).min(lines.len()))
                .skip(i + 1)
            {
                let mut bail = false;
                let mut k = 0;
                while k < continuation.len() {
                    let ch = byte(continuation, k);
                    if matches!(ch, b'\'' | b'"') {
                        k += 1;
                        while k < continuation.len() && byte(continuation, k) != ch {
                            if byte(continuation, k) == b'\\' {
                                k += 1;
                            }
                            k += 1;
                        }
                    } else if matches!(ch, b';' | b'{' | b'}') {
                        bail = true;
                        break;
                    } else if ch == b'(' {
                        depth += 1;
                    } else if ch == b')' {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(k);
                            end_line = j;
                            break;
                        }
                    }
                    k += 1;
                }
                if bail || close.is_some() {
                    break;
                }
            }
        }
        let Some(close) = close else {
            i += 1;
            continue;
        };
        let end_text = lines[end_line].clone();
        let after = end_text[close + 1..].trim();
        if after.is_empty() {
            let first = lines[end_line + 1..]
                .iter()
                .find(|line| !line.trim().is_empty())
                .and_then(|line| line.trim().bytes().next());
            if !first.is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c == b'{') {
                i += 1;
                continue;
            }
        } else if after != "{" {
            i += 1;
            continue;
        }
        let ident_start = m.get(1).unwrap().start();
        if statement_lines_cache.is_none() {
            statement_lines_cache = Some(statement_lines(&mask_code(source)?)?);
        }
        let kept = if statement_lines_cache
            .as_ref()
            .unwrap()
            .get(i)
            .copied()
            .unwrap_or(false)
        {
            format!("{};", &m[1])
        } else {
            String::new()
        };
        if end_line == i {
            let width = utf16_len(&line[ident_start..close + 1]);
            lines[i] = format!(
                "{}{}{}{}",
                &line[..ident_start],
                kept,
                " ".repeat(width.saturating_sub(kept.len())),
                &line[close + 1..]
            );
        } else {
            lines[i] = format!(
                "{}{}{}",
                &line[..ident_start],
                kept,
                blank(&line[ident_start + kept.len()..], true)
            );
            for line in &mut lines[i + 1..end_line] {
                *line = blank(line, true);
            }
            lines[end_line] = format!(
                "{}{}",
                blank(&end_text[..close + 1], true),
                &end_text[close + 1..]
            );
            i = end_line;
        }
        i += 1;
    }
    raw.restore(&lines.join("\n"))
}
fn unbalanced_conditionals(source: &str) -> Result<String> {
    if !source.contains('#') {
        return Ok(source.to_owned());
    }
    let mut lines: Vec<_> = source.split('\n').map(str::to_owned).collect();
    let mut masked: Vec<_> = mask_literals(source, false)?
        .split('\n')
        .map(str::to_owned)
        .collect();
    struct Conditional {
        marks: Vec<usize>,
        dead: bool,
        literal: bool,
        positive: bool,
        else_at: Option<usize>,
    }
    let mut stack = Vec::<Conditional>::new();
    let mut directives = vec![false; lines.len()];
    let mut continued = vec![false; lines.len()];
    let mut continuation = false;
    for i in 0..lines.len() {
        let line = masked[i].trim_end_matches('\r');
        let directive = continuation || test(r"^[ \t]*#", line)?;
        directives[i] = directive;
        continued[i] = continuation;
        let was_continuation = continuation;
        continuation = directive && test(r"\\\s*$", lines[i].trim_end_matches('\r'))?;
        if !directive || was_continuation {
            continue;
        }
        let opens = captures(C_COND_OPEN_RE, line)?;
        if let Some(m) = opens.first() {
            let condition = m[2].trim();
            let dead = &m[1] == "if" && test(r"^0\b", condition)?;
            let live = &m[1] == "if" && test(r"^1\b", condition)?;
            let positive =
                &m[1] == "ifdef" || &m[1] == "if" && !dead && !live && !condition.starts_with('!');
            stack.push(Conditional {
                marks: vec![i],
                dead,
                literal: dead || live,
                positive,
                else_at: None,
            });
            continue;
        }
        let Some(top) = stack.last_mut() else {
            continue;
        };
        if test(C_COND_NEXT_RE, line)? {
            if test(r"^[ \t]*#[ \t]*else\b", line)? {
                top.else_at = Some(top.marks.len());
            }
            top.marks.push(i);
            continue;
        }
        if !test(C_COND_ENDIF_RE, line)? {
            continue;
        }
        let mut top = stack.pop().unwrap();
        top.marks.push(i);
        let keep = if top.dead {
            1
        } else if top.positive {
            top.else_at.unwrap_or(0)
        } else {
            0
        };
        if keep >= top.marks.len() - 1 {
            continue;
        }
        let mut damaged = false;
        let mut balanced = true;
        for bounds in top.marks.windows(2) {
            let body: Vec<_> = (bounds[0] + 1..bounds[1])
                .filter(|k| !directives[*k])
                .map(|k| masked[k].trim())
                .filter(|line| !line.is_empty())
                .collect();
            let (Some(first), Some(last)) = (body.first(), body.last()) else {
                continue;
            };
            let mut depth = 0;
            let mut parens = 0;
            let mut dips = false;
            for line in &body {
                for ch in line.bytes() {
                    match ch {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        b'(' => parens += 1,
                        b')' => parens -= 1,
                        _ => {}
                    }
                    if depth < 0 {
                        dips = true;
                    }
                }
            }
            if depth != 0 || parens != 0 {
                balanced = false;
            }
            damaged |= depth != 0
                || dips
                || test(r"^else\b", first)?
                || test(r"^(\|\||&&|,|\)|:|\?)", first)?
                || test(r"(\|\||&&|,|\(|\?|:)$", last)?
                || test(r"^(else[ \t]+)?if[ \t]*\(", last)? && last.ends_with(')');
        }
        if !damaged {
            continue;
        }
        let keep_all = balanced && !top.literal;
        for (b, bounds) in top.marks.windows(2).enumerate() {
            let from = bounds[0];
            let to = bounds[1];
            let mut blank_line = |k: usize| {
                lines[k] = blank(&lines[k], true);
                masked[k] = blank(&masked[k], true);
            };
            for (k, continuation) in continued.iter().enumerate().take(to).skip(from) {
                if k != from && !*continuation {
                    break;
                }
                blank_line(k);
            }
            if b != keep && !keep_all {
                for k in from + 1..to {
                    blank_line(k);
                }
            }
        }
        lines[i] = blank(&lines[i], true);
        masked[i] = blank(&masked[i], true);
    }
    Ok(lines.join("\n"))
}
