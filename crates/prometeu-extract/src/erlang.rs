//! Pinned Erlang forms, arities, macros and OTP call extraction.
//! Provenance: ../UPSTREAM.md; the grammar is the WhatsApp/ELP revision.
use crate::supplemental::{children, docstring, Context};
use crate::textutil::slice_utf16;
use crate::ExtractionResult;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub fn extract(path: &str, source: &str) -> Result<ExtractionResult, String> {
    crate::supplemental::run(path, source, "erlang", |context, root| {
        let mut state = State::new(context, root);
        if let Some(module) = children(root)
            .into_iter()
            .find(|node| node.kind() == "module_attribute")
        {
            if let Some(name) = module.child_by_field_name("name") {
                if let Some(namespace) =
                    context.create_node("namespace", atom(context, name), module, |_| {})
                {
                    context.push_scope(namespace);
                }
            }
        }
        state.walk(context, root);
    })
}

fn atom<'a>(context: &Context<'a>, node: Node<'_>) -> &'a str {
    let text = context.text(node);
    text.strip_prefix('\'')
        .and_then(|text| text.strip_suffix('\''))
        .unwrap_or(text)
}

fn collapsed(text: &str, cap: usize) -> String {
    slice_utf16(&text.split_whitespace().collect::<Vec<_>>().join(" "), cap).0
}

fn arity(node: Option<Node<'_>>) -> usize {
    node.map_or(0, |node| {
        children(node)
            .into_iter()
            .filter(|node| node.kind() != "comment")
            .count()
    })
}

fn named_arity(context: &Context<'_>, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("arity")
        .and_then(|node| node.child_by_field_name("value"))
        .map(|node| context.text(node).into())
}

struct State {
    exports: HashSet<String>,
    export_all: bool,
    last_function: Option<(String, usize, usize)>,
    macros: HashMap<String, String>,
}

impl State {
    fn new(context: &Context<'_>, root: Node<'_>) -> Self {
        let mut state = Self {
            exports: HashSet::new(),
            export_all: false,
            last_function: None,
            macros: HashMap::new(),
        };
        for form in children(root) {
            if form.kind() == "compile_options_attribute"
                && context.text(form).contains("export_all")
            {
                state.export_all = true;
            }
            if form.kind() == "export_attribute" {
                for function in children(form)
                    .into_iter()
                    .filter(|node| node.kind() == "fa")
                {
                    if let Some(name) = function.child_by_field_name("fun") {
                        let name = atom(context, name);
                        state.exports.insert(
                            named_arity(context, function)
                                .map_or_else(|| name.to_owned(), |arity| format!("{name}/{arity}")),
                        );
                    }
                }
            }
            if form.kind() == "pp_define" {
                let name = form
                    .child_by_field_name("lhs")
                    .and_then(|node| node.child_by_field_name("name"));
                let replacement = form.child_by_field_name("replacement");
                if let (Some(name), Some(replacement)) = (name, replacement) {
                    if replacement.kind() == "atom" {
                        state
                            .macros
                            .insert(context.text(name).into(), atom(context, replacement).into());
                    } else if replacement.kind() == "macro_call_expr"
                        && replacement
                            .child_by_field_name("name")
                            .is_some_and(|node| context.text(node) == "MODULE")
                    {
                        state
                            .macros
                            .insert(context.text(name).into(), self_module(context).into());
                    }
                }
            }
        }
        state
    }

