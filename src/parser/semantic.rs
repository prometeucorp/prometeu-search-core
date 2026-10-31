use super::{Import, ParsedFile, Reference, declaration, text};
use std::collections::HashMap;
use std::ops::Range;
use tree_sitter::Node;

struct Binding {
    name: String,
    receiver_type: Option<String>,
    inferred: bool,
    visible: Range<usize>,
    shadow_range: Range<usize>,
    declared_at: usize,
    callable: bool,
}

pub(super) fn enrich(
    root: Node<'_>,
    source: &str,
    parsed: &mut ParsedFile,
    calls: &[(usize, Node<'_>)],
) {
    let symbols: HashMap<_, _> = parsed
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.kind != "file")
        .map(|(index, symbol)| ((symbol.start_byte, symbol.end_byte), index))
        .collect();
    let mut bindings: HashMap<String, Vec<Binding>> = HashMap::new();
    let mut exports = Vec::new();
    let mut inheritance = Vec::new();
    let mut pending = vec![(root, None)];
    while let Some((node, parent)) = pending.pop() {
        // Most syntax nodes never need an enclosing semantic scope.
        let scope = std::cell::LazyCell::new(|| scope(node, source));
        if let Some(&index) = symbols.get(&(node.start_byte(), node.end_byte())) {
            let symbol = &mut parsed.symbols[index];
            symbol.qualified_name = qualify(&scope, &symbol.name);
            symbol.container = (!scope.is_empty()).then_some(scope.clone());
            if parsed.language == "ruby" || has_child(node, "visibility_modifier") {
                symbol.exports.push(symbol.name.clone());
            } else if let Some(export) = export_parent(node) {
                symbol
                    .exports
                    .push(if text(export, source).starts_with("export default") {
                        "default".into()
                    } else {
                        symbol.name.clone()
                    });
            }
        }
        match node.kind() {
            "import_statement" | "export_statement" => {
                javascript_imports(node, source, &scope, &mut parsed.imports, &mut exports);
            }
            "use_declaration" => {
                if let Some(argument) = node.child_by_field_name("argument") {
                    rust_imports(
                        argument,
                        source,
                        &scope,
                        has_child(node, "visibility_modifier"),
                        &mut parsed.imports,
                    );
                }
            }
            "mod_item" if node.child_by_field_name("body").is_none() => {
                if let Some(name) = node.child_by_field_name("name") {
                    let name = text(name, source);
                    parsed.imports.push(Import {
                        local: name.clone(),
                        imported: "*".into(),
                        module: format!("self::{name}"),
                        scope: scope.clone(),
                        reexport: has_child(node, "visibility_modifier"),
                    });
                }
            }
            "call" if parsed.language == "ruby" => {
                if node.child_by_field_name("receiver").is_none()
                    && node
                        .child_by_field_name("method")
                        .is_some_and(|method| text(method, source) == "require_relative")
                    && let Some(module) = node
                        .child_by_field_name("arguments")
                        .and_then(|arguments| arguments.named_child(0))
                        .and_then(|argument| literal(argument, source))
                {
                    parsed.imports.push(Import {
                        local: "*".into(),
                        imported: "*".into(),
                        module: format!("./{}", module.trim_start_matches("./")),
                        scope: scope.clone(),
                        reexport: false,
                    });
                }
            }
            "extends_clause" | "implements_clause" | "extends_type_clause" | "superclass" => {
                let kind = if node.kind() == "implements_clause" {
                    "implements"
                } else {
                    "extends"
                };
                for child in children(node) {
                    if child.kind() != "type_arguments"
                        && let Some(target) = type_name(child, source)
                    {
                        inheritance.push((
                            scope.clone(),
                            target,
                            kind,
                            node.start_position().row + 1,
                        ));
                    }
                }
            }
            "class_heritage" if parsed.language == "javascript" => {
                for child in children(node) {
                    if let Some(target) = type_name(child, source) {
                        inheritance.push((
                            scope.clone(),
                            target,
                            "extends",
                            node.start_position().row + 1,
                        ));
                    }
                }
            }
            "impl_item" => {
                if let (Some(owner), Some(target)) = (
                    node.child_by_field_name("type")
                        .and_then(|node| type_name(node, source)),
                    node.child_by_field_name("trait")
                        .and_then(|node| type_name(node, source)),
                ) {
                    inheritance.push((
                        qualify(&scope, &owner),
                        target,
                        "implements",
                        node.start_position().row + 1,
                    ));
                }
            }
            _ => {}
        }
        for binding in bindings_at(node, parent, source) {
            bindings
                .entry(binding.name.clone())
                .or_default()
                .push(binding);
        }
        pending.extend(
            children(node)
                .into_iter()
                .rev()
                .map(|child| (child, Some(node))),
        );
    }
    let mut qualified: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, symbol) in parsed.symbols.iter().enumerate() {
        qualified
            .entry(symbol.qualified_name.clone())
            .or_default()
            .push(index);
    }
    for (name, exported, export_scope) in exports {
        for &index in qualified
            .get(&qualify(&export_scope, &name))
            .into_iter()
            .flatten()
        {
            let symbol = &mut parsed.symbols[index];
            if !symbol.exports.contains(&exported) {
                symbol.exports.push(exported.clone());
            }
        }
        // Exporting an imported binding retains its original module and name.
        let imports: Vec<_> = parsed
            .imports
            .iter()
            .filter(|import| {
                !import.reexport && import.local == name && import.scope == export_scope
            })
            .cloned()
            .collect();
        for mut import in imports {
            import.local = exported.clone();
            import.reexport = true;
            parsed.imports.push(import);
        }
    }
    for &(index, node) in calls {
        let reference = &mut parsed.references[index];
        let receiver = receiver(node);
        let current_scope = scope(node, source);
        let active_binding = |name: &str, offset: usize| {
            bindings
                .get(name)
                .into_iter()
                .flatten()
                .filter(|binding| binding.shadow_range.contains(&offset))
                .max_by_key(|binding| (binding.shadow_range.start, binding.declared_at))
        };
        let imported = |name: &str| {
            parsed.imports.iter().any(|import| {
                (!import.reexport || parsed.language == "rust")
                    && import.local == name
                    && (import.scope.is_empty()
                        || current_scope == import.scope
                        || current_scope.starts_with(&format!("{}::", import.scope)))
            })
        };
        reference.qualifier = receiver.map(|receiver| normalize(&text(receiver, source)));
        if parsed.language != "ruby"
            && receiver.is_none()
            && let Some(binding) = active_binding(&reference.target, node.start_byte())
        {
            reference.shadowed = !binding.callable || node.start_byte() < binding.declared_at;
        }
        reference.receiver_type = receiver.and_then(|receiver| {
            let name = text(receiver, source);
            if matches!(name.as_str(), "this" | "self" | "Self") {
                return owner(node, source);
            }
            if let Some(receiver_type) = constructed_type(receiver, source) {
                let constructor = receiver_type.split("::").next().unwrap();
                if active_binding(constructor, node.start_byte()).is_some() {
                    // Construction follows value bindings, not the imported type namespace.
                    reference.shadowed = imported(constructor);
                    return None;
                }
                return Some(receiver_type);
            }
            let binding = active_binding(&name, node.start_byte())?;
            let receiver_type = binding
                .visible
                .contains(&node.start_byte())
                .then(|| binding.receiver_type.clone())
                .flatten();
            if binding.inferred
                && let Some(constructor) = receiver_type
                    .as_deref()
                    .and_then(|name| name.split("::").next())
                && active_binding(constructor, binding.declared_at).is_some()
            {
                reference.shadowed = imported(constructor);
                return None;
            }
            if receiver_type.is_none() && imported(&name) {
                reference.shadowed = true;
            }
            receiver_type
        });
    }
    for (owner, target, kind, line) in inheritance {
        let (qualifier, target) = split_name(&target);
        let callers: Vec<_> = match qualified.get(&owner) {
            Some(indices) => indices.iter().copied().map(Some).collect(),
            None => vec![None],
        };
        for caller in callers {
            parsed.references.push(Reference {
                caller,
                target: target.clone(),
                line,
                qualifier: qualifier.clone(),
                kind: kind.into(),
                receiver_type: None,
                shadowed: false,
                owner: Some(owner.clone()),
            });
        }
    }
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn has_child(node: Node<'_>, kind: &str) -> bool {
    children(node).iter().any(|child| child.kind() == kind)
}

fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() || name.starts_with(&format!("{scope}::")) {
        name.to_string()
    } else {
        format!("{scope}::{name}")
    }
}

