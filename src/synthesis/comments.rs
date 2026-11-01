//! Offset-preserving language comment masks from the reference synthesis helpers.

/// Blank string contents while leaving quotes and recognized regex literals intact.
pub(crate) fn blank_string_contents(source: &str) -> anyhow::Result<String> {
    let regex_start = super::regex(
        r"(?:^|[=(:,)!&|?;{}\[\]+*%~^<>-]|\b(?:return|throw|case|yield|await|else|do|typeof|void|delete|new|in|of|instanceof))\s*$",
    )?;
    let bytes = source.as_bytes();
    let mut output = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' {
            let mut units = 0;
            let start = source[..i]
                .char_indices()
                .rev()
                .find_map(|(at, ch)| {
                    units += ch.len_utf16();
                    (units >= 32).then_some(at)
                })
                .unwrap_or(0);
            if regex_start.is_match(&source[start..i])? {
                let mut end = i + 1;
                let mut class = false;
                while end < bytes.len() && bytes[end] != b'\n' {
                    if bytes[end] == b'\\' {
                        end += 2;
                        continue;
                    }
                    if bytes[end] == b'[' {
                        class = true;
                    }
                    if bytes[end] == b']' {
                        class = false;
                    }
                    if bytes[end] == b'/' && !class {
                        break;
                    }
                    end += 1;
                }
                if bytes.get(end) == Some(&b'/') {
                    i = end + 1;
                    continue;
                }
            }
        }
        if matches!(bytes[i], b'\'' | b'"' | b'`') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    output[i] = b' ';
                    output[i + 1] = b' ';
                    i += 2;
                    continue;
                }
                if quote != b'`' && bytes[i] == b'\n' {
                    break;
                }
                if bytes[i] != b'\n' {
                    output[i] = b' ';
                }
                i += 1;
            }
            if bytes.get(i) == Some(&quote) {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    Ok(String::from_utf8(output).expect("string masking retains UTF-8 outside blanked spans"))
}

pub(crate) fn strip_for_regex(source: &str, language: &str) -> String {
    match language {
        "python" => strip(source, Language::Python),
        "ruby" => strip(source, Language::Ruby),
        "php" => strip(source, Language::Php),
        "erlang" => strip(source, Language::Erlang),
        "go" | "rust" => strip_go_rust(source, language == "rust"),
        "javascript" | "typescript" => super::strip_cstyle(source, true),
        "java" | "csharp" | "swift" | "c" | "cpp" => super::strip_cstyle(source, false),
        _ => source.to_owned(),
    }
}

fn strip_go_rust(source: &str, rust: bool) -> String {
    let bytes = source.as_bytes();
    let mut output = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"/*") {
            let start = i;
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if rust && bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            blank(&mut output, start, i);
        } else if bytes[i..].starts_with(b"//") {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            blank(&mut output, start, i);
        } else if !rust && bytes[i] == b'`' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'`' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else if matches!(bytes[i], b'\'' | b'"') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if (!rust || quote == b'\'') && bytes[i] == b'\n' {
                    break;
                }
                i += 1;
            }
            if bytes.get(i) == Some(&quote) {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8(output).expect("comment masks retain UTF-8 outside blanked spans")
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Language {
    Python,
    Ruby,
    Php,
    Erlang,
}

pub(super) fn strip(source: &str, language: Language) -> String {
    let bytes = source.as_bytes();
    let mut output = bytes.to_vec();
    let mut i = 0;
    let mut line_start = true;
    while i < bytes.len() {
        let c = bytes[i];
        if language == Language::Python
            && matches!(c, b'\'' | b'"')
            && bytes.get(i + 1) == Some(&c)
            && bytes.get(i + 2) == Some(&c)
        {
            let start = i;
            i += 3;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else if bytes[i] == c
                    && bytes.get(i + 1) == Some(&c)
                    && bytes.get(i + 2) == Some(&c)
                {
                    i += 3;
                    break;
                } else {
                    i += 1;
                }
            }
            blank(&mut output, start, i);
            continue;
        }
        if language == Language::Ruby && line_start && bytes[i..].starts_with(b"=begin") {
            let start = i;
            i += 6;
            while i < bytes.len() {
                if bytes[i] == b'\n' {
                    let mut j = i + 1;
                    while matches!(bytes.get(j), Some(b' ' | b'\t')) {
                        j += 1;
                    }
                    if bytes[j..].starts_with(b"=end") {
                        i = j + 4;
                        while i < bytes.len() && bytes[i] != b'\n' {
                            i += 1;
                        }
                        break;
                    }
                }
                i += 1;
            }
            blank(&mut output, start, i);
            line_start = i > 0 && bytes[i - 1] == b'\n';
            continue;
        }
        if language == Language::Php && bytes[i..].starts_with(b"/*") {
            let start = i;
            i += 2;
            while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            blank(&mut output, start, i);
            continue;
        }
        if matches!(c, b'\'' | b'"') || language == Language::Php && c == b'`' {
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if language != Language::Erlang && bytes[i] == b'\n' {
                    break;
                }
                i += 1;
            }
            if bytes.get(i) == Some(&c) {
                i += 1;
            }
            line_start = false;
            continue;
        }
        if language == Language::Erlang && c == b'$' {
            i += 1;
            if bytes.get(i) == Some(&b'\\') {
                i += 1;
            }
            // The reference advances one UTF-16 code unit here. Any non-ASCII
            // continuation bytes cannot start a comment, so scanning them is inert.
            i += 1;
            continue;
        }
        let comment = if language == Language::Erlang {
            c == b'%'
        } else {
            c == b'#' || language == Language::Php && bytes[i..].starts_with(b"//")
        };
        if comment {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            blank(&mut output, start, i);
            line_start = false;
            continue;
        }
        if c == b'\n' {
            line_start = true;
        } else if c != b' ' && c != b'\t' {
            line_start = false;
        }
        i += 1;
    }
    String::from_utf8(output).expect("comment masks retain UTF-8 outside blanked spans")
}

fn blank(output: &mut [u8], start: usize, end: usize) {
    for byte in &mut output[start..end] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}
