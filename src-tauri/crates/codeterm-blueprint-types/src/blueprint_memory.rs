use crate::blueprint_document::Document;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
mod credentials;
pub mod curator;
mod generalize;
pub use credentials::contains_credentials;
mod quality;
pub mod semantic_canon;
pub use generalize::{abstract_document, portable_document, safe_template};
pub use quality::{decide, CuratorEvidence, Evidence, Qualification};

pub const AREA_CAP: usize = 1000;
pub const MERGE_BUDGET: usize = 8;
pub const MAX_INTENTS: usize = 5;
pub const MAX_INTENT_CHARS: usize = 200;

pub fn parse_document(source: &str) -> Result<Document, String> {
    if source.len() > crate::blueprint_recipe::MAX_RECIPE_BYTES {
        return Err("Blueprint solution is too large".into());
    }
    crate::blueprint_document::parse("solution.yaml", source)
        .map(|p| p.document)
        .map_err(|errors| format!("{errors:?}"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub document: Document,
    pub uses: u64,
    pub last_used: i64,
    pub intents: Vec<String>,
    pub successes: u64,
    pub failures: u64,
    pub kept: bool,
}

impl Entry {
    pub fn when_to_use(&self) -> String {
        self.intents.join("; ")
    }
}

pub fn validate_intents(intents: &[String]) -> Result<Vec<String>, String> {
    let intents = intents
        .iter()
        .map(|intent| intent.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>();
    if intents.is_empty()
        || intents.len() > MAX_INTENTS
        || intents.iter().any(|intent| {
            intent.is_empty()
                || intent.chars().count() > MAX_INTENT_CHARS
                || intent.chars().any(char::is_control)
        })
    {
        return Err(format!(
            "Give 1..{MAX_INTENTS} intents of 1..{MAX_INTENT_CHARS} plain-text characters each"
        ));
    }
    Ok(intents)
}

pub fn merge_intents(first: &[String], second: &[String]) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    for intent in first.iter().chain(second) {
        if merged.len() < MAX_INTENTS
            && !merged.iter().any(|kept| kept.eq_ignore_ascii_case(intent))
        {
            merged.push(intent.clone());
        }
    }
    merged
}

pub fn score(entry: &Entry, now: i64) -> f64 {
    let weight =
        entry.successes as f64 + 2.0 * entry.uses as f64 + if entry.kept { 5.0 } else { 0.0 }
            - entry.failures as f64;
    let days = now.saturating_sub(entry.last_used).max(0) as f64 / 86400.0;
    weight / (1.0 + days / 30.0)
}

pub fn structure(document: &Document) -> Value {
    if document.trigger.is_none() && document.result.port.is_some() {
        return json!({"callable": document});
    }
    let ids = document
        .nodes()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    json!({"kind": document.kind, "trigger":document.trigger.as_ref().map(|t|match t {crate::blueprint_document::Trigger::Schedule{..}=>"schedule",crate::blueprint_document::Trigger::Event{..}=>"event",crate::blueprint_document::Trigger::Input{..}=>"input"}),
        "nodes":document.nodes().map(|n|json!({"kind":n.kind(),"in":n.input,"out":n.output,"caps":n.caps,"buckets":n.buckets,"bind":n.bind,"call":n.call,"routes":n.routes.iter().map(|(outcome,target)|(outcome, ids.get(target.as_str()).map_or_else(||json!(target),|i|json!(i)))).collect::<BTreeMap<_,_>>()})).collect::<Vec<_>>()})
}

pub fn duplicate<'a>(entries: &'a [Entry], candidate: &Document) -> Option<&'a Entry> {
    let shape = structure(candidate);
    entries
        .iter()
        .filter(|entry| structure(&entry.document) == shape)
        .max_by_key(|entry| (entry.successes.saturating_add(entry.uses), entry.last_used))
}

pub fn evictions(entries: &[Entry], cap: usize, now: i64, saved_id: &str) -> Vec<String> {
    let mut ordered = entries
        .iter()
        .filter(|entry| entry.id != saved_id)
        .collect::<Vec<_>>();
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
            let terms = words(&entry.intents.join(" "));
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
        .take(limit)
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
    keeper.intents = merge_intents(&keeper.intents, &lesser.intents);
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
