use codeterm_blueprint_types::blueprint_document::{Document, Input};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const ITERATIONS: usize = 3;
const FAILURE_OUTCOMES: [&str; 4] = ["failed", "down", "failing", "offline"];
const SOURCE_KINDS: [&str; 5] = ["schedule", "event", "input", "probe", "observe"];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Signature {
    pub hash: String,
    pub features: BTreeMap<String, u32>,
    pub edges: BTreeMap<String, u32>,
}

pub fn signature(document: &Document) -> Signature {
    let graph = Graph::lift(document);
    let iterations = graph.iterations();
    let mut features = BTreeMap::new();
    for (index, labels) in iterations.iter().enumerate() {
        for label in labels {
            *features.entry(format!("{index}:{label}")).or_insert(0) += 1;
        }
    }
    let mut last = iterations.last().cloned().unwrap_or_default();
    last.sort();
    let multiset = format!(
        "[{}]",
        last.iter()
            .map(|label| format!("\"{label}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Signature {
        hash: hex(&Sha256::digest(multiset.as_bytes())),
        features,
        edges: graph.edge_phrases(),
    }
}

pub fn kernel(a: &BTreeMap<String, u32>, b: &BTreeMap<String, u32>) -> f64 {
    let dot: f64 = a
        .iter()
        .map(|(key, count)| f64::from(*count) * f64::from(b.get(key).copied().unwrap_or(0)))
        .sum();
    let norm = |map: &BTreeMap<String, u32>| {
        map.values()
            .map(|count| f64::from(*count).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let denominator = norm(a) * norm(b);
    if denominator > 0.0 {
        dot / denominator
    } else {
        0.0
    }
}

pub fn shared_edges(a: &Signature, b: &Signature) -> Vec<String> {
    a.edges
        .iter()
        .filter_map(|(edge, count)| {
            let shared = (*count).min(b.edges.get(edge).copied().unwrap_or(0));
            (shared > 0).then(|| vec![edge.clone(); shared as usize])
        })
        .flatten()
        .collect()
}

struct Node {
    kind: String,
    media: Option<String>,
    outcomes: Vec<String>,
    routes: BTreeMap<String, Target>,
    buckets: Vec<String>,
}

#[derive(Clone)]
enum Target {
    Next,
    Stop,
    Node(usize),
}

struct Graph {
    labels: Vec<String>,
    edges: Vec<(usize, usize, &'static str)>,
}

impl Graph {
    fn lift(document: &Document) -> Self {
        let all = document.nodes().collect::<Vec<_>>();
        let ids = all
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect::<BTreeMap<_, _>>();
        let nodes = all
            .iter()
            .map(|n| {
                let mut node = plain(n.kind());
                node.buckets = n.buckets.clone();
                if n.kind() == "filter" {
                    node.buckets.extend(
                        n.params
                            .get("bucket")
                            .and_then(|v| v.value())
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    );
                }
                node.media = Some(serde_json::json!({"in":n.input,"out":n.output}).to_string());
                node.routes = n
                    .routes
                    .iter()
                    .map(|(outcome, target)| {
                        let target = match target.as_str() {
                            "next" | "@next" => Target::Next,
                            "stop" | "@stop" => Target::Stop,
                            other => ids.get(other).map_or(Target::Stop, |i| Target::Node(*i)),
                        };
                        (outcome.clone(), target)
                    })
                    .collect();
                node.outcomes = if n.outcomes.is_empty() {
                    default_outcomes(&node, n.params.get("resource"), &document.inputs)
                } else {
                    n.outcomes.clone()
                };
                node
            })
            .collect::<Vec<_>>();
        let trigger = document.trigger.as_ref().map(|t| {
            match t {
                codeterm_blueprint_types::blueprint_document::Trigger::Schedule { .. } => {
                    "schedule"
                }
                codeterm_blueprint_types::blueprint_document::Trigger::Webhook { .. } => "webhook",
                codeterm_blueprint_types::blueprint_document::Trigger::Event { .. } => "event",
                codeterm_blueprint_types::blueprint_document::Trigger::Input { .. } => "input",
            }
            .to_owned()
        });
        let result = document
            .result
            .to
            .as_ref()
            .and_then(codeterm_blueprint_types::blueprint_document::ResultTarget::fixed)
            .map(|to| to.split(':').next().unwrap_or_default().to_owned());
        let mut labels = nodes.iter().map(label).collect::<Vec<_>>();
        let stop = labels.len();
        labels.push("stop".into());
        let mut edges = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            let next = if index + 1 < nodes.len() {
                index + 1
            } else {
                stop
            };
            if node.outcomes.is_empty()
                && node.routes.is_empty()
                && SOURCE_KINDS.contains(&node.kind.as_str())
                && next != stop
            {
                edges.push((index, next, "success"));
            }
            let outcomes = node
                .outcomes
                .iter()
                .cloned()
                .chain(node.routes.keys().cloned())
                .collect::<BTreeSet<_>>();
            for outcome in outcomes {
                let target = node
                    .routes
                    .get(&outcome)
                    .cloned()
                    .unwrap_or(if outcome == "failed" {
                        Target::Stop
                    } else {
                        Target::Next
                    });
                let destination = match target {
                    Target::Stop => stop,
                    Target::Next => next,
                    Target::Node(target) => target,
                };
                edges.push((index, destination, outcome_class(&outcome, node)));
            }
        }
        if let Some(kind) = trigger.filter(|_| !nodes.is_empty()) {
            labels.push(format!("trigger:{kind}"));
            edges.push((labels.len() - 1, 0, "success"));
        }
        if let Some(sink) = result {
            labels.push(format!("result:{sink}"));
        }
        Self { labels, edges }
    }

    fn iterations(&self) -> Vec<Vec<String>> {
        let mut outgoing = vec![Vec::new(); self.labels.len()];
        let mut incoming = vec![Vec::new(); self.labels.len()];
        for &(source, destination, class) in &self.edges {
            outgoing[source].push((class, destination));
            incoming[destination].push((class, source));
        }
        let mut current = self.labels.clone();
        let mut iterations = vec![current.clone()];
        for _ in 0..ITERATIONS {
            let next = (0..current.len())
                .map(|vertex| {
                    let mut outs = outgoing[vertex]
                        .iter()
                        .map(|(class, destination)| format!(">{class}:{}", current[*destination]))
                        .collect::<Vec<_>>();
                    outs.sort();
                    let mut ins = incoming[vertex]
                        .iter()
                        .map(|(class, source)| format!("<{class}:{}", current[*source]))
                        .collect::<Vec<_>>();
                    ins.sort();
                    let text = format!("{}|{}|{}", current[vertex], outs.join(";"), ins.join(";"));
                    hex(&Sha256::digest(text.as_bytes()))[..16].to_owned()
                })
                .collect::<Vec<_>>();
            current = next;
            iterations.push(current.clone());
        }
        iterations
    }

    fn edge_phrases(&self) -> BTreeMap<String, u32> {
        let short = |label: &str| label.split('[').next().unwrap_or(label).to_owned();
        let mut phrases = BTreeMap::new();
        for &(source, destination, class) in &self.edges {
            *phrases
                .entry(format!(
                    "{} -{class}-> {}",
                    short(&self.labels[source]),
                    short(&self.labels[destination])
                ))
                .or_insert(0) += 1;
        }
        phrases
    }
}

fn plain(kind: &str) -> Node {
    Node {
        kind: kind.into(),
        media: None,
        outcomes: Vec::new(),
        routes: BTreeMap::new(),
        buckets: Vec::new(),
    }
}

fn default_outcomes(
    node: &Node,
    resource: Option<&codeterm_blueprint_types::blueprint_document::ValueForm>,
    inputs: &BTreeMap<String, Input>,
) -> Vec<String> {
    use codeterm_blueprint_types::blueprint_document::ValueForm;
    let strings = |xs: &[&str]| xs.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match node.kind.as_str() {
        "http" => strings(&["up", "down", "failed"]),
        "github" => {
            let values = match resource {
                Some(ValueForm::Lit { lit }) => vec![lit.clone()],
                Some(ValueForm::Input { input }) => inputs
                    .get(input)
                    .and_then(|p| p.choices.clone())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let mut outcomes = BTreeSet::new();
            for value in values
                .iter()
                .map(|v| v.as_str().unwrap_or_default())
                .chain(values.is_empty().then_some("run"))
            {
                outcomes.extend(match value {
                    "runners" => strings(&["online", "offline", "failed"]),
                    "pr_checks" => strings(&["done", "failed"]),
                    _ => strings(&["done", "failing", "failed"]),
                });
            }
            outcomes.into_iter().collect()
        }
        "ask" => node
            .buckets
            .iter()
            .cloned()
            .chain(std::iter::once("other".into()))
            .collect(),
        "filter" => strings(&["done", "failed"])
            .into_iter()
            .chain(node.buckets.iter().cloned())
            .collect(),
        "command" | "convert" | "agent" | "code" | "call" => strings(&["done", "failed"]),
        _ => Vec::new(),
    }
}

fn label(node: &Node) -> String {
    let ports = node
        .media
        .as_ref()
        .map(|media| format!("in:{media}"))
        .unwrap_or_default();
    format!("{}[{ports}]<{}>", node.kind, effect_class(&node.kind))
}

fn effect_class(kind: &str) -> &'static str {
    match kind {
        "schedule" | "event" | "input" => "trigger",
        "probe" | "observe" | "http" | "github" => "read",
        "command" => "exec",
        "code" => "compute",
        "filter" | "convert" => "pure",
        "agent" | "ask" => "model",
        "notify" | "send" => "message",
        "respond_prompt" => "prompt",
        "escalate" => "escalate",
        "cron" | "blueprint" => "activate",
        "stop" => "halt",
        _ => "other",
    }
}

fn outcome_class(outcome: &str, node: &Node) -> &'static str {
    if outcome == "other" {
        "other"
    } else if node.buckets.iter().any(|bucket| bucket == outcome) {
        "bucket"
    } else if FAILURE_OUTCOMES.contains(&outcome) {
        "failure"
    } else {
        "success"
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
