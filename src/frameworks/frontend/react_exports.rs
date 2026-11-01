//! Local export aliases and reexports used by React component forwarding.
use super::regex;
use anyhow::Result;
use serde::Serialize;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Alias {
    pub exported_name: String,
    pub local_name: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reexport {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
    pub source: String,
}
pub(crate) fn local(source: &str) -> Result<Vec<Alias>> {
    let mut out = Vec::new();
    for clause in regex(r"export\s*\{([^}]*)\}\s*(?!\s*from)[;\n]")?.captures_iter(source) {
        let clause = clause?;
        for raw in clause[1].split(',') {
            let item = raw.trim();
            if item.is_empty() || regex(r"^type\s")?.is_match(item)? {
                continue;
            }
            if let Some(rename) =
                regex(r"^([A-Za-z_$][\w$]*)\s+as\s+([A-Za-z_$][\w$]*)$")?.captures(item)?
            {
                out.push(Alias {
                    local_name: rename[1].into(),
                    exported_name: rename[2].into(),
                });
            } else if regex(r"^[A-Za-z_$][\w$]*$")?.is_match(item)? {
                out.push(Alias {
                    local_name: item.into(),
                    exported_name: item.into(),
                });
            }
        }
    }
    for member in regex(r"(?m)^[ \t]*(?:module\.)?exports\.([A-Za-z_$][\w$]*)\s*=\s*(?:(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)\s*\(|class\s+(?!extends\b)([A-Za-z_$][\w$]*)|([A-Za-z_$][\w$]*)\s*;?[ \t]*$)")?.captures_iter(source){let m=member?;out.push(Alias{exported_name:m[1].into(),local_name:m.get(2).or_else(||m.get(3)).or_else(||m.get(4)).unwrap().as_str().into()});}
    for object in regex(r"(?m)^[ \t]*module\.exports\s*=\s*\{([^{}]*)\}")?.captures_iter(source) {
        let object = object?;
        for raw in object[1].split(',') {
            if let Some(entry) =
                regex(r"^\s*([A-Za-z_$][\w$]*)\s*(?::\s*([A-Za-z_$][\w$]*)\s*)?$")?.captures(raw)?
            {
                out.push(Alias {
                    exported_name: entry[1].into(),
                    local_name: entry
                        .get(2)
                        .unwrap_or(entry.get(1).unwrap())
                        .as_str()
                        .into(),
                });
            }
        }
    }
    Ok(out)
}
pub(crate) fn default_binding(source: &str) -> Result<Option<String>> {
    if let Some(found) =
        regex(r"(?m)(?:^|;)\s*export\s+default\s+([A-Za-z_$][\w$]*)\s*(?:;|$)")?.captures(source)?
    {
        return Ok(Some(found[1].into()));
    }
    Ok(local(source)?
        .into_iter()
        .find(|item| item.exported_name == "default")
        .map(|item| item.local_name))
}
pub(crate) fn reexports(source: &str) -> Result<Vec<Reexport>> {
    use crate::graph::js_imports::{comments, specifier, split};
    let ident = r"[$_\p{ID_Start}][$\u{200c}\u{200d}\p{ID_Continue}]*";
    let string = r#""(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'"#;
    let clean = comments(source);
    let mut out = Vec::new();
    for capture in regex(&format!(r#"(?<![$.\p{{ID_Continue}}])export(?![$\p{{ID_Continue}}])\s*(?:type(?![$\p{{ID_Continue}}])\s*)?\*(?:\s*as(?:\s+({ident})|\s*({string})))?\s*from\s*['"]([^'"]+)['"]"#))?.captures_iter(&clean){let capture=capture?;let ns=capture.get(1).map(|m|m.as_str().to_owned()).or_else(||capture.get(2).map(|m|m.as_str()[1..m.as_str().len()-1].to_owned()));out.push(Reexport{kind:if ns.is_some(){"namespace"}else{"wildcard"}.into(),exported_name:ns,original_name:None,source:capture[3].into()});}
    for capture in regex(r#"(?<![$.\p{ID_Continue}])export(?![$\p{ID_Continue}])\s*(?:type(?![$\p{ID_Continue}])\s*)?\{([^{}]+)\}\s*from\s*['"]([^'"]+)['"]"#)?.captures_iter(&clean){let capture=capture?;let inner=comments(&capture[1]);for raw in split(&inner){if let Some((original,exported))=specifier(raw)?{out.push(Reexport{kind:"named".into(),exported_name:Some(exported),original_name:Some(original),source:capture[2].into()});}}}
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_readers_match_independent_reference() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/upstream/synthesis/frontend/react-exports.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            assert_eq!(
                serde_json::json!({"source":source,"aliases":local(source).unwrap(),"default":default_binding(source).unwrap(),"reexports":reexports(source).unwrap()}),
                *case,
                "{source}"
            );
        }
    }
}
