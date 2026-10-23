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

pub(super) fn intent_response(
    intent: &str,
    recalled: Result<Value, String>,
) -> Result<Value, String> {
    let scaffold = json!({
        "schema_version":2,"kind":"pipeline","name":"custom-pipeline","intent":intent,"owner":"team",
        "trigger":{"type":"schedule","every":{"lit":"1m"}},"steps":[{"id":"inspect","run":{"lit":"echo ready"},"routes":{"done":"@stop","failed":"@stop"}}]
    });
    let yaml = serde_yaml::to_string(&scaffold).map_err(|error| error.to_string())?;
    let mut response = match recalled {
        Ok(value) if value["cards"].is_array() && value["recall"].is_object() => value,
        Ok(_) => {
            json!({"cards":[],"recall":{"status":"degraded","reason":"Host returned no typed recall envelope"},"unavailable":[]})
        }
        Err(error) => {
            json!({"cards":[],"recall":{"status":"degraded","reason":error},"unavailable":[]})
        }
    };
    response["scaffold"] = scaffold;
    response["yaml"] = json!(yaml);
    Ok(response)
}

pub(super) fn render_intent(response: &Value) -> String {
    let mut text = String::new();
    let status = response["recall"]["status"].as_str().unwrap_or("degraded");
    text.push_str(&format!("Recall: {status}"));
    if let Some(reason) = response["recall"]["reason"].as_str() {
        text.push_str(&format!(
            " — {}",
            reason.split_whitespace().collect::<Vec<_>>().join(" ")
        ));
    }
    text.push('\n');
    let cards = response["cards"].as_array();
    if cards.is_none_or(Vec::is_empty) {
        text.push_str(
            "No fitting typed card returned. Author the required operation from scratch.\n",
        );
    }
    for card in cards.into_iter().flatten() {
        text.push_str(&format!("\n{} score={} hash={}\n  {}\n  WHY: {}\n  inputs: {} -> result: {}\n  node kinds: {}; enrichment_pending={}\n",
            card["id"].as_str().unwrap_or("?"),card["score"],card["hash"].as_str().unwrap_or("?"),
            card["when_to_use"].as_str().unwrap_or(""),card["why"].as_str().unwrap_or(""),
            card["signature"]["inputs"],card["signature"]["result"],card["node_kinds"],card["enrichment_pending"]));
    }
    for item in response["unavailable"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "Unavailable {}: {}\n",
            item["id"].as_str().unwrap_or("?"),
            item["reason"].as_str().unwrap_or("?")
        ));
    }
    text.push_str("\nOpen a fitting body with blueprint show <id>. Save the scaffold below and author, then check/apply.\n\n");
    text.push_str(response["yaml"].as_str().unwrap_or(""));
    text
}