fn normalize(name: &str) -> String {
    name.trim().replace('.', "::")
}

fn split_name(name: &str) -> (Option<String>, String) {
    match name.rsplit_once("::") {
        Some((qualifier, name)) => (Some(qualifier.to_string()), name.to_string()),
        None => (None, name.to_string()),
    }
}

fn scope(node: Node<'_>, source: &str) -> String {
    let mut scopes = Vec::new();
    let mut ancestor = node.parent();
    while let Some(node) = ancestor {
        if node.kind() == "impl_item" {
            if let Some(name) = node
                .child_by_field_name("type")
                .and_then(|node| type_name(node, source))
            {
                scopes.push(name);
            }
        } else if let Some((name, kind)) = declaration(node, source, false, true)
            && matches!(
                kind,
                "function"
                    | "method"
                    | "class"
                    | "interface"
                    | "module"
                    | "trait"
                    | "struct"
                    | "enum"
            )
        {
            scopes.push(name);
        }
        ancestor = node.parent();
    }
    scopes
        .into_iter()
        .rev()
        .fold(String::new(), |scope, name| qualify(&scope, &name))
}

fn owner(mut node: Node<'_>, source: &str) -> Option<String> {
    loop {
        let name = if node.kind() == "impl_item" {
            node.child_by_field_name("type")
        } else if matches!(
            node.kind(),
            "class_declaration" | "class" | "abstract_class_declaration" | "trait_item" | "module"
        ) {
            node.child_by_field_name("name")
        } else {
            None
        };
        if let Some(name) = name.and_then(|name| type_name(name, source)) {
            return Some(qualify(&scope(node, source), &name));
        }
        node = node.parent()?;
    }
}

