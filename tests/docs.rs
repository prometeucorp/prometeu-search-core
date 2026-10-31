use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn markdown_under(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            markdown_under(&entry.path(), files)?;
        } else if entry.path().extension().is_some_and(|value| value == "md") {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn yaml_string(raw: &str) -> Option<String> {
    let value = if raw.starts_with('"') {
        serde_json::from_str::<String>(raw).ok()?
    } else if raw.starts_with('\'') {
        let mut chars = raw.strip_prefix('\'')?.strip_suffix('\'')?.chars();
        let mut value = String::new();
        while let Some(character) = chars.next() {
            if character == '\'' && chars.next() != Some('\'') {
                return None;
            }
            value.push(character);
        }
        value
    } else {
        let first = raw.chars().next()?;
        if !(first.is_alphabetic() || first.is_ascii_digit() && raw.parse::<f64>().is_err())
            || raw.ends_with(':')
            || raw.chars().zip(raw.chars().skip(1)).any(|(left, right)| {
                left == ':' && right.is_whitespace() || left.is_whitespace() && right == '#'
            })
            || ["true", "false", "null", "yes", "no", "on", "off"]
                .iter()
                .any(|value| raw.eq_ignore_ascii_case(value))
        {
            return None;
        }
        raw.to_owned()
    };
    (!value.trim().is_empty() && !value.contains(['\r', '\n'])).then_some(value)
}

fn check_metadata(file: &Path, text: &str, root: &Path, errors: &mut Vec<String>) {
    let name = file.strip_prefix(root).unwrap().display().to_string();
    let lines: Vec<_> = text.lines().collect();
    let end = lines.iter().skip(1).position(|line| *line == "---");
    if lines.first() != Some(&"---") || end.is_none() {
        errors.push(format!("{name}: missing YAML frontmatter"));
        return;
    }
    let end = end.unwrap() + 1;
    let mut fields = BTreeMap::new();
    // ponytail: four single-line YAML strings only; use a YAML parser for richer metadata.
    for line in &lines[1..end] {
        let field = line.split_once(": ");
        let Some((key, raw)) = field.filter(|(key, _)| {
            ["type", "title", "description", "status"].contains(key) && !fields.contains_key(key)
        }) else {
            errors.push(format!(
                "{name}: unknown, malformed or duplicate metadata field: {line}"
            ));
            continue;
        };
        let value = yaml_string(raw.trim());
        if value.is_none() {
            errors.push(format!(
                "{name}: {key} must be a nonempty single-line YAML string"
            ));
        }
        fields.insert(key, value);
    }
    for key in ["type", "title", "description", "status"] {
        if !fields.contains_key(key) {
            errors.push(format!("{name}: missing metadata field: {key}"));
        }
    }
    let value = |key| fields.get(key).and_then(|value| value.as_deref());
    let relative = file.strip_prefix(root.join("docs")).unwrap();
    let area = relative.components().next().unwrap().as_os_str();
    let expected = [
        ("architecture", "Architecture"),
        ("contracts", "Contract"),
        ("decisions", "Decision"),
        ("operations", "Operations"),
        ("quality", "Quality"),
    ]
    .into_iter()
    .find_map(|(name, kind)| (area == name).then_some(kind));
    if expected.is_none() || value("type") != expected {
        errors.push(format!("{name}: type must match its bundle area"));
    }
    if !matches!(value("status"), Some("draft" | "stable" | "deprecated")) {
        errors.push(format!(
            "{name}: status must be draft, stable or deprecated"
        ));
    }
    if lines[end + 1..]
        .iter()
        .any(|line| line.starts_with("Status:"))
    {
        errors.push(format!("{name}: use only YAML status"));
    }
}

fn local_link(target: &str) -> Result<Option<PathBuf>, String> {
    if target.starts_with('#') || target.starts_with("//") || target.is_empty() {
        return Ok(None);
    }
    if let Some((scheme, _)) = target.split_once(':')
        && scheme.starts_with(|character: char| character.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "+.-".contains(character))
    {
        return Ok(None);
    }
    let target = target.split('#').next().unwrap();
    let mut bytes = target.bytes();
    let mut decoded = Vec::new();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().and_then(|value| (value as char).to_digit(16));
            let low = bytes.next().and_then(|value| (value as char).to_digit(16));
            let (Some(high), Some(low)) = (high, low) else {
                return Err(format!("invalid link escape: {target}"));
            };
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded)
        .map(|value| Some(PathBuf::from(value)))
        .map_err(|_| format!("invalid UTF-8 link escape: {target}"))
}

