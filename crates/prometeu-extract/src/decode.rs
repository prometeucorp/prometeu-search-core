//! Typed Rust equivalent of the upstream kernel buffer decoder.
use crate::buffers::*;
use crate::ExtractBuffers;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub qualified_name: String,
    pub file_path: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_column: u32,
    pub end_column: u32,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docstring: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_exported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_async: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_static: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_abstract: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decorators: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_parameters: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedReference {
    pub from_node_id: String,
    pub reference_name: String,
    pub reference_kind: String,
    pub line: u32,
    pub column: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub unresolved_references: Vec<UnresolvedReference>,
    pub errors: Vec<Value>,
    pub duration_ms: f64,
}

fn u32_at(row: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(row[at..at + 4].try_into().expect("validated row length"))
}

fn string_at(arena: &[u8], row: &[u8], at: usize) -> Result<Option<String>, String> {
    let offset = u32_at(row, at);
    if offset == NONE {
        return Ok(None);
    }
    let end = (offset as usize)
        .checked_add(u32_at(row, at + 4) as usize)
        .ok_or("kernel arena offset overflow")?;
    let bytes = arena
        .get(offset as usize..end)
        .ok_or("kernel string outside arena")?;
    std::str::from_utf8(bytes)
        .map(|text| Some(text.to_owned()))
        .map_err(|error| error.to_string())
}

fn required_string(arena: &[u8], row: &[u8], at: usize) -> Result<String, String> {
    string_at(arena, row, at)?.ok_or_else(|| "required kernel string is absent".into())
}

fn indexed<'a>(table: &[&'a str], index: u8) -> Result<&'a str, String> {
    table
        .get(index as usize)
        .copied()
        .ok_or_else(|| format!("invalid kernel enum index {index}"))
}

fn endpoint(
    arena: &[u8],
    row: &[u8],
    at: usize,
    text_at: usize,
    ids: &[String],
) -> Result<String, String> {
    let index = u32_at(row, at);
    if index == NONE {
        required_string(arena, row, text_at)
    } else {
        ids.get(index as usize)
            .cloned()
            .ok_or_else(|| "kernel node index outside table".into())
    }
}

fn string_fields(
    object: &mut Map<String, Value>,
    arena: &[u8],
    row: &[u8],
    fields: &[(&str, usize)],
) -> Result<(), String> {
    for &(name, at) in fields {
        if let Some(value) = string_at(arena, row, at)? {
            object.insert(name.into(), value.into());
        }
    }
    Ok(())
}

pub(crate) fn decode(
    out: ExtractBuffers,
    file_path: &str,
    language: &str,
) -> Result<ExtractionResult, String> {
    if out.meta.len() != META_SIZE || out.meta[0] != KERNEL_ABI_VERSION {
        return Err("invalid kernel metadata or ABI version".into());
    }
    for (bytes, count_at, row_size) in [
        (&out.nodes, 4, NODE_ROW_SIZE),
        (&out.edges, 8, EDGE_ROW_SIZE),
        (&out.refs, 12, REF_ROW_SIZE),
    ] {
        if (u32_at(&out.meta, count_at) as usize).checked_mul(row_size) != Some(bytes.len()) {
            return Err("kernel table length does not match metadata".into());
        }
    }
    if u32_at(&out.meta, 16) as usize != out.arena.len() {
        return Err("kernel arena length does not match metadata".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis() as u64;
    let arena = &out.arena;
    let mut nodes = Vec::with_capacity(u32_at(&out.meta, 4) as usize);
    let mut ids = Vec::with_capacity(nodes.capacity());
    for row in out.nodes.chunks_exact(NODE_ROW_SIZE) {
        let id = required_string(arena, row, 36)?;
        ids.push(id.clone());
        let flags = u16::from_le_bytes(row[2..4].try_into().expect("validated node row"));
        let mut node = json!({
            "id": id, "kind": indexed(&NODE_KINDS, row[0])?,
            "name": required_string(arena, row, 20)?,
            "qualifiedName": required_string(arena, row, 28)?,
            "filePath": file_path, "language": language,
            "startLine": u32_at(row, 4), "endLine": u32_at(row, 8),
            "startColumn": u32_at(row, 12), "endColumn": u32_at(row, 16),
            "updatedAt": now
        });
        let object = node.as_object_mut().expect("node object");
        string_fields(
            object,
            arena,
            row,
            &[("docstring", 44), ("signature", 52), ("returnType", 76)],
        )?;
        if row[1] != 0 {
            object.insert(
                "visibility".into(),
                indexed(&["", "public", "private", "protected", "internal"], row[1])?.into(),
            );
        }
        for (pair, name) in ["isExported", "isAsync", "isStatic", "isAbstract"]
            .into_iter()
            .enumerate()
        {
            if flags & (1 << (pair * 2)) != 0 {
                object.insert(name.into(), (flags & (1 << (pair * 2 + 1)) != 0).into());
            }
        }
        for (name, at) in [("decorators", 60), ("typeParameters", 68)] {
            if let Some(value) = string_at(arena, row, at)? {
                object.insert(name.into(), json!(value.split('\0').collect::<Vec<_>>()));
            }
        }
        if let Some(extra) = string_at(arena, row, 84)? {
            let fields: Map<String, Value> =
                serde_json::from_str(&extra).map_err(|error| error.to_string())?;
            object.extend(fields);
        }
        nodes.push(serde_json::from_value(node).map_err(|error| error.to_string())?);
    }
    let mut edges = Vec::with_capacity(u32_at(&out.meta, 8) as usize);
    for row in out.edges.chunks_exact(EDGE_ROW_SIZE) {
        edges.push(Edge {
            source: endpoint(arena, row, 0, 28, &ids)?,
            target: endpoint(arena, row, 4, 36, &ids)?,
            kind: indexed(&EDGE_KINDS, row[8])?.into(),
            line: (u32_at(row, 12) != NONE).then(|| u32_at(row, 12)),
            column: (u32_at(row, 16) != NONE).then(|| u32_at(row, 16)),
            provenance: if row[9] == 0 {
                None
            } else {
                Some(indexed(&["", "tree-sitter", "scip", "heuristic"], row[9])?.into())
            },
            metadata: string_at(arena, row, 20)?
                .map(|text| serde_json::from_str(&text))
                .transpose()
                .map_err(|error| error.to_string())?,
        });
    }
    let mut unresolved_references = Vec::with_capacity(u32_at(&out.meta, 12) as usize);
    for row in out.refs.chunks_exact(REF_ROW_SIZE) {
        unresolved_references.push(UnresolvedReference {
            from_node_id: endpoint(arena, row, 0, 32, &ids)?,
            reference_name: required_string(arena, row, 16)?,
            reference_kind: if row[4] == FUNCTION_REF_CODE {
                "function_ref"
            } else {
                indexed(&EDGE_KINDS, row[4])?
            }
            .into(),
            line: u32_at(row, 8),
            column: u32_at(row, 12),
            file_path: (row[5] & REF_FLAG_FILE_PATH != 0).then(|| file_path.to_owned()),
            language: None,
            candidates: string_at(arena, row, 24)?
                .map(|value| value.split('\0').map(str::to_owned).collect()),
        });
    }
    let errors = string_at(arena, &out.meta, 20)?
        .map(|text| serde_json::from_str(&text))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    Ok(ExtractionResult {
        nodes,
        edges,
        unresolved_references,
        errors,
        duration_ms: 0.0,
    })
}