fn export_parent(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        let parent = node.parent()?;
        if parent.kind() == "export_statement" {
            return Some(parent);
        }
        if !matches!(
            parent.kind(),
            "lexical_declaration" | "variable_declaration"
        ) {
            return None;
        }
        node = parent;
    }
}

fn literal(node: Node<'_>, source: &str) -> Option<String> {
    let value = text(node, source);
    let quote = value.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"') || value.as_bytes().last() != Some(quote) || value.len() < 2 {
        return None;
    }
    let value = &value[1..value.len() - 1];
    (!value.contains(['\\', '#', '$'])).then(|| value.to_string())
}

fn javascript_imports(
    node: Node<'_>,
    source: &str,
    scope: &str,
    imports: &mut Vec<Import>,
    exports: &mut Vec<(String, String, String)>,
) {
    let module = node
        .child_by_field_name("source")
        .and_then(|node| literal(node, source));
    let reexport = node.kind() == "export_statement";
    let mut pending = children(node);
    let mut found = false;
    while let Some(part) = pending.pop() {
        let binding = match part.kind() {
            "import_specifier" | "export_specifier" => {
                let Some(name) = part.child_by_field_name("name") else {
                    continue;
                };
                let imported = text(name, source).trim_matches(['\'', '"']).to_string();
                let local = part
                    .child_by_field_name("alias")
                    .map(|node| text(node, source))
                    .unwrap_or_else(|| imported.clone());
                Some((local, imported))
            }
            "namespace_import" | "namespace_export" => part
                .named_child(0)
                .map(|name| (text(name, source), "*".into())),
            "identifier"
                if part
                    .parent()
                    .is_some_and(|parent| parent.kind() == "import_clause") =>
            {
                Some((text(part, source), "default".into()))
            }
            "import_clause" | "named_imports" | "export_clause" => {
                pending.extend(children(part));
                None
            }
            _ => None,
        };
        if let Some((local, imported)) = binding {
            found = true;
            if let Some(module) = &module {
                imports.push(Import {
                    local,
                    imported,
                    module: module.clone(),
                    scope: scope.into(),
                    reexport,
                });
            } else if reexport {
                exports.push((imported, local, scope.into()));
            }
        }
    }
    if let Some(module) = module
        && !found
    {
        imports.push(Import {
            local: if reexport { "*" } else { "" }.into(),
            imported: if reexport { "*" } else { "" }.into(),
            module,
            scope: scope.into(),
            reexport,
        });
    }
    if reexport
        && text(node, source).starts_with("export default")
        && let Some(value) = node.child_by_field_name("value")
        && value.kind() == "identifier"
    {
        exports.push((text(value, source), "default".into(), scope.into()));
    }
}

