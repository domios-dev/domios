//! The source-essence parser: it turns `capabilities.md` and `core.md` into the
//! node and edge inventory the map is generated from. Every element id is
//! authored in source metadata, so it survives a heading or label rename.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::sha256;
use super::{EdgeRecord, EdgeType, NodeRecord, NodeType, SourceRef};
use super::{CAPABILITIES_PATH, CORE_PATH};

pub(super) struct SourceFile {
    pub(super) path: &'static str,
    pub(super) text: String,
    pub(super) sha256: String,
}

#[derive(Debug, Clone)]
struct VectorDef {
    label: String,
    id: String,
}

#[derive(Debug, Clone)]
struct CapabilityDef {
    id: String,
    label: String,
    vector: String,
    runs_on: Vec<String>,
    runs_on_reason: Option<String>,
    feeds: Vec<String>,
    status: String,
}

#[derive(Debug, Clone)]
struct CoreDef {
    id: String,
    label: String,
    root: bool,
    part_of: Option<String>,
}

#[derive(Debug, Clone)]
struct ClientDef {
    id: String,
    label: String,
}

pub(super) struct Inventory {
    pub(super) nodes: Vec<NodeRecord>,
    pub(super) edges: Vec<EdgeRecord>,
    pub(super) source_files: BTreeMap<&'static str, SourceFile>,
}

fn read_source(root: &Path, path: &'static str) -> Result<SourceFile, String> {
    let full_path = root.join(path);
    let bytes = fs::read(&full_path)
        .map_err(|error| format!("cannot read source essence {path}: {error}"))?;
    let text = String::from_utf8(bytes.clone())
        .map_err(|error| format!("source essence {path} is not UTF-8: {error}"))?;
    let canonical_bytes = text.replace("\r\n", "\n");
    let sha256 = sha256(canonical_bytes.as_bytes());
    Ok(SourceFile {
        path,
        text: canonical_bytes,
        sha256,
    })
}

pub(super) fn build_inventory(root: &Path) -> Result<Inventory, String> {
    let capabilities = read_source(root, CAPABILITIES_PATH)?;
    let core = read_source(root, CORE_PATH)?;
    let vectors_and_capabilities = parse_capabilities(&capabilities.text)?;
    let core_data = parse_core(&core.text)?;

    let mut nodes = Vec::new();
    for vector in &vectors_and_capabilities.0 {
        nodes.push(NodeRecord {
            id: vector.id.clone(),
            node_type: NodeType::Vector,
            label: vector.label.clone(),
            vector: None,
            source: source_ref(&capabilities, &vector.id),
            runs_on_reason: None,
            status: None,
        });
    }
    for capability in &vectors_and_capabilities.1 {
        nodes.push(NodeRecord {
            id: capability.id.clone(),
            node_type: NodeType::Capability,
            label: capability.label.clone(),
            vector: Some(capability.vector.clone()),
            source: source_ref(&capabilities, &capability.id),
            runs_on_reason: capability.runs_on_reason.clone(),
            status: Some(capability.status.clone()),
        });
    }
    for part in &core_data.0 {
        nodes.push(NodeRecord {
            id: part.id.clone(),
            node_type: NodeType::Core,
            label: part.label.clone(),
            vector: None,
            source: source_ref(&core, &part.id),
            runs_on_reason: None,
            status: None,
        });
    }
    for client in &core_data.1 {
        nodes.push(NodeRecord {
            id: client.id.clone(),
            node_type: NodeType::Client,
            label: client.label.clone(),
            vector: None,
            source: source_ref(&core, &client.id),
            runs_on_reason: None,
            status: None,
        });
    }

    let node_ids: BTreeSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
    if node_ids.len() != nodes.len() {
        return Err("source essences produce duplicate node ids".to_string());
    }
    super::documents::validate(root, &node_ids)?;

    let core_root = declared_root(&core_data.0)?;
    let mut edges = Vec::new();
    for capability in &vectors_and_capabilities.1 {
        let source = source_ref(&capabilities, &capability.id);
        for target in &capability.runs_on {
            require_node(&node_ids, target).map_err(|_| {
                format!(
                    "capability `{}` runs on unknown essence id `{target}`",
                    capability.label
                )
            })?;
            edges.push(EdgeRecord {
                from: capability.id.clone(),
                to: target.clone(),
                edge_type: EdgeType::RunsOn,
                source: source.clone(),
            });
        }
        for target in &capability.feeds {
            require_node(&node_ids, target).map_err(|_| {
                format!(
                    "capability `{}` feeds unknown capability id `{target}`",
                    capability.label
                )
            })?;
            edges.push(EdgeRecord {
                from: capability.id.clone(),
                to: target.clone(),
                edge_type: EdgeType::Feeds,
                source: source.clone(),
            });
        }
    }
    for part in &core_data.0 {
        let Some(target) = part.part_of.as_deref() else {
            continue;
        };
        require_node(&node_ids, target).map_err(|_| {
            format!(
                "core part `{}` has unknown parent id `{target}`",
                part.label
            )
        })?;
        edges.push(EdgeRecord {
            from: part.id.clone(),
            to: target.to_string(),
            edge_type: EdgeType::PartOf,
            source: source_ref(&core, &part.id),
        });
    }
    for client in &core_data.1 {
        edges.push(EdgeRecord {
            from: client.id.clone(),
            to: core_root.clone(),
            edge_type: EdgeType::ClientOf,
            source: source_ref(&core, &client.id),
        });
    }

    Ok(Inventory {
        nodes,
        edges,
        source_files: [(CAPABILITIES_PATH, capabilities), (CORE_PATH, core)]
            .into_iter()
            .collect(),
    })
}

