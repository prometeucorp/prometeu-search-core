use super::*;
#[derive(Clone)]
pub(super) struct Definition {
    params: Vec<String>,
    expansion: String,
}
#[derive(Clone, Default)]
pub(super) struct Environment {
    pub functions: HashMap<String, Definition>,
    pub objects: HashMap<String, String>,
    pub defined: HashSet<String>,
}
struct Patterns {
    function: fancy_regex::Regex,
    object: fancy_regex::Regex,
    defined: fancy_regex::Regex,
    ifdef: fancy_regex::Regex,
    ifndef: fancy_regex::Regex,
    if_expr: fancy_regex::Regex,
    branch: fancy_regex::Regex,
    endif: fancy_regex::Regex,
    condition: fancy_regex::Regex,
    negative: fancy_regex::Regex,
    alias: fancy_regex::Regex,
    tokens: fancy_regex::Regex,
    call: fancy_regex::Regex,
}
fn patterns() -> &'static Patterns {
    static PATTERNS: std::sync::LazyLock<Patterns> = std::sync::LazyLock::new(|| {
        let r = |p| regex(p).expect("valid reference pattern");
        Patterns {
            function: r(r"(?m)^[ \t]*#[ \t]*define[ \t]+(\w+)\(([^)]*)\)\s+(.+)$"),
            object: r(r"(?m)^[ \t]*#[ \t]*define[ \t]+(\w+)[ \t]+(\S[^\n]*)$"),
            defined: r(r"(?m)^[ \t]*#[ \t]*define[ \t]+(\w+)"),
            ifdef: r(r"^#\s*ifdef\s+(\w+)"),
            ifndef: r(r"^#\s*ifndef\s+(\w+)"),
            if_expr: r(r"^#\s*if\s+(.+)$"),
            branch: r(r"^#\s*(?:elif|else)\b"),
            endif: r(r"^#\s*endif\b"),
            condition: r(r"^defined\s*\(?\s*(\w+)\s*\)?$"),
            negative: r(r"^!\s*defined\s*\(?\s*(\w+)\s*\)?$"),
            alias: r(r"^(?:(?:struct|union)\s+)?(\w+)$"),
            tokens: r(r"\b\w+\b"),
            call: r(r"\b(\w+)\s*\("),
        }
    });
    &PATTERNS
}
pub(super) fn parse(source: &str) -> Result<Environment> {
    let mut env = Environment::default();
    if !source.contains("#define") && !source.contains("# define") {
        return Ok(env);
    }
    let joined = source.replace("\\\r\n", " ").replace("\\\n", " ");
    for m in patterns().function.captures_iter(&joined) {
        let m = m?;
        let params: Vec<_> = m
            .get(2)
            .unwrap()
            .as_str()
            .split(',')
            .map(trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect();
        if params.iter().any(|p| p.ends_with("...")) {
            continue;
        }
        env.functions.insert(
            m.get(1).unwrap().as_str().to_owned(),
            Definition {
                params,
                expansion: trim(m.get(3).unwrap().as_str()).to_owned(),
            },
        );
    }
    for m in patterns().object.captures_iter(&joined) {
        let m = m?;
        env.objects.insert(
            m.get(1).unwrap().as_str().to_owned(),
            trim(m.get(2).unwrap().as_str()).to_owned(),
        );
    }
    for m in patterns().defined.captures_iter(source) {
        env.defined.insert(m?.get(1).unwrap().as_str().to_owned());
    }
    Ok(env)
}
pub(super) fn conditionals(text: &str, defined: &HashSet<String>) -> Result<String> {
    let mut output = String::with_capacity(text.len());
    let mut stack: Vec<(bool, bool, bool)> = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            output.push('\n')
        }
        let t = trim(line);
        let active = stack.last().is_none_or(|frame| frame.1);
        if let Some(m) = patterns().ifdef.captures(t)? {
            let condition = defined.contains(m.get(1).unwrap().as_str());
            stack.push((active, active && condition, condition));
            continue;
        }
        if let Some(m) = patterns().ifndef.captures(t)? {
            let condition = !defined.contains(m.get(1).unwrap().as_str());
            stack.push((active, active && condition, condition));
            continue;
        }
        if let Some(m) = patterns().if_expr.captures(t)? {
            let expr = trim(m.get(1).unwrap().as_str());
            let condition = if let Some(m) = patterns().condition.captures(expr)? {
                defined.contains(m.get(1).unwrap().as_str())
            } else if let Some(m) = patterns().negative.captures(expr)? {
                !defined.contains(m.get(1).unwrap().as_str())
            } else {
                true
            };
            stack.push((active, active && condition, condition));
            continue;
        }
        if patterns().branch.is_match(t)? {
            if let Some(frame) = stack.last_mut() {
                frame.1 = frame.0 && !frame.2;
                frame.2 = true;
            }
            continue;
        }
        if patterns().endif.is_match(t)? {
            stack.pop();
            continue;
        }
        if active {
            output.push_str(line)
        }
    }
    Ok(output)
}
pub(super) fn type_name(name: &str, env: &Environment) -> Result<String> {
    let mut name = name.to_owned();
    for _ in 0..5 {
        let Some(value) = env.objects.get(&name) else {
            break;
        };
        let Some(m) = patterns().alias.captures(trim(value))? else {
            break;
        };
        name = m.get(1).unwrap().as_str().to_owned();
    }
    Ok(name)
}
pub(super) fn expand(text: &str, env: &Environment, ctx: &Context<'_>) -> Result<String> {
    let mut output = text.to_owned();
    if env.functions.is_empty() {
        return Ok(output);
    }
    for _ in 0..6 {
        ctx.steps(output.len() / 32 + 1)?;
        let mut replacement = None;
        for m in patterns().call.captures_iter(&output) {
            let m = m?;
            let Some(def) = env.functions.get(m.get(1).unwrap().as_str()) else {
                continue;
            };
            let open = m.get(0).unwrap().end() - 1;
            let Some(close) = matching(&output, open, b'(', b')') else {
                continue;
            };
            let args: Vec<_> = split(&output[open + 1..close], b',')
                .into_iter()
                .map(trim)
                .collect();
            let mut expansion = String::new();
            let mut start = 0;
            for token in patterns().tokens.find_iter(&def.expansion) {
                let token = token?;
                expansion.push_str(&def.expansion[start..token.start()]);
                if let Some(i) = def.params.iter().position(|p| p == token.as_str()) {
                    expansion.push_str(args.get(i).copied().unwrap_or(""))
                } else {
                    expansion.push_str(token.as_str())
                }
                start = token.end();
                ensure!(
                    expansion.len() <= 8 * 1024 * 1024,
                    "C macro expansion exceeds limit"
                );
            }
            expansion.push_str(&def.expansion[start..]);
            replacement = Some((m.get(0).unwrap().start(), close + 1, expansion));
            break;
        }
        let Some((start, end, expansion)) = replacement else {
            break;
        };
        ensure!(
            output.len() - end + start + expansion.len() <= 8 * 1024 * 1024,
            "C macro expansion exceeds limit"
        );
        output.replace_range(start..end, &expansion);
    }
    Ok(output)
}
