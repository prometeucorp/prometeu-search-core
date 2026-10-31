//! Syntax extraction only: call names are candidates, not type-resolved graph edges.
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use tree_sitter::{Language, Node, Parser};

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone)]
pub struct Reference {
    /// Index into this file's symbols, or `None` outside an indexed scope.
    pub caller: Option<usize>,
    pub target: String,
    pub line: usize,
}

#[derive(Debug)]
pub struct ParsedFile {
    pub language: String,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub has_errors: bool,
}

struct RubyBinding {
    name: String,
    visible: Range<usize>,
}

fn language(path: &Path) -> Option<(&'static str, Language)> {
    if matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Gemfile" | "Rakefile" | "Guardfile")
    ) {
        return Some(("ruby", tree_sitter_ruby::LANGUAGE.into()));
    }
    match path.extension()?.to_str()? {
        "ts" | "mts" | "cts" => Some((
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        )),
        "tsx" => Some(("tsx", tree_sitter_typescript::LANGUAGE_TSX.into())),
        "js" | "jsx" | "mjs" | "cjs" => {
            Some(("javascript", tree_sitter_javascript::LANGUAGE.into()))
        }
        "rs" => Some(("rust", tree_sitter_rust::LANGUAGE.into())),
        "rb" | "rake" | "gemspec" | "ru" => Some(("ruby", tree_sitter_ruby::LANGUAGE.into())),
        _ => None,
    }
}

pub fn supports(path: &Path) -> bool {
    language(path).is_some()
}

pub fn parse(path: &Path, source: &str) -> Result<ParsedFile> {
    let Some((name, grammar)) = language(path) else {
        bail!("unsupported language: {}", path.display());
    };
    let mut parser = Parser::new();
    parser.set_language(&grammar)?;
    let tree = parser
        .parse(source, None)
        .context("parser did not return a syntax tree")?;
    let mut result = ParsedFile {
        language: name.into(),
        symbols: Vec::new(),
        references: Vec::new(),
        has_errors: tree.root_node().has_error(),
    };
    let ruby = name == "ruby";
    // Ruby scripts and build files often consist entirely of executable DSL calls.
    let file_scope = if ruby {
        result.symbols.push(file_symbol(path, source));
        Some(0)
    } else {
        None
    };
    let mut locals: HashMap<Option<usize>, Vec<RubyBinding>> = HashMap::new();
    let mut bindings = HashSet::new();
    let mut bare_calls = Vec::new();
    // An explicit stack also handles deeply nested, partially edited source files.
    let mut pending = vec![(tree.root_node(), file_scope)];
    while let Some((node, inherited_caller)) = pending.pop() {
        let mut caller = inherited_caller;
        if let Some((symbol_name, kind)) = declaration(node, source, caller.is_none(), ruby) {
            let index = result.symbols.len();
            let start_line = node.start_position().row + 1;
            let end = node.end_position();
            result.symbols.push(Symbol {
                name: symbol_name,
                kind: kind.into(),
                start_line,
                end_line: (end.row + usize::from(end.column > 0)).max(start_line),
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
            });
            if matches!(kind, "function" | "method") || (ruby && matches!(kind, "class" | "module"))
            {
                caller = Some(index);
            }
        }
        let target = match node.kind() {
            "call_expression" => node.child_by_field_name("function"),
            "new_expression" => node.child_by_field_name("constructor"),
            "call" if ruby => node.child_by_field_name("method"),
            _ => None,
        };
        if let Some(mut target) = target.and_then(|target| call_name(target, source)) {
            if ruby
                && node.parent().is_some_and(|parent| {
                    parent.kind() == "assignment"
                        && parent.child_by_field_name("left") == Some(node)
                })
            {
                target.push('=');
            }
            result.references.push(Reference {
                caller,
                target,
                line: node.start_position().row + 1,
            });
        }
        if ruby {
            let binding = match node.kind() {
                "assignment" | "operator_assignment" => node.child_by_field_name("left"),
                "method_parameters" | "block_parameters" | "lambda_parameters"
                | "exception_variable" => Some(node),
                "for" => node.child_by_field_name("pattern"),
                _ => None,
            };
            if let Some(binding) = binding {
                ruby_bindings(
                    binding,
                    source,
                    ruby_scope_end(node),
                    locals.entry(caller).or_default(),
                    &mut bindings,
                );
            }
            if node.kind() == "identifier" && ruby_bare_call(node) {
                bare_calls.push((
                    node.id(),
                    caller,
                    text(node, source),
                    node.start_position().row + 1,
                    node.start_byte(),
                ));
            }
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push((child, caller));
            }
        }
    }
    for (id, caller, target, line, offset) in bare_calls {
        if !bindings.contains(&id)
            && !locals.get(&caller).is_some_and(|bindings| {
                bindings
                    .iter()
                    .any(|binding| binding.name == target && binding.visible.contains(&offset))
            })
        {
            result.references.push(Reference {
                caller,
                target,
                line,
            });
        }
    }
    if !ruby {
        let root = tree.root_node();
        let uncovered = (0..root.named_child_count()).any(|index| {
            let node = root.named_child(index).unwrap();
            node.kind() != "comment"
                && !result.symbols.iter().any(|symbol| {
                    symbol.start_byte <= node.start_byte() && symbol.end_byte >= node.end_byte()
                })
        });
        if uncovered
            || result
                .references
                .iter()
                .any(|reference| reference.caller.is_none())
        {
            let file_scope = result.symbols.len();
            result.symbols.push(file_symbol(path, source));
            for reference in &mut result.references {
                if reference.caller.is_none() {
                    reference.caller = Some(file_scope);
                }
            }
        }
    }
    Ok(result)
}

