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
        let (nodes, trigger, result) = if let Some(pipeline) = &document.pipeline {
            let trigger = pipeline["trigger"]
                .as_object()
                .filter(|trigger| !trigger.is_empty())
                .map(|trigger| python_str(trigger.get("type")));
            let result = pipeline["result"]
                .as_object()
                .filter(|result| !result.is_empty())
                .map(|result| {
                    python_str(result.get("to").or(Some(&Value::String(String::new()))))
                        .split(':')
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                });
            (pipeline_nodes(document, pipeline), trigger, result)
        } else {
            (
                document.watch.as_ref().map(watch_nodes).unwrap_or_default(),
                None,
                None,
            )
        };
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

fn pipeline_nodes(document: &Document, pipeline: &Value) -> Vec<Node> {
    let steps = pipeline["steps"].as_array().cloned().unwrap_or_default();
    let names = steps
        .iter()
        .enumerate()
        .map(|(index, step)| (python_str(step.get("name")), index))
        .collect::<BTreeMap<_, _>>();
    steps
        .iter()
        .map(|step| {
            let with = step["with"].as_object();
            let kind = if step.get("run").is_some() {
                "command".to_owned()
            } else {
                python_str(step.get("uses"))
            };
            let buckets = match (kind.as_str(), with) {
                ("ask", Some(with)) => with
                    .get("buckets")
                    .and_then(Value::as_array)
                    .map(|buckets| buckets.iter().map(|b| python_str(Some(b))).collect())
                    .unwrap_or_default(),
                ("filter", Some(with)) => with
                    .get("bucket")
                    .filter(|bucket| !bucket.is_null())
                    .map(|bucket| vec![python_str(Some(bucket))])
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let routes = step["on"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(outcome, target)| {
                    let target = match target.as_str() {
                        Some("next") => Target::Next,
                        Some("stop") => Target::Stop,
                        _ => names
                            .get(&python_str(Some(target)))
                            .map_or(Target::Stop, |index| Target::Node(*index)),
                    };
                    (outcome.clone(), target)
                })
                .collect();
            let resource = with.and_then(|with| with.get("resource"));
            let mut node = Node {
                outcomes: Vec::new(),
                media: Some(step["input"].as_str().unwrap_or("text").to_owned()),
                kind,
                routes,
                buckets,
            };
            node.outcomes = default_outcomes(&node, resource, &document.inputs);
            node
        })
        .collect()
}

fn watch_nodes(watch: &Value) -> Vec<Node> {
    let source = ["probe", "observe", "input"]
        .into_iter()
        .find(|key| watch.get(*key).is_some())
        .unwrap_or("probe");
    let buckets = watch["buckets"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|bucket| python_str(Some(bucket)))
        .collect::<Vec<_>>();
    let routes = watch["routes"].as_object();
    let handlers = watch["handlers"].as_object();
    let mut keys = buckets.clone();
    keys.push("other".into());
    let extra = routes
        .into_iter()
        .chain(handlers)
        .flat_map(|map| map.keys().cloned())
        .filter(|key| !keys.contains(key))
        .collect::<BTreeSet<_>>();
    keys.extend(extra);
    let mut nodes = vec![
        plain(source),
        Node {
            buckets: buckets.clone(),
            ..plain("ask")
        },
    ];
    let mut decide_routes = BTreeMap::new();
    for bucket in keys {
        if let Some(chain) = handlers.and_then(|handlers| handlers.get(&bucket)) {
            let chain = chain.as_array().cloned().unwrap_or_default();
            if chain.is_empty() {
                decide_routes.insert(bucket, Target::Stop);
                continue;
            }
            decide_routes.insert(bucket, Target::Node(nodes.len()));
            let start = nodes.len();
            for (position, handler) in chain.iter().enumerate() {
                let mut node = plain(&python_str(handler.get("type")));
                if position + 1 < chain.len() {
                    node.outcomes = vec!["done".into()];
                    node.routes =
                        BTreeMap::from([("done".into(), Target::Node(start + position + 1))]);
                } else {
                    node.outcomes = default_outcomes(&node, None, &BTreeMap::new());
                }
                nodes.push(node);
            }
        } else if let Some(route) = routes.and_then(|routes| routes.get(&bucket)) {
            let route = python_str(Some(route));
            let (kind, argument) = route.split_once(':').unwrap_or((route.as_str(), ""));
            let kind = match (kind, argument.is_empty()) {
                ("notify" | "stop", true) => Some(kind),
                ("send" | "cron" | "blueprint", false) => Some(kind),
                _ => None,
            };
            match kind {
                Some(kind) => {
                    decide_routes.insert(bucket, Target::Node(nodes.len()));
                    let mut node = plain(kind);
                    node.outcomes = default_outcomes(&node, None, &BTreeMap::new());
                    nodes.push(node);
                }
                None => {
                    decide_routes.insert(bucket, Target::Stop);
                }
            }
        }
    }
    nodes[1].routes = decide_routes;
    nodes[1].outcomes = default_outcomes(&nodes[1], None, &BTreeMap::new());
    nodes
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
    resource: Option<&Value>,
    inputs: &BTreeMap<String, Input>,
) -> Vec<String> {
    let strings = |values: &[&str]| values.iter().map(|value| (*value).to_owned()).collect();
    let github = |resource: &str| match resource {
        "pr_checks" => strings(&["done", "failed"]),
        "runners" => strings(&["online", "offline", "failed"]),
        _ => strings(&["done", "failing", "failed"]),
    };
    match node.kind.as_str() {
        "http" => strings(&["up", "down", "failed"]),
        "github" => match resource {
            Some(Value::String(text)) => match whole_input(text) {
                Some(input) => {
                    let choices = inputs
                        .get(input)
                        .and_then(|input| input.choices.clone())
                        .unwrap_or_else(|| vec![Value::String("run".into())]);
                    let mut merged: Vec<String> = Vec::new();
                    for choice in choices {
                        let choice = python_str(Some(&choice));
                        let outcomes: Vec<String> = match choice.as_str() {
                            "run" | "jobs" | "pr_checks" | "runners" => github(&choice),
                            _ => Vec::new(),
                        };
                        for outcome in outcomes {
                            if !merged.contains(&outcome) {
                                merged.push(outcome);
                            }
                        }
                    }
                    if merged.is_empty() {
                        github("run")
                    } else {
                        merged
                    }
                }
                None => github(text),
            },
            Some(other) => github(&python_str(Some(other))),
            None => github("run"),
        },
        "filter" => {
            let mut outcomes = strings(&["done", "failed"]);
            outcomes.extend(node.buckets.iter().cloned());
            outcomes
        }
        "ask" => {
            let mut outcomes = node.buckets.clone();
            outcomes.push("other".into());
            outcomes
        }
        "command" | "convert" | "agent" | "code" => strings(&["done", "failed"]),
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

fn whole_input(text: &str) -> Option<&str> {
    let inner = text.strip_prefix("{{")?.strip_suffix("}}")?.trim();
    let mut chars = inner.chars();
    let first = chars.next()?;
    ((first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'))
    .then_some(inner)
}

fn python_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        Some(other) => other.to_string(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