fn check_docs(root: &Path) -> Vec<String> {
    let root = root.canonicalize().unwrap();
    let mut errors = Vec::new();
    let mut docs = Vec::new();
    if let Err(error) = markdown_under(&root.join("docs"), &mut docs) {
        return vec![format!("cannot read documentation bundle: {error}")];
    }
    docs.sort();
    let mut files = docs.clone();
    for name in [
        "README.md",
        "CONTRIBUTING.md",
        "AGENTS.md",
        "CLAUDE.md",
        "ARCHITECTURE.md",
    ] {
        let file = root.join(name);
        if file.exists() {
            files.push(file);
        }
    }
    let mut links = BTreeMap::<PathBuf, BTreeSet<PathBuf>>::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap();
        let name = file.strip_prefix(&root).unwrap().display();
        let targets = links.entry(file.clone()).or_default();
        let mut image_depth = 0;
        for event in Parser::new(&text) {
            let (target, navigation) = match event {
                Event::Start(Tag::Link { dest_url, .. }) => (dest_url, image_depth == 0),
                Event::Start(Tag::Image { dest_url, .. }) => {
                    image_depth += 1;
                    (dest_url, false)
                }
                Event::End(TagEnd::Image) => {
                    image_depth -= 1;
                    continue;
                }
                _ => continue,
            };
            let path = match local_link(&target) {
                Ok(Some(path)) => file.parent().unwrap().join(path),
                Ok(None) => continue,
                Err(error) => {
                    errors.push(format!("{name}: {error}"));
                    continue;
                }
            };
            match path.canonicalize() {
                Ok(path) if navigation => {
                    targets.insert(path);
                }
                Ok(_) => {}
                Err(_) => errors.push(format!("{name}: target does not exist: {target}")),
            }
        }
        if file.starts_with(root.join("docs")) && file.file_name().unwrap() != "README.md" {
            check_metadata(file, &text, &root, &mut errors);
        }
    }
    let bundle_index = root.join("docs/README.md");
    if !bundle_index.is_file() {
        errors.push("docs/README.md: missing bundle index".into());
    }
    for file in &docs {
        if file == &bundle_index {
            continue;
        }
        let index = if file.file_name().unwrap() == "README.md" {
            &bundle_index
        } else {
            &file.parent().unwrap().join("README.md")
        };
        if !links
            .get(index)
            .is_some_and(|targets| targets.contains(file))
        {
            errors.push(format!(
                "{}: does not index {}",
                index.strip_prefix(&root).unwrap().display(),
                file.strip_prefix(&root).unwrap().display()
            ));
        }
    }
    match fs::read_to_string(root.join("AGENTS.md")) {
        Ok(text) if text.split('\n').count() > 200 => {
            errors.push("AGENTS.md: short-map limit is 200 lines".into());
        }
        Ok(_) => {}
        Err(error) => errors.push(format!("AGENTS.md: {error}")),
    }
    let claude = root.join("CLAUDE.md");
    if claude.exists() && !fs::read_to_string(claude).unwrap().contains("@AGENTS.md") {
        errors.push("CLAUDE.md: does not import @AGENTS.md".into());
    }
    errors
}

const PAGE: &str = "---\ntype: Contract\ntitle: \"Sample\"\ndescription: \"Example concept.\"\nstatus: stable\n---\n\n# Sample\n";

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn fixture() -> TempDir {
    let temporary = TempDir::new().unwrap();
    for (name, text) in [
        ("AGENTS.md", "# Standing instructions\n"),
        ("README.md", "[Documentation](docs/README.md)\n"),
        ("docs/README.md", "[Contracts](contracts/README.md)\n"),
        (
            "docs/contracts/README.md",
            "- [Sample](page.md) — Example concept.\n",
        ),
        ("docs/contracts/page.md", PAGE),
    ] {
        write(temporary.path(), name, text);
    }
    temporary
}

