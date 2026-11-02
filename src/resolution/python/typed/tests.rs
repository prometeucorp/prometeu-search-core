use super::*;

#[test]
fn python_typed_receivers_match_reference_projects() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/upstream/resolution/python/cases.json"
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
        let mapped = sources
            .iter()
            .map(|(&path, &source)| {
                (
                    path.to_owned(),
                    imports::mappings(source)
                        .unwrap()
                        .into_iter()
                        .map(|mapping| crate::parser::Import {
                            local: mapping.local_name,
                            imported: mapping.exported_name,
                            module: mapping.source,
                            scope: String::new(),
                            reexport: false,
                        })
                        .collect(),
                )
            })
            .collect();
        let modules =
            crate::graph::PythonModules::new(files.keys().cloned(), &files, &nodes, mapped)
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
        let typed = Typed::new(&nodes, &sources, &module, &scopes, &imports).unwrap();
        let names = crate::frameworks::python_resolution::Bindings::new(&nodes);
        let environment =
            crate::frameworks::python_resolution::Environment::new(&sources, &module, &supertypes);
        let normalize = |answer: Option<Answer<'_>>| {
            answer.map_or(serde_json::Value::Null,|answer|serde_json::json!({"target":answer.node.id,"confidence":answer.confidence,"resolvedBy":answer.resolved_by,"metadata":answer.metadata}))
        };
        for query in project["references"].as_array().unwrap() {
            let reference: UnresolvedReference =
                serde_json::from_value(query["reference"].clone()).unwrap();
            let builtin = typed.builtin(&reference).unwrap();
            if query["builtin"] != builtin {
                differences.push(serde_json::json!({"project":project["name"],"kind":"builtin","reference":query["reference"],"actual":builtin,"expected":query["builtin"]}));
            }
            let chained = normalize(typed.chain(&reference).unwrap());
            if query["factory"] != chained {
                differences.push(serde_json::json!({"project":project["name"],"kind":"factory","reference":query["reference"],"actual":chained,"expected":query["factory"]}));
            }
            if let selected @ (Match::Rejected | Match::Resolved(_)) =
                typed.select(&reference).unwrap()
            {
                let actual = match selected {
                    Match::Resolved(answer) => normalize(Some(answer)),
                    _ => serde_json::Value::Null,
                };
                if actual != query["expected"] {
                    differences.push(serde_json::json!({"project":project["name"],"kind":"selected","reference":query["reference"],"actual":actual,"expected":query["expected"]}));
                }
            }
            let actual = normalize(typed.local(&reference).unwrap());
            if actual != query["typedLocal"] {
                differences.push(serde_json::json!({"project":project["name"],"kind":"local","reference":query["reference"],"actual":actual,"expected":query["typedLocal"]}));
            }
            if reference.reference_kind == "calls"
                && let Some((receiver, method)) = reference.reference_name.rsplit_once('.')
            {
                let actual = match typed.attribute(receiver, method, &reference).unwrap() {
                    Match::Unhandled => serde_json::json!({"handled":false,"answer":null}),
                    Match::Rejected => serde_json::json!({"handled":true,"answer":null}),
                    Match::Resolved(answer) => {
                        serde_json::json!({"handled":true,"answer":normalize(Some(answer))})
                    }
                };
                if actual != query["attribute"] {
                    differences.push(serde_json::json!({"project":project["name"],"kind":"attribute","reference":query["reference"],"actual":actual,"expected":query["attribute"]}));
                }
            }
            if matches!(typed.select(&reference).unwrap(), Match::Unhandled) {
                let answer = crate::graph::resolution::python::general::select(
                    &reference,
                    &typed,
                    &scopes,
                    &imports,
                    &names,
                    &environment,
                )
                .unwrap();
                if !matches!(answer, Match::Unhandled) {
                    let actual = normalize(match answer {
                        Match::Resolved(answer) => Some(answer),
                        _ => None,
                    });
                    if actual != query["expected"] {
                        differences.push(serde_json::json!({"project":project["name"],"kind":"general","reference":query["reference"],"actual":actual,"expected":query["expected"]}));
                    }
                }
            }
        }
    }
    std::fs::write(
        std::env::temp_dir().join("prometeu-python-decisions-diffs.json"),
        serde_json::to_vec_pretty(&differences).unwrap(),
    )
    .unwrap();
    assert!(
        differences.is_empty(),
        "{} differing typed decisions:\n{}",
        differences.len(),
        serde_json::to_string_pretty(&differences[..differences.len().min(15)]).unwrap()
    );
}
