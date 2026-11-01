use super::*;

pub(super) fn extract(path: &str, source: &str) -> ExtractionResult {
    let mut context = Context::new(path, source, "pascal");
    let lines: Vec<_> = source.split('\n').collect();
    let file = &mut context.result.nodes[0];
    file.id = crate::node_id(path, "file", path, 1);
    file.is_exported = None;
    file.end_column = lines.last().unwrap().encode_utf16().count() as u32;
    let mut stack = vec![0];
    let mut multiline = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(end) = multiline {
            if line.trim_end().ends_with(end) {
                multiline = None;
            }
            continue;
        }
        if re!(r"=\s*\(\s*$").is_match(line) {
            multiline = Some(')');
            continue;
        }
        if re!(r"=\s*<\s*$").is_match(line) {
            multiline = Some('>');
            continue;
        }
        if let Some(c) =
            re!(r"^\s*(object|inherited|inline)\s+([A-Za-z0-9_]+)\s*:\s*([A-Za-z0-9_]+)")
                .captures(line)
        {
            let mut node = context.result.nodes[0].clone();
            node.id = crate::node_id(path, "component", &c[2], i as u32 + 1);
            node.kind = "component".into();
            node.name = c[2].into();
            node.qualified_name = format!("{path}#{}", &c[2]);
            node.start_line = i as u32 + 1;
            node.end_line = node.start_line;
            node.start_column = 0;
            node.end_column = line.encode_utf16().count() as u32;
            node.signature = Some(c[3].into());
            let index = context.result.nodes.len();
            context.result.edges.push(Edge {
                source: context.result.nodes[*stack.last().unwrap()].id.clone(),
                target: node.id.clone(),
                kind: "contains".into(),
                line: None,
                column: None,
                provenance: None,
                metadata: None,
            });
            context.result.nodes.push(node);
            stack.push(index);
            continue;
        }
        if let Some(c) = re!(r"^\s*(On[A-Za-z0-9_]+)\s*=\s*([A-Za-z0-9_]+)\s*$").captures(line) {
            context.reference_at(*stack.last().unwrap(), &c[2], "references", i as u32 + 1, 0);
            continue;
        }
        if re!(r"^\s*end\s*$").is_match(line) && stack.len() > 1 {
            let node = &mut context.result.nodes[stack.pop().unwrap()];
            node.end_line = i as u32 + 1;
            node.end_column = line.encode_utf16().count() as u32;
        }
    }
    context.result
}
