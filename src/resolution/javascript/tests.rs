use super::*;
use serde_json::{Value, json};
#[test]
fn declared_field_receivers_match_reference_projects() -> Result<()> {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/javascript-fields/cases.json"
    ))?;
    for input in corpus["ambientInputs"].as_array().into_iter().flatten() {
        assert_eq!(
            serde_json::to_value(ambient::scan(input["source"].as_str().unwrap()))?,
            input["expected"],
            "ambient scanner {}",
            input["source"]
        );
    }
    for case in corpus["projects"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let nodes: Vec<Node> = case["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                let mut value = value.clone();
                value["updatedAt"] = json!(0);
                serde_json::from_value(value)
            })
            .collect::<std::result::Result<_, _>>()?;
        let sources: BTreeMap<_, _> = case["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(path, value)| (path.as_str(), value.as_str().unwrap()))
            .collect();
        let imports: Vec<_> = case["imports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| ImportBinding {
                file: value["file"].as_str().unwrap().into(),
                local_name: value["localName"].as_str().unwrap().into(),
                resolved_path: None,
                mapped_path: None,
                external: value["external"].as_bool().unwrap(),
                source: value["source"].as_str().unwrap_or("").into(),
                imported_name: value["exportedName"].as_str().unwrap_or("").into(),
                is_default: value["isDefault"].as_bool().unwrap_or(false),
                is_namespace: value["isNamespace"].as_bool().unwrap_or(false),
            })
            .collect();
        let resolve = |reference: &UnresolvedReference| -> Result<Option<String>> {
            let answers: Vec<_> = case["importAnswers"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|answer| {
                    answer["reference"]["filePath"].as_str() == reference.file_path.as_deref()
                        && answer["reference"]["referenceName"].as_str()
                            == Some(&reference.reference_name)
                })
                .collect();
            ensure!(
                !answers.is_empty(),
                "{id}: uncaptured import {} from {:?}",
                reference.reference_name,
                reference.file_path
            );
            ensure!(
                answers
                    .iter()
                    .all(|answer| answer["answer"] == answers[0]["answer"]),
                "{id}: ambiguous import oracle"
            );
            Ok(answers[0]["answer"].as_str().map(str::to_string))
        };
        let edges: Vec<Edge> = serde_json::from_value(case["edges"].clone())?;
        let context = Context::new(&nodes, &sources, &edges, &imports, &resolve);
        let scopes = crate::frameworks::python_resolution::Bindings::new(&nodes);
        let mut references = Vec::new();
        for input in case["fields"].as_array().unwrap() {
            let reference: UnresolvedReference =
                serde_json::from_value(input["reference"].clone())?;
            let actual = context
                .field_call(
                    input["field"].as_str().unwrap(),
                    input["method"].as_str().unwrap(),
                    &reference,
                    input["element"].as_bool().unwrap_or(false),
                )?
                .map(|node| json!({"id":node.id,"confidence":0.85,"resolvedBy":"instance-method"}));
            assert_eq!(
                actual.unwrap_or(Value::Null),
                input["expected"],
                "{id}: field {}",
                input["reference"]
            );
            references.push(reference);
        }
        assert_eq!(
            bindings(&nodes, &sources, &edges, &references, &imports, &resolve)?.len(),
            references.len()
        );
        for input in case["locals"].as_array().into_iter().flatten() {
            let reference: UnresolvedReference =
                serde_json::from_value(input["reference"].clone())?;
            let receiver = input["receiver"].as_str().unwrap();
            let reading = context.infer_local(receiver, &reference)?;
            assert_eq!(
                reading.as_ref().map(|reading| reading.type_name.as_str()),
                input["inferred"].as_str(),
                "{id}: inferred local {}",
                input["reference"]
            );
            if let Some(reading) = reading {
                assert_eq!(
                    json!({"raw":reading.raw,"line":reading.line}),
                    input["declaration"],
                    "{id}: inferred declaration {}",
                    input["reference"]
                );
            }
            if let Some(answer) =
                context.local_call(receiver, input["method"].as_str().unwrap(), &reference)?
            {
                let actual=answer.map(|node|json!({"id":node.id,"confidence":0.9,"resolvedBy":"instance-method"})).unwrap_or(Value::Null);
                assert_eq!(
                    actual, input["expected"],
                    "{id}: typed local {}",
                    input["reference"]
                );
            } else if let Some((answer, confidence)) = context.typed_name_fallback(
                receiver,
                input["method"].as_str().unwrap(),
                &reference,
                &scopes,
            )? {
                let actual = answer.map(|node| json!({"id":node.id,"confidence":confidence,"resolvedBy":"instance-method"})).unwrap_or(Value::Null);
                assert_eq!(
                    actual, input["expected"],
                    "{id}: local name fallback {}",
                    input["reference"]
                );
            }
        }
        for input in case["declarations"].as_array().unwrap() {
            let node = context.ids[input["node"].as_str().unwrap()];
            let reference=references.first().cloned().unwrap_or(serde_json::from_value(json!({"fromNodeId":node.id,"referenceName":"","referenceKind":"calls","line":node.start_line,"column":0,"filePath":node.file_path,"language":node.language}))?);
            let actual = serde_json::to_value(context.field(
                node,
                input["field"].as_str().unwrap(),
                &reference,
            )?)?;
            assert_eq!(actual, input["expected"], "{id}: declaration {input}");
        }
        for input in case["headers"].as_array().unwrap() {
            let node = context.ids[input["node"].as_str().unwrap()];
            assert_eq!(
                serde_json::to_value(context.head_supers(node)?)?,
                input["supers"],
                "{id}: header {}",
                node.name
            );
            if node.kind == "type_alias" {
                assert_eq!(
                    serde_json::to_value(context.alias_value(node)?)?,
                    input["alias"],
                    "{id}: alias {}",
                    node.name
                );
            }
        }
        for input in case["returns"].as_array().unwrap() {
            let node = context.ids[input["node"].as_str().unwrap()];
            assert_eq!(
                json!(context.returns_argument(node)?),
                input["expected"],
                "{id}: argument return {}",
                node.name
            );
        }
    }
    Ok(())
}

