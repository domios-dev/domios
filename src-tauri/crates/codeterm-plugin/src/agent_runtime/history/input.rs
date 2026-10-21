use serde_json::Value;

pub fn sanitize_history_input(value: &mut Value) {
    match value {
        Value::String(text) => {
            if text.starts_with("data:image/") && text.contains(";base64,") {
                *text = format!("[image omitted: {} bytes]", text.len());
            } else if text.len() > 16 * 1024 {
                let bytes = text.len();
                let prefix: String = text.chars().take(2048).collect();
                *text = format!("{prefix}\n[content truncated: {bytes} bytes total]");
            }
        }
        Value::Array(values) => values.iter_mut().for_each(sanitize_history_input),
        Value::Object(fields) => {
            let image = fields.get("type").and_then(Value::as_str) == Some("image");
            let generated = fields.get("type").and_then(Value::as_str) == Some("imageGeneration");
            for (key, value) in fields.iter_mut() {
                if (image && key == "data") || (generated && key == "result") {
                    if let Some(data) = value.as_str() {
                        if !data.starts_with("[image omitted:") {
                            *value =
                                Value::String(format!("[image omitted: {} bytes]", data.len()));
                        }
                    }
                } else {
                    sanitize_history_input(value);
                }
            }
        }
        _ => {}
    }
}

pub const HISTORY_PAGE_TOO_LARGE: i64 = -32097;
const HISTORY_ITEM_BYTES: usize = 64 * 1024;

pub fn prepare_history_reply(
    state: &Value,
    reply: crate::agent_runtime::RpcReply,
) -> crate::agent_runtime::RpcReply {
    use crate::agent_runtime::*;
    let limit = state["historyItemLimit"].as_u64().unwrap_or(1);
    if state["stage"].as_str() == Some("items")
        && limit > 1
        && state["offset"].as_u64().unwrap_or(0) == 0
        && validate_runtime_reply(&reply).is_err()
        && matches!(&reply, RpcReply::Success { .. })
    {
        log::debug!(
            "[history-input] stage={} oversized page limit={limit} retry_limit={}",
            state["stage"],
            limit / 2
        );
        return RpcReply::Error {
            error: RpcFailure {
                code: HISTORY_PAGE_TOO_LARGE,
                message: "History page exceeds hook input budget".into(),
                data: Some(serde_json::json!({"historyPageLimit":limit / 2})),
            },
        };
    }
    let mut projected = reply;
    if let RpcReply::Success { value } = &mut projected {
        sanitize_history_input(value);
        if let Some(records) = value.get_mut("data").and_then(Value::as_array_mut) {
            let item_budget = HISTORY_ITEM_BYTES
                .min((crate::agent_runtime::MAX_RUNTIME_VALUE_BYTES / 2) / records.len().max(1));
            for record in records {
                let item = if record.get("item").is_some() {
                    &mut record["item"]
                } else {
                    record
                };
                if validate_runtime_size(item, item_budget).is_err() {
                    log::debug!("[history-input] oversized item projected budget={item_budget}");
                    let id = item.get("id").cloned().unwrap_or(Value::Null);
                    let kind = item.get("type").cloned().unwrap_or(Value::Null);
                    let status = item.get("status").cloned().unwrap_or(Value::Null);
                    let started_at = item.get("startedAt").and_then(Value::as_i64);
                    *item = serde_json::json!({"id":id,"type":kind,"status":status,"startedAt":started_at,"historyTruncated":true});
                }
            }
        }
    }
    projected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::*;
    use serde_json::json;

    #[test]
    fn oversized_history_page_retries_smaller_limits_before_entering_hook() {
        let reply = RpcReply::Success {
            value: json!({"data":(0..16).map(|id| json!({
            "item":{"id":id.to_string(),"type":"agentMessage","text":"x".repeat(512 * 1024)}
        })).collect::<Vec<_>>(),"nextCursor":"older"}),
        };
        for limit in [16, 8, 4, 2] {
            let retry = prepare_history_reply(
                &json!({"stage":"items","historyItemLimit":limit}),
                reply.clone(),
            );
            validate_runtime_reply(&retry).unwrap();
            let RpcReply::Error { error } = retry else {
                panic!("expected smaller page")
            };
            assert_eq!(error.code, HISTORY_PAGE_TOO_LARGE);
            assert_eq!(error.data.unwrap()["historyPageLimit"], limit / 2);
        }
    }

    #[test]
    fn twenty_mb_single_item_enters_hook_as_identity_preserving_truncated_projection() {
        let reply = RpcReply::Success {
            value: json!({"data":[{"item":{
            "id":"huge","type":"userMessage","content":(0..20480).map(|_| json!({"type":"text","text":"x".repeat(1024)})).collect::<Vec<_>>()
        }}],"nextCursor":"older"}),
        };
        let projected = prepare_history_reply(&json!({"historyItemLimit":1}), reply);
        validate_runtime_reply(&projected).unwrap();
        let RpcReply::Success { value } = projected else {
            panic!("expected projected item")
        };
        assert_eq!(value["data"][0]["item"]["id"], "huge");
        assert_eq!(value["data"][0]["item"]["type"], "userMessage");
        assert_eq!(value["data"][0]["item"]["historyTruncated"], true);
        assert_eq!(value["nextCursor"], "older");
    }

    #[test]
    fn byte_split_checkpoint_projects_without_changing_server_page_size() {
        let reply = RpcReply::Success {
            value: json!({"data":[{"item":{
                "id":"huge","type":"agentMessage","text":"x".repeat(20 * 1024 * 1024)
            }}]}),
        };
        let projected = prepare_history_reply(&json!({"historyItemLimit":16,"offset":1}), reply);
        validate_runtime_reply(&projected).unwrap();
        let RpcReply::Success { value } = projected else {
            panic!("expected same-page projection")
        };
        assert!(value["data"][0]["item"]["text"]
            .as_str()
            .unwrap()
            .contains("[content truncated:"));
    }
}
