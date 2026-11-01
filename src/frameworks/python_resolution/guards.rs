use super::*;

pub(super) enum Shape {
    Bare,
    Chained(String),
}
fn trim(text: &str) -> &str {
    text.trim_matches(crate::search::query_whitespace)
}
fn text<'a>(reference: &UnresolvedReference, environment: &'a Environment<'_>) -> Option<&'a str> {
    let line = *environment
        .lines(reference.file_path.as_deref()?)
        .get(reference.line.checked_sub(1)? as usize)?;
    let mut units = 0;
    for (at, ch) in line.char_indices() {
        if units == reference.column {
            return Some(&line[at..]);
        }
        units += ch.len_utf16() as u32;
        if units > reference.column {
            return None;
        }
    }
    (units == reference.column).then_some("")
}
pub(super) fn shape(
    reference: &UnresolvedReference,
    environment: &Environment<'_>,
) -> Result<Option<Shape>> {
    if reference.language.as_deref() != Some("python")
        || reference.reference_kind != "calls"
        || !regex(r"^[A-Za-z_]\w*$")?.is_match(&reference.reference_name)?
    {
        return Ok(None);
    }
    let Some(text) = text(reference, environment) else {
        return Ok(None);
    };
    let name = &reference.reference_name;
    if text.strip_prefix(name).is_some_and(|tail| {
        tail.trim_start_matches(crate::search::query_whitespace)
            .starts_with('(')
    }) {
        return Ok(Some(Shape::Bare));
    }
    if self_receiver(reference, environment)? {
        return Ok(None);
    }
    let pattern = format!(r"^(.*?)\.\s*{}\s*\(", regex::escape(name));
    let Some(chain) = regex(&pattern)?.captures(text)? else {
        return Ok(None);
    };
    Ok(regex(r"(\w+)\s*(?:\([^()]*\)|\[[^\[\]]*\])?\s*$")?
        .captures(&chain[1])?
        .map(|capture| Shape::Chained(capture[1].into())))
}
pub(super) fn self_receiver(
    reference: &UnresolvedReference,
    environment: &Environment<'_>,
) -> Result<bool> {
    let Some(text) = text(reference, environment) else {
        return Ok(false);
    };
    let pattern = format!(
        r"^(?:self|cls)\s*\.\s*{}\s*\(",
        regex::escape(&reference.reference_name)
    );
    Ok(regex(&pattern)?.is_match(text)?)
}
pub(super) fn imported_self_call(
    reference: &UnresolvedReference,
    environment: &Environment<'_>,
) -> Result<bool> {
    if reference.reference_kind != "calls" {
        return Ok(false);
    }
    let Some(first) = text(reference, environment) else {
        return Ok(false);
    };
    let pattern = regex(&format!(
        r"(?<![\w$]){}\s*(?:<[^<>()]*>|\[(?:[^\[\]]|\[[^\[\]]*\])*\])?\s*[({{]",
        regex::escape(&reference.reference_name)
    ))?;
    let found = pattern.find(first)?;
    let joined;
    let text = if found.is_none_or(|found| {
        first
            .find(&reference.reference_name)
            .is_some_and(|at| at < found.start())
    }) {
        joined = std::iter::once(first)
            .chain(
                environment
                    .lines(reference.file_path.as_deref().unwrap_or_default())
                    .iter()
                    .skip(reference.line as usize)
                    .take(7)
                    .copied(),
            )
            .collect::<Vec<_>>()
            .join("\n");
        joined.as_str()
    } else {
        first
    };
    let Some(found) = pattern.find(text)? else {
        return Ok(false);
    };
    let before = text[..found.start()].trim_end_matches(crate::search::query_whitespace);
    let Some(head) = before.strip_suffix('.') else {
        return Ok(false);
    };
    let head = head.strip_suffix('?').unwrap_or(head);
    let head = head.trim_end_matches(crate::search::query_whitespace);
    Ok(
        regex(r"(?:^|[^\w$.])(?:this|self|super|Self)$|(?:^|[^\w$.])super\s*\([^()]*\)$")?
            .is_match(head)?,
    )
}
pub(super) fn super_receiver(
    reference: &UnresolvedReference,
    environment: &Environment<'_>,
) -> Result<bool> {
    let Some(text) = text(reference, environment) else {
        return Ok(false);
    };
    let pattern = format!(
        r"^super\s*\([^)]*\)\s*\.\s*{}\s*\(",
        regex::escape(&reference.reference_name)
    );
    Ok(regex(&pattern)?.is_match(text)?)
}
impl<'a> Bindings<'a> {
    pub(super) fn imported_from(
        &self,
        path: &str,
        name: &str,
        environment: &Environment<'_>,
    ) -> Result<Option<String>> {
        if !self.imports.borrow().contains_key(path) {
            let mut names = HashMap::new();
            let source = environment.sources.get(path).copied().unwrap_or_default();
            for capture in regex(r"(?m)^\s*from\s+([\w.]+)\s+import\s+(\([^)]*\)|[^\n#]+)")?
                .captures_iter(source)
            {
                let capture = capture?;
                for item in capture[2].replace(['(', ')'], "").split(',') {
                    if let Some(bound) = regex(r"(\w+)\s*$")?.captures(trim(item))? {
                        names.insert(bound[1].to_owned(), capture[1].to_owned());
                    }
                }
            }
            self.imports.borrow_mut().insert(path.into(), names);
        }
        Ok(self
            .imports
            .borrow()
            .get(path)
            .and_then(|names| names.get(name))
            .cloned())
    }
    pub(crate) fn local_binding(
        &self,
        reference: &UnresolvedReference,
        environment: &Environment<'_>,
    ) -> Result<bool> {
        let path = reference.file_path.as_deref().unwrap_or_default();
        let name = &reference.reference_name;
        let function = self
            .in_file(path)
            .iter()
            .copied()
            .filter(|node| {
                matches!(node.kind.as_str(), "function" | "method")
                    && node.start_line <= reference.line
                    && node.end_line >= reference.line
            })
            .min_by_key(|node| node.end_line - node.start_line);
        let key = (
            path.to_owned(),
            function.map_or("", |node| node.id.as_str()).to_owned(),
            name.clone(),
            if function.is_some() {
                0
            } else {
                reference.line
            },
        );
        if let Some(found) = self.local_bindings.borrow().get(&key) {
            return Ok(*found);
        }
        if self.imported_from(path, name, environment)?.is_some() {
            self.local_bindings.borrow_mut().insert(key, false);
            return Ok(false);
        }
        let mut file = self.safe_file.borrow_mut();
        if file.as_ref().is_none_or(|(file, _)| file != path) {
            let safe = crate::synthesis::comments::strip_for_regex(
                environment.sources.get(path).copied().unwrap_or_default(),
                "python",
            );
            *file = Some((
                path.into(),
                safe.split('\n')
                    .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
                    .collect(),
            ));
        }
        let lines = &file.as_ref().unwrap().1;
        let name = regex::escape(name);
        let assignment = regex(&format!(
            r"^\s*(?:[\w\s,*()\[\]]*,\s*)?\(?\*?{name}\)?\s*(?:,[\w\s,*()\[\]]*)?(?::[^=]+)?=(?!=)"
        ))?;
        let targets = regex(&format!(
            r"\bfor\s+[\w\s,()]*\b{name}\b[\w\s,()]*\s+in\b|\bas\s+{name}\b"
        ))?;
        let params = regex(&format!(r"[(,]\s*\*{{0,2}}{name}\s*(?:[:=,)]|$)"))?;
        let mut bound = false;
        if let Some(function) = function {
            let mut line = function.start_line.saturating_sub(1) as usize;
            let mut signature = String::new();
            while line < lines.len().min(function.start_line as usize + 20) {
                signature.push_str(&lines[line]);
                if regex(r"\)\s*(?:->[^:]*)?:\s*(?:#.*)?$")?.is_match(&lines[line])? {
                    break;
                }
                line += 1;
            }
            let signature = signature.find('(').map_or("", |at| &signature[at..]);
            bound = params.is_match(signature)?;
            for line in line + 1..reference.line.saturating_sub(1) as usize {
                if bound {
                    break;
                }
                self.charge(1)?;
                let text = lines.get(line).map(String::as_str).unwrap_or_default();
                bound = assignment.is_match(text)? || targets.is_match(text)?;
            }
        }
        if !bound {
            let top = regex(&format!(
                r"^(?:[\w,\s]*,\s*)?{name}\s*(?:,[\w\s,]*)?(?::[^=]+)?=(?!=)"
            ))?;
            for line in lines {
                self.charge(1)?;
                if top.is_match(line)? {
                    bound = true;
                    break;
                }
            }
        }
        self.local_bindings.borrow_mut().insert(key, bound);
        Ok(bound)
    }
    fn fixture(&self, node: &Node, environment: &Environment<'_>) -> Result<bool> {
        if node.file_path.rsplit('/').next() == Some("conftest.py") {
            return Ok(true);
        }
        if let Some(decorators) = &node.decorators {
            for decorator in decorators {
                if regex(r"(?:^|\.)fixture\b")?.is_match(decorator)? {
                    return Ok(true);
                }
            }
        }
        if node.language != "python" || node.kind != "function" {
            return Ok(false);
        }
        let lines = environment.lines(&node.file_path);
        let mut open = 0i64;
        let start = node.start_line.saturating_sub(16) as usize;
        for at in (start..node.start_line.saturating_sub(1) as usize).rev() {
            let line = trim(lines.get(at).copied().unwrap_or_default());
            open += line.matches(')').count() as i64 - line.matches('(').count() as i64;
            if open > 0 {
                continue;
            }
            if !line.starts_with('@') {
                return Ok(false);
            }
            if regex(r"^@(?:\w+\.)*fixture\b")?.is_match(line)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(crate) fn fixture_in_reach(
        &self,
        node: &Node,
        path: &str,
        environment: &Environment<'_>,
    ) -> Result<bool> {
        if node.file_path == path {
            return self.fixture(node, environment);
        }
        if node.file_path.rsplit('/').next() == Some("conftest.py") {
            let directory = node.file_path.strip_suffix("conftest.py").unwrap();
            return Ok(path.starts_with(directory));
        }
        if !self.fixture(node, environment)? {
            return Ok(false);
        }
        let mut directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
        loop {
            let conftest = if directory.is_empty() {
                "conftest.py".into()
            } else {
                format!("{directory}/conftest.py")
            };
            if let Some(source) = environment.sources.get(conftest.as_str()) {
                let mut modules = Vec::new();
                for capture in
                    regex(r"(?m)^\s*from\s+([\w.]+)\s+import\s+\*")?.captures_iter(source)
                {
                    modules.push(format!("{}.py", capture?[1].replace('.', "/")));
                }
                if let Some(capture) =
                    regex(r"(?m)^\s*pytest_plugins\s*=\s*[[(]([^\])]*)[\])]")?.captures(source)?
                {
                    for capture in regex(r#"["']([\w.]+)["']"#)?.captures_iter(&capture[1]) {
                        modules.push(format!("{}.py", capture?[1].replace('.', "/")));
                    }
                }
                if modules.iter().any(|module| {
                    node.file_path == *module || node.file_path.ends_with(&format!("/{module}"))
                }) {
                    return Ok(true);
                }
            }
            if directory.is_empty() {
                break;
            }
            directory = directory
                .rsplit_once('/')
                .map_or("", |(directory, _)| directory);
        }
        Ok(false)
    }
    pub(super) fn fits_shape(
        &self,
        node: &Node,
        reference: &UnresolvedReference,
        shape: Option<&Shape>,
        environment: &Environment<'_>,
    ) -> Result<bool> {
        match shape {
            None => Ok(true),
            Some(Shape::Bare) => {
                if node.kind == "method" {
                    return Ok(false);
                }
                let path = reference.file_path.as_deref().unwrap_or_default();
                if node.file_path == path {
                    return Ok(true);
                }
                if let Some(module) =
                    self.imported_from(path, &reference.reference_name, environment)?
                    && !module.starts_with('.')
                    && (environment.module)(&module, path).is_none()
                {
                    return Ok(false);
                }
                Ok(self.fixture_in_reach(node, path, environment)?
                    || !self.local_binding(reference, environment)?)
            }
            Some(Shape::Chained(owner)) => {
                if node.kind == "method" {
                    let qualifier = node
                        .qualified_name
                        .rsplit_once("::")
                        .map_or("", |(qualifier, _)| {
                            qualifier.rsplit("::").next().unwrap_or_default()
                        });
                    return Ok(!qualifier.is_empty()
                        && qualifier.replace('_', "").to_lowercase()
                            == owner.replace('_', "").to_lowercase());
                }
                let mut parts = node.file_path.rsplit('/');
                let file = parts.next().unwrap_or_default();
                let stem = file
                    .strip_suffix(".pyi")
                    .or_else(|| file.strip_suffix(".py"))
                    .unwrap_or(file);
                Ok(stem == owner || stem == "__init__" && parts.next() == Some(owner.as_str()))
            }
        }
    }
}