    fn function(&mut self, context: &mut Context<'_>, node: Node<'_>) {
        let clauses: Vec<_> = children(node)
            .into_iter()
            .filter(|node| node.kind() == "function_clause")
            .collect();
        let Some(first) = clauses.first().copied() else {
            return;
        };
        let Some(name) = first
            .child_by_field_name("name")
            .map(|name| atom(context, name))
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        let count = arity(first.child_by_field_name("args"));
        if let Some((old_name, old_count, index)) = &self.last_function {
            if old_name == name && *old_count == count {
                context.result.nodes[*index].end_line = context.result.nodes[*index]
                    .end_line
                    .max(node.end_position().row as u32 + 1);
                context.push_scope(*index);
                for clause in clauses {
                    self.body(context, clause);
                }
                context.pop_scope();
                return;
            }
        }
        let mut previous = node.prev_named_sibling();
        while previous.is_some_and(|node| node.kind() == "comment") {
            previous = previous.and_then(|node| node.prev_named_sibling());
        }
        let spec = previous.filter(|spec| {
            if spec.kind() != "spec"
                || !spec
                    .child_by_field_name("fun")
                    .is_some_and(|function| atom(context, function) == name)
            {
                return false;
            }
            let signatures: Vec<_> = children(*spec)
                .into_iter()
                .filter(|node| node.kind() == "type_sig")
                .collect();
            signatures.is_empty()
                || signatures
                    .iter()
                    .any(|node| arity(node.child_by_field_name("args")) == count)
        });
        let docs = docstring(spec.unwrap_or(node), context.source);
        let signature = if let Some(spec) = spec {
            Some(collapsed(context.text(spec), 300))
        } else {
            let end = first
                .child_by_field_name("body")
                .map_or(first.end_byte(), |body| body.start_byte());
            let header = collapsed(&context.source[first.start_byte()..end], usize::MAX);
            (!header.is_empty()).then_some(header)
        };
        let exported = self.export_all
            || self.exports.contains(name)
            || self.exports.contains(&format!("{name}/{count}"));
        let Some(index) = context.create_node("function", name, node, |node| {
            node.docstring = docs;
            node.signature = signature;
            node.is_exported = Some(exported);
            node.qualified_name.push_str(&format!("/{count}"));
        }) else {
            return;
        };
        context.push_scope(index);
        for clause in clauses {
            self.body(context, clause);
        }
        context.pop_scope();
        self.last_function = Some((name.into(), count, index));
    }

    fn record(&mut self, context: &mut Context<'_>, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let docs = docstring(node, context.source);
        let signature = collapsed(context.text(node), 300);
        if let Some(record) = context.create_node("struct", atom(context, name), node, |node| {
            node.docstring = docs;
            node.signature = Some(signature);
        }) {
            context.push_scope(record);
            for field in children(node)
                .into_iter()
                .filter(|node| node.kind() == "record_field")
            {
                if let Some(name) = field.child_by_field_name("name") {
                    context.create_node("field", atom(context, name), field, |_| {});
                }
            }
            context.pop_scope();
        }
    }

