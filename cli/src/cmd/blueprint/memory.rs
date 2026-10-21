use codeterm_blueprint_types::{
    blueprint_document::Document,
    blueprint_memory::{self as model, Entry},
};
use serde_json::{json, Value};

pub(super) fn offers(stored: Value, intent: &str) -> Value {
    let mut entries = stored
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|offer| {
            Some(Entry {
                id: format!("memory:{}", offer["id"].as_str()?),
                document: model::parse_document(offer["yaml"].as_str()?).ok()?,
                when_to_use: offer["when_to_use"].as_str()?.into(),
                uses: offer["uses"].as_u64().unwrap_or(0),
                successes: offer["successes"].as_u64().unwrap_or(0),
                failures: 0,
                kept: true,
                last_used: offer["last_used"].as_i64().unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();
    for (name, source) in super::recipe_files::FILES {
        if let Ok(document) = serde_yaml::from_str::<Document>(source) {
            let when_to_use = document
                .note
                .clone()
                .unwrap_or_else(|| document.name.clone());
            entries.push(Entry {
                id: (*name).into(),
                document,
                when_to_use,
                uses: 0,
                successes: 0,
                failures: 0,
                kept: false,
                last_used: 0,
            });
        }
    }
    json!(model::recall(&entries,intent,5,chrono::Utc::now().timestamp()).into_iter().map(|entry| {
        json!({"id":entry.id,"when_to_use":entry.when_to_use,"how":entry.document.note,"yaml":entry.document.yaml().unwrap_or_default(),"uses":entry.uses,"successes":entry.successes})
    }).collect::<Vec<_>>())
}
