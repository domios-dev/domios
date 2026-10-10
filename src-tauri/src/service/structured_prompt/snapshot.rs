use std::collections::VecDeque;

use serde_json::Value;

use super::opencode_sse::{normalize_chat_part, ChatPartEvent, ChatPartUpdate, MessageMeta};
use crate::agent::chat_types::ChatMsg;
use crate::service::ServiceContext;

pub(crate) const RETAINED_ROWS: usize = 440;

fn normalize_with(
    session_id: &str,
    value: &Value,
    mut map: impl FnMut(&Value, &mut ChatMsg),
) -> Result<Vec<ChatMsg>, String> {
    let messages = value
        .as_array()
        .ok_or("Invalid structured transcript snapshot")?;
    let mut rows = VecDeque::new();
    let mut sequence = 0;
    for message in messages {
        let info = &message["info"];
        if info["sessionID"].as_str() != Some(session_id) {
            continue;
        }
        let Some(message_id) = info["id"].as_str() else {
            continue;
        };
        let meta = MessageMeta {
            model: super::opencode_sse::assistant_model(info),
            session_id: session_id.into(),
            message_id: message_id.into(),
            role: info["role"].as_str().unwrap_or("assistant").into(),
            timestamp: info["time"]["created"]
                .as_i64()
                .map(|time| time.to_string()),
        };
        for part in message["parts"].as_array().into_iter().flatten() {
            let Some(part_id) = part["id"].as_str() else {
                continue;
            };
            if part["sessionID"].as_str() != Some(session_id)
                || part["messageID"].as_str() != Some(message_id)
            {
                continue;
            }
            let update = ChatPartUpdate {
                session_id: session_id.into(),
                message_id: message_id.into(),
                part_id: part_id.into(),
                part: part.clone(),
                delta: None,
                event: ChatPartEvent::Updated,
            };
            if let Some(mut row) = normalize_chat_part(&update, &meta, None) {
                map(part, &mut row);
                row.seq = sequence;
                sequence += 1;
                if rows.len() == RETAINED_ROWS {
                    rows.pop_front();
                }
                rows.push_back(row);
            }
        }
    }
    Ok(rows.into_iter().collect())
}

pub(crate) fn read(svc: &ServiceContext, tab_id: &str) -> Result<Vec<ChatMsg>, String> {
    let (base_url, cwd) = super::endpoint_for_tab(tab_id).ok_or("No structured endpoint")?;
    let binding = || {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
        Ok::<_, String>((
            tab.agent_session_id.clone().ok_or("Tab has no session")?,
            tab.session_binding_nonce.clone(),
        ))
    };
    let (session_id, nonce) = binding()?;
    let value = reqwest::blocking::Client::builder()
        .no_proxy()
        .connect_timeout(super::recovery::CONNECT_DEADLINE)
        .timeout(super::recovery::RECONCILE_DEADLINE)
        .build()
        .map_err(|error| error.to_string())?
        .get(format!("{base_url}/session/{session_id}/message"))
        .query(&[("directory", cwd), ("limit", RETAINED_ROWS.to_string())])
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.json::<Value>())
        .map_err(|error| error.to_string())?;
    if binding()? != (session_id.clone(), nonce) {
        return Err("Transcript binding changed during snapshot".into());
    }
    normalize_with(&session_id, &value, |part, row| {
        super::tool_diffs::apply(svc, tab_id, part, row)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn snapshot_file_diffs_use_the_provider_mapping() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../plugins/provider-opencode/plugin.json");
        let provider = crate::plugin::js_runtime::loader::load_one(&manifest).unwrap();
        let fixtures: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/opencode-1.18.35-tool-parts.json"
        )))
        .unwrap();
        let mut part = fixtures[0]["part"].clone();
        part["id"] = json!("write");
        part["sessionID"] = json!("own");
        part["messageID"] = json!("message");
        let value =
            json!([{"info":{"id":"message","sessionID":"own","role":"assistant"},"parts":[part]}]);
        let rows = normalize_with("own", &value, |part, row| {
            super::super::tool_diffs::map(&provider, part, row);
        })
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_diffs.as_ref().unwrap()[0]["additions"], 1);
        assert_eq!(
            rows[0].file_diffs.as_ref().unwrap()[0]["path"],
            "/repo/open.txt"
        );
    }

    #[test]
    fn structured_snapshot_replays_own_session_with_live_ids_and_bounded_tail() {
        let parts: Vec<_> = (0..500)
            .map(|index| {
                json!({
                    "id": format!("part-{index}"), "sessionID":"own", "messageID":"message",
                    "type":"text", "text":format!("text-{index}")
                })
            })
            .collect();
        let value = json!([
            {"info":{"id":"foreign","sessionID":"other","role":"assistant"},"parts":parts},
            {"info":{"id":"message","sessionID":"own","role":"assistant"},"parts":parts}
        ]);
        let rows = normalize_with("own", &value, |_, _| {}).unwrap();
        assert_eq!(rows.len(), RETAINED_ROWS);
        assert_eq!(rows[0].id, "part-60");
        assert_eq!(rows[439].content, "text-499");
        assert_eq!(normalize_with("own", &value, |_, _| {}).unwrap(), rows);
    }
}