    fn app_resource(&mut self, context: &mut Context<'_>, node: Node<'_>) {
        let Some(owner) = context.current() else {
            return;
        };
        let Some(props) = node.named_child(2).filter(|node| node.kind() == "list") else {
            return;
        };
        for property in children(props)
            .into_iter()
            .filter(|node| node.kind() == "tuple" && node.named_child_count() >= 2)
        {
            let Some(key) = property.named_child(0).filter(|node| node.kind() == "atom") else {
                continue;
            };
            let value = property.named_child(1).unwrap();
            match atom(context, key) {
                "mod" if value.kind() == "tuple" => {
                    if let Some(module) = value.named_child(0).filter(|node| node.kind() == "atom")
                    {
                        let name = atom(context, module);
                        if !name.is_empty() {
                            context.reference(owner, name, "references", module);
                        }
                    }
                }
                "applications" | "included_applications" if value.kind() == "list" => {
                    for app in children(value)
                        .into_iter()
                        .filter(|node| node.kind() == "atom")
                    {
                        let name = atom(context, app);
                        if !name.is_empty() {
                            context.reference(owner, name, "imports", app);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn walk(&mut self, context: &mut Context<'_>, node: Node<'_>) {
        stack_guard!();
        match node.kind() {
            "fun_decl" => {
                self.function(context, node);
                return;
            }
            "record_decl" => {
                self.record(context, node);
                return;
            }
            "type_alias" | "opaque" => {
                if let Some(name) = node
                    .child_by_field_name("name")
                    .and_then(|node| node.child_by_field_name("name"))
                {
                    let signature = collapsed(context.text(node), 200);
                    context.create_node("type_alias", atom(context, name), node, |node| {
                        node.signature = Some(signature)
                    });
                }
                return;
            }
            "pp_define" => {
                let Some(name) = node
                    .child_by_field_name("lhs")
                    .and_then(|node| node.child_by_field_name("name"))
                else {
                    return;
                };
                let signature = collapsed(context.text(node), 200);
                if let Some(index) =
                    context.create_node("constant", context.text(name), node, |node| {
                        node.signature = Some(signature)
                    })
                {
                    if let Some(replacement) = node.child_by_field_name("replacement") {
                        context.push_scope(index);
                        self.body(context, replacement);
                        context.pop_scope();
                    }
                }
                return;
            }
            "behaviour_attribute" => {
                if let (Some(name), Some(owner)) =
                    (node.child_by_field_name("name"), context.current())
                {
                    context.reference(owner, atom(context, name), "implements", node);
                }
                return;
            }
            "spec" | "callback" => return,
            "tuple"
                if node
                    .parent()
                    .is_some_and(|node| node.kind() == "source_file")
                    && (context.path.to_ascii_lowercase().ends_with(".app")
                        || context.path.to_ascii_lowercase().ends_with(".app.src"))
                    && node.named_child(0).is_some_and(|node| {
                        node.kind() == "atom" && atom(context, node) == "application"
                    }) =>
            {
                self.app_resource(context, node);
                return;
            }
            "import_attribute" | "pp_include" | "pp_include_lib" => {
                let module = node.child_by_field_name(if node.kind() == "import_attribute" {
                    "module"
                } else {
                    "file"
                });
                if let Some(module) = module {
                    let name = if node.kind() == "import_attribute" {
                        atom(context, module)
                    } else {
                        context
                            .text(module)
                            .trim_start_matches('"')
                            .trim_end_matches('"')
                    };
                    if !name.is_empty() {
                        let signature = if node.kind() == "import_attribute" {
                            collapsed(context.text(node), 200)
                        } else {
                            context.text(node).trim().into()
                        };
                        context.create_node("import", name, node, |node| {
                            node.signature = Some(signature)
                        });
                        if let Some(owner) = context.current() {
                            context.reference(owner, name, "imports", node);
                        }
                    }
                }
            }
            _ => self.call(context, node),
        }
        for child in children(node) {
            self.walk(context, child);
        }
    }

    fn body(&mut self, context: &mut Context<'_>, node: Node<'_>) {
        stack_guard!();
        self.call(context, node);
        for child in children(node) {
            self.body(context, child);
        }
    }

    fn call(&self, context: &mut Context<'_>, node: Node<'_>) {
        let Some(owner) = context.current() else {
            return;
        };
        match node.kind() {
            "call" => {
                let mut callee = node.child_by_field_name("expr");
                let module =
                    if let Some(parent) = node.parent().filter(|node| node.kind() == "remote") {
                        parent.child_by_field_name("module")
                    } else if let Some(remote) = callee.filter(|node| node.kind() == "remote") {
                        callee = remote.child_by_field_name("fun");
                        remote.child_by_field_name("module")
                    } else {
                        None
                    };
                let Some(callee) = callee.filter(|node| node.kind() == "atom") else {
                    return;
                };
                let name = atom(context, callee);
                let module_expression = module.and_then(|node| node.child_by_field_name("module"));
                let module_atom = module_expression
                    .filter(|node| node.kind() == "atom")
                    .map(|node| atom(context, node));
                if let Some(module) = module_expression {
                    if module_atom.is_none()
                        && !(module.kind() == "macro_call_expr"
                            && module
                                .child_by_field_name("name")
                                .is_some_and(|node| context.text(node) == "MODULE"))
                    {
                        return;
                    }
                }
                let qualified = module_atom
                    .map_or_else(|| name.to_owned(), |module| format!("{module}::{name}"));
                let count = arity(node.child_by_field_name("args"));
                context.reference(owner, &format!("{qualified}/{count}"), "calls", node);
                if module_atom == Some("gen_server")
                    && matches!(name, "call" | "cast" | "send_request")
                {
                    if let Some(target) = node
                        .child_by_field_name("args")
                        .and_then(|args| args.named_child(0))
                    {
                        if let Some(module) = self.server_target(context, target) {
                            let callback = if name == "cast" {
                                "handle_cast/2"
                            } else {
                                "handle_call/3"
                            };
                            context.reference(
                                owner,
                                &format!("{module}::{callback}"),
                                "calls",
                                node,
                            );
                        }
                    }
                }
                let family = module_atom
                    .map_or_else(|| name.to_owned(), |module| format!("{module}:{name}"));
                if mfa_family(&family) {
                    let arguments = node
                        .child_by_field_name("args")
                        .map(children)
                        .unwrap_or_default();
                    for (index, pair) in arguments.windows(2).enumerate() {
                        let (module, function) = (pair[0], pair[1]);
                        if function.kind() != "atom" {
                            continue;
                        }
                        let local = module.kind() == "macro_call_expr"
                            && module
                                .child_by_field_name("name")
                                .is_some_and(|node| context.text(node) == "MODULE");
                        if module.kind() != "atom" && !local {
                            continue;
                        }
                        let suffix = arguments
                            .get(index + 2)
                            .filter(|node| node.kind() == "list")
                            .map_or_else(String::new, |list| format!("/{}", arity(Some(*list))));
                        let name = if local {
                            atom(context, function).to_owned()
                        } else {
                            format!("{}::{}", atom(context, module), atom(context, function))
                        };
                        context.reference(owner, &format!("{name}{suffix}"), "calls", function);
                        break;
                    }
                }
            }
            "internal_fun" | "external_fun" => {
                let Some(function) = node
                    .child_by_field_name("fun")
                    .filter(|node| node.kind() == "atom")
                else {
                    return;
                };
                let mut name = atom(context, function).to_owned();
                if node.kind() == "external_fun" {
                    let Some(module) = node
                        .child_by_field_name("module")
                        .and_then(|module| module.child_by_field_name("name"))
                        .filter(|node| node.kind() == "atom")
                    else {
                        return;
                    };
                    name = format!("{}::{name}", atom(context, module));
                }
                if let Some(arity) = named_arity(context, node) {
                    name.push_str(&format!("/{arity}"));
                }
                context.reference(owner, &name, "references", node);
            }
            "macro_call_expr" => {
                let Some(name) = node
                    .child_by_field_name("name")
                    .map(|name| context.text(name))
                else {
                    return;
                };
                if matches!(
                    name,
                    "MODULE"
                        | "MODULE_STRING"
                        | "FILE"
                        | "LINE"
                        | "MACHINE"
                        | "FUNCTION_NAME"
                        | "FUNCTION_ARITY"
                        | "OTP_RELEASE"
                        | "FEATURE_AVAILABLE"
                        | "FEATURE_ENABLED"
                ) {
                    return;
                }
                let kind = if node.child_by_field_name("args").is_some() {
                    "calls"
                } else {
                    "references"
                };
                context.reference(owner, name, kind, node);
            }
            "record_expr" | "record_update_expr" | "record_index_expr" | "record_field_expr" => {
                if let Some(name) = node
                    .child_by_field_name("name")
                    .filter(|node| node.kind() == "record_name")
                    .and_then(|node| node.child_by_field_name("name"))
                    .filter(|node| node.kind() == "atom")
                {
                    context.reference(owner, atom(context, name), "references", node);
                }
            }
            _ => {}
        }
    }

    fn server_target(&self, context: &Context<'_>, target: Node<'_>) -> Option<String> {
        if target.kind() == "atom" {
            return Some(atom(context, target).into()).filter(|name: &String| !name.is_empty());
        }
        if target.kind() != "macro_call_expr" {
            return None;
        }
        let name = context.text(target.child_by_field_name("name")?);
        if name == "MODULE" {
            return Some(self_module(context).into()).filter(|name: &String| !name.is_empty());
        }
        self.macros
            .get(name)
            .cloned()
            .filter(|name| !name.is_empty())
    }
}

fn self_module<'a>(context: &Context<'a>) -> &'a str {
    let file = context.path.rsplit('/').next().unwrap_or("");
    file.strip_suffix(".erl").unwrap_or(file)
}

fn mfa_family(name: &str) -> bool {
    matches!(
        name,
        "spawn"
            | "spawn_link"
            | "spawn_monitor"
            | "spawn_opt"
            | "apply"
            | "erlang:spawn"
            | "erlang:spawn_link"
            | "erlang:spawn_monitor"
            | "erlang:spawn_opt"
            | "erlang:apply"
            | "proc_lib:spawn"
            | "proc_lib:spawn_link"
            | "proc_lib:spawn_opt"
            | "proc_lib:start"
            | "proc_lib:start_link"
            | "timer:apply_after"
            | "timer:apply_interval"
            | "rpc:call"
            | "rpc:cast"
            | "rpc:async_call"
            | "erpc:call"
            | "erpc:cast"
    )
}