fn file_symbol(path: &Path, source: &str) -> Symbol {
    Symbol {
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        kind: "file".into(),
        start_line: 1,
        end_line: source.lines().count().max(1),
        start_byte: 0,
        end_byte: source.len(),
    }
}

fn ruby_bare_call(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        !["name", "method", "object"]
            .iter()
            .any(|field| parent.child_by_field_name(field) == Some(node))
    })
}

fn ruby_bindings(
    node: Node<'_>,
    source: &str,
    scope_end: usize,
    locals: &mut Vec<RubyBinding>,
    bindings: &mut HashSet<usize>,
) {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if node.kind() == "identifier" {
            locals.push(RubyBinding {
                name: text(node, source),
                visible: node.start_byte()..scope_end,
            });
            bindings.insert(node.id());
        } else if let Some(name) = node.child_by_field_name("name") {
            pending.push(name);
        } else if !matches!(node.kind(), "call" | "scope_resolution") {
            for index in 0..node.named_child_count() {
                let child = node.named_child(index).unwrap();
                if node.child_by_field_name("value") != Some(child)
                    && node.child_by_field_name("right") != Some(child)
                {
                    pending.push(child);
                }
            }
        }
    }
}

fn ruby_scope_end(mut node: Node<'_>) -> usize {
    loop {
        // Blocks/lambdas capture existing locals but do not export new bindings.
        if matches!(
            node.kind(),
            "block"
                | "do_block"
                | "lambda"
                | "method"
                | "singleton_method"
                | "class"
                | "module"
                | "program"
        ) {
            return node.end_byte();
        }
        match node.parent() {
            Some(parent) => node = parent,
            None => return node.end_byte(),
        }
    }
}

fn text(node: Node<'_>, source: &str) -> String {
    source[node.byte_range()].to_owned()
}

