use super::*;
impl<'a> Bindings<'a> {
    pub(super) fn qualified(&self, r: &UnresolvedReference) -> Result<Option<Answer<'a>>> {
        if r.language.as_deref() != Some("go") || r.reference_kind == "imports" {
            return Ok(None);
        }
        let file = r.file_path.as_deref().unwrap_or("");
        let name = r.reference_name.rsplit('.').next().unwrap_or("");
        if !regex(r"^[A-Za-z_]\w*$")?.is_match(name)? {
            return Ok(None);
        }
        let dotted = r.reference_name.split_once('.');
        let written = if let Some((head, _)) = dotted {
            regex(r"^[A-Za-z_]\w*$")?
                .is_match(head)?
                .then(|| head.to_owned())
        } else {
            let line = self
                .line(file, r.line.saturating_sub(1) as usize)
                .unwrap_or("");
            self.step(line.len() / 32 + 1)?;
            let units: Vec<u16> = line.encode_utf16().collect();
            let column = (r.column as usize).min(units.len());
            let suffix = String::from_utf16_lossy(&units[column..]);
            if suffix.starts_with(name) {
                regex(r"(?:^|[^\w.]|\.{3})([A-Za-z_]\w*)\.$")?
                    .captures(&String::from_utf16_lossy(&units[..column]))?
                    .map(|m| m[1].to_owned())
            } else if !regex(&format!(r"(?<![\w.]){}\b", regex::escape(name)))?.is_match(line)? {
                regex(&format!(
                    r"(?:^|[^\w.]|\.{{3}})([A-Za-z_]\w*)\.{}\b",
                    regex::escape(name)
                ))?
                .captures(line)?
                .map(|m| m[1].to_owned())
            } else {
                None
            }
        };
        let Some(written) = written else {
            return Ok(None);
        };
        let Some((binding, package)) = self
            .imports
            .get(file)
            .into_iter()
            .flatten()
            .find(|(binding, _)| binding.local_name == written)
        else {
            return Ok(None);
        };
        if dotted.is_some() && r.reference_kind == "calls" {
            let mut receiver = r.clone();
            receiver.reference_name = written;
            if self.locally_bound(&receiver)? {
                return Ok(None);
            }
        }
        let Some(package) = package else {
            return Ok((!binding.source.starts_with('.')
                && !binding.source.contains("/internal/"))
            .then_some(Answer {
                node: None,
                confidence: 0.,
                resolved_by: "import",
            }));
        };
        let member = dotted.map_or(r.reference_name.as_str(), |(_, tail)| tail);
        for &node in self.by_name.get(member).into_iter().flatten() {
            self.step(1)?;
            if node.language == "go"
                && node.kind != "method"
                && node.is_exported == Some(true)
                && dir(&node.file_path.replace('\\', "/")) == package
            {
                return Ok(Some(Answer {
                    node: Some(node),
                    confidence: 0.9,
                    resolved_by: "import",
                }));
            }
        }
        Ok(Some(Answer {
            node: None,
            confidence: 0.,
            resolved_by: "import",
        }))
    }
}
