use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::args::essence_map::{EssenceMapAction, EssenceMapPaths};

pub(crate) const MAX_NODE_COUNT: usize = 100;
const CAPABILITIES_PATH: &str = "essences/CAPABILITIES.md";
const CORE_PATH: &str = "essences/CORE.md";
const MAP_PATH: &str = "essences/map.jsonl";
const DECISIONS_PATH: &str = "essences/decisions.jsonl";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NodeType {
    Vector,
    Capability,
    Core,
    Client,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EdgeType {
    RunsOn,
    Feeds,
    PartOf,
    ClientOf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceRef {
    pub path: String,
    pub anchor: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub label: String,
    pub vector: Option<String>,
    pub source: SourceRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs_on_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct EdgeRecord {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    pub source: SourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MapRecord {
    Node(NodeRecord),
    Edge(EdgeRecord),
}

impl Serialize for MapRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            MapRecord::Node(node) => {
                let mut state = serializer.serialize_struct(
                    "MapRecord",
                    6 + usize::from(node.runs_on_reason.is_some()),
                )?;
                state.serialize_field("kind", "node")?;
                state.serialize_field("id", &node.id)?;
                state.serialize_field("type", &node.node_type)?;
                state.serialize_field("label", &node.label)?;
                state.serialize_field("vector", &node.vector)?;
                state.serialize_field("source", &node.source)?;
                if let Some(reason) = &node.runs_on_reason {
                    state.serialize_field("runs_on_reason", reason)?;
                }
                state.end()
            }
            MapRecord::Edge(edge) => {
                let mut state = serializer.serialize_struct("MapRecord", 5)?;
                state.serialize_field("kind", "edge")?;
                state.serialize_field("from", &edge.from)?;
                state.serialize_field("to", &edge.to)?;
                state.serialize_field("type", &edge.edge_type)?;
                state.serialize_field("source", &edge.source)?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for MapRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let Some(object) = value.as_object() else {
            return Err(serde::de::Error::custom("map record must be a JSON object"));
        };
        let Some(kind) = object.get("kind").and_then(serde_json::Value::as_str) else {
            return Err(serde::de::Error::custom("map record requires kind"));
        };
        let mut payload = object.clone();
        payload.remove("kind");
        match kind {
            "node" => serde_json::from_value(serde_json::Value::Object(payload))
                .map(MapRecord::Node)
                .map_err(serde::de::Error::custom),
            "edge" => serde_json::from_value(serde_json::Value::Object(payload))
                .map(MapRecord::Edge)
                .map_err(serde::de::Error::custom),
            other => Err(serde::de::Error::custom(format!(
                "unknown map record kind `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionRecord {
    pub kind: DecisionKind,
    pub id: String,
    pub map_sha256: String,
    pub summary: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum DecisionKind {
    #[serde(rename = "decision")]
    Decision,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub(crate) struct MapSummary {
    pub nodes: usize,
    pub edges: usize,
    pub vectors: usize,
    pub capabilities: usize,
    pub core: usize,
    pub clients: usize,
    pub runs_on: usize,
    pub feeds: usize,
    pub part_of: usize,
    pub client_of: usize,
}

#[derive(Debug, Clone)]
struct SourceFile {
    path: &'static str,
    text: String,
    sha256: String,
}

#[derive(Debug, Clone)]
struct VectorDef {
    label: String,
    id: String,
}

#[derive(Debug, Clone)]
struct CapabilityDef {
    label: String,
    id: String,
    vector: String,
    runs_on: Vec<String>,
    runs_on_reason: Option<String>,
    feeds: Vec<String>,
}

#[derive(Debug, Clone)]
struct CoreDef {
    label: String,
    id: String,
    part_of: Option<String>,
}

#[derive(Debug, Clone)]
struct ClientDef {
    label: String,
    id: String,
}

#[derive(Debug, Clone)]
struct Inventory {
    nodes: Vec<NodeRecord>,
    edges: Vec<EdgeRecord>,
    source_files: BTreeMap<&'static str, SourceFile>,
}

pub(crate) fn handle(json_output: bool, action: EssenceMapAction) {
    let result = match action {
        EssenceMapAction::Generate(paths) => generate_from_paths(&paths),
        EssenceMapAction::Validate(paths) => validate_from_paths(&paths),
    };

    match result {
        Ok(summary) if json_output => {
            println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        }
        Ok(summary) => print_summary(&summary),
        Err(error) => {
            eprintln!("✗ essence-map failed: {error}");
            eprintln!("fix: codeterm essence-map generate --root .");
            std::process::exit(1);
        }
    }
}

fn generate_from_paths(paths: &EssenceMapPaths) -> Result<MapSummary, String> {
    let root = canonical_root(&paths.root)?;
    let map_path = resolve_path(&root, paths.map.as_deref(), MAP_PATH);
    let inventory = build_inventory(&root)?;
    let records = records_for(&inventory);
    enforce_budget(&records)?;
    let contents = records_to_jsonl(&records)?;
    write_text(&map_path, &contents)?;
    Ok(summary(&records))
}

fn validate_from_paths(paths: &EssenceMapPaths) -> Result<MapSummary, String> {
    let root = canonical_root(&paths.root)?;
    let map_path = resolve_path(&root, paths.map.as_deref(), MAP_PATH);
    let decisions_path = resolve_path(&root, paths.decisions.as_deref(), DECISIONS_PATH);
    let inventory = build_inventory(&root)?;
    let actual = read_map(&map_path)?;
    validate_map_records(&actual, &inventory, &map_path)?;
    validate_decisions(&decisions_path)?;
    Ok(summary(&actual))
}

pub(crate) fn generate(root: &Path, map_path: &Path) -> Result<MapSummary, String> {
    let inventory = build_inventory(root)?;
    let records = records_for(&inventory);
    enforce_budget(&records)?;
    write_text(map_path, &records_to_jsonl(&records)?)?;
    Ok(summary(&records))
}

pub(crate) fn validate(
    root: &Path,
    map_path: &Path,
    decisions_path: &Path,
) -> Result<MapSummary, String> {
    let inventory = build_inventory(root)?;
    let actual = read_map(map_path)?;
    validate_map_records(&actual, &inventory, map_path)?;
    validate_decisions(decisions_path)?;
    Ok(summary(&actual))
}

fn canonical_root(root: &Path) -> Result<PathBuf, String> {
    let root = if root.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        root.to_path_buf()
    };
    if !root.is_dir() {
        return Err(format!(
            "repository root is not a directory: {}",
            root.display()
        ));
    }
    Ok(root)
}

fn resolve_path(root: &Path, supplied: Option<&Path>, default: &str) -> PathBuf {
    let path = supplied.unwrap_or_else(|| Path::new(default));
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn read_source(root: &Path, path: &'static str) -> Result<SourceFile, String> {
    let full_path = root.join(path);
    let bytes = fs::read(&full_path)
        .map_err(|error| format!("cannot read source essence {path}: {error}"))?;
    let text = String::from_utf8(bytes.clone())
        .map_err(|error| format!("source essence {path} is not UTF-8: {error}"))?;
    let canonical_bytes = text.replace("\r\n", "\n");
    Ok(SourceFile {
        path,
        text,
        sha256: sha256(canonical_bytes.as_bytes()),
    })
}

fn build_inventory(root: &Path) -> Result<Inventory, String> {
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
        });
    }

    let node_ids: BTreeSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
    if node_ids.len() != nodes.len() {
        return Err("source essences produce duplicate node ids".to_string());
    }
    let mut labels = BTreeMap::new();
    for node in &nodes {
        labels.insert(label_key(&node.label), node.id.clone());
    }

    let mut edges = Vec::new();
    for capability in &vectors_and_capabilities.1 {
        let source = source_ref(&capabilities, &capability.id);
        for target in &capability.runs_on {
            let target_id = labels.get(&label_key(target)).ok_or_else(|| {
                format!(
                    "capability `{}` runs on unknown essence `{target}`",
                    capability.label
                )
            })?;
            edges.push(EdgeRecord {
                from: capability.id.clone(),
                to: target_id.clone(),
                edge_type: EdgeType::RunsOn,
                source: source.clone(),
            });
        }
        for target in &capability.feeds {
            let target_id = labels.get(&label_key(target)).ok_or_else(|| {
                format!(
                    "capability `{}` feeds unknown capability `{target}`",
                    capability.label
                )
            })?;
            edges.push(EdgeRecord {
                from: capability.id.clone(),
                to: target_id.clone(),
                edge_type: EdgeType::Feeds,
                source: source.clone(),
            });
        }
    }
    for part in &core_data.0 {
        let target = part
            .part_of
            .as_deref()
            .map(|parent| {
                labels.get(&label_key(parent)).cloned().ok_or_else(|| {
                    format!("core part `{}` has unknown parent `{parent}`", part.label)
                })
            })
            .transpose()?;
        edges.push(EdgeRecord {
            from: part.id.clone(),
            to: target.unwrap_or_else(|| part.id.clone()),
            edge_type: EdgeType::PartOf,
            source: source_ref(&core, &part.id),
        });
    }
    let service_layer = labels
        .get(&label_key("Service layer"))
        .ok_or_else(|| "core essence has no `Service layer` part".to_string())?;
    for client in &core_data.1 {
        edges.push(EdgeRecord {
            from: client.id.clone(),
            to: service_layer.clone(),
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

fn parse_capabilities(text: &str) -> Result<(Vec<VectorDef>, Vec<CapabilityDef>), String> {
    let mut vectors = Vec::new();
    let mut capabilities = Vec::new();
    let mut current_vector = None;
    let mut in_table = false;

    for line in text.lines() {
        if let Some(label) = parse_vector_heading(line) {
            let id = node_id(NodeType::Vector, &label);
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
        if cells.first().is_some_and(|cell| cell == "Capability") {
            in_table = true;
            continue;
        }
        if !in_table || is_table_separator(&cells) || cells.len() < 7 {
            continue;
        }
        let vector = current_vector
            .clone()
            .ok_or_else(|| "capability table appears outside a vector heading".to_string())?;
        let label = cells[0].clone();
        if label.is_empty() {
            return Err("capability table contains an empty capability name".to_string());
        }
        let (runs_on, runs_on_reason) = parse_relation_cell(&cells[5]);
        let feeds = parse_relation_cell(&cells[6]).0;
        capabilities.push(CapabilityDef {
            id: node_id(NodeType::Capability, &label),
            label,
            vector,
            runs_on,
            runs_on_reason,
            feeds,
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
        if let Some(label) = line.strip_prefix("### ") {
            if let Some(part) = current_part.take() {
                parts.push(part);
            }
            let label = label.trim().to_string();
            current_part = Some(CoreDef {
                id: node_id(NodeType::Core, &label),
                label,
                part_of: None,
            });
            continue;
        }
        if let Some(parent) = line.trim().strip_prefix("- Part of:") {
            if let Some(part) = current_part.as_mut() {
                let parent = parent.trim().trim_end_matches('.').trim();
                if parent.eq_ignore_ascii_case("the running daemon itself") {
                    part.part_of = None;
                } else {
                    part.part_of = Some(strip_leading_the(parent).to_string());
                }
            }
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
        if cells.first().is_some_and(|cell| cell == "Client") {
            in_clients = true;
            continue;
        }
        if !in_clients || is_table_separator(&cells) || cells.is_empty() {
            continue;
        }
        let label = cells[0].clone();
        if label.is_empty() {
            return Err("CORE.md contains an empty client name".to_string());
        }
        clients.push(ClientDef {
            id: node_id(NodeType::Client, &label),
            label,
        });
    }

    if parts.is_empty() {
        return Err("CORE.md contains no core part headings".to_string());
    }
    if clients.is_empty() {
        return Err("CORE.md contains no client rows".to_string());
    }
    Ok((parts, clients))
}

fn parse_vector_heading(line: &str) -> Option<String> {
    let rest = line.strip_prefix("## ")?;
    let (number, label) = rest.split_once('.')?;
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let label = label.trim();
    (!label.is_empty()).then(|| label.to_string())
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

fn strip_leading_the(value: &str) -> &str {
    value
        .strip_prefix("the ")
        .or_else(|| value.strip_prefix("The "))
        .unwrap_or(value)
}

fn source_ref(source: &SourceFile, anchor: &str) -> SourceRef {
    SourceRef {
        path: source.path.to_string(),
        anchor: anchor.to_string(),
        sha256: source.sha256.clone(),
    }
}

fn node_id(node_type: NodeType, label: &str) -> String {
    let prefix = match node_type {
        NodeType::Vector => "vector",
        NodeType::Capability => "capability",
        NodeType::Core => "core",
        NodeType::Client => "client",
    };
    format!("{prefix}:{}", slug(label))
}

fn slug(value: &str) -> String {
    let mut output = String::new();
    let mut pending_dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !output.is_empty() {
                output.push('-');
            }
            output.push(character.to_ascii_lowercase());
            pending_dash = false;
        } else if !output.is_empty() {
            pending_dash = true;
        }
    }
    output
}

fn label_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn records_for(inventory: &Inventory) -> Vec<MapRecord> {
    let mut records = Vec::with_capacity(inventory.nodes.len() + inventory.edges.len());
    records.extend(inventory.nodes.iter().cloned().map(MapRecord::Node));
    records.extend(inventory.edges.iter().cloned().map(MapRecord::Edge));
    records
}

fn records_to_jsonl(records: &[MapRecord]) -> Result<String, String> {
    let mut output = String::new();
    for record in records {
        output.push_str(
            &serde_json::to_string(record)
                .map_err(|error| format!("cannot serialize generated map record: {error}"))?,
        );
        output.push('\n');
    }
    Ok(output)
}

fn read_map(path: &Path) -> Result<Vec<MapRecord>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read map {}: {error}", path.display()))?;
    let mut records = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        if line.trim().is_empty() {
            return Err(format!(
                "map {} line {line_number}: blank JSONL record",
                path.display()
            ));
        }
        let record = serde_json::from_str(line).map_err(|error| {
            format!(
                "map {} line {line_number}: malformed record: {error}",
                path.display()
            )
        })?;
        records.push(record);
    }
    if records.is_empty() {
        return Err(format!("map {} is empty", path.display()));
    }
    Ok(records)
}

fn validate_map_records(
    records: &[MapRecord],
    inventory: &Inventory,
    map_path: &Path,
) -> Result<(), String> {
    enforce_budget(records)?;
    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    let mut edge_keys = BTreeSet::new();
    for record in records {
        if let MapRecord::Node(node) = record {
            validate_node_shape(node, inventory)?;
            if nodes.insert(node.id.clone(), node).is_some() {
                return Err(format!(
                    "map {} has duplicate node `{}`",
                    map_path.display(),
                    node.id
                ));
            }
        }
    }
    for record in records {
        if let MapRecord::Edge(edge) = record {
            validate_edge_shape(edge, inventory, &nodes)?;
            let key = edge_key(edge);
            if !edge_keys.insert(key) {
                return Err(format!(
                    "map {} has duplicate {} edge {} -> {}",
                    map_path.display(),
                    edge_type_name(edge.edge_type),
                    edge.from,
                    edge.to
                ));
            }
            edges.push(edge);
        }
    }

    let expected_nodes: BTreeMap<&str, &NodeRecord> = inventory
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    for (id, expected) in &expected_nodes {
        let Some(actual) = nodes.get(*id) else {
            return Err(format!("map {} is missing node `{id}`", map_path.display()));
        };
        if *actual != *expected {
            return Err(format!(
                "map {} node `{id}` does not match the current source essence; regenerate with `codeterm essence-map generate`",
                map_path.display()
            ));
        }
    }
    if let Some(unexpected) = nodes
        .keys()
        .find(|id| !expected_nodes.contains_key(id.as_str()))
    {
        return Err(format!(
            "map {} has node `{unexpected}` that is not generated from the current essences",
            map_path.display()
        ));
    }

    let expected_edges: BTreeSet<String> = inventory.edges.iter().map(edge_key).collect();
    let actual_edges: BTreeSet<String> = edges.iter().map(|edge| edge_key(edge)).collect();
    if actual_edges != expected_edges {
        let missing = expected_edges.difference(&actual_edges).next();
        let extra = actual_edges.difference(&expected_edges).next();
        return Err(format!(
            "map {} edges differ from the current source essences (missing: {}; unexpected: {}); regenerate with `codeterm essence-map generate`",
            map_path.display(),
            missing.map_or("none", String::as_str),
            extra.map_or("none", String::as_str)
        ));
    }

    validate_flow_links(&nodes, &edges, map_path)?;
    Ok(())
}

fn validate_node_shape(node: &NodeRecord, inventory: &Inventory) -> Result<(), String> {
    if node.id.trim().is_empty() || node.label.trim().is_empty() {
        return Err(format!("node `{}` has an empty id or label", node.id));
    }
    let prefix = match node.node_type {
        NodeType::Vector => "vector",
        NodeType::Capability => "capability",
        NodeType::Core => "core",
        NodeType::Client => "client",
    };
    if !node.id.starts_with(&format!("{prefix}:")) || node.id.len() <= prefix.len() + 1 {
        return Err(format!(
            "node `{}` has id incompatible with type `{prefix}`",
            node.id
        ));
    }
    if !is_sha256(&node.source.sha256) {
        return Err(format!("node `{}` has malformed source sha256", node.id));
    }
    let expected_path = match node.node_type {
        NodeType::Vector | NodeType::Capability => CAPABILITIES_PATH,
        NodeType::Core | NodeType::Client => CORE_PATH,
    };
    if node.source.path != expected_path {
        return Err(format!(
            "node `{}` references `{}`, expected `{expected_path}`",
            node.id, node.source.path
        ));
    }
    let current_hash = inventory
        .source_files
        .get(expected_path)
        .map(|source| source.sha256.as_str())
        .unwrap_or_default();
    if node.source.sha256 != current_hash {
        return Err(format!(
            "node `{}` is stale: source essence {} changed since map generation; regenerate the map",
            node.id, node.source.path
        ));
    }
    if node.source.anchor != node.id {
        return Err(format!(
            "node `{}` has source anchor `{}`, expected `{}`",
            node.id, node.source.anchor, node.id
        ));
    }
    match node.node_type {
        NodeType::Capability if node.vector.is_none() => Err(format!(
            "capability node `{}` must name its vector",
            node.id
        )),
        NodeType::Capability => Ok(()),
        _ if node.vector.is_some() => Err(format!(
            "node `{}` of type `{prefix}` must not carry a vector membership",
            node.id
        )),
        _ if node.runs_on_reason.is_some() => Err(format!(
            "node `{}` carries runs_on_reason but is not a capability",
            node.id
        )),
        _ => Ok(()),
    }?;
    if node
        .runs_on_reason
        .as_deref()
        .is_some_and(|reason| reason.trim().is_empty())
    {
        return Err(format!("node `{}` has an empty runs_on_reason", node.id));
    }
    Ok(())
}

fn validate_edge_shape(
    edge: &EdgeRecord,
    inventory: &Inventory,
    nodes: &BTreeMap<String, &NodeRecord>,
) -> Result<(), String> {
    let Some(from) = nodes.get(&edge.from) else {
        return Err(format!(
            "edge {} -> {} references dangling source",
            edge.from, edge.to
        ));
    };
    let Some(to) = nodes.get(&edge.to) else {
        return Err(format!(
            "edge {} -> {} references dangling target",
            edge.from, edge.to
        ));
    };
    if !is_sha256(&edge.source.sha256) {
        return Err(format!(
            "edge {} -> {} has malformed source sha256",
            edge.from, edge.to
        ));
    }
    if edge.source != from.source {
        let current_hash = inventory
            .source_files
            .get(edge.source.path.as_str())
            .map(|source| source.sha256.as_str());
        if current_hash == Some(edge.source.sha256.as_str()) {
            return Err(format!(
                "edge {} -> {} source does not match its source node",
                edge.from, edge.to
            ));
        }
        return Err(format!(
            "edge {} -> {} is stale: source essence changed since map generation",
            edge.from, edge.to
        ));
    }
    let valid = match edge.edge_type {
        EdgeType::RunsOn => {
            from.node_type == NodeType::Capability
                && matches!(to.node_type, NodeType::Core | NodeType::Client)
        }
        EdgeType::Feeds => {
            from.node_type == NodeType::Capability && to.node_type == NodeType::Capability
        }
        EdgeType::PartOf => from.node_type == NodeType::Core && to.node_type == NodeType::Core,
        EdgeType::ClientOf => from.node_type == NodeType::Client && to.node_type == NodeType::Core,
    };
    if !valid {
        return Err(format!(
            "{} edge {} -> {} has incompatible node kinds",
            edge_type_name(edge.edge_type),
            edge.from,
            edge.to
        ));
    }
    Ok(())
}

fn validate_flow_links(
    nodes: &BTreeMap<String, &NodeRecord>,
    edges: &[&EdgeRecord],
    map_path: &Path,
) -> Result<(), String> {
    for node in nodes
        .values()
        .filter(|node| node.node_type == NodeType::Capability)
    {
        let has_runs_on = edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::RunsOn && edge.from == node.id);
        if !has_runs_on && node.runs_on_reason.is_none() {
            return Err(format!(
                "map {} capability `{}` has no runs_on edge and no runs_on_reason",
                map_path.display(),
                node.id
            ));
        }
        let has_flow = edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Feeds && (edge.from == node.id || edge.to == node.id)
        });
        if !has_flow {
            return Err(format!(
                "map {} capability `{}` has no inbound or outbound feeds edge",
                map_path.display(),
                node.id
            ));
        }
    }
    Ok(())
}

fn validate_decisions(path: &Path) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read decisions {}: {error}", path.display()))?;
    let mut previous_number = 0u64;
    let mut ids = BTreeSet::new();
    for (line_index, line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        if line.trim().is_empty() {
            return Err(format!(
                "decisions {} line {line_number}: blank JSONL record",
                path.display()
            ));
        }
        let decision: DecisionRecord = serde_json::from_str(line).map_err(|error| {
            format!(
                "decisions {} line {line_number}: malformed record: {error}",
                path.display()
            )
        })?;
        if decision.id.trim().is_empty()
            || decision.summary.trim().is_empty()
            || decision.reason.trim().is_empty()
        {
            return Err(format!(
                "decisions {} line {line_number}: id, summary and reason are required",
                path.display()
            ));
        }
        if !is_sha256(&decision.map_sha256) {
            return Err(format!(
                "decisions {} line {line_number}: map_sha256 must be 64 lowercase hex characters",
                path.display()
            ));
        }
        let Some(number) = decision
            .id
            .strip_prefix("decision-")
            .and_then(|value| value.parse().ok())
        else {
            return Err(format!(
                "decisions {} line {line_number}: id `{}` must use decision-N numbering",
                path.display(),
                decision.id
            ));
        };
        if number <= previous_number {
            return Err(format!(
                "decisions {} line {line_number}: decision ids must be strictly increasing (append-only)",
                path.display()
            ));
        }
        previous_number = number;
        if !ids.insert(decision.id.clone()) {
            return Err(format!(
                "decisions {} line {line_number}: duplicate decision id `{}`",
                path.display(),
                decision.id
            ));
        }
    }
    Ok(())
}

fn enforce_budget(records: &[MapRecord]) -> Result<(), String> {
    let nodes = records
        .iter()
        .filter(|record| matches!(record, MapRecord::Node(_)))
        .count();
    if nodes > MAX_NODE_COUNT {
        return Err(format!(
            "essence map contains {nodes} nodes, exceeding the per-project limit of {MAX_NODE_COUNT}; simplify the essences"
        ));
    }
    Ok(())
}

fn summary(records: &[MapRecord]) -> MapSummary {
    let mut summary = MapSummary {
        nodes: records
            .iter()
            .filter(|record| matches!(record, MapRecord::Node(_)))
            .count(),
        edges: records
            .iter()
            .filter(|record| matches!(record, MapRecord::Edge(_)))
            .count(),
        ..MapSummary::default()
    };
    for record in records {
        match record {
            MapRecord::Node(node) => match node.node_type {
                NodeType::Vector => summary.vectors += 1,
                NodeType::Capability => summary.capabilities += 1,
                NodeType::Core => summary.core += 1,
                NodeType::Client => summary.clients += 1,
            },
            MapRecord::Edge(edge) => match edge.edge_type {
                EdgeType::RunsOn => summary.runs_on += 1,
                EdgeType::Feeds => summary.feeds += 1,
                EdgeType::PartOf => summary.part_of += 1,
                EdgeType::ClientOf => summary.client_of += 1,
            },
        }
    }
    summary
}

fn edge_key(edge: &EdgeRecord) -> String {
    format!(
        "{}:{}->{}",
        edge_type_name(edge.edge_type),
        edge.from,
        edge.to
    )
}

fn edge_type_name(edge_type: EdgeType) -> &'static str {
    match edge_type {
        EdgeType::RunsOn => "runs_on",
        EdgeType::Feeds => "feeds",
        EdgeType::PartOf => "part_of",
        EdgeType::ClientOf => "client_of",
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn write_text(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!("cannot create map directory {}: {error}", parent.display())
        })?;
    }
    fs::write(path, contents)
        .map_err(|error| format!("cannot write generated map {}: {error}", path.display()))
}

fn print_summary(summary: &MapSummary) {
    println!(
        "essence map valid: {} nodes ({} vectors, {} capabilities, {} core, {} clients), {} edges ({} runs_on, {} feeds, {} part_of, {} client_of)",
        summary.nodes,
        summary.vectors,
        summary.capabilities,
        summary.core,
        summary.clients,
        summary.edges,
        summary.runs_on,
        summary.feeds,
        summary.part_of,
        summary.client_of
    );
}

#[cfg(test)]
mod tests;