fn require_node(node_ids: &BTreeSet<String>, id: &str) -> Result<(), ()> {
    node_ids.contains(id).then_some(()).ok_or(())
}

/// The single core part a project declares as its root. The root is authored,
/// never inferred from a part's display name, so any project topology — including
/// a library with no clients — is representable.
fn declared_root(parts: &[CoreDef]) -> Result<String, String> {
    let mut roots = parts.iter().filter(|part| part.root);
    let root = roots.next().ok_or_else(|| {
        "CORE.md declares no core root; mark exactly one part `- Root: true`".to_string()
    })?;
    if roots.next().is_some() {
        return Err("CORE.md declares more than one core root".to_string());
    }
    for part in parts {
        if part.root && part.part_of.is_some() {
            return Err(format!(
                "core part `{}` is the root and cannot also be part of another part",
                part.label
            ));
        }
        if !part.root && part.part_of.is_none() {
            return Err(format!(
                "core part `{}` must declare `- Part of:` or `- Root: true`",
                part.label
            ));
        }
    }
    Ok(root.id.clone())
}

fn parse_capabilities(text: &str) -> Result<(Vec<VectorDef>, Vec<CapabilityDef>), String> {
    let mut vectors = Vec::new();
    let mut capabilities = Vec::new();
    let mut current_vector = None;
    let mut in_table = false;

    for line in text.lines() {
        if let Some((label, id)) = parse_vector_heading(line)? {
            require_stable_id(&id, NodeType::Vector)
                .map_err(|error| format!("vector heading `{label}` {error}"))?;
            vectors.push(VectorDef {
                label,
                id: id.clone(),
            });
            current_vector = Some(id);
            in_table = false;
            continue;
        }
        if line.starts_with("## ") {
            current_vector = None;
            in_table = false;
            continue;
        }
        let Some(cells) = table_cells(line) else {
            continue;
        };
        if cells.first().is_some_and(|cell| cell == "ID")
            && cells.get(1).is_some_and(|cell| cell == "Capability")
        {
            in_table = true;
            continue;
        }
        if !in_table || is_table_separator(&cells) || cells.len() < 8 {
            continue;
        }
        let vector = current_vector
            .clone()
            .ok_or_else(|| "capability table appears outside a vector heading".to_string())?;
        let id = cells[0].clone();
        require_stable_id(&id, NodeType::Capability)
            .map_err(|error| format!("capability row {error}"))?;
        let label = cells[1].clone();
        if label.is_empty() {
            return Err("capability table contains an empty capability name".to_string());
        }
        let (runs_on, runs_on_reason) = parse_relation_cell(&cells[6]);
        let feeds = parse_relation_cell(&cells[7]).0;
        let status = cells[5].trim().to_string();
        if status.is_empty() {
            return Err(format!("capability `{label}` has an empty status"));
        }
        capabilities.push(CapabilityDef {
            id,
            label,
            vector,
            runs_on,
            runs_on_reason,
            feeds,
            status,
        });
    }

    if vectors.is_empty() {
        return Err("CAPABILITIES.md contains no numbered vector headings".to_string());
    }
    if capabilities.is_empty() {
        return Err("CAPABILITIES.md contains no capability rows".to_string());
    }
    Ok((vectors, capabilities))
}

