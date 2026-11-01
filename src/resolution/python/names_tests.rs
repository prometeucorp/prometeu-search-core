use super::*;
use crate::frameworks::python_resolution::{Bindings, Environment};
use serde_json::Value;

#[test]
fn python_exact_names_match_reference_projects() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/resolution/python/cases.json"
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
        let names = Bindings::new(&nodes);
        let environment = Environment::new(&sources, &module, &supertypes);
        for query in project["references"].as_array().unwrap() {
            let reference: UnresolvedReference =
                serde_json::from_value(query["reference"].clone()).unwrap();
            let actual=names.exact_name(&reference,&environment).unwrap().map_or(Value::Null,|answer|serde_json::json!({"target":answer.node.id,"confidence":answer.confidence,"resolvedBy":answer.resolved_by,"metadata":null}));
            if actual != query["exact"] {
                differences.push(serde_json::json!({"project":project["name"],"reference":query["reference"],"actual":actual,"expected":query["exact"]}));
            }
        }
    }
    assert!(
        differences.is_empty(),
        "{} differing exact-name decisions:\n{}",
        differences.len(),
        serde_json::to_string_pretty(&differences[..differences.len().min(20)]).unwrap()
    );
}
