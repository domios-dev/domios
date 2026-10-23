use codeterm_blueprint_types::{
    blueprint_document::Document,
    blueprint_memory::{self as model, Entry},
};
use serde_json::{json, Value};

const OFFERS: usize = 5;

pub(super) fn save_receipt(saved: &Value) -> String {
    let mut text = format!(
        "Saved {} in Blueprint memory.",
        saved["id"].as_str().unwrap_or("?")
    );
    for id in saved["evicted_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        text.push_str(&format!(
            "\nArchived Blueprint memory entry {id} (area storage cap)."
        ));
    }
    text
}

pub(super) fn offers(stored: Value, intent: &str) -> Value {
    let stored = stored
        .as_array()
        .into_iter()
        .flatten()
        .cloned()
        .map(|mut offer| {
            if let Some(id) = offer["id"].as_str() {
                offer["id"] = Value::String(format!("memory:{id}"));
            }
            offer
        });
    let builtins = super::recipe_files::FILES
        .iter()
        .filter_map(|(name, source)| {
            let document = serde_yaml::from_str::<Document>(source).ok()?;
            Some(Entry {
                id: (*name).into(),
                intents: vec![document
                    .note
                    .clone()
                    .unwrap_or_else(|| document.name.clone())],
                document,
                uses: 0,
                successes: 0,
                failures: 0,
                kept: false,
                last_used: 0,
            })
        })
        .collect::<Vec<_>>();
    let matched = model::recall(&builtins, intent, OFFERS, chrono::Utc::now().timestamp())
        .into_iter()
        .map(|entry| json!({"id":entry.id,"when_to_use":entry.when_to_use(),"intents":entry.intents,"how":entry.document.note,"yaml":entry.document.yaml().unwrap_or_default(),"uses":entry.uses,"successes":entry.successes,"why":null}));
    json!(stored.chain(matched).take(OFFERS).collect::<Vec<_>>())
}

pub(super) fn render_offer(offer: &Value) -> String {
    let line = |key| {
        offer[key]
            .as_str()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut text = format!("# Why: {}\n", line("when_to_use"));
    if offer["why"].is_string() {
        text.push_str(&format!("# Match: {}\n", line("why")));
    }
    text.push_str(offer["yaml"].as_str().unwrap_or_default());
    text
}
