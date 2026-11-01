//! Receiver-name fallback after a local declaration supplies no matching method.
use super::*;

fn words(name: &str) -> Result<Vec<String>> {
    let name = regex(r"([a-z])([A-Z])")?.replace_all(name, "$1 $2");
    let name = regex(r"([A-Z]+)([A-Z][a-z])")?.replace_all(&name, "$1 $2");
    Ok(regex(r"[\s._:/\\]+")?
        .split(&name)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|word| word.encode_utf16().count() > 1)
        .map(str::to_lowercase)
        .collect())
}
fn owner(node: &Node) -> &str {
    node.qualified_name
        .rsplit_once("::")
        .map_or("", |(prefix, _)| {
            prefix.rsplit([':', '.']).next().unwrap_or("")
        })
}
impl<'a> Context<'a> {
    pub(super) fn typed_name_fallback(
        &self,
        receiver: &str,
        method: &str,
        at: &UnresolvedReference,
        scopes: &crate::frameworks::python_resolution::Bindings<'a>,
    ) -> Result<Option<(Option<&'a Node>, f64)>> {
        let file = at.file_path.as_deref().unwrap_or("");
        let language = at.language.as_deref().unwrap_or("");
        // Other receiver strategies retain ownership of class, namespace,
        // and object-literal bindings.
        if receiver.contains('.') || self.imports.contains_key(&(file, receiver)) {
            return Ok(None);
        }
        if self.named(receiver).any(|node| {
            matches!(node.kind.as_str(), "constant" | "variable")
                && node
                    .signature
                    .as_deref()
                    .is_some_and(|signature| signature.contains('{'))
        }) {
            return Ok(None);
        }
        let capitalized = receiver
            .chars()
            .next()
            .map(|head| head.to_uppercase().to_string() + &receiver[head.len_utf8()..])
            .unwrap_or_default();
        for name in [receiver, capitalized.as_str()] {
            if self.named(name).any(|node| {
                matches!(
                    node.kind.as_str(),
                    "class" | "struct" | "union" | "interface"
                )
            }) {
                return Ok(None);
            }
        }
        let candidates: Vec<_> = self.named(method).collect();
        if candidates.len() > 500 {
            return Ok(Some((None, 0.0)));
        }
        self.steps(candidates.len())?;
        let mut methods: Vec<_> = candidates
            .into_iter()
            .filter(|node| node.kind == "method")
            .collect();
        if methods.iter().any(|node| node.language == language) {
            methods.retain(|node| node.language == language);
        }
        let original = methods.len();
        let actions = [
            "get",
            "post",
            "put",
            "patch",
            "delete",
            "head",
            "options",
            "index",
            "show",
            "store",
            "update",
            "destroy",
            "create",
            "edit",
            "list",
            "retrieve",
            "partial_update",
        ];
        methods.retain(|node| {
            !(actions.contains(&node.name.as_str())
                && [
                    "View",
                    "ViewSet",
                    "APIView",
                    "Controller",
                    "Endpoint",
                    "ViewMixin",
                ]
                .iter()
                .any(|suffix| owner(node).ends_with(suffix)))
        });
        let mut narrowed = methods.len() != original;
        if !crate::classify::is_test_path(file) {
            methods.retain(|node| !crate::classify::is_test_path(&node.file_path));
        }
        let mut reachable = Vec::new();
        for node in &methods {
            self.steps(self.files.get(node.file_path.as_str()).map_or(0, Vec::len))?;
            if scopes.lexically_reachable(node, at)? {
                reachable.push(*node);
            }
        }
        let scoped_out = reachable.len() != methods.len();
        methods = reachable;
        let mut kept = Vec::new();
        for node in &methods {
            let vue = node.file_path.strip_suffix(".vue").is_some_and(|path| {
                node.qualified_name
                    == format!("{}::{}", path.rsplit('/').next().unwrap_or(""), node.name)
            });
            let lifecycle = [
                "render",
                "componentDidMount",
                "componentDidUpdate",
                "componentWillUnmount",
                "shouldComponentUpdate",
                "getSnapshotBeforeUpdate",
                "componentDidCatch",
                "componentWillMount",
                "componentWillReceiveProps",
                "componentWillUpdate",
                "UNSAFE_componentWillMount",
                "UNSAFE_componentWillReceiveProps",
                "UNSAFE_componentWillUpdate",
            ]
            .contains(&node.name.as_str());
            let class = self.in_file(&node.file_path).find(|candidate| {
                candidate.kind == "class"
                    && candidate.name == owner(node)
                    && candidate.start_line <= node.start_line
                    && candidate.end_line >= node.end_line
            });
            let react = if let Some(class) = class.filter(|_| lifecycle) {
                let head = self
                    .lines(&class.file_path)?
                    .skip(class.start_line.saturating_sub(1) as usize)
                    .take(
                        (class.start_line + 3)
                            .min(node.start_line)
                            .saturating_sub(class.start_line - 1) as usize,
                    )
                    .collect::<Vec<_>>()
                    .join(" ");
                test(
                    r"\bextends\s+(?:React\s*\.\s*)?(?:Pure)?Component\b",
                    head.split('{').next().unwrap_or(""),
                )?
            } else {
                false
            };
            if !vue && !react {
                kept.push(*node);
            }
        }
        narrowed |= kept.len() != methods.len();
        methods = kept;
        let receiver_words = words(receiver)?;
        let source = self.sources.get(file).copied().unwrap_or("");
        self.steps(source.len() / 64 + 1)?;
        let unnamed_double = |node: &Node| -> Result<bool> {
            let doubles = ["fake", "mock", "mocked", "stub", "dummy", "spy"];
            Ok(words(owner(node))?
                .iter()
                .any(|word| doubles.contains(&word.as_str()))
                && !receiver_words
                    .iter()
                    .any(|word| doubles.contains(&word.as_str()))
                && !source.contains(owner(node)))
        };
        if methods.len() == 1
            && !narrowed
            && !scoped_out
            && methods[0].language == language
            && !unnamed_double(methods[0])?
        {
            let target = methods[0];
            let assigned = target.file_path != file
                && test(
                    &format!(
                        r"\.prototype\.{}\s*=\s*(?:async\s+)?function\b",
                        ::regex::escape(method)
                    ),
                    source,
                )?;
            if !assigned && target.id != at.from_node_id {
                return Ok(Some((Some(target), 0.7)));
            }
        }
        let mut best = None;
        let mut best_score = 0;
        if methods.len() > 1 || methods.len() == 1 && scoped_out && !narrowed {
            methods.sort_by_key(|node| node.file_path != file);
            for node in methods {
                if unnamed_double(node)? {
                    continue;
                }
                let class = words(owner(node))?;
                let mut score = receiver_words
                    .iter()
                    .filter(|word| class.contains(word))
                    .count();
                score += usize::from(
                    receiver_words.last().is_some() && receiver_words.last() == class.last(),
                );
                score += 2 * usize::from(
                    !class.is_empty() && receiver.replace('_', "").to_lowercase() == class.join(""),
                );
                score += usize::from(node.language == language);
                if score > best_score {
                    best_score = score;
                    best = Some(node);
                }
            }
        }
        Ok(Some((
            best.filter(|node| best_score >= 2 && node.id != at.from_node_id),
            0.65,
        )))
    }
}
