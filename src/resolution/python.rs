//! Python name ownership, import scopes, and typed receiver evidence.
use crate::frameworks::python_resolution::regex;
use anyhow::{Result, ensure};
use prometeu_extract::{Node, UnresolvedReference};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

#[path = "python/general.rs"]
mod general;
#[path = "python/imports.rs"]
mod imports;
#[path = "python/scope.rs"]
mod scope;
#[path = "python/statements.rs"]
mod statements;
#[path = "python/typed.rs"]
mod typed;
#[path = "python/value.rs"]
mod value;

pub(in crate::graph) struct EvidenceAnswer<'a> {
    pub reference: &'a UnresolvedReference,
    pub target: Option<String>,
    pub confidence: f64,
    pub resolved_by: &'static str,
    pub metadata: Option<serde_json::Value>,
    pub general: bool,
    pub by_name: bool,
}

pub(in crate::graph) fn evidence_decisions<'a, 'r>(
    nodes: &'a [Node],
    sources: &'a BTreeMap<&'a str, &'a str>,
    resolve_module: &'a dyn Fn(&str, &str) -> Option<String>,
    supertypes: &'a dyn Fn(&str) -> Vec<&'a Node>,
    references: &'r [UnresolvedReference],
) -> Result<Vec<EvidenceAnswer<'r>>> {
    let scopes = scope::Scopes::new(nodes, sources);
    let imports = imports::Imports::new(nodes, sources, resolve_module, &scopes, supertypes)?;
    let typed = typed::Typed::new(nodes, sources, resolve_module, &scopes, &imports)?;
    let names = crate::frameworks::python_resolution::Bindings::new(nodes);
    let environment =
        crate::frameworks::python_resolution::Environment::new(sources, resolve_module, supertypes);
    let mut answers = Vec::new();
    for reference in references
        .iter()
        .filter(|reference| reference.language.as_deref() == Some("python"))
    {
        let (answer, general) = match typed.select(reference)? {
            typed::Match::Unhandled => (
                general::select(reference, &typed, &scopes, &imports, &names, &environment)?,
                true,
            ),
            answer => (answer, false),
        };
        match answer {
            typed::Match::Unhandled => {}
            typed::Match::Rejected => answers.push(EvidenceAnswer {
                reference,
                target: None,
                confidence: 0.0,
                resolved_by: "",
                metadata: None,
                general,
                by_name: false,
            }),
            typed::Match::Resolved(answer) => answers.push(EvidenceAnswer {
                reference,
                target: Some(answer.node.id.clone()),
                confidence: answer.confidence,
                resolved_by: answer.resolved_by,
                metadata: answer.metadata,
                general,
                by_name: general
                    && answer.resolved_by == "instance-method"
                    && answer.confidence <= 0.8,
            }),
        }
    }
    Ok(answers)
}

pub(in crate::graph) struct ImportAnswer<'a> {
    pub reference: &'a UnresolvedReference,
    pub target: String,
    pub confidence: f64,
    pub resolved_by: &'static str,
}

pub(in crate::graph) fn module_imports<'a, 'r>(
    nodes: &'a [Node],
    sources: &'a BTreeMap<&'a str, &'a str>,
    resolve_module: &'a dyn Fn(&str, &str) -> Option<String>,
    supertypes: &'a dyn Fn(&str) -> Vec<&'a Node>,
    references: &'r [UnresolvedReference],
) -> Result<Vec<ImportAnswer<'r>>> {
    let scopes = scope::Scopes::new(nodes, sources);
    let imports = imports::Imports::new(nodes, sources, resolve_module, &scopes, supertypes)?;
    let mut answers = Vec::new();
    for reference in references {
        if reference.language.as_deref() != Some("python") || reference.reference_kind != "imports"
        {
            continue;
        }
        if let Some(answer) = imports.resolve(reference)?
            && answer.confidence >= 0.9
        {
            answers.push(ImportAnswer {
                reference,
                target: answer.node.id.clone(),
                confidence: answer.confidence,
                resolved_by: answer.resolved_by,
            });
        }
    }
    Ok(answers)
}

fn trim(text: &str) -> &str {
    text.trim_matches(crate::search::query_whitespace)
}

