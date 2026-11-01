//! Declared type ancestry, subtype fans, and name-based member override matches.
use anyhow::{Context, Result};
use prometeu_extract::{Edge, Node};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MAX_ANCESTOR_DEPTH: usize = 8;
pub const MAX_DESCENDANT_DEPTH: usize = 6;
pub const MAX_DESCENDANTS: usize = 400;
pub const DISPATCH_MIN_IMPLEMENTERS: usize = 8;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyEntry {
    pub node: Node,
    pub depth: usize,
    pub parent_id: String,
    pub relation: String,
    pub edge: Edge,
    pub synthesized: bool,
    pub hidden_subtypes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OverrideMatch {
    pub member_id: String,
    pub base_id: String,
    pub base_type_id: String,
    pub base_type_name: String,
    pub relation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TypeHierarchy {
    pub focus: Node,
    pub ancestors: Vec<HierarchyEntry>,
    pub descendants: Vec<HierarchyEntry>,
    pub direct_subtypes: usize,
    pub direct_implementers: usize,
    pub bounded: bool,
    pub polymorphic: bool,
    pub overrides: BTreeMap<String, OverrideMatch>,
}

pub fn can_have_hierarchy(node: &Node) -> bool {
    matches!(
        node.kind.as_str(),
        "class" | "interface" | "struct" | "trait" | "protocol" | "enum" | "type_alias" | "union"
    )
}

/// Count distinct direct subtypes, including parsed and synthesized inheritance.
pub fn count_implementers(edges: &[Edge], type_id: &str) -> usize {
    edges
        .iter()
        .filter(|edge| edge.target == type_id && inheritance(edge))
        .map(|edge| edge.source.as_str())
        .collect::<HashSet<_>>()
        .len()
}

fn inheritance(edge: &Edge) -> bool {
    matches!(edge.kind.as_str(), "extends" | "implements")
}

fn entry(node: &Node, depth: usize, parent: &str, edge: &Edge) -> HierarchyEntry {
    HierarchyEntry {
        node: node.clone(),
        depth,
        parent_id: parent.into(),
        relation: edge.kind.clone(),
        edge: edge.clone(),
        synthesized: edge.provenance.as_deref() == Some("heuristic"),
        hidden_subtypes: 0,
    }
}

fn sort_level(level: &mut [HierarchyEntry]) {
    static COLLATOR: std::sync::LazyLock<icu_collator::CollatorBorrowed<'static>> =
        std::sync::LazyLock::new(|| {
            let locale: icu_locale_core::Locale = "en".parse().expect("valid English locale");
            icu_collator::Collator::try_new(locale.into(), Default::default())
                .expect("compiled English collation data")
        });
    level.sort_by(|a, b| {
        a.relation
            .cmp(&b.relation)
            .then_with(|| COLLATOR.compare(&a.node.name, &b.node.name))
            .then_with(|| COLLATOR.compare(&a.node.file_path, &b.node.file_path))
            .then(a.node.start_line.cmp(&b.node.start_line))
    });
}

fn selected_edges<'a>(edges: &'a [Edge], ids: &[String], up: bool, members: bool) -> Vec<&'a Edge> {
    let ids = ids.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut found = edges
        .iter()
        .filter(|edge| {
            (if members {
                edge.kind == "contains"
            } else {
                inheritance(edge)
            }) && ids.contains(if up {
                edge.source.as_str()
            } else {
                edge.target.as_str()
            })
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| {
        let endpoint = |edge: &'a Edge| if up { &edge.source } else { &edge.target };
        endpoint(a).cmp(endpoint(b)).then(a.kind.cmp(&b.kind))
    });
    found
}

/// Derive a hierarchy from canonical records without filesystem or database access.
/// Override matches compare member names within known ancestry, not signatures.
pub fn build_type_hierarchy(
    nodes: &[Node],
    edges: &[Edge],
    focus_id: &str,
    overrides: bool,
) -> Result<Option<TypeHierarchy>> {
    let nodes = nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let focus = *nodes
        .get(focus_id)
        .context("hierarchy focus is absent from the snapshot")?;
    if !can_have_hierarchy(focus) {
        return Ok(None);
    }
    let mut result = TypeHierarchy {
        focus: focus.clone(),
        ancestors: vec![],
        descendants: vec![],
        direct_subtypes: 0,
        direct_implementers: 0,
        bounded: false,
        polymorphic: false,
        overrides: BTreeMap::new(),
    };
    let mut seen = HashSet::from([focus_id.to_owned()]);
    let mut frontier = vec![focus_id.to_owned()];
    for depth in 1..=MAX_ANCESTOR_DEPTH {
        let mut level = Vec::new();
        for edge in selected_edges(edges, &frontier, true, false) {
            if let Some(node) = nodes.get(edge.target.as_str())
                && seen.insert(node.id.clone())
            {
                level.push(entry(node, depth, &edge.source, edge));
            }
        }
        sort_level(&mut level);
        frontier = level.iter().map(|entry| entry.node.id.clone()).collect();
        result.ancestors.extend(level);
        if frontier.is_empty() {
            break;
        }
    }
    seen = HashSet::from([focus_id.to_owned()]);
    frontier = vec![focus_id.to_owned()];
    let mut by_id = HashMap::<String, usize>::new();
    for depth in 1..=MAX_DESCENDANT_DEPTH {
        let mut level: Vec<HierarchyEntry> = Vec::new();
        let mut level_seen = HashSet::new();
        let mut overflow = HashMap::<&str, usize>::new();
        for edge in selected_edges(edges, &frontier, false, false) {
            let Some(node) = nodes
                .get(edge.source.as_str())
                .filter(|node| !seen.contains(&node.id))
            else {
                continue;
            };
            if level_seen.contains(&node.id)
                && let Some(existing) = level.iter_mut().find(|entry| entry.node.id == node.id)
            {
                if existing.relation == "implements" && edge.kind == "extends" {
                    existing.relation = "extends".into();
                    existing.edge = edge.clone();
                    existing.synthesized = edge.provenance.as_deref() == Some("heuristic");
                }
                continue;
            }
            if depth == 1 {
                result.direct_subtypes += 1;
                result.direct_implementers += usize::from(edge.kind == "implements");
            }
            level_seen.insert(node.id.clone());
            if result.descendants.len() + level.len() >= MAX_DESCENDANTS {
                result.bounded = true;
                *overflow.entry(&edge.target).or_default() += 1;
            } else {
                level.push(entry(node, depth, &edge.target, edge));
            }
        }
        sort_level(&mut level);
        frontier = level.iter().map(|entry| entry.node.id.clone()).collect();
        for entry in level {
            seen.insert(entry.node.id.clone());
            by_id.insert(entry.node.id.clone(), result.descendants.len());
            result.descendants.push(entry);
        }
        for (parent, count) in overflow {
            if let Some(&index) = by_id.get(parent) {
                result.descendants[index].hidden_subtypes += count;
            }
        }
        if result.bounded || frontier.is_empty() {
            break;
        }
        if depth == MAX_DESCENDANT_DEPTH {
            for edge in selected_edges(edges, &frontier, false, false) {
                if !seen.contains(&edge.source) {
                    result.bounded = true;
                    if let Some(&index) = by_id.get(&edge.target) {
                        result.descendants[index].hidden_subtypes += 1;
                    }
                }
            }
        }
    }
    if result.ancestors.is_empty() && result.descendants.is_empty() {
        return Ok(None);
    }
    result.polymorphic = result.direct_implementers >= DISPATCH_MIN_IMPLEMENTERS;
    if overrides {
        let chain = result.ancestors.iter().take(12).collect::<Vec<_>>();
        let ids = chain
            .iter()
            .map(|entry| entry.node.id.clone())
            .collect::<Vec<_>>();
        let rank = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect::<HashMap<_, _>>();
        let mut members = selected_edges(edges, &ids, true, true)
            .into_iter()
            .filter_map(|edge| {
                nodes
                    .get(edge.target.as_str())
                    .map(|&node| (node, edge.source.as_str()))
            })
            .collect::<Vec<_>>();
        members.sort_by_key(|(node, owner)| (rank[owner], node.start_line));
        let mut by_name = HashMap::new();
        for (node, owner) in members {
            by_name.entry(node.name.as_str()).or_insert((node, owner));
        }
        for edge in selected_edges(edges, &[focus_id.to_owned()], true, true) {
            let Some(member) = nodes.get(edge.target.as_str()).filter(|node| {
                matches!(
                    node.kind.as_str(),
                    "method" | "function" | "property" | "field"
                )
            }) else {
                continue;
            };
            let Some(&(base, owner)) = by_name.get(member.name.as_str()) else {
                continue;
            };
            if member.id == base.id {
                continue;
            }
            let ancestor = chain[rank[owner]];
            result.overrides.insert(
                member.id.clone(),
                OverrideMatch {
                    member_id: member.id.clone(),
                    base_id: base.id.clone(),
                    base_type_id: ancestor.node.id.clone(),
                    base_type_name: ancestor.node.name.clone(),
                    relation: ancestor.relation.clone(),
                },
            );
        }
    }
    Ok(Some(result))
}
