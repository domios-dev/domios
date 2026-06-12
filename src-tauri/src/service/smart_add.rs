use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug)]
pub struct SmartAddResult {
    pub suggested_name: Option<String>,
    pub recommended_health_mode: String,
    pub recommended_health_path: Option<String>,
    pub field_mapping: serde_json::Value,
    pub raw_response: serde_json::Value,
}

pub async fn inspect(url: &str) -> Result<SmartAddResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let body = response.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::json!({}));

    let mut field_mapping = serde_json::Map::new();
    if parsed.get("status").is_some() {
        field_mapping.insert("ui_status".into(), serde_json::json!("$.status"));
    }
    if parsed.get("version").is_some() {
        field_mapping.insert("ui_version".into(), serde_json::json!("$.version"));
    }
    if parsed.get("uptime").is_some() {
        field_mapping.insert("ui_uptime".into(), serde_json::json!("$.uptime"));
    }

    Ok(SmartAddResult {
        suggested_name: None,
        recommended_health_mode: "http".into(),
        recommended_health_path: Some("/healthz".into()),
        field_mapping: serde_json::Value::Object(field_mapping),
        raw_response: parsed,
    })
}