#[test]
fn repository_documentation_is_valid() {
    let errors = check_docs(Path::new(env!("CARGO_MANIFEST_DIR")));
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn accepts_supported_metadata_and_encoded_navigation() {
    let temporary = fixture();
    let root = temporary.path();
    for (title, description) in [
        ("Workspace Git", "'The owner''s Git operations.'"),
        ("2026 roadmap", "3D rendering"),
        (r#""Git: the \"owner\" #1""#, r#""2026""#),
    ] {
        let page = PAGE
            .replace("\"Sample\"", title)
            .replace("\"Example concept.\"", description);
        write(root, "docs/contracts/page.md", &page);
        for status in ["draft", "stable", "deprecated"] {
            write(
                root,
                "docs/contracts/page.md",
                &page.replace("status: stable", &format!("status: {status}")),
            );
            assert!(
                check_docs(root).is_empty(),
                "{}",
                check_docs(root).join("\n")
            );
        }
    }
    fs::rename(
        root.join("docs/contracts/page.md"),
        root.join("docs/contracts/workspace git.md"),
    )
    .unwrap();
    write(
        root,
        "docs/contracts/README.md",
        "```md\n[Sample](missing.md)\n```\n\n[Sample](./workspace%20git.md#sample)\n",
    );
    write(root, "CLAUDE.md", "@AGENTS.md\n");
    assert!(
        check_docs(root).is_empty(),
        "{}",
        check_docs(root).join("\n")
    );
}

#[test]
fn rejects_invalid_metadata() {
    for (before, after, expected) in [
        ("---\n", "", "missing YAML frontmatter"),
        (
            "description: \"Example concept.\"\n",
            "",
            "missing metadata field: description",
        ),
        (
            "title: \"Sample\"",
            "title: 2026",
            "title must be a nonempty",
        ),
        (
            "title: \"Sample\"",
            "title: \" \"",
            "title must be a nonempty",
        ),
        (
            "title: \"Sample\"",
            "title: \"Sample",
            "title must be a nonempty",
        ),
        (
            "title: \"Sample\"",
            "title: Git: operations",
            "title must be a nonempty",
        ),
        (
            "description: \"Example concept.\"",
            "description: null",
            "description must be a nonempty",
        ),
        (
            "status: stable",
            "status: stable\nstatus: draft",
            "duplicate metadata field",
        ),
        (
            "status: stable",
            "status: stable\ngenerated: today",
            "unknown, malformed",
        ),
        (
            "type: Contract",
            "type: Quality",
            "type must match its bundle area",
        ),
        (
            "status: stable",
            "status: Accepted",
            "status must be draft, stable or deprecated",
        ),
        (
            "# Sample",
            "# Sample\n\nStatus: Accepted",
            "use only YAML status",
        ),
    ] {
        let temporary = fixture();
        write(
            temporary.path(),
            "docs/contracts/page.md",
            &PAGE.replacen(before, after, 1),
        );
        let errors = check_docs(temporary.path()).join("\n");
        assert!(errors.contains(expected), "{after}: {errors}");
    }
}

#[test]
fn index_requires_rendered_navigation() {
    for sample in [
        "![Sample](page.md)",
        "![An image containing a [Sample](page.md)](page.md)",
        "`[Sample](page.md)`",
        "```md\n[Sample](page.md)\n```",
        "~~~md\n[Sample](page.md)\n~~~",
        "````md\n```\n[Sample](page.md)\n````",
        "> ```md\n> [Sample](page.md)\n> ```",
        "```md\n[Sample](page.md)",
    ] {
        let temporary = fixture();
        write(temporary.path(), "docs/contracts/README.md", sample);
        // A shortcut from the bundle root does not replace the concept's folder index.
        write(
            temporary.path(),
            "docs/README.md",
            "[Contracts](contracts/README.md)\n[Sample](contracts/page.md)\n",
        );
        let errors = check_docs(temporary.path()).join("\n");
        assert!(errors.contains("does not index"), "{sample}: {errors}");
    }
}

#[test]
fn rejects_broken_navigation_and_standing_instructions() {
    for (file, text, expected) in [
        (
            "README.md",
            "[Missing](missing.md)",
            "target does not exist",
        ),
        (
            "README.md",
            "![Missing](missing.png)",
            "target does not exist",
        ),
        ("README.md", "[Invalid](%ZZ.md)", "invalid link escape"),
        ("docs/README.md", "# Documentation", "does not index"),
        (
            "docs/README.md",
            "```md\n[Contracts](contracts/README.md)\n```",
            "does not index",
        ),
        ("CLAUDE.md", "# Instructions", "does not import @AGENTS.md"),
    ] {
        let temporary = fixture();
        write(temporary.path(), file, text);
        let errors = check_docs(temporary.path()).join("\n");
        assert!(errors.contains(expected), "{file}: {errors}");
    }
    let temporary = fixture();
    write(temporary.path(), "AGENTS.md", &"Instruction\n".repeat(201));
    fs::remove_file(temporary.path().join("docs/contracts/README.md")).unwrap();
    fs::remove_file(temporary.path().join("docs/README.md")).unwrap();
    let errors = check_docs(temporary.path()).join("\n");
    for expected in [
        "short-map limit is 200",
        "missing bundle index",
        "does not index",
    ] {
        assert!(errors.contains(expected), "{errors}");
    }
}
