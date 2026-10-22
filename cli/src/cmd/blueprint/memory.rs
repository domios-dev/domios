use codeterm_blueprint_types::{
    blueprint_document::Document,
    blueprint_memory::{self as model, Entry},
};
use serde_json::{json, Value};

const OFFERS: usize = 5;

pub(super) fn offers(stored: Value, intent: &str) -> Value {
    let stored = stored
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|offer| {
            let entry = Entry {
                id: format!("memory:{}", offer["id"].as_str()?),
                document: model::parse_document(offer["yaml"].as_str()?).ok()?,
                intents: serde_json::from_value(offer["intents"].clone()).ok()?,
                uses: offer["uses"].as_u64().unwrap_or(0),
                successes: offer["successes"].as_u64().unwrap_or(0),
                failures: 0,
                kept: true,
                last_used: offer["last_used"].as_i64().unwrap_or(0),
            };
            Some((entry, offer["why"].as_str().map(str::to_owned)))
        })
        .collect::<Vec<_>>();
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
        .map(|entry| (entry, None));
    json!(stored
        .into_iter()
        .chain(matched)
        .take(OFFERS)
        .map(|(entry, why)| json!({"id":entry.id,"when_to_use":entry.when_to_use(),"intents":entry.intents,"how":entry.document.note,"yaml":entry.document.yaml().unwrap_or_default(),"uses":entry.uses,"successes":entry.successes,"why":why}))
        .collect::<Vec<_>>())
}