fn rust_imports(
    node: Node<'_>,
    source: &str,
    scope: &str,
    reexport: bool,
    imports: &mut Vec<Import>,
) {
    let mut pending = vec![(node, String::new())];
    while let Some((node, prefix)) = pending.pop() {
        match node.kind() {
            "scoped_use_list" => {
                let prefix = node
                    .child_by_field_name("path")
                    .map(|node| qualify(&prefix, &text(node, source)))
                    .unwrap_or(prefix);
                if let Some(list) = node.child_by_field_name("list") {
                    pending.push((list, prefix));
                }
            }
            "use_list" => pending.extend(
                children(node)
                    .into_iter()
                    .rev()
                    .map(|child| (child, prefix.clone())),
            ),
            _ => {
                let path = if node.kind() == "use_as_clause" {
                    node.child_by_field_name("path")
                        .map(|node| text(node, source))
                        .unwrap_or_default()
                } else {
                    text(node, source)
                };
                let path = qualify(&prefix, &path);
                let path = path.trim_end_matches("::self");
                let (module, imported) = path.rsplit_once("::").unwrap_or(("self", path));
                let local = node
                    .child_by_field_name("alias")
                    .map(|node| text(node, source))
                    .unwrap_or_else(|| imported.into());
                imports.push(Import {
                    local,
                    imported: imported.into(),
                    module: module.into(),
                    scope: scope.into(),
                    reexport,
                });
            }
        }
    }
}

fn type_name(mut node: Node<'_>, source: &str) -> Option<String> {
    loop {
        match node.kind() {
            "type_identifier"
            | "identifier"
            | "constant"
            | "scoped_identifier"
            | "scoped_type_identifier"
            | "nested_type_identifier"
            | "scope_resolution"
            | "member_expression"
            | "self" => return Some(normalize(&text(node, source))),
            "generic_type" | "generic_type_with_turbofish" | "reference_type" => {
                node = node.child_by_field_name("type")?
            }
            "type_annotation" | "parenthesized_type" | "superclass" => {
                node = node.named_child(0)?
            }
            _ => return None,
        }
    }
}

fn receiver(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "call" {
        return node.child_by_field_name("receiver");
    }
    let mut function = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"))?;
    loop {
        match function.kind() {
            "member_expression" | "field_expression" => {
                return function
                    .child_by_field_name("object")
                    .or_else(|| function.child_by_field_name("value"));
            }
            "scoped_identifier" | "scoped_type_identifier" => {
                return function.child_by_field_name("path");
            }
            "generic_function" => function = function.child_by_field_name("function")?,
            "parenthesized_expression" | "non_null_expression" | "as_expression" => {
                function = function.named_child(0)?
            }
            _ => return None,
        }
    }
}

fn constructed_type(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "new_expression" => node
            .child_by_field_name("constructor")
            .and_then(|node| type_name(node, source)),
        "struct_expression" => node
            .child_by_field_name("name")
            .and_then(|node| type_name(node, source)),
        "call_expression" => {
            let function = node.child_by_field_name("function")?;
            let name = function.child_by_field_name("name")?;
            if text(name, source) != "new" {
                return None;
            }
            function
                .child_by_field_name("path")
                .and_then(|node| type_name(node, source))
        }
        "call"
            if node
                .child_by_field_name("method")
                .is_some_and(|node| text(node, source) == "new") =>
        {
            node.child_by_field_name("receiver")
                .and_then(|node| type_name(node, source))
        }
        _ => None,
    }
}

