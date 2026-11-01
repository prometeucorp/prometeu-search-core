//! Native effect classification and response status inference.
//! SPDX-License-Identifier: MIT. See crates/prometeu-extract/UPSTREAM.md.
use super::*;
use fancy_regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

struct Pattern(Regex);
impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Source {
            source: String,
            flags: String,
        }
        let source = Source::deserialize(deserializer)?;
        let pattern = format!(
            "{}{}",
            if source.flags.contains('i') {
                "(?i)"
            } else {
                ""
            },
            crate::parser::javascript_regex::pattern(&source.source)
        );
        Regex::new(&pattern)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}
impl Pattern {
    fn matches(&self, text: &str) -> bool {
        self.0.is_match(text).unwrap_or(false)
    }
}
#[derive(Deserialize)]
struct Rule {
    category: String,
    test: Pattern,
    #[serde(default)]
    only: Vec<String>,
    #[serde(default)]
    instantiates: bool,
}
#[derive(Deserialize)]
struct ReceiverRule {
    category: String,
    test: Pattern,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rules {
    rules: Vec<Rule>,
    families: HashMap<String, Vec<String>>,
    receiver_types: Vec<ReceiverRule>,
    read: Pattern,
    write: Pattern,
    model_suffix: Pattern,
    status_names: BTreeMap<String, u16>,
    exception: Pattern,
    not_response: Pattern,
    swift_not_effect: Pattern,
}
fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        serde_json::from_str(include_str!("effect-rules.json")).expect("embedded effect rules")
    })
}
fn regex(pattern: &'static str) -> Arc<Regex> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<Regex>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    cache
        .entry(pattern)
        .or_insert_with(|| {
            Arc::new(crate::frameworks::source_pattern(pattern).expect("constant effect pattern"))
        })
        .clone()
}
fn matches(pattern: &'static str, text: &str) -> bool {
    regex(pattern).is_match(text).unwrap_or(false)
}
fn replace(pattern: &'static str, text: &str, replacement: &str) -> String {
    regex(pattern).replace_all(text, replacement).into_owned()
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct Input {
    pub text: String,
    pub kind: String,
    pub language: Option<String>,
    pub project: Option<String>,
    pub receiver_type: Option<String>,
    pub args: Option<String>,
    #[serde(default)]
    pub project_type: bool,
}
#[derive(Debug, Serialize, PartialEq)]
pub(super) struct Effect {
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<String>,
}
fn effect(category: &str) -> Effect {
    Effect {
        category: category.into(),
        model: None,
        access: None,
    }
}
pub(super) fn normalise(text: &str) -> String {
    let text = replace(r"^\s*(?:await|new|yield|return|throw)\s+", text, "");
    let text = replace(r"^\s*(?:await|new)\s+", &text, "");
    let text = text.replace("?.", ".").replace("!.", ".");
    let text = replace(r"\s+", &text, "");
    replace(r"<[^<>]*>", &text, "")
}
pub(super) fn classify(input: &Input) -> Option<Effect> {
    let text = replace(r"\([^()]*\)", &normalise(&input.text), "");
    if text.is_empty() {
        return None;
    }
    let rules = rules();
    let family = input.language.as_deref().and_then(|language| {
        rules
            .families
            .iter()
            .find(|(_, languages)| languages.iter().any(|item| item == language))
            .map(|(name, _)| name.as_str())
    });
    if family == Some("swift") && rules.swift_not_effect.matches(&text) {
        return None;
    }
    for rule in &rules.rules {
        if input.project_type && !matches!(rule.category.as_str(), "database" | "response") {
            continue;
        }
        if !rule.only.is_empty()
            && !rule.only.iter().any(|name| Some(name.as_str()) == family)
            && (input.language.is_some() || !rule.only.iter().any(|name| name == "js"))
        {
            continue;
        }
        if rule.instantiates && input.kind != "instantiates" {
            continue;
        }
        if rule.test.matches(&text) {
            return Some(shaped(
                &rule.category,
                &text,
                input.args.as_deref(),
                input.language.as_deref(),
                None,
            ));
        }
    }
    if let Some(receiver) = &input.receiver_type {
        let receiver = replace(
            r"^(?:readonly|private|public|protected|final|static)\s+",
            receiver,
            "",
        );
        let receiver = receiver.trim();
        for rule in &rules.receiver_types {
            if rule.test.matches(receiver) {
                return Some(shaped(
                    &rule.category,
                    &text,
                    input.args.as_deref(),
                    input.language.as_deref(),
                    Some(receiver),
                ));
            }
        }
    }
    if input.project.as_deref().unwrap_or("app") != "app"
        && (input.kind == "instantiates" || matches!(family, Some("py" | "swift")))
    {
        let last = text
            .rsplit(['.', ':'])
            .next()
            .unwrap_or(&text)
            .rsplit('\\')
            .next()
            .unwrap_or(&text);
        if rules.exception.matches(&text) && !rules.not_response.matches(last) {
            return Some(effect("response"));
        }
    }
    None
}
fn shaped(
    category: &str,
    text: &str,
    args: Option<&str>,
    language: Option<&str>,
    receiver: Option<&str>,
) -> Effect {
    let mut value = effect(category);
    if category != "database" {
        return value;
    }
    let text = replace(r"\([^()]*\)", text, "");
    let segments = text
        .split(['.', ':'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let last = segments.last().copied().unwrap_or("");
    value.model = model(&segments, args, receiver);
    let operation = last.trim_end_matches(['!', '?']);
    if operation == "exec" && language == Some("python") {
        value.access = Some("read".into());
    } else if rules().write.matches(operation) {
        value.access = Some("write".into());
    } else if rules().read.matches(operation) {
        value.access = Some("read".into());
    }
    value
}
fn model(segments: &[&str], args: Option<&str>, receiver_type: Option<&str>) -> Option<String> {
    let parts = if segments
        .first()
        .is_some_and(|part| matches!(*part, "this" | "self"))
    {
        &segments[1..]
    } else {
        segments
    };
    let first = parts.first().copied().unwrap_or("");
    let second_last = if parts.len() >= 3 {
        parts[parts.len() - 2]
    } else {
        ""
    };
    if parts.len() >= 3
        && matches(r"^[A-Za-z_$][\w$]*$", second_last)
        && !matches(
            r"^(?:objects|query|session|db|\$|from|table|collection|opsForValue|Items)$",
            second_last,
        )
        && !rules().model_suffix.matches(second_last)
        && second_last != "$transaction"
        && (matches(
            r"(?i)^(?:prisma|db|database|orm|em|drizzle|sql|_context|context|dbContext|_db|ctx|this)$",
            first,
        ) || matches(r"(?:Context|Client|Prisma|Db|DB)$", first))
    {
        return Some(second_last.into());
    }
    let receiver = if parts.len() >= 2 { parts[0] } else { "" };
    if !receiver.is_empty()
        && rules().model_suffix.matches(receiver)
        && !matches!(receiver, "this" | "self")
    {
        let stem = rules()
            .model_suffix
            .0
            .replace(receiver.trim_start_matches('_'), "")
            .into_owned();
        if !stem.is_empty() {
            return Some(stem);
        }
    }
    if parts.len() >= 2
        && matches(r"^[A-Z][A-Za-z0-9]*$", first)
        && !matches(r"^(?:DB|ActiveRecord|Files|File|Path|Base)$", first)
    {
        return Some(first.into());
    }
    if let Some(receiver) = receiver_type {
        let stem = replace(r"<[^>]*>", receiver, "");
        let stem = replace(r"^I(?=[A-Z])", &stem, "");
        let stem = rules().model_suffix.0.replace(&stem, "").into_owned();
        if !stem.is_empty()
            && stem != receiver
            && matches(r"^[A-Z]", &stem)
            && !matches(r"^(?:Entity|Db|Data|Jdbc|Mongo|Session|Async)$", &stem)
        {
            return Some(stem);
        }
    }
    if let Some(args) = args
        && matches(
            r"(?i)^(?:knex|db|collection|table|from|into|selectFrom|insertInto|updateTable|deleteFrom|sql\.table|query|getRepository|getCollection|model)$",
            parts.last().unwrap_or(&""),
        )
        && let Ok(Some(captures)) = regex(r#"^['"`]([\w.-]+)['"`]"#).captures(args)
    {
        return Some(captures[1].into());
    }
    None
}
fn status_name(raw: &str) -> Option<u16> {
    let name = replace(
        r"^(?:HttpStatus|HTTPStatus|StatusCodes|StatusCode|http|status|HttpStatusCode|Status|HTTP_|HTTP)[._]?",
        raw,
        "",
    );
    let name = replace(
        r"(?:Exception|Error|Response|Result|Async|Http|Status|Code)$",
        &name,
        "",
    );
    let name = replace(r"^Http(?=[A-Z])", &name, "");
    let key = replace(r"[^A-Za-z0-9]", &name, "").to_lowercase();
    if key.is_empty() {
        return None;
    }
    if matches(r"^\d{3}$", &key) {
        return key.parse().ok();
    }
    for pattern in [
        r"^(?:http)?(\d{3})$",
        r"(?:^|[a-z_])([1-5]\d{2})(?:[a-z_]|$)",
    ] {
        if let Ok(Some(captures)) = regex(pattern).captures(&key) {
            return captures[1].parse().ok();
        }
    }
    if let Some(&code) = rules().status_names.get(&key) {
        return Some(code);
    }
    rules()
        .status_names
        .iter()
        .find(|(known, _)| known.len() >= 6 && key.ends_with(known.as_str()))
        .map(|(_, code)| *code)
}
fn literal_status(token: &str) -> Option<u16> {
    let token = token.trim();
    if matches(r"^\d{3}$", token) {
        return token.parse().ok();
    }
    for pattern in [
        r"^(?:HttpStatus|HTTPStatus|StatusCodes|HttpStatusCode|http|status|Status|HttpStatusCodes|StatusCode|HTTPResponseStatus)\.(\w+)$",
        r"^\.(\w+)$",
        r"^(?:HTTP_|StatusCodes\.)(\w+)$",
    ] {
        if let Ok(Some(captures)) = regex(pattern).captures(token) {
            return status_name(&captures[1]);
        }
    }
    if matches(r"^[A-Z][A-Z_]+$", token) {
        return status_name(token);
    }
    None
}
fn split_args(args: &str) -> Vec<&str> {
    let mut parts = vec![];
    let mut depth: i32 = 0;
    let mut start = 0;
    for (index, ch) in args.char_indices() {
        if matches!(ch, '(' | '[' | '{') {
            depth += 1;
        } else if matches!(ch, ')' | ']' | '}') {
            depth = (depth - 1).max(0);
        }
        if ch == ',' && depth == 0 {
            parts.push(args[start..index].trim());
            start = index + 1;
        }
    }
    if !args[start..].trim().is_empty() {
        parts.push(args[start..].trim());
    }
    parts
}
pub(super) fn response_status(text: &str, args: Option<&str>) -> Option<u16> {
    let call = normalise(text);
    if let Ok(Some(captures))=regex(r"(?:^|\.)(?:status|sendStatus|code|Status|StatusCode|SendStatus|withStatus|with_status)\(([^()]+)\)").captures(&call) && let Some(code)=literal_status(&captures[1]){return Some(code);}
    let bare = replace(r"\([^()]*\)", &call, "");
    let last = bare
        .rsplit(['.', ':'])
        .next()
        .unwrap_or(&call)
        .trim_start_matches("new");
    let args = args.unwrap_or("");
    if matches(
        r"^(?:status|sendStatus|code|abort|StatusCode|Status|SendStatus|WriteHeader|String|Data|JSON|IndentedJSON|XML|YAML|HTML|AbortWithStatus|AbortWithStatusJSON|Error|Redirect|head|Problem|error)$",
        last,
    ) {
        let parts = split_args(args);
        let token = if matches!(last, "Error" | "Redirect") {
            parts.last()
        } else {
            parts.first()
        };
        if let Some(code) = token.and_then(|token| literal_status(token)) {
            return Some(code);
        }
        if last == "Redirect" {
            return Some(302);
        }
        if matches!(last, "Problem" | "error") {
            return Some(500);
        }
    }
    if let Ok(Some(captures)) =
        regex(r"(?:status_code|statusCode|status|code)\s*[=:]\s*([\w.]+)").captures(args)
        && let Some(code) = literal_status(&captures[1])
    {
        return Some(code);
    }
    for pattern in [
        r"\b(?:HttpStatus|HTTPStatus|StatusCodes|HttpStatusCode|http)\.(\w+)",
        r"(?:^|[\s,(])\.(\w+)",
    ] {
        if let Ok(Some(captures)) = regex(pattern).captures(args)
            && let Some(code) = status_name(&captures[1])
        {
            return Some(code);
        }
    }
    if matches(r"Exception$|Error$|^Abort$|^abort$|^HTTPError$", last) {
        for part in split_args(args) {
            if let Some(code) = literal_status(part) {
                return Some(code);
            }
        }
    }
    for segment in bare
        .split(['.', ':'])
        .filter(|segment| !segment.is_empty())
        .rev()
    {
        if matches(
            r"^(?:json|send|end|render|view|View|Json|Content|File|jsonify|JSONResponse|Response|HttpResponse|JsonResponse|render_template|body|text|html|build|status|type|header|headers|set|res|response|reply|rep|ctx|c|context|this|self|http|w|rw|ResponseEntity|Results|TypedResults|HttpStatus|NextResponse)$",
            segment,
        ) {
            continue;
        }
        if let Some(code) = status_name(segment) {
            return Some(code);
        }
        break;
    }
    if matches(
        r"^(?:redirect|redirect_to|RedirectResponse|HttpResponseRedirect|RedirectToAction|RedirectToPage|RedirectToRoute|LocalRedirect|permanentRedirect|Redirect|RedirectView)$",
        last,
    ) {
        return Some(if last.to_lowercase().contains("permanent") {
            308
        } else {
            302
        });
    }
    if last == "ModelAndView" && matches(r#"^\s*"redirect:"#, args) {
        return Some(302);
    }
    if matches(r"^(?:redirect->\w+|back|to_route)(?:->\w+)*$", &bare) {
        return Some(302);
    }
    if matches(
        r"^(?:Created|CreatedAtAction|CreatedAtRoute|created)$",
        last,
    ) {
        return Some(201);
    }
    None
}
pub(super) fn implicit_status(text: &str) -> Option<u16> {
    let call = normalise(text);
    if matches(
        r"(?:^|\.)(?:status|sendStatus|code|Status|StatusCode|SendStatus|withStatus|with_status|writeHead)\(",
        &call,
    ) {
        return None;
    }
    let bare = replace(r"\([^()]*\)", &call, "");
    if matches(
        r"^(?:res|response|reply|rep|ctx|c|context)(?:\.(?:type|set|header|headers|append|cookie|clearCookie|vary|location|links|format))*\.(?:json|jsonp|send|render|sendFile|download|text|html|body|stream|file|view)$",
        &bare,
    ) || matches(r"^(?:NextResponse|Response)\.json$", &bare)
        || matches(
            r"^(?:JSONResponse|HTMLResponse|PlainTextResponse|ORJSONResponse|UJSONResponse|jsonify|render_template|render|make_response|HttpResponse|JsonResponse|send_file|send_from_directory)$",
            &bare,
        )
        || matches(
            r"^(?:render|render_to_string|respond_with|ModelAndView)$",
            &bare,
        )
        || matches(
            r"^response\(\)->(?:json|view)$|^response->json$|^view$",
            &call,
        )
    {
        Some(200)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effects_and_status_match_independent_language_fixtures() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/viewer/effects.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let input: Input = serde_json::from_value(case["input"].clone()).unwrap();
            assert_eq!(
                serde_json::to_value(classify(&input)).unwrap(),
                case["effect"],
                "{:?}",
                input
            );
            assert_eq!(
                json!(response_status(&input.text, input.args.as_deref())),
                case["status"],
                "status {:?}",
                input
            );
            assert_eq!(
                json!(implicit_status(&input.text)),
                case["implicitStatus"],
                "implicit {:?}",
                input
            );
        }
    }
}
