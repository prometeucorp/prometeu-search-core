use super::*;

const NOT_ROUTINES: &[&str] = &[
    "self", "result", "true", "false", "nil", "exit", "break", "continue",
];
const SYSTEM: &str = "abs addr arctan assert assigned assignfile atomiccmpexchange atomicdecrement atomicexchange atomicincrement blockread blockwrite break chdir chr closefile concat continue copy cos dec default delete dispose eof eoln exclude exit exp fail filepos filesize fillchar finalize frac freemem getdir getmem gettypekind halt hasweakref hi high inc include initialize insert int ioresult isconstvalue ismanagedtype length ln lo low mkdir move new odd ord paramcount paramstr pi pred ptr random randomize read readln reallocmem returnaddress rmdir round runerror setlength setstring sin sizeof slice sqr sqrt str succ swap trunc typehandle typeinfo typeof uniquestring upcase val vararrayredim write writeln abort date datetostr floattostr format freeandnil inttostr lowercase now pos strtodate strtofloat strtoint time trim uppercase ansichar ansistring boolean byte bytebool cardinal char comp currency double extended int16 int32 int64 int8 integer longbool longint longword nativeint nativeuint olevariant pansichar pchar pointer pwidechar rawbytestring real shortint shortstring single smallint string uint16 uint32 uint64 uint8 unicodestring utf8string variant widechar widestring word wordbool";