fn parse_core(text: &str) -> Result<(Vec<CoreDef>, Vec<ClientDef>), String> {
    let mut parts = Vec::new();
    let mut current_part: Option<CoreDef> = None;
    let mut in_core_parts = false;
    for line in text.lines() {
        if line.trim() == "## Core parts" {
            in_core_parts = true;
            continue;
        }
        if line.starts_with("## ") {
            if let Some(part) = current_part.take() {
                parts.push(part);
            }
            in_core_parts = false;
            continue;
        }
        if !in_core_parts {
            continue;
        }
        if let Some(heading) = line.strip_prefix("### ") {
            if let Some(part) = current_part.take() {
                parts.push(part);
            }
            let (label, id) = split_id_attribute(heading)
                .map_err(|error| format!("core part heading `{heading}` {error}"))?;
            let label = label.trim().to_string();
            if label.is_empty() {
                return Err("CORE.md contains an empty core part heading".to_string());
            }
            require_stable_id(id, NodeType::Core)
                .map_err(|error| format!("core part `{label}` {error}"))?;
            current_part = Some(CoreDef {
                id: id.to_string(),
                label,
                root: false,
                part_of: None,
            });
            continue;
        }
        let Some(part) = current_part.as_mut() else {
            continue;
        };
        let trimmed = line.trim().trim_end_matches('.').trim();
        if let Some(value) = trimmed.strip_prefix("- Root:") {
            part.root = value.trim().eq_ignore_ascii_case("true");
            continue;
        }
        if let Some(parent) = trimmed.strip_prefix("- Part of:") {
            let parent = parent.trim();
            if parent.is_empty() {
                return Err(format!("core part `{}` has an empty parent id", part.label));
            }
            part.part_of = Some(parent.to_string());
        }
    }
    if let Some(part) = current_part {
        parts.push(part);
    }

    let mut clients = Vec::new();
    let mut in_clients = false;
    for line in text.lines() {
        let Some(cells) = table_cells(line) else {
            if in_clients && (line.starts_with('#') || line.trim().is_empty()) {
                in_clients = false;
            }
            continue;
        };
        if cells.first().is_some_and(|cell| cell == "ID")
            && cells.get(1).is_some_and(|cell| cell == "Client")
        {
            in_clients = true;
            continue;
        }
        if !in_clients || is_table_separator(&cells) || cells.len() < 2 {
            continue;
        }
        let id = cells[0].clone();
        require_stable_id(&id, NodeType::Client).map_err(|error| format!("client row {error}"))?;
        let label = cells[1].clone();
        if label.is_empty() {
            return Err("CORE.md contains an empty client name".to_string());
        }
        clients.push(ClientDef { id, label });
    }

    if parts.is_empty() {
        return Err("CORE.md contains no core part headings".to_string());
    }
    Ok((parts, clients))
}

fn parse_vector_heading(line: &str) -> Result<Option<(String, String)>, String> {
    let Some(rest) = line.strip_prefix("## ") else {
        return Ok(None);
    };
    let Some((number, remainder)) = rest.split_once('.') else {
        return Ok(None);
    };
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return Ok(None);
    }
    let (label, id) = split_id_attribute(remainder)
        .map_err(|error| format!("vector heading `{rest}` {error}"))?;
    let label = label.trim().to_string();
    if label.is_empty() {
        return Err("CAPABILITIES.md contains an empty vector heading".to_string());
    }
    Ok(Some((label, id.to_string())))
}

/// Split a trailing `{#<id>}` metadata attribute from an element heading. The id
/// is authored, not derived from the heading text, so a heading rewrite keeps it.
fn split_id_attribute(text: &str) -> Result<(&str, &str), String> {
    let trimmed = text.trim_end();
    let Some(open) = trimmed.rfind("{#") else {
        return Err("is missing its `{#<id>}` stable id metadata".to_string());
    };
    let Some(inner) = trimmed[open + 2..].strip_suffix('}') else {
        return Err("has malformed `{#<id>}` metadata".to_string());
    };
    if inner.is_empty() || inner.split_whitespace().count() != 1 || inner.contains('=') {
        return Err("`{#<id>}` metadata must name exactly one stable id".to_string());
    }
    Ok((&trimmed[..open], inner.trim()))
}

fn require_stable_id(value: &str, node_type: NodeType) -> Result<(), String> {
    let prefix = match node_type {
        NodeType::Vector => "vector:",
        NodeType::Capability => "capability:",
        NodeType::Core => "core:",
        NodeType::Client => "client:",
    };
    let Some(slug) = value.strip_prefix(prefix) else {
        return Err(format!("id `{value}` must start with `{prefix}`"));
    };
    let well_formed = !slug.is_empty()
        && slug.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        });
    if !well_formed {
        return Err(format!(
            "id `{value}` must be a `{prefix}` prefix followed by a lowercase hyphenated slug"
        ));
    }
    Ok(())
}

fn table_cells(line: &str) -> Option<Vec<String>> {
    let line = line.trim();
    if !line.starts_with('|') || !line.ends_with('|') {
        return None;
    }
    Some(
        line[1..line.len() - 1]
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect(),
    )
}

fn is_table_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|cell| {
            !cell.is_empty()
                && cell
                    .chars()
                    .all(|character| matches!(character, '-' | ':' | ' '))
        })
}

fn parse_relation_cell(cell: &str) -> (Vec<String>, Option<String>) {
    let cell = cell.trim();
    if cell.starts_with('—') || cell == "-" {
        let reason = cell
            .trim_start_matches('—')
            .trim_start_matches('-')
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();
        return (Vec::new(), (!reason.is_empty()).then(|| reason.to_string()));
    }
    (
        cell.split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        None,
    )
}

fn source_ref(source: &SourceFile, anchor: &str) -> SourceRef {
    SourceRef {
        path: source.path.to_string(),
        anchor: anchor.to_string(),
        sha256: source.sha256.clone(),
    }
}
