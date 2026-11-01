use prometeu_search::parser;
use std::collections::HashSet;
use std::path::Path;

#[test]
fn embedded_blocks_keep_distinct_ids_and_call_owners() {
    for (path, language, source, calls) in [
        (
            "Separate.astro",
            "astro",
            "---\nfunction init() { server(); }\n---\n<script>function init() { browser(); }</script>",
            ["server", "browser"],
        ),
        (
            "Separate.cfm",
            "cfml",
            "<cfscript>function init() { first(); }</cfscript>\n<cfscript>function init() { second(); }</cfscript>",
            ["first", "second"],
        ),
        (
            "Separate.vue",
            "vue",
            "<script>function init() { moduleCall(); }</script><script setup>function init() { setupCall(); }</script>",
            ["moduleCall", "setupCall"],
        ),
        (
            "Separate.svelte",
            "svelte",
            "<script context=\"module\">function init() { moduleCall(); }</script><script>function init() { instanceCall(); }</script>",
            ["moduleCall", "instanceCall"],
        ),
    ] {
        let result = parser::extract_as(Path::new(path), source, language).unwrap();
        assert!(result.errors.is_empty(), "{path}: {:?}", result.errors);
        let functions: Vec<_> = result
            .nodes
            .iter()
            .filter(|node| node.name == "init")
            .collect();
        assert_eq!(functions.len(), 2, "{path}");
        assert_ne!(functions[0].id, functions[1].id, "{path}");
        let ids: HashSet<_> = result.nodes.iter().map(|node| &node.id).collect();
        assert_eq!(ids.len(), result.nodes.len(), "{path}");
        for (function, call) in functions.iter().zip(calls) {
            let reference = result
                .unresolved_references
                .iter()
                .find(|reference| reference.reference_name == call)
                .unwrap();
            assert_eq!(reference.from_node_id, function.id, "{path}: {call}");
        }
        for edge in result.edges {
            assert!(ids.contains(&edge.source), "{path}: {}", edge.source);
            if edge.kind == "contains" {
                assert!(ids.contains(&edge.target), "{path}: {}", edge.target);
            }
        }
    }
}

#[test]
fn unicode_directives_do_not_shift_cpp_rewrite_ranges() {
    for directive in ["#define π 1", "#define π é"] {
        let source = format!("{directive}\nclass API Widget {{}};\n");
        let result = parser::extract_as(Path::new("widget.cpp"), &source, "cpp").unwrap();
        assert!(
            result
                .nodes
                .iter()
                .any(|node| node.kind == "class" && node.name == "Widget"),
            "{directive}: {:?}",
            result.nodes
        );
    }
}

#[test]
fn vue_regular_expressions_do_not_hide_options_methods() {
    let source = r#"<script>export default { methods: {
        test(value) { return /\(/.test(value); },
        characterClass(value) { return /[{},\]]/.test(value); },
        divide(value) { return value / 2; },
        template(value) { return `${/\(/.test(value)}`; }
    } }</script>"#;
    let result = parser::extract_as(Path::new("Methods.vue"), source, "vue").unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    for name in ["test", "characterClass", "divide", "template"] {
        assert!(
            result
                .nodes
                .iter()
                .any(|node| node.kind == "method" && node.name == name),
            "missing {name}"
        );
    }
}

#[test]
fn razor_comments_do_not_add_dependencies_or_calls() {
    let source = "@* é😀\n@inject OldService service\n<OldWidget />\n@code { void Old() { OldCall(); } }\n*@\n@inject NewService service\n<NewWidget />\n@code { void New() { NewCall(); } }";
    let result = parser::extract_as(Path::new("View.razor"), source, "razor").unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let names: HashSet<_> = result
        .unresolved_references
        .iter()
        .map(|reference| reference.reference_name.as_str())
        .collect();
    for name in ["OldService", "OldWidget", "OldCall"] {
        assert!(!names.contains(name), "comment leaked {name}");
    }
    for name in ["NewService", "NewWidget", "NewCall"] {
        assert!(names.contains(name), "missing {name}");
    }
}

#[test]
fn multiline_liquid_records_keep_complete_source_spans() {
    let source = "{% render\n 'card' %}\n{% assign\n handle = 1 %}";
    let result = parser::extract_as(Path::new("view.liquid"), source, "liquid").unwrap();
    for node in result.nodes.iter().filter(|node| node.name == "card") {
        assert_eq!((node.start_line, node.end_line, node.end_column), (1, 2, 7));
    }
    let variable = result
        .nodes
        .iter()
        .find(|node| node.name == "handle")
        .unwrap();
    assert_eq!((variable.start_line, variable.end_line), (3, 4));
    let parsed = parser::parse_with_override(Path::new("view.liquid"), source, None).unwrap();
    let import = parsed
        .symbols
        .iter()
        .find(|node| node.kind == "import" && node.name == "card")
        .unwrap();
    assert_eq!(
        &source[import.start_byte..import.end_byte],
        "{% render\n 'card'"
    );
    let variable = parsed
        .symbols
        .iter()
        .find(|node| node.name == "handle")
        .unwrap();
    assert_eq!(
        &source[variable.start_byte..variable.end_byte],
        "{% assign\n handle ="
    );
}

#[test]
fn inline_templates_survive_script_and_style_blocks() {
    for (path, language, source) in [
        (
            "Inline.svelte",
            "svelte",
            "<script>let x = 'é😀';</script><style>p{color:red}</style><Child />{load()}",
        ),
        (
            "Inline.vue",
            "vue",
            "<script>const x = 'é😀';</script><style>p{color:red}</style><template><Child /><button @click=\"load()\">go</button></template>",
        ),
    ] {
        let result = parser::extract_as(Path::new(path), source, language).unwrap();
        let child = result
            .unresolved_references
            .iter()
            .find(|reference| reference.reference_name == "Child")
            .unwrap();
        assert_eq!(
            child.column,
            source[..source.find("<Child").unwrap() + 1]
                .encode_utf16()
                .count() as u32
        );
        assert!(
            result
                .unresolved_references
                .iter()
                .any(|reference| reference.reference_name == "load"),
            "{path}"
        );
    }
}

#[test]
fn commonjs_imports_skip_literal_text_but_keep_template_substitutions() {
    let source = r#"const text = "require('./string')";
const pattern = /require('#regex')/;
const template = `require('./quasi') ${require('./substitution')}`;
const actual = require /* é😀 */ ('./actual');
/* é😀 */ const after = require('./after-comment');
"#;
    for language in ["javascript", "typescript", "tsx"] {
        let result = parser::extract_as(Path::new("imports.ts"), source, language).unwrap();
        let imports: HashSet<_> = result
            .unresolved_references
            .iter()
            .filter(|reference| reference.reference_kind == "imports")
            .map(|reference| reference.reference_name.as_str())
            .collect();
        assert_eq!(
            imports,
            HashSet::from(["./substitution", "./actual", "./after-comment"]),
            "{language}"
        );
        let after_comment = result
            .unresolved_references
            .iter()
            .find(|reference| reference.reference_name == "./after-comment")
            .unwrap();
        assert_eq!(
            (after_comment.line, after_comment.column),
            (5, "/* é😀 */ const after = ".encode_utf16().count() as u32)
        );
    }
}