#[test]
fn declared_field_edges_match_index_and_reopened_snapshot() -> Result<()> {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/javascript-fields/cases.json"
    ))?;
    let mut differences = Vec::new();
    for case in corpus["projects"].as_array().unwrap() {
        let selected: std::collections::HashSet<_> = case["fields"]
            .as_array()
            .unwrap()
            .iter()
            .chain(
                case["locals"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|local| !local["inferred"].is_null()),
            )
            .chain(case["functionRefs"].as_array().into_iter().flatten())
            .map(|input| {
                let reference = &input["reference"];
                (
                    reference["fromNodeId"].as_str().unwrap().to_string(),
                    reference["line"].as_u64().unwrap() as u32,
                    reference["column"].as_u64().unwrap() as u32,
                )
            })
            .collect();
        let normalize = |edges: Vec<Edge>| {
            let mut records: Vec<_> = edges
                .into_iter()
                .filter(|edge| {
                    edge.kind == "instantiates"
                        || ((edge.kind == "calls"
                            || edge.kind == "references"
                                && edge
                                    .metadata
                                    .as_ref()
                                    .is_some_and(|metadata| metadata["fnRef"] == true))
                            && selected.contains(&(
                                edge.source.clone(),
                                edge.line.unwrap_or(1),
                                edge.column.unwrap_or(0),
                            )))
                })
                .map(|edge| {
                    let mut value = serde_json::to_value(edge).unwrap();
                    value.sort_all_objects();
                    value.to_string()
                })
                .collect();
            records.sort();
            records
        };
        let expected = normalize(serde_json::from_value(case["edges"].clone())?);
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().join("project");
        let database = temporary.path().join("index.sqlite");
        for (path, source) in case["files"].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, source.as_str().unwrap())?;
        }
        let mut engine = crate::Engine::open(&root, Some(&database))?;
        engine.sync()?;
        for reopened in [false, true] {
            if reopened {
                drop(engine);
                for path in case["files"].as_object().unwrap().keys() {
                    std::fs::write(root.join(path), "changed outside the committed index")?;
                }
                engine = crate::Engine::open(&root, Some(&database))?;
            }
            let graph = crate::graph::Graph::load(&engine.connection, 1)?;
            let actual = normalize(graph.canonical_edges.iter().flatten().cloned().collect());
            if actual != expected {
                differences.push(json!({"project":case["id"],"reopened":reopened,"actual":actual.iter().map(|row|serde_json::from_str::<Value>(row).unwrap()).collect::<Vec<_>>(),"expected":expected.iter().map(|row|serde_json::from_str::<Value>(row).unwrap()).collect::<Vec<_>>()}));
            }
        }
    }
    if !differences.is_empty() {
        let path = std::env::temp_dir().join("prometeu-javascript-index-diffs.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&differences)?)?;
        anyhow::bail!(
            "{} JavaScript field scenarios differ; records in {}",
            differences.len(),
            path.display()
        );
    }
    Ok(())
}