#[cfg(test)]
#[path = "python/index_tests.rs"]
mod index_tests;
#[cfg(test)]
#[path = "python/names_tests.rs"]
mod names_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn python_type_evidence_matches_reference_source_expressions() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/resolution/python/cases.json"
        ))
        .unwrap();
        for case in fixture["evidence"]["annotations"].as_array().unwrap() {
            let raw = case["raw"].as_str().unwrap();
            assert_eq!(
                serde_json::to_value(value::annotation(raw).unwrap()).unwrap(),
                case["expected"],
                "annotation: {raw}"
            );
        }
        for case in fixture["evidence"]["values"].as_array().unwrap() {
            let raw = case["raw"].as_str().unwrap();
            assert_eq!(
                serde_json::to_value(value::expression(raw).unwrap()).unwrap(),
                case["expression"],
                "expression: {raw}"
            );
            assert_eq!(
                serde_json::to_value(value::value(raw, case["signature"].as_str()).unwrap())
                    .unwrap(),
                case["expected"],
                "value: {raw}"
            );
        }
    }

    #[test]
    fn python_scope_ownership_matches_reference_projects() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/resolution/python/cases.json"
        ))
        .unwrap();
        for project in fixture["projects"].as_array().unwrap() {
            let nodes = project["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| {
                    let mut node = node.clone();
                    node["updatedAt"] = 0.into();
                    serde_json::from_value::<Node>(node).unwrap()
                })
                .collect::<Vec<_>>();
            let sources = project["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str().unwrap()))
                .collect();
            let scopes = scope::Scopes::new(&nodes, &sources);
            for query in project["references"].as_array().unwrap() {
                let Some(name) = query["scope"]["name"].as_str() else {
                    continue;
                };
                let reference: UnresolvedReference =
                    serde_json::from_value(query["reference"].clone()).unwrap();
                let actual = scopes.receiver(name, &reference).unwrap();
                assert_eq!(
                    serde_json::to_value(actual).unwrap(),
                    query["scope"]["binding"],
                    "{} {}",
                    project["name"],
                    query["reference"]
                );
                let local = scopes.binding_scope(name, &reference).unwrap().map_or(
                    Value::Null,
                    |(node, details)| serde_json::json!({"node":node.id,"details":&*details}),
                );
                assert_eq!(
                    local,
                    *query["scope"].get("local").unwrap(),
                    "{} {}",
                    project["name"],
                    query["reference"]
                );
                assert_eq!(
                    serde_json::to_value(scopes.function_module_alias(name, &reference).unwrap())
                        .unwrap(),
                    query["scope"]["alias"],
                    "{} {}",
                    project["name"],
                    query["reference"]
                );
                assert_eq!(
                    serde_json::to_value(
                        scopes
                            .module_import_keys(name, reference.file_path.as_deref().unwrap())
                            .unwrap()
                    )
                    .unwrap(),
                    query["scope"]["moduleKeys"],
                    "{} {}",
                    project["name"],
                    query["reference"]
                );
            }
        }
    }

    #[test]
    fn python_import_targets_match_reference_projects() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/resolution/python/cases.json"
        ))
        .unwrap();
        let mut differences = Vec::new();
        for project in fixture["projects"].as_array().unwrap() {
            let nodes = project["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| {
                    let mut node = node.clone();
                    node["updatedAt"] = 0.into();
                    serde_json::from_value::<Node>(node).unwrap()
                })
                .collect::<Vec<_>>();
            let files: BTreeMap<String, String> =
                serde_json::from_value(project["files"].clone()).unwrap();
            let sources = files
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect::<BTreeMap<_, _>>();
            let mut mapped = BTreeMap::new();
            for (path, source) in &sources {
                let mappings = imports::mappings(source).unwrap();
                if let Some(expected) = project["mapping"].get(path) {
                    let expected: Vec<imports::Mapping> =
                        serde_json::from_value(expected.clone()).unwrap();
                    assert_eq!(mappings, expected, "{} {path}", project["name"]);
                }
                mapped.insert(
                    (*path).to_owned(),
                    mappings
                        .into_iter()
                        .map(|mapping| crate::parser::Import {
                            local: mapping.local_name,
                            imported: mapping.exported_name,
                            module: mapping.source,
                            scope: String::new(),
                            reexport: false,
                        })
                        .collect(),
                );
            }
            let modules =
                super::super::PythonModules::new(files.keys().cloned(), &files, &nodes, mapped)
                    .unwrap();
            let module = |specifier: &str, from: &str| modules.resolve(specifier, from);
            let scopes = scope::Scopes::new(&nodes, &sources);
            let supertypes = |id: &str| {
                project["supertypes"][id]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|id| {
                        nodes
                            .iter()
                            .find(|node| Some(node.id.as_str()) == id.as_str())
                    })
                    .collect()
            };
            let imports =
                imports::Imports::new(&nodes, &sources, &module, &scopes, &supertypes).unwrap();
            for query in project["references"].as_array().unwrap() {
                let reference: UnresolvedReference =
                    serde_json::from_value(query["reference"].clone()).unwrap();
                let actual = imports.resolve(&reference).unwrap().map_or(Value::Null, |answer| serde_json::json!({"target":answer.node.id,"confidence":answer.confidence,"resolvedBy":answer.resolved_by,"metadata":null}));
                if actual != query["imported"] {
                    differences.push(serde_json::json!({"project":project["name"],"reference":query["reference"],"actual":actual,"expected":query["imported"]}));
                }
            }
        }
        assert!(
            differences.is_empty(),
            "{} differing import decisions:\n{}",
            differences.len(),
            serde_json::to_string_pretty(&differences[..differences.len().min(15)]).unwrap()
        );
    }
}
