use prometeu_search::{Direction, Engine};
use std::fs;

#[test]
fn loop_bindings_shadow_only_their_lexical_or_function_scope() {
    for extension in ["js", "ts"] {
        for (header, function_scoped) in [
            ("let save = 0; save < 1; save++", false),
            ("const [save] = [0]; save < 1;", false),
            ("var save = 0; save < 1; save++", true),
            ("let save of values", false),
            ("const {save} of values", false),
            ("const [save] of values", false),
            ("let save in values", false),
            ("const save in values", false),
            ("var save of values", true),
            ("var save in values", true),
        ] {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("project");
            fs::create_dir(&root).unwrap();
            fs::write(
                root.join(format!("store.{extension}")),
                "export function save() {}",
            )
            .unwrap();
            fs::write(
                root.join(format!("entry.{extension}")),
                format!(
                    "import {{ save }} from './store';\nfunction run(values) {{\n  save();\n  for ({header}) {{\n    save();\n  }}\n  save();\n}}"
                ),
            )
            .unwrap();
            let mut engine =
                Engine::open(&root, Some(&temporary.path().join("index.sqlite"))).unwrap();
            assert_eq!(
                engine.sync().unwrap().syntax_errors,
                0,
                "{extension}: {header}"
            );
            let result = engine
                .related("run", Direction::Callees, 50, 65536)
                .unwrap();
            assert_eq!(
                result.relations.len(),
                3,
                "{extension}: {header}: {result:?}"
            );
            for relation in &result.relations {
                let shadowed = function_scoped || relation.call_line == 5;
                assert_eq!(
                    relation.resolution,
                    if shadowed {
                        "unresolved"
                    } else {
                        "import_match"
                    },
                    "{extension}: {header}: {relation:?}"
                );
                assert_eq!(relation.candidate.is_none(), shadowed);
            }
        }
    }
}
