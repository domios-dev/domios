use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::args::essence_map::{EssenceMapAction, EssenceMapPaths};

mod documents;
mod inventory;
pub(crate) mod journal;

use inventory::{build_inventory, Inventory};

pub(crate) const MAX_NODE_COUNT: usize = 100;
const CAPABILITIES_PATH: &str = "essences/capabilities.md";
const CORE_PATH: &str = "essences/core.md";
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
    pub status: Option<String>,
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
                    6 + usize::from(node.runs_on_reason.is_some())
                        + usize::from(node.status.is_some()),
                )?;
                state.serialize_field("kind", "node")?;
                state.serialize_field("id", &node.id)?;
                state.serialize_field("type", &node.node_type)?;
                state.serialize_field("label", &node.label)?;
                state.serialize_field("vector", &node.vector)?;
                state.serialize_field("source", &node.source)?;
                if let Some(status) = &node.status {
                    state.serialize_field("status", status)?;
                }
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

/// What one essence-map invocation produced. Generation and validation report
/// the inventory; the journal actions report the decision log.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum Report {
    Map(MapSummary),
    Record(journal::RecordOutcome),
    Release(journal::ReleaseSummary),
    Revert(journal::RevertOutcome),
}

pub(crate) fn handle(json_output: bool, action: EssenceMapAction) {
    let result = match action {
        EssenceMapAction::Generate(paths) => generate_from_paths(&paths).map(Report::Map),
        EssenceMapAction::Validate(paths) => validate_from_paths(&paths).map(Report::Map),
        EssenceMapAction::Record(record) => {
            let (map_path, decisions_path) = match journal_paths(&record.paths) {
                Ok(paths) => paths,
                Err(error) => fail(error),
            };
            journal::record(&map_path, &decisions_path, &record.summary, &record.reason)
                .map(Report::Record)
        }
        EssenceMapAction::ReleaseSummary(release) => {
            let (map_path, decisions_path) = match journal_paths(&release.paths) {
                Ok(paths) => paths,
                Err(error) => fail(error),
            };
            journal::release_summary(&map_path, &decisions_path, release.since.as_deref())
                .map(Report::Release)
        }
        EssenceMapAction::Revert(revert) => {
            let (map_path, decisions_path) = match journal_paths(&revert.paths) {
                Ok(paths) => paths,
                Err(error) => fail(error),
            };
            journal::revert(&map_path, &decisions_path, &revert.to).map(Report::Revert)
        }
    };

    match result {
        Ok(report) if json_output => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
        Ok(report) => print_report(&report),
        Err(error) => fail(error),
    }
}

fn fail(error: String) -> ! {
    eprintln!("✗ essence-map failed: {error}");
    eprintln!("fix: codeterm essence-map generate --root .");
    std::process::exit(1);
}

fn journal_paths(paths: &EssenceMapPaths) -> Result<(PathBuf, PathBuf), String> {
    let root = canonical_root(&paths.root)?;
    Ok((
        resolve_path(&root, paths.map.as_deref(), MAP_PATH),
        resolve_path(&root, paths.decisions.as_deref(), DECISIONS_PATH),
    ))
}

fn generate_from_paths(paths: &EssenceMapPaths) -> Result<MapSummary, String> {
    let root = canonical_root(&paths.root)?;
    let map_path = resolve_path(&root, paths.map.as_deref(), MAP_PATH);
    generate(&root, &map_path)
}

fn validate_from_paths(paths: &EssenceMapPaths) -> Result<MapSummary, String> {
    let root = canonical_root(&paths.root)?;
    let map_path = resolve_path(&root, paths.map.as_deref(), MAP_PATH);
    let decisions_path = resolve_path(&root, paths.decisions.as_deref(), DECISIONS_PATH);
    validate(&root, &map_path, &decisions_path)
}

pub(crate) fn generate(root: &Path, map_path: &Path) -> Result<MapSummary, String> {
    let inventory = build_inventory(root)?;
    let records = records_for(&inventory);
    enforce_budget(&records)?;
    let contents = records_to_jsonl(&records)?;
    journal::retain_superseded_map(map_path, &contents)?;
    write_text(map_path, &contents)?;
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
    journal::validate_decisions(decisions_path)?;
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
    if is_linked_worktree(&root) {
        return Err(format!(
            "essence-map writes only the canonical checkout; {} is a linked worktree. Run it from the primary checkout.",
            root.display()
        ));
    }
    Ok(root)
}

/// A linked worktree shares its object store with the primary checkout, so a map
/// written there is a copy, not the project's map. Detect it the same way the
/// map view does: the git dir differs from the common dir.
fn is_linked_worktree(root: &Path) -> bool {
    git_checkout_paths(root).is_some_and(|(git_dir, common_dir)| git_dir != common_dir)
}

fn git_checkout_paths(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    match (lines.next(), lines.next()) {
        (Some(git_dir), Some(common_dir)) => Some((
            std::fs::canonicalize(git_dir).unwrap_or_else(|_| PathBuf::from(git_dir)),
            std::fs::canonicalize(common_dir).unwrap_or_else(|_| PathBuf::from(common_dir)),
        )),
        _ => None,
    }
}

fn resolve_path(root: &Path, supplied: Option<&Path>, default: &str) -> PathBuf {
    let path = supplied.unwrap_or_else(|| Path::new(default));
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
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
    if node.node_type != NodeType::Capability && node.status.is_some() {
        return Err(format!(
            "node `{}` carries status but is not a capability",
            node.id
        ));
    }
    if node.node_type == NodeType::Capability
        && node
            .status
            .as_deref()
            .map_or(true, |status| status.trim().is_empty())
    {
        return Err(format!("capability node `{}` has no status", node.id));
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
    if edge.edge_type == EdgeType::PartOf && edge.from == edge.to {
        return Err(format!(
            "part_of edge {} -> {} is a self-parent cycle",
            edge.from, edge.to
        ));
    }
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

fn print_report(report: &Report) {
    match report {
        Report::Map(summary) => print_summary(summary),
        Report::Record(journal::RecordOutcome::Appended { id, decisions, .. }) => {
            println!("recorded {id}; the decision log now holds {decisions} record(s)");
        }
        Report::Record(journal::RecordOutcome::Unchanged { id, .. }) => {
            println!("map unchanged since {id}; nothing recorded");
        }
        Report::Release(release) => print_release(release),
        Report::Revert(revert) => {
            println!(
                "restored the map recorded by {} and logged {}",
                revert.restored, revert.decision
            );
        }
    }
}

fn print_release(release: &journal::ReleaseSummary) {
    let baseline = release.baseline.as_deref().unwrap_or("no baseline");
    println!(
        "map decisions since {baseline}: {} change(s)",
        release.changes.len()
    );
    for change in &release.changes {
        println!("- {}: {}", change.id, change.summary);
    }
    for choice in &release.choices {
        println!("choice: {choice}");
    }
    if let (Some(_), Some(baseline)) = (release.revert_choice(), release.baseline.as_deref()) {
        println!("revert: codeterm essence-map revert --to {baseline}");
    }
}

#[cfg(test)]
mod cartographer_tests;
#[cfg(test)]
mod tests;
