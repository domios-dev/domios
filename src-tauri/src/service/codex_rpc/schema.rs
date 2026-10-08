use serde::Deserialize;
#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
pub const SNAPSHOT_VERSION: &str = "0.160.0";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub user_agent: String,
}

#[derive(Debug, Deserialize)]
pub struct ThreadResponse {
    pub thread: Thread,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_schema_ignores_new_fields() {
        let response: ThreadResponse = serde_json::from_value(serde_json::json!({
            "thread": {"id":"t", "future": {}}, "future": true
        }))
        .unwrap();
        assert_eq!(response.thread.id, "t");
        let snapshot: Value = serde_json::from_str(include_str!(
            "schema/codex_app_server_protocol.schemas.json"
        ))
        .unwrap();
        assert!(snapshot.is_object());
        assert_eq!(SNAPSHOT_VERSION, "0.160.0");
    }
}