fn bindings_at(node: Node<'_>, parent: Option<Node<'_>>, source: &str) -> Vec<Binding> {
    let loop_declaration = node.kind() == "for_in_statement"
        && node
            .child_by_field_name("kind")
            .is_some_and(|kind| matches!(kind.kind(), "let" | "const" | "var"));
    let plain_parameter = parent.is_some_and(|parent| {
        parent.kind() == "formal_parameters"
            && !matches!(node.kind(), "required_parameter" | "optional_parameter")
            || parent.kind() == "arrow_function"
                && parent.child_by_field_name("parameter") == Some(node)
    });
    if !plain_parameter
        && !loop_declaration
        && !matches!(
            node.kind(),
            "variable_declarator"
                | "let_declaration"
                | "parameter"
                | "required_parameter"
                | "optional_parameter"
                | "assignment"
                | "assignment_expression"
                | "catch_clause"
        )
    {
        return Vec::new();
    }
    let pattern = if plain_parameter {
        Some(node)
    } else {
        node.child_by_field_name("name")
            .or_else(|| node.child_by_field_name("pattern"))
            .or_else(|| node.child_by_field_name("left"))
            .or_else(|| node.child_by_field_name("parameter"))
    };
    let Some(pattern) = pattern else {
        return Vec::new();
    };
    let names = pattern_names(pattern);
    if names.is_empty() {
        return Vec::new();
    }
    // The iterable is not the initializer of each loop binding.
    let value = (!loop_declaration)
        .then(|| {
            node.child_by_field_name("value")
                .or_else(|| node.child_by_field_name("right"))
        })
        .flatten();
    // A pattern's annotation/initializer describes the container, not every member.
    let annotation = (pattern.kind() == "identifier")
        .then(|| {
            node.child_by_field_name("type")
                .and_then(|node| type_name(node, source))
        })
        .flatten();
    let construction = (pattern.kind() == "identifier")
        .then(|| value.and_then(|node| constructed_type(node, source)))
        .flatten();
    let inferred = annotation.is_none() && construction.is_some();
    let receiver_type = annotation.or(construction);
    let callable = pattern.kind() == "identifier"
        && value.is_some_and(|value| {
            matches!(
                value.kind(),
                "arrow_function"
                    | "function_expression"
                    | "generator_function"
                    | "closure_expression"
            )
        });
    let hoisted = node.kind() == "variable_declarator"
        || loop_declaration
        || plain_parameter
        || matches!(node.kind(), "required_parameter" | "optional_parameter");
    let function_scoped = (node.kind() == "variable_declarator"
        && parent.is_some_and(|parent| parent.kind() == "variable_declaration"))
        || (loop_declaration
            && node
                .child_by_field_name("kind")
                .is_some_and(|kind| kind.kind() == "var"));
    let mut parent = node;
    let region = loop {
        if !function_scoped
            && (loop_declaration || node.kind() == "variable_declarator")
            && matches!(parent.kind(), "for_statement" | "for_in_statement")
        {
            break parent.byte_range();
        }
        if matches!(
            parent.kind(),
            "statement_block"
                | "block"
                | "do_block"
                | "program"
                | "source_file"
                | "method"
                | "singleton_method"
                | "class"
                | "module"
                | "function_declaration"
                | "function_expression"
                | "arrow_function"
                | "function_item"
                | "method_definition"
                | "catch_clause"
        ) && !(function_scoped
            && matches!(parent.kind(), "statement_block" | "block" | "catch_clause"))
        {
            break parent.byte_range();
        }
        let Some(ancestor) = parent.parent() else {
            return Vec::new();
        };
        parent = ancestor;
    };
    names
        .into_iter()
        .map(|name| Binding {
            name: text(name, source),
            receiver_type: receiver_type.clone(),
            inferred,
            visible: node.start_byte()..region.end,
            shadow_range: if hoisted {
                region.clone()
            } else {
                node.start_byte()..region.end
            },
            declared_at: node.start_byte(),
            callable,
        })
        .collect()
}

fn pattern_names(pattern: Node<'_>) -> Vec<Node<'_>> {
    let mut names = Vec::new();
    let mut pending = vec![pattern];
    while let Some(node) = pending.pop() {
        match node.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => names.push(node),
            "object_pattern" | "array_pattern" | "rest_pattern" => {
                pending.extend(children(node));
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                pending.extend(node.child_by_field_name("left"));
            }
            "pair_pattern" => pending.extend(node.child_by_field_name("value")),
            _ => {}
        }
    }
    names
}
