use crate::blueprint_document::Document;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
mod credentials;
pub mod curator;
mod generalize;
pub use credentials::contains_credentials;
mod quality;
pub use generalize::{abstract_document, safe_template};
pub use quality::{decide, CuratorEvidence, Evidence, Qualification};

pub const AREA_CAP: usize = 50;
pub const MERGE_BUDGET: usize = 8;

pub fn parse_document(source: &str) -> Result<Document, String> {
    if source.len() > crate::blueprint_recipe::MAX_RECIPE_BYTES {
        return Err("Blueprint solution is too large".into());
    }
    serde_yaml::from_str(source).map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub document: Document,
    pub uses: u64,
    pub last_used: i64,
    pub when_to_use: String,
    pub successes: u64,
    pub failures: u64,
    pub kept: bool,
}

pub fn score(entry: &Entry, now: i64) -> f64 {
    let weight =
        entry.successes as f64 + 2.0 * entry.uses as f64 + if entry.kept { 5.0 } else { 0.0 }
            - entry.failures as f64;
    let days = now.saturating_sub(entry.last_used).max(0) as f64 / 86400.0;
    weight / (1.0 + days / 30.0)
}

pub fn structure(document: &Document) -> Value {
    if let Some(pipeline) = &document.pipeline {
        let steps = pipeline["steps"].as_array().cloned().unwrap_or_default();
        let names = steps
            .iter()
            .enumerate()
            .filter_map(|(i, step)| step["name"].as_str().map(|name| (name.to_owned(), i)))
            .collect::<BTreeMap<_, _>>();
        let nodes = steps.iter().map(|step| {
            let routes = step["on"].as_object().map(|routes| routes.iter().map(|(bucket,target)| {
                let target = target.as_str().unwrap_or("");
                (bucket.clone(), names.get(target).map_or_else(|| json!(target), |i| json!(i)))
            }).collect::<BTreeMap<_, _>>());
            json!({"kind":if step.get("run").is_some(){"command"}else{step["uses"].as_str().unwrap_or("")},"input":step["input"],"routes":routes})
        }).collect::<Vec<_>>();
        json!({"trigger":pipeline["trigger"]["type"],"event":pipeline["trigger"]["kind"],"nodes":nodes,"result":pipeline["result"]})
    } else {
        let watch = document.watch.as_ref().unwrap_or(&Value::Null);
        let source = if watch.get("probe").is_some() {
            "probe"
        } else if watch.get("observe").is_some() {
            "observe"
        } else {
            "input"
        };
        let handlers = watch["handlers"].as_object().map(|handlers| {
            handlers
                .iter()
                .map(|(bucket, items)| {
                    (
                        bucket.clone(),
                        items
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|handler| handler["type"].clone())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        });
        json!({"source":source,"buckets":watch["buckets"],"routes":watch["routes"],"handlers":handlers,"until":watch["until"]})
    }
}

pub fn duplicate<'a>(entries: &'a [Entry], candidate: &Document) -> Option<&'a Entry> {
    let shape = structure(candidate);
    entries
        .iter()
        .filter(|entry| structure(&entry.document) == shape)
        .max_by_key(|entry| (entry.successes.saturating_add(entry.uses), entry.last_used))
}

pub fn evictions(entries: &[Entry], cap: usize, now: i64) -> Vec<String> {
    let mut ordered = entries.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| {
        score(a, now)
            .total_cmp(&score(b, now))
            .then(a.last_used.cmp(&b.last_used))
            .then(a.id.cmp(&b.id))
    });
    ordered
        .iter()
        .take(entries.len().saturating_sub(cap))
        .map(|entry| entry.id.clone())
        .collect()
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() > 1)
        .map(str::to_lowercase)
        .filter(|word| {
            !matches!(
                word.as_str(),
                "the"
                    | "and"
                    | "for"
                    | "with"
                    | "watch"
                    | "me"
                    | "on"
                    | "to"
                    | "when"
                    | "then"
                    | "my"
                    | "of"
                    | "it"
                    | "in"
                    | "at"
                    | "is"
            )
        })
        .collect()
}

pub fn recall(entries: &[Entry], intent: &str, limit: usize, now: i64) -> Vec<Entry> {
    let query = words(intent);
    let mut ranked = entries
        .iter()
        .filter_map(|entry| {
            let terms = words(&entry.when_to_use);
            let hits = query.intersection(&terms).count();
            (query.is_empty() || hits > 0).then_some((hits, entry))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(a_hits, a), (b_hits, b)| {
        b_hits
            .cmp(a_hits)
            .then(score(b, now).total_cmp(&score(a, now)))
            .then(b.last_used.cmp(&a.last_used))
            .then(a.id.cmp(&b.id))
    });
    ranked
        .into_iter()
        .take(limit.min(AREA_CAP))
        .map(|(_, entry)| entry.clone())
        .collect()
}

#[derive(Debug)]
pub struct Merge {
    pub keeper: Entry,
    pub superseded: String,
}

pub fn merge_pair(a: &Entry, b: &Entry) -> Option<Merge> {
    if a.id == b.id || structure(&a.document) != structure(&b.document) {
        return None;
    }
    let (best, lesser) = if (a.uses, a.last_used) >= (b.uses, b.last_used) {
        (a, b)
    } else {
        (b, a)
    };
    let mut keeper = best.clone();
    generalize::merge_variants(&mut keeper.document, &lesser.document);
    for (key, input) in &lesser.document.inputs {
        let kept = keeper
            .document
            .inputs
            .entry(key.clone())
            .or_insert_with(|| input.clone());
        if let (Some(a), Some(b)) = (&mut kept.choices, &input.choices) {
            for choice in b {
                if !a.contains(choice) {
                    a.push(choice.clone());
                }
            }
        }
    }
    if lesser.document.note.as_ref().map_or(0, String::len)
        > keeper.document.note.as_ref().map_or(0, String::len)
    {
        keeper.document.note = lesser.document.note.clone();
    }
    keeper.uses = a.uses.saturating_add(b.uses);
    keeper.successes = a.successes.saturating_add(b.successes);
    keeper.failures = a.failures.saturating_add(b.failures);
    keeper.kept = a.kept || b.kept;
    if lesser.when_to_use.len() > keeper.when_to_use.len() {
        keeper.when_to_use = lesser.when_to_use.clone();
    }
    keeper.last_used = a.last_used.max(b.last_used);
    Some(Merge {
        keeper,
        superseded: lesser.id.clone(),
    })
}

pub fn dream_merges(entries: &[Entry], budget: usize) -> Vec<Merge> {
    let mut merges = Vec::new();
    let mut remaining = budget.min(MERGE_BUDGET);
    let mut groups = BTreeMap::<String, Vec<&Entry>>::new();
    for entry in entries.iter().take(AREA_CAP) {
        groups
            .entry(structure(&entry.document).to_string())
            .or_default()
            .push(entry);
    }
    for group in groups.values() {
        for [first, second] in group.as_chunks::<2>().0 {
            if remaining == 0 {
                return merges;
            }
            remaining -= 1;
            if let Some(merge) = merge_pair(first, second) {
                merges.push(merge);
            }
        }
    }
    merges
}

#[cfg(test)]
mod tests;