impl<'s, 't> Walker<'s, 't> {
    fn simple_callee(&self, node: SyntaxNode<'_>) -> String {
        if node.kind() == "identifier" {
            self.text(node).into()
        } else {
            children(node)
                .into_iter()
                .filter(|c| c.kind() == "identifier")
                .map(|c| self.text(c))
                .collect::<Vec<_>>()
                .join(".")
        }
    }
    fn identifiers(&self, mut node: SyntaxNode<'_>, calls: bool) -> Option<Vec<String>> {
        let mut links = vec![];
        loop {
            match node.kind() {
                "exprDot" => {
                    let name = last(node)?;
                    if name.kind() != "identifier" {
                        return None;
                    }
                    links.push(self.text(name).to_owned());
                }
                "exprCall" if calls => {}
                "exprUnary" if calls && last(node)?.kind() == "kHat" => {}
                _ => break,
            }
            node = node.named_child(0)?;
        }
        if node.kind() != "identifier" {
            return None;
        }
        links.push(self.text(node).to_owned());
        links.reverse();
        Some(links)
    }
    pub(super) fn call(&mut self, node: SyntaxNode<'t>, receiver_link: bool) {
        stack_guard!();
        if self.context.current().is_none() {
            return;
        }
        let Some(first) = node.named_child(0) else {
            return;
        };
        let mut callee = String::new();
        if first.kind() == "exprDot" {
            let inner = child(first, "exprCall");
            let outer = children(first)
                .into_iter()
                .rfind(|c| c.kind() == "identifier");
            let method = outer.map(|n| self.text(n)).unwrap_or("").to_owned();
            if let Some(inner) =
                inner.filter(|_| !method.is_empty() && re!(r"^[A-Za-z0-9_]+$").is_match(&method))
            {
                let inner_callee = inner
                    .named_child(0)
                    .filter(|n| matches!(n.kind(), "exprDot" | "identifier"))
                    .map(|n| self.simple_callee(n))
                    .unwrap_or_default();
                if re!(r"^[TI][A-Z]").is_match(&inner_callee) {
                    callee = format!("{inner_callee}().{method}");
                } else {
                    self.chain_call(first, "args", node);
                }
            } else if first.named_child(0).is_some_and(|n| n.kind() == "exprDot") {
                self.chain_call(first, "args", node);
            } else {
                callee = self.simple_callee(first);
            }
        } else if first.kind() == "identifier" {
            callee = self.text(first).into();
        }
        let vague = receiver_link && first.kind() == "exprDot" && !callee.contains('.');
        let with_member = self.with_depth > 0
            && first.kind() == "identifier"
            && SYSTEM
                .split_whitespace()
                .any(|name| name == callee.to_lowercase());
        if !callee.is_empty() && !vague && !with_member {
            self.reference(&callee, "calls", node);
        }
        if first.kind() == "exprDot" {
            self.receiver_calls(first.named_child(0), true);
        } else if first.kind() != "identifier" {
            self.receiver_calls(Some(first), true);
        }
        if let Some(args) = child(node, "exprArgs") {
            self.block(args, false);
        }
    }
    fn receiver_calls(&mut self, node: Option<SyntaxNode<'t>>, names: bool) {
        stack_guard!();
        let Some(node) = node else {
            return;
        };
        match node.kind() {
            "identifier" => {
                if names {
                    self.parenless_name(node, None);
                }
            }
            "exprCall" => self.receiver_call(node, true),
            "exprDot" => {
                if names {
                    self.parenless_name(node, None);
                }
                self.receiver_calls(node.named_child(0), names);
            }
            "exprTpl" => self.receiver_calls(node.named_child(0), false),
            "exprSubscript" => {
                self.receiver_calls(node.named_child(0), names);
                if let Some(args) = child(node, "exprArgs") {
                    self.block(args, false);
                }
            }
            _ => self.block(node, false),
        }
    }
    fn receiver_call(&mut self, node: SyntaxNode<'t>, link: bool) {
        self.call(node, link);
        if let Some(args) = child(node, "exprArgs") {
            self.capture(args);
        }
    }
    fn parenless_call(&mut self, node: SyntaxNode<'t>) {
        stack_guard!();
        if self.context.current().is_none() {
            return;
        }
        let receiver = node.named_child(0);
        let outer = children(node)
            .into_iter()
            .rfind(|c| c.kind() == "identifier");
        let method = outer.map(|n| self.text(n)).unwrap_or("").to_owned();
        if method.is_empty() {
            return;
        }
        let mut callee = String::new();
        if let Some(receiver) = receiver.filter(|n| {
            matches!(n.kind(), "exprDot" | "exprCall") && re!(r"^[A-Za-z0-9_]+$").is_match(&method)
        }) {
            let inner = if receiver.kind() == "exprCall" {
                receiver.named_child(0)
            } else {
                Some(receiver)
            }
            .map(|n| self.simple_callee(n))
            .unwrap_or_default();
            if re!(r"^[TI][A-Z]").is_match(&inner) {
                callee = format!("{inner}().{method}");
                if receiver.kind() == "exprCall" {
                    self.receiver_call(receiver, false);
                } else {
                    self.parenless_call(receiver);
                }
            } else {
                self.chain_call(node, "call", node);
                self.receiver_calls(Some(receiver), true);
            }
        } else {
            callee = self.simple_callee(node);
            self.receiver_calls(receiver, true);
        }
        if !callee.is_empty() {
            self.reference(&callee, "calls", node);
        }
    }
    fn chain_call(&mut self, callee: SyntaxNode<'t>, mode: &str, node: SyntaxNode<'t>) {
        if self.header_name(callee) {
            return;
        }
        let Some(scope) = self.scope_at(node.start_position().row as u32 + 1) else {
            return;
        };
        let Some(mut links) = self.identifiers(callee, true) else {
            return;
        };
        let local = self.local(&links[0].to_lowercase(), &scope);
        if self.with_depth > 0
            && local.as_ref().and_then(|v| v.as_ref()).is_none()
            && !links[0].eq_ignore_ascii_case("self")
        {
            return;
        }
        if let Some(local) = local {
            let Some(local) = local.filter(|s| s != "procedure") else {
                return;
            };
            links[0] = format!(":{local}");
        }
        self.reference(
            &format!(
                "{{{mode}{}}}.{}",
                scope.class.map_or(String::new(), |c| format!("@{c}")),
                links.join(".")
            ),
            "references",
            node,
        );
    }
    pub(super) fn block(&mut self, node: SyntaxNode<'t>, declarations: bool) {
        stack_guard!();
        for c in children(node) {
            if declarations
                && (c.kind().starts_with("decl")
                    || matches!(c.kind(), "defProc" | "interface" | "implementation"))
            {
                self.visit(c);
                continue;
            }
            self.capture(c);
            self.inline(c);
            self.learn_variable(c);
            match c.kind() {
                "exprCall" => {
                    self.call(c, false);
                    if let Some(args) = child(c, "exprArgs") {
                        self.capture(args);
                    }
                }
                "exprDot" => {
                    if node.kind() == "statement" {
                        if !self.header_name(c) {
                            self.parenless_call(c);
                        }
                    } else {
                        let receiver = c.named_child(0);
                        if let Some(receiver) = receiver.filter(|n| n.kind() == "exprCall") {
                            self.receiver_call(receiver, false);
                        } else {
                            self.receiver_calls(receiver, true);
                        }
                        self.parenless_name(c, Some(node));
                    }
                }
                "identifier" => self.parenless_name(c, Some(node)),
                "with" => {
                    self.with_depth += 1;
                    self.block(c, declarations);
                    self.with_depth -= 1;
                }
                "lambda" => {
                    let outer = self.scopes.last().cloned().unwrap_or_default();
                    self.scopes.push(Scope {
                        locals: Rc::new(self.routine_locals(c, false)),
                        class: outer.class,
                        routine: None,
                        line: outer.line,
                    });
                    self.block(c, declarations);
                    self.scopes.pop();
                }
                _ => self.block(c, declarations && c.kind() != "block"),
            }
        }
    }
    fn name_mode(&self, name: SyntaxNode<'_>, parent: SyntaxNode<'_>) -> Option<&'static str> {
        let is = |field| {
            parent
                .child_by_field_name(field)
                .is_some_and(|n| n.id() == name.id())
        };
        match parent.kind() {
            "statement" => {
                (parent.named_child_count() == 1 && name.kind() == "identifier").then_some("call")
            }
            "exprSubscript" => (parent.named_child(0)?.id() == name.id()).then_some("recv"),
            "assignment" => {
                let target = parent.named_child(0)?;
                if target.id() == name.id() {
                    return None;
                }
                let last = if target.kind() == "exprDot" {
                    children(target)
                        .into_iter()
                        .rfind(|c| c.kind() == "identifier")
                } else {
                    Some(target)
                };
                Some(
                    if last.is_some_and(|n| re!(r"^F?On[A-Z]").is_match(self.text(n))) {
                        "ref"
                    } else {
                        "read"
                    },
                )
            }
            "exprArgs" => parent
                .parent()
                .is_some_and(|p| matches!(p.kind(), "exprCall" | "exprSubscript" | "inherited"))
                .then_some("read"),
            "exprBinary" => {
                let op = children(parent)
                    .into_iter()
                    .find(|n| n.id() != name.id() && re!(r"^k[A-Z]").is_match(n.kind()));
                if op.is_some_and(|n| matches!(n.kind(), "kIs" | "kAs"))
                    && parent.named_child(0)?.id() != name.id()
                {
                    None
                } else {
                    Some("read")
                }
            }
            "exprUnary" => {
                if parent.child(0)?.kind() == "kAt" {
                    (name.kind() == "exprDot").then_some("ref")
                } else {
                    Some("read")
                }
            }
            "if" | "ifElse" | "while" | "repeat" => is("condition").then_some("read"),
            "case" | "exprParens" => Some("read"),
            "foreach" => is("iterable").then_some("read"),
            "for" => is("end").then_some("read"),
            _ => None,
        }
    }
    fn parenless_name(&mut self, name: SyntaxNode<'t>, parent: Option<SyntaxNode<'t>>) {
        let Some(scope) = self.scope_at(name.start_position().row as u32 + 1) else {
            return;
        };
        if self.header_name(name) {
            return;
        }
        let Some(from) = self.context.current() else {
            return;
        };
        if name.kind() == "identifier" {
            let lower = self.text(name).to_lowercase();
            if NOT_ROUTINES.contains(&lower.as_str())
                || self.local(&lower, &scope).is_some()
                || self.data_names(scope.class.as_deref()).contains(&lower)
            {
                return;
            }
        }
        let Some(mut mode) = parent.map_or(Some("recv"), |p| self.name_mode(name, p)) else {
            return;
        };
        if self.with_depth > 0 && (mode != "ref" || name.kind() != "identifier") {
            return;
        }
        let mut target = String::new();
        if let Some(parent) = parent.filter(|p| p.kind() == "assignment" && self.with_depth == 0) {
            if let Some(mut links) = parent
                .named_child(0)
                .and_then(|n| self.identifiers(n, false))
            {
                match self.local(&links[0].to_lowercase(), &scope) {
                    Some(local)
                        if links.len() == 1
                            && (local.is_none() || local.as_deref() == Some("procedure")) =>
                    {
                        mode = if local.is_some() { "ref" } else { "read" }
                    }
                    Some(Some(local)) if local != "procedure" => {
                        links[0] = format!(":{local}");
                        target = format!("={}", links.join("."));
                    }
                    None => target = format!("={}", links.join(".")),
                    _ => {}
                }
            }
        }
        let Some(mut links) = self.identifiers(name, false) else {
            return;
        };
        let head = links[0].clone();
        let lower = head.to_lowercase();
        let local = self.local(&lower, &scope);
        let read = mode != "call" && mode != "ref";
        if read
            && re!(r"^(?:F[A-Z][a-z]|[TE][A-Z][a-z]|[A-Z][A-Z0-9_]*$)")
                .is_match(links.last().unwrap())
        {
            return;
        }
        if links.len() == 1 {
            if NOT_ROUTINES.contains(&lower.as_str())
                || local.is_some()
                || read && re!(r"^[a-z][A-Za-z]*$").is_match(&head)
                || self.data_names(scope.class.as_deref()).contains(&lower)
            {
                return;
            }
        } else if let Some(local) = local {
            let Some(local) = local else {
                return;
            };
            if local == "procedure" || mode == "recv" || re!(r"^P[A-Z]").is_match(&local) {
                return;
            }
            links[0] = format!(":{local}");
        } else if lower != "self" && NOT_ROUTINES.contains(&lower.as_str()) {
            return;
        }
        let (line, column) = self.context.position(name.start_byte());
        let reference = UnresolvedReference {
            from_node_id: self.context.result.nodes[from].id.clone(),
            reference_name: format!(
                "{{{mode}{target}{}}}.{}",
                scope
                    .class
                    .as_ref()
                    .map_or(String::new(), |c| format!("@{c}")),
                links.join(".")
            ),
            reference_kind: "references".into(),
            line,
            column,
            file_path: None,
            language: None,
            candidates: None,
        };
        if links.len() == 1 && mode != "call" {
            self.reads
                .push((reference, scope.class.unwrap_or_default(), lower));
        } else {
            self.context.result.unresolved_references.push(reference);
        }
    }
    fn learn_variable(&mut self, node: SyntaxNode<'t>) {
        if self.with_depth > 0 {
            return;
        }
        let target = if node.kind() == "assignment" {
            node.named_child(0)
        } else if node.kind() == "exprCall"
            && node.named_child(0).is_some_and(|n| {
                n.kind() == "identifier"
                    && ["inc", "dec"].contains(&self.text(n).to_lowercase().as_str())
            })
        {
            child(node, "exprArgs").and_then(|a| a.named_child(0))
        } else {
            None
        };
        let Some(target) = target.filter(|n| n.kind() == "identifier") else {
            return;
        };
        let Some(scope) = self.scope_at(target.start_position().row as u32 + 1) else {
            return;
        };
        let name = self.text(target).to_lowercase();
        if scope.routine.as_deref() == Some(&name) || self.local(&name, &scope).is_some() {
            return;
        }
        self.assigned
            .entry(scope.class.unwrap_or_default())
            .or_default()
            .insert(name);
    }
    fn header_name(&self, node: SyntaxNode<'_>) -> bool {
        if !self.broken {
            return false;
        }
        let start = self.context.source[..node.start_byte()]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        re!(r"(?i)^\s*(?:class\s+)?(?:procedure|function|constructor|destructor|operator)\s+$")
            .is_match(&self.context.source[start..node.start_byte()])
    }
    fn scope_at(&mut self, line: u32) -> Option<Scope> {
        let own = self.scopes.last()?.clone();
        if !self.broken {
            return Some(own);
        }
        if self.text_routines.is_none() {
            self.text_routines = Some(self.read_text_routines());
        }
        let text = self
            .text_routines
            .as_ref()
            .unwrap()
            .iter()
            .rev()
            .find(|s| s.line <= line);
        Some(match text {
            Some(text) if text.line > own.line && !(own.line > 0 && text.routine.is_none()) => {
                text.clone()
            }
            _ => own,
        })
    }
    fn text_declarations(
        &self,
        lines: &[&str],
        from: usize,
        mut text: String,
        locals: &mut HashMap<String, Option<String>>,
    ) -> String {
        let header = re!(
            r"(?i)^(\s*)(?:class\s+)?(?:procedure|function|constructor|destructor|operator)\s+([A-Za-z_][A-Za-z0-9_]*(?:<[^>]*>)?(?:\.[A-Za-z_][A-Za-z0-9_]*(?:<[^>]*>)?)*)"
        );
        for line in lines.iter().skip(from + 1).take(200) {
            if re!(r"(?i)^\s*(?:begin|asm)\b").is_match(line) || header.is_match(line) {
                break;
            }
            text.push('\n');
            text.push_str(line);
        }
        let text = re!(r"\{[^}]*\}|\(\*(?s:.*?)\*\)|//[^\n]*")
            .replace_all(&text, " ")
            .into_owned();
        for capture in re!(r"([A-Za-z_][A-Za-z0-9_]*(?:\s*,\s*[A-Za-z_][A-Za-z0-9_]*)*)\s*:\s*([A-Za-z_][A-Za-z0-9_.]*)?").captures_iter(&text){let colon=capture.get(0).unwrap();if text[colon.start()..].find(':').is_some_and(|i|text.as_bytes().get(colon.start()+i+1)==Some(&b'=')){continue;}let ty=capture.get(2).map(|m|if ["procedure","function","reference"].contains(&m.as_str().to_lowercase().as_str()){"procedure".into()}else{m.as_str().rsplit('.').next().unwrap().into()});for name in capture[1].split(','){locals.insert(name.trim().to_lowercase(),ty.clone());}}
        text
    }
    fn read_text_routines(&self) -> Vec<Scope> {
        let lines: Vec<_> = self
            .context
            .source
            .split('\n')
            .map(|s| s.strip_suffix('\r').unwrap_or(s))
            .collect();
        let mut routines = vec![];
        let mut outer: Option<Scope> = None;
        let mut open: Vec<(usize, HashMap<String, Option<String>>)> = vec![];
        let header = re!(
            r"(?i)^(\s*)(?:class\s+)?(?:procedure|function|constructor|destructor|operator)\s+([A-Za-z_][A-Za-z0-9_]*(?:<[^>]*>)?(?:\.[A-Za-z_][A-Za-z0-9_]*(?:<[^>]*>)?)*)"
        );
        for (i, text) in lines.iter().enumerate() {
            if re!(r"(?i)^(?:end;|\s*(?:initialization|finalization)\b)").is_match(text) {
                routines.push(Scope {
                    line: i as u32 + 1,
                    ..Scope::default()
                });
                outer = None;
                open.clear();
                continue;
            }
            let Some(m) = header.captures(text) else {
                if open.len() > 1 {
                    if let Some(section) = re!(
                        r"(?i)^(\s*)(var|const|type|label|resourcestring|threadvar|begin|asm)\b"
                    )
                    .captures(text)
                    {
                        let indent = section[1].encode_utf16().count();
                        if indent < open.last().unwrap().0 {
                            while open.len() > 1 && open.last().unwrap().0 > indent {
                                open.pop();
                            }
                            let resumed = open.last_mut().unwrap();
                            if ["var", "const"].contains(&section[2].to_lowercase().as_str()) {
                                self.text_declarations(
                                    &lines,
                                    i,
                                    text[section.get(0).unwrap().end()..].into(),
                                    &mut resumed.1,
                                );
                            }
                            let mut scope = outer.clone().unwrap();
                            scope.line = i as u32 + 1;
                            scope.locals = Rc::new(resumed.1.clone());
                            routines.push(scope);
                        }
                    }
                }
                continue;
            };
            let indent = m[1].encode_utf16().count();
            let nested = indent > 0 && outer.is_some();
            let plain = no_generics(&m[2]);
            let mut parts: Vec<_> = plain.split('.').collect();
            let routine = parts.pop().unwrap().to_lowercase();
            while nested && open.len() > 1 && open.last().unwrap().0 >= indent {
                open.pop();
            }
            let mut locals = if nested {
                open.last().unwrap().1.clone()
            } else {
                HashMap::new()
            };
            let decl = self.text_declarations(
                &lines,
                i,
                text[m.get(0).unwrap().end()..].into(),
                &mut locals,
            );
            let result = re!(r"^\s*(?:\([^)]*\))?\s*:\s*([A-Za-z_][A-Za-z0-9_.]*)")
                .captures(&decl)
                .map(|c| c[1].rsplit('.').next().unwrap().to_owned());
            locals.insert("result".into(), result);
            if nested {
                let mut scope = outer.clone().unwrap();
                scope.line = i as u32 + 1;
                scope.locals = Rc::new(locals.clone());
                routines.push(scope);
                open.push((indent, locals));
            } else {
                let scope = Scope {
                    line: i as u32 + 1,
                    class: (!parts.is_empty()).then(|| parts.join("::")),
                    routine: Some(routine),
                    locals: Rc::new(locals.clone()),
                };
                outer = Some(scope.clone());
                routines.push(scope);
                open = vec![(indent, locals)];
            }
        }
        routines
    }
}