fn declaration(
    node: Node<'_>,
    source: &str,
    outside_function: bool,
    ruby: bool,
) -> Option<(String, &'static str)> {
    let kind = match node.kind() {
        "function_declaration"
        | "generator_function_declaration"
        | "function_item"
        | "function_signature_item" => "function",
        "method_definition" | "method_signature" | "abstract_method_signature" => "method",
        "class_declaration" | "abstract_class_declaration" => "class",
        "interface_declaration" => "interface",
        "struct_item" => "struct",
        "enum_item" | "enum_declaration" => "enum",
        "trait_item" => "trait",
        "type_item" | "type_alias_declaration" => "type",
        "const_item" | "static_item" => "constant",
        "mod_item" => "module",
        "class" if ruby => "class",
        "module" if ruby => "module",
        "method" | "singleton_method" if ruby => "method",
        "variable_declarator" | "let_declaration" => {
            let value = node.child_by_field_name("value");
            if value.is_some_and(|value| {
                matches!(
                    value.kind(),
                    "arrow_function"
                        | "function_expression"
                        | "generator_function"
                        | "closure_expression"
                )
            }) {
                "function"
            } else if node.kind() == "variable_declarator" && outside_function {
                "variable"
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let name = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("pattern"))?;
    if !(matches!(
        name.kind(),
        "identifier"
            | "type_identifier"
            | "property_identifier"
            | "private_property_identifier"
            | "field_identifier"
    ) || ruby
        && matches!(
            name.kind(),
            "constant" | "scope_resolution" | "setter" | "operator"
        ))
    {
        return None;
    }
    Some((text(name, source), kind))
}

fn call_name(mut node: Node<'_>, source: &str) -> Option<String> {
    loop {
        node = match node.kind() {
            "identifier"
            | "property_identifier"
            | "field_identifier"
            | "type_identifier"
            | "operator" => {
                return Some(text(node, source));
            }
            "member_expression" => node.child_by_field_name("property")?,
            "field_expression" => node.child_by_field_name("field")?,
            "scoped_identifier" | "scoped_type_identifier" => node.child_by_field_name("name")?,
            "generic_function" => node.child_by_field_name("function")?,
            "parenthesized_expression" | "non_null_expression" | "as_expression" => {
                node.named_child(0)?
            }
            // Computed properties and dynamically constructed callees remain unresolved.
            _ => return None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typescript_methods_and_nested_arrows_keep_their_callers() {
        let source = "export class Store {\n  save() { persist(); const retry = () => persist(); retry(); }\n}\nconst start = () => new Store().save();";
        let parsed = parse(Path::new("store.ts"), source).unwrap();
        assert!(!parsed.has_errors);
        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.name == "Store" && symbol.kind == "class")
        );
        let callers: Vec<_> = parsed
            .references
            .iter()
            .map(|reference| {
                (
                    reference
                        .caller
                        .map(|index| parsed.symbols[index].name.as_str()),
                    reference.target.as_str(),
                )
            })
            .collect();
        assert!(callers.contains(&(Some("save"), "persist")));
        assert!(callers.contains(&(Some("retry"), "persist")));
        assert!(callers.contains(&(Some("save"), "retry")));
        assert!(callers.contains(&(Some("start"), "save")));
        assert!(callers.contains(&(Some("start"), "Store")));
    }

    #[test]
    fn tsx_and_javascript_use_their_grammars() {
        let tsx = parse(
            Path::new("view.tsx"),
            "export const View = () => <p>{label()}</p>;",
        )
        .unwrap();
        assert!(!tsx.has_errors);
        assert_eq!(tsx.references[0].target, "label");
        for extension in ["js", "jsx", "mjs", "cjs"] {
            let js = parse(
                Path::new(&format!("view.{extension}")),
                "function render() { return <View/>; }",
            )
            .unwrap();
            assert!(!js.has_errors);
            assert_eq!(js.symbols[0].name, "render");
        }
    }

    #[test]
    fn rust_calls_and_unicode_offsets_match_source() {
        let source = "// café\nstruct Store;\nimpl Store { fn save(&self) { disk::write::<u8>(); self.flush(); } }\nfn café() { let retry = || Store::new(); retry(); }\n";
        let parsed = parse(Path::new("store.rs"), source).unwrap();
        assert!(!parsed.has_errors);
        let function = parsed
            .symbols
            .iter()
            .find(|symbol| symbol.name == "café")
            .unwrap();
        assert_eq!(function.start_line, 4);
        assert_eq!(function.end_line, 4);
        assert_eq!(
            &source[function.start_byte..function.end_byte],
            "fn café() { let retry = || Store::new(); retry(); }"
        );
        let calls: Vec<_> = parsed
            .references
            .iter()
            .map(|reference| {
                (
                    reference
                        .caller
                        .map(|index| parsed.symbols[index].name.as_str()),
                    reference.target.as_str(),
                )
            })
            .collect();
        assert!(calls.contains(&(Some("save"), "write")));
        assert!(calls.contains(&(Some("save"), "flush")));
        assert!(calls.contains(&(Some("retry"), "new")));
        assert!(calls.contains(&(Some("café"), "retry")));
    }

    #[test]
    fn incomplete_source_reports_errors_and_retains_valid_symbols() {
        let parsed = parse(
            Path::new("editing.ts"),
            "function intact() { done(); }\nfunction broken( {",
        )
        .unwrap();
        assert!(parsed.has_errors);
        assert!(parsed.symbols.iter().any(|symbol| symbol.name == "intact"));
        assert!(!supports(Path::new("image.png")));
        assert!(parse(Path::new("image.png"), "").is_err());
    }

    #[test]
    fn local_values_and_computed_calls_do_not_invent_symbols() {
        let parsed = parse(
            Path::new("code.ts"),
            "const global = 1; function run() { const local = 2; handlers[name](); }",
        )
        .unwrap();
        assert!(parsed.symbols.iter().any(|symbol| symbol.name == "global"));
        assert!(!parsed.symbols.iter().any(|symbol| symbol.name == "local"));
        assert!(parsed.references.is_empty());
    }

    #[test]
    fn ruby_methods_scopes_and_punctuation_preserve_lexical_calls() {
        let source = r#"module Admin
  class Store
    attr_reader :name
    def self.build(record)
      store = new(record)
      store.save!
      store.name = record
      store
    end
    def save!
      persist @record
      ready?
    end
    def ready?
      true
    end
    def name=(value)
      @name = value
    end
    def nested
      def inside
        touch()
      end
      finish()
    end
  end
end
"#;
        let parsed = parse(Path::new("store.rb"), source).unwrap();
        assert!(!parsed.has_errors);
        for (name, kind) in [
            ("Admin", "module"),
            ("Store", "class"),
            ("build", "method"),
            ("save!", "method"),
            ("name=", "method"),
        ] {
            assert!(
                parsed
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name == name && symbol.kind == kind)
            );
        }
        let calls: Vec<_> = parsed
            .references
            .iter()
            .map(|reference| {
                (
                    parsed.symbols[reference.caller.unwrap()].name.as_str(),
                    reference.target.as_str(),
                )
            })
            .collect();
        assert!(calls.contains(&("Store", "attr_reader")));
        for target in ["new", "save!", "name="] {
            assert!(
                calls.contains(&("build", target)),
                "missing {target}: {calls:?}"
            );
        }
        assert!(calls.contains(&("save!", "persist")));
        assert!(calls.contains(&("save!", "ready?")));
        assert!(calls.contains(&("inside", "touch")));
        assert!(calls.contains(&("nested", "finish")));
        assert!(
            !calls
                .iter()
                .any(|(_, target)| ["store", "record", "value"].contains(target))
        );
    }

    #[test]
    fn ruby_scripts_extensions_unicode_and_incomplete_source_are_supported() {
        for filename in [
            "app.rb",
            "tasks.rake",
            "sample.gemspec",
            "config.ru",
            "Gemfile",
            "Rakefile",
            "Guardfile",
        ] {
            assert!(supports(Path::new(filename)));
            let parsed = parse(Path::new(filename), "gem 'rake'\n").unwrap();
            assert_eq!(parsed.language, "ruby");
            assert_eq!(parsed.symbols[0].kind, "file");
            assert_eq!(parsed.references[0].target, "gem");
        }
        assert!(!supports(Path::new("view.html.erb")));
        assert!(!supports(Path::new("Gemfile.lock")));
        let source = "# café\nclass Admin::Store\n  def café?\n    true\n  end\nend\n";
        let parsed = parse(Path::new("unicode.rb"), source).unwrap();
        assert!(!parsed.has_errors);
        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.name == "Admin::Store")
        );
        let method = parsed
            .symbols
            .iter()
            .find(|symbol| symbol.name == "café?")
            .unwrap();
        assert_eq!((method.start_line, method.end_line), (3, 5));
        assert_eq!(
            &source[method.start_byte..method.end_byte],
            "def café?\n    true\n  end"
        );
        let incomplete = parse(
            Path::new("editing.rb"),
            "def intact; done(); end\ndef unfinished(",
        )
        .unwrap();
        assert!(incomplete.has_errors);
        assert!(
            incomplete
                .symbols
                .iter()
                .any(|symbol| symbol.name == "intact")
        );
    }

    #[test]
    fn ruby_parameters_and_local_bindings_are_not_bare_method_calls() {
        let source = r#"def run(input, optional = fallback, keyword: default_value, *rest, **options, &block)
  first, second = input
  state ||= optional
  input.each { |entry| consume entry }
  [first, second, state, keyword, rest, options, block]
end
"#;
        let parsed = parse(Path::new("bindings.rb"), source).unwrap();
        assert!(!parsed.has_errors);
        let targets: Vec<_> = parsed
            .references
            .iter()
            .map(|reference| reference.target.as_str())
            .collect();
        assert!(targets.contains(&"fallback"));
        assert!(targets.contains(&"default_value"));
        assert!(targets.contains(&"each"));
        assert!(targets.contains(&"consume"));
        for local in [
            "input", "optional", "keyword", "rest", "options", "block", "first", "second", "state",
            "entry",
        ] {
            assert!(
                !targets.contains(&local),
                "local {local} became a method call: {targets:?}"
            );
        }
    }

    #[test]
    fn ruby_bindings_follow_declaration_order_and_block_scope() {
        let source = r#"def run(items)
  items.each { |save| save }
  save
  publish
  publish = 1
  publish
  items.each do
    temporary = 1
    temporary
    publish
  end
  temporary
end
"#;
        let parsed = parse(Path::new("scopes.rb"), source).unwrap();
        assert!(!parsed.has_errors);
        let calls: Vec<_> = parsed
            .references
            .iter()
            .map(|reference| (reference.target.as_str(), reference.line))
            .collect();
        assert_eq!(
            calls
                .iter()
                .filter(|(name, _)| *name == "save")
                .copied()
                .collect::<Vec<_>>(),
            vec![("save", 3)]
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(name, _)| *name == "publish")
                .copied()
                .collect::<Vec<_>>(),
            vec![("publish", 4)]
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(name, _)| *name == "temporary")
                .copied()
                .collect::<Vec<_>>(),
            vec![("temporary", 12)]
        );
    }

    #[test]
    fn ruby_lambda_parameters_and_rescue_variables_remain_local() {
        let source = r#"def run
  callback = ->(item) { consume(item) }
  callback.call(1)
  item
  begin
    risky()
  rescue StandardError => error
    puts error
  end
  error
end
"#;
        let parsed = parse(Path::new("scopes.rb"), source).unwrap();
        assert!(!parsed.has_errors);
        let calls: Vec<_> = parsed
            .references
            .iter()
            .map(|reference| (reference.target.as_str(), reference.line))
            .collect();
        assert_eq!(
            calls
                .iter()
                .filter(|(name, _)| *name == "item")
                .copied()
                .collect::<Vec<_>>(),
            vec![("item", 4)]
        );
        assert!(
            !calls
                .iter()
                .any(|(name, _)| *name == "error" || *name == "callback")
        );
        for target in ["consume", "call", "risky", "puts"] {
            assert!(calls.iter().any(|(name, _)| *name == target));
        }
    }
}
