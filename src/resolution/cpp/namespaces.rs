//! Namespace aliases and scopes opened by declaration macros.
use super::*;
#[derive(Default)]
pub(super) struct Namespaces {
    pub aliases: BTreeMap<String, Vec<String>>,
    frames: BTreeMap<String, Vec<(u32, u32, Vec<String>)>>,
}
impl Namespaces {
    pub fn read(sources: &BTreeMap<&str, &str>) -> Result<Self> {
        let mut result = Self::default();
        let mut openers = BTreeMap::new();
        let mut function_openers = HashSet::new();
        let mut closers = HashSet::new();
        let mut replacements = BTreeMap::new();
        let mut bodies = Vec::new();
        for (&file, &source) in sources {
            if !regex(r"(?i)\.(?:h|hh|hpp|hxx|inl|c|cc|cpp|cxx)$")?.is_match(file)? {
                continue;
            }
            for found in regex(r"(?m)^[ \t]*namespace[ \t]+([A-Za-z_]\w*)[ \t]*=[ \t]*(?:::)?([A-Za-z_][\w:]*)[ \t]*;")?.captures_iter(source) {
                let found=found?;let paths=result.aliases.entry(found[1].to_string()).or_default();if !paths.contains(&found[2].to_string()){paths.push(found[2].to_string());}
            }
            if !regex(r"(?i)\.(?:h|hh|hpp|hxx|h\+\+|inl|ipp|tcc)$")?.is_match(file)? {
                continue;
            }
            let joined = regex(r"\\\r?\n")?.replace_all(source, " ");
            for found in regex(r"(?m)^[ \t]*#[ \t]*define[ \t]+([A-Za-z_]\w*)(\(\s*([A-Za-z_]\w*)?\s*\))?[ \t]+([^\n]*)$")?.captures_iter(&joined) {
                let found=found?;let name=found[1].to_string();let body=found[4].trim();
                let closes=regex(r"^(?:[A-Za-z_]\w*\s+)*\}(?:\s*\})*\s*;?$")?.is_match(body)?;
                if found.get(2).is_some(){if let Some(arg)=found.get(3) && regex(&format!(r"^namespace\s+{}\s*\{{[\w\s]*$",::regex::escape(arg.as_str())))?.is_match(body)? {function_openers.insert(name);}else if closes{closers.insert(name);}continue;}
                if regex(r"^[A-Za-z_]\w*$")?.is_match(body)? {replacements.insert(name,body.to_string());}
                else if closes{closers.insert(name);}
                else if regex(r"^(?:inline\s+namespace\s+[^{}]*\{\s*|namespace\s+[A-Za-z_]\w*\s*\{\s*)+[\w\s]*$")?.is_match(body)? {bodies.push((name,body.to_string()));}
            }
        }
        for (name, body) in bodies {
            let body = regex(r"inline\s+namespace\s+[^{}]*\{")?.replace_all(&body, "");
            let mut path = Vec::new();
            for found in regex(r"namespace\s+([A-Za-z_]\w*)")?.captures_iter(&body) {
                let found = found?;
                path.push(
                    replacements
                        .get(&found[1])
                        .cloned()
                        .unwrap_or_else(|| found[1].to_string()),
                );
            }
            if !path.is_empty() {
                openers.entry(name).or_insert(path);
            }
        }
        for (&file, &source) in sources {
            let mut stack = Vec::new();
            let mut frames = Vec::new();
            let mut last = 0;
            for (i, line) in source.lines().enumerate() {
                last = i as u32 + 1;
                let Some(found)=regex(r"^[ \t]*([A-Z_][A-Z0-9_]*)(?:\(\s*([A-Za-z_]\w*)?\s*\))?[ \t]*;?[ \t]*(?://.*|/\*.*\*/[ \t]*)?\r?$")?.captures(line)? else{continue;};
                let path = if let Some(arg) = found.get(2)
                    && function_openers.contains(&found[1])
                {
                    Some(vec![
                        replacements
                            .get(arg.as_str())
                            .cloned()
                            .unwrap_or_else(|| arg.as_str().to_string()),
                    ])
                } else if found.get(2).is_none() {
                    openers.get(&found[1]).cloned()
                } else {
                    None
                };
                if let Some(path) = path {
                    stack.push((last, path));
                } else if closers.contains(&found[1])
                    && let Some((start, path)) = stack.pop()
                {
                    frames.push((start, last, path));
                }
            }
            frames.extend(stack.into_iter().map(|(start, path)| (start, last, path)));
            if !frames.is_empty() {
                result.frames.insert(file.to_string(), frames);
            }
        }
        Ok(result)
    }
    pub fn qualified(&self, node: &Node) -> String {
        let mut path = Vec::new();
        if let Some(frames) = self.frames.get(&node.file_path) {
            let mut active = frames
                .iter()
                .filter(|(start, end, _)| *start <= node.start_line && *end >= node.start_line)
                .collect::<Vec<_>>();
            active.sort_by_key(|(start, _, _)| *start);
            for (_, _, names) in active {
                path.extend(names.iter().cloned());
            }
        }
        path.push(node.qualified_name.clone());
        path.join("::")
    }
}