#[test]
fn javascript_complete_graph_audit() -> Result<()> {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/javascript-fields/cases.json"
    ))?;
    let difference = |left: &[String], right: &[String]| -> Vec<Value> {
        let mut counts = std::collections::BTreeMap::new();
        for item in right {
            *counts.entry(item.as_str()).or_insert(0usize) += 1;
        }
        let mut result = Vec::new();
        for item in left {
            let count = counts.entry(item.as_str()).or_insert(0);
            if *count > 0 {
                *count -= 1;
            } else {
                result.push(serde_json::from_str(item).unwrap());
            }
        }
        result.sort_by_key(Value::to_string);
        result
    };
    let mut differences = Vec::new();
    let normalize = |mut node: Value| {
        if let Some(object) = node.as_object_mut() {
            object.remove("updatedAt");
            if object.contains_key("qualifiedName") {
                for key in ["isExported", "isAsync", "isStatic", "isAbstract"] {
                    object.entry(key).or_insert(json!(false));
                }
                object.entry("visibility").or_insert(Value::Null);
            }
        }
        node.sort_all_objects();
        node.to_string()
    };
    for case in corpus["projects"].as_array().unwrap() {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().join("project");
        let database = temporary.path().join("index.sqlite");
        for (path, source) in case["files"].as_object().unwrap() {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, source.as_str().unwrap())?;
        }
        let mut engine = crate::Engine::open(&root, Some(&database))?;
        engine.sync()?;
        let graph = crate::graph::Graph::load(&engine.connection, 1)?;
        let actual_nodes: Vec<_> = graph
            .files
            .values()
            .filter_map(|file| file.analysis.extraction.as_ref())
            .flat_map(|extraction| extraction.nodes.iter())
            .map(|node| normalize(serde_json::to_value(node).unwrap()))
            .collect();
        let expected_nodes: Vec<_> = case["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .cloned()
            .map(normalize)
            .collect();
        let actual_edges: Vec<_> = graph
            .canonical_edges
            .iter()
            .flatten()
            .map(|edge| normalize(serde_json::to_value(edge).unwrap()))
            .collect();
        let expected_edges: Vec<_> = case["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| {
                normalize(
                    serde_json::to_value(serde_json::from_value::<Edge>(edge.clone()).unwrap())
                        .unwrap(),
                )
            })
            .collect();
        drop(graph);
        drop(engine);
        for path in case["files"].as_object().unwrap().keys() {
            std::fs::write(root.join(path), "modified outside the committed snapshot")?;
        }
        let engine = crate::Engine::open(&root, Some(&database))?;
        let reopened = crate::graph::Graph::load(&engine.connection, 1)?;
        let reopened_nodes: Vec<_> = reopened
            .files
            .values()
            .filter_map(|file| file.analysis.extraction.as_ref())
            .flat_map(|extraction| extraction.nodes.iter())
            .map(|node| normalize(serde_json::to_value(node).unwrap()))
            .collect();
        let reopened_edges: Vec<_> = reopened
            .canonical_edges
            .iter()
            .flatten()
            .map(|edge| normalize(serde_json::to_value(edge).unwrap()))
            .collect();
        ensure!(
            difference(&actual_nodes, &reopened_nodes).is_empty()
                && difference(&reopened_nodes, &actual_nodes).is_empty(),
            "{} nodes changed after reopening",
            case["id"]
        );
        ensure!(
            difference(&actual_edges, &reopened_edges).is_empty()
                && difference(&reopened_edges, &actual_edges).is_empty(),
            "{} edges changed after reopening",
            case["id"]
        );
        let missing_nodes = difference(&expected_nodes, &actual_nodes);
        let extra_nodes = difference(&actual_nodes, &expected_nodes);
        let missing_edges = difference(&expected_edges, &actual_edges);
        let extra_edges = difference(&actual_edges, &expected_edges);
        if !missing_nodes.is_empty()
            || !extra_nodes.is_empty()
            || !missing_edges.is_empty()
            || !extra_edges.is_empty()
        {
            differences.push(json!({"project":case["id"],"missingNodes":missing_nodes,"extraNodes":extra_nodes,"missingEdges":missing_edges,"extraEdges":extra_edges}));
        }
    }
    let path = std::env::temp_dir().join("prometeu-javascript-complete-diffs.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&differences)?)?;
    let accepted: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/upstream/javascript-fields/uncertainty.json"
    ))?;
    let accepted: Vec<_> = accepted
        .into_iter()
        .map(|mut difference| {
            ensure!(
                difference["reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty()),
                "compatibility exception needs its reason"
            );
            difference.as_object_mut().unwrap().remove("reason");
            Ok(difference)
        })
        .collect::<Result<_>>()?;
    ensure!(
        differences == accepted,
        "full JavaScript graph differs from the reference beyond documented compatibility and uncertainty decisions; records in {}",
        path.display()
    );
    Ok(())
}
