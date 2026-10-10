use super::{
    client::Client,
    lifecycle::{for_tab, rpc_error},
    normalize::Normalizer,
    protocol::Frame,
};
use crate::{agent::chat_types::ChatMsg, service::ServiceContext};
use serde_json::{json, Value};
use std::{collections::HashSet, time::Duration};

const OPTIONAL_GOAL_DEADLINE: Duration = Duration::from_secs(1);

pub struct History {
    pub messages: Vec<ChatMsg>,
    pub active_turn: Option<String>,
    pub completed_turns: Vec<String>,
    pub final_assistant_ids: HashSet<String>,
}

pub async fn history(client: &Client, thread: &str) -> Result<History, String> {
    let turns = pages(
        client,
        "thread/turns/list",
        json!({"threadId":thread,"itemsView":"notLoaded","sortDirection":"asc","limit":100}),
    )
    .await?;
    let active_turn = turns
        .iter()
        .find(|turn| turn["status"] == "inProgress" && !turn["startedAt"].is_null())
        .and_then(|turn| turn["id"].as_str())
        .map(str::to_owned);
    let completed_turns = turns
        .iter()
        .filter(|turn| {
            matches!(
                turn["status"].as_str(),
                Some("completed" | "interrupted" | "failed")
            )
        })
        .filter_map(|turn| turn["id"].as_str().map(str::to_owned))
        .collect();
    let mut normalizer = Normalizer::default();
    let mut messages = Vec::new();
    let mut final_assistant_ids = HashSet::new();
    for turn in turns {
        let turn_id = turn
            .get("id")
            .and_then(Value::as_str)
            .ok_or("Missing history turn id")?;
        if turn["status"] != "inProgress" || !turn["startedAt"].is_null() {
            messages.extend(normalizer.apply_all(&Frame::Notification {
                method: "turn/started".into(),
                params: json!({"threadId":thread,"turn":turn}),
            }));
        }
        let entries = pages(
            client,
            "thread/items/list",
            json!({"threadId":thread,"turnId":turn_id,"sortDirection":"asc","limit":100}),
        )
        .await?;
        for entry in entries {
            if turn["status"] == "completed" {
                if let Some(id) = final_assistant_id(&entry) {
                    final_assistant_ids.insert(id.to_string());
                }
            }
            messages.extend(normalize_entries(&mut normalizer, thread, &entry));
        }
        if turn["status"] != "inProgress" {
            messages.extend(normalizer.apply_all(&Frame::Notification {
                method: "turn/completed".into(),
                params: json!({"threadId":thread,"turn":turn}),
            }));
        }
    }
    let goal = tokio::time::timeout(
        OPTIONAL_GOAL_DEADLINE,
        client.request("thread/goal/get", json!({"threadId":thread})),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .and_then(|response| response.get("goal").filter(|goal| !goal.is_null()).cloned());
    if let Some(goal) = goal {
        messages.extend(normalizer.apply_all(&Frame::Notification {
            method: "thread/goal/updated".into(),
            params: json!({"threadId":thread,"goal":goal}),
        }));
    }
    Ok(History {
        messages,
        active_turn,
        completed_turns,
        final_assistant_ids,
    })
}

async fn pages(client: &Client, method: &str, mut params: Value) -> Result<Vec<Value>, String> {
    let mut all = Vec::new();
    let mut cursors = HashSet::new();
    loop {
        let response = client
            .request(method, params.clone())
            .await
            .map_err(rpc_error)?;
        all.extend(
            response
                .get("data")
                .and_then(Value::as_array)
                .ok_or("Missing history page data")?
                .iter()
                .cloned(),
        );
        let Some(cursor) = response.get("nextCursor").and_then(Value::as_str) else {
            return Ok(all);
        };
        if !cursors.insert(cursor.to_owned()) {
            return Err("Repeated history cursor".into());
        }
        params["cursor"] = json!(cursor);
    }
}

#[cfg(test)]
pub fn normalize_entry(
    normalizer: &mut Normalizer,
    thread: &str,
    entry: &Value,
) -> Option<ChatMsg> {
    normalize_entries(normalizer, thread, entry)
        .into_iter()
        .next()
}

fn normalize_entries(normalizer: &mut Normalizer, thread: &str, entry: &Value) -> Vec<ChatMsg> {
    let mut params = entry.clone();
    params["threadId"] = json!(thread);
    if params.get("item").is_none() {
        params["item"] = entry.clone();
    }
    normalizer.apply_all(&Frame::Notification {
        method: "item/completed".into(),
        params,
    })
}

fn final_assistant_id(entry: &Value) -> Option<&str> {
    let item = entry.get("item").unwrap_or(entry);
    (item["type"] == "agentMessage" && item["phase"] == "final_answer")
        .then(|| item["id"].as_str())
        .flatten()
}

pub fn snapshot(
    svc: &ServiceContext,
    tab: &str,
) -> Result<(Vec<ChatMsg>, String, Option<String>), String> {
    let sidecar = for_tab(tab).ok_or("No RPC transcript endpoint")?;
    let thread = sidecar.thread.lock().clone();
    let nonce = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.session_binding_nonce.clone());
    let messages = sidecar.normalizer.lock().snapshot(&thread);
    let current = svc.mux.read().get_tab(tab).is_some_and(|tab| {
        tab.agent_session_id.as_deref() == Some(&thread) && tab.session_binding_nonce == nonce
    });
    if !current {
        return Err("Transcript binding changed during snapshot".into());
    }
    Ok((messages, thread, nonce))
}

pub fn start(
    svc: &ServiceContext,
    tab: &str,
    subscription: String,
) -> Result<crate::commands::ChatStartResult, String> {
    let (messages, thread, nonce) = snapshot(svc, tab)?;
    let total = messages.len();
    let start = total.saturating_sub(200);
    Ok(crate::commands::ChatStartResult {
        text: String::new(),
        start_offset: start as u64,
        offset: total as u64,
        pending: false,
        messages: Some(messages.into_iter().skip(start).collect()),
        cursor: None,
        source_session_id: Some(thread),
        session_binding_nonce: nonce,
        subscription_id: Some(subscription),
        source_missing: false,
    })
}

pub fn backfill(
    svc: &ServiceContext,
    tab: &str,
    before: u64,
) -> Result<crate::commands::ChatBackfillResult, String> {
    let (messages, thread, nonce) = snapshot(svc, tab)?;
    let end = (before as usize).min(messages.len());
    let start = end.saturating_sub(200);
    Ok(crate::commands::ChatBackfillResult {
        text: String::new(),
        start_offset: start as u64,
        end_offset: end as u64,
        messages: messages.into_iter().skip(start).take(end - start).collect(),
        source_session_id: Some(thread),
        session_binding_nonce: nonce,
    })
}

pub fn emit(svc: &ServiceContext, tab: &str, thread: &str, message: ChatMsg) {
    let nonce = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.session_binding_nonce.clone());
    crate::service::tab_chat::structured_sink::make_sink(
        svc.app_handle.clone(),
        svc.mux.clone(),
        Some(thread.into()),
        nonce,
        Some(svc.chat_send_ledger.clone()),
    )
    .emit(tab, vec![message], None);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cron_completion_requires_final_assistant_record_from_history() {
        let item = json!({"id":"supplied-final-id", "type":"agentMessage", "phase":"final_answer"});
        assert_eq!(final_assistant_id(&item), item["id"].as_str());
        assert_eq!(
            final_assistant_id(&json!({"item":item})),
            item["id"].as_str()
        );
        assert_eq!(
            final_assistant_id(
                &json!({"id":"commentary", "type":"agentMessage", "phase":"commentary"})
            ),
            None
        );
        assert_eq!(
            final_assistant_id(
                &json!({"id":"tool", "type":"commandExecution", "phase":"final_answer"})
            ),
            None
        );
    }

    #[test]
    fn codex_rpc_control_live_and_backfill_share_stable_item_ids() {
        let entry = json!({"turnId":"t","item":{"id":"item-id","type":"agentMessage","text":"whole"},"startedAtMs":1000});
        let mut normalizer = Normalizer::default();
        let snapshot = normalize_entry(&mut normalizer, "bound", &entry).unwrap();
        let live = normalizer
            .apply(&Frame::Notification {
                method: "item/completed".into(),
                params: json!({"threadId":"bound","item":entry["item"]}),
            })
            .unwrap();
        assert_eq!(snapshot.id, "item-id");
        assert_eq!(snapshot.id, live.id);
        assert_eq!(snapshot.seq, live.seq);
    }
    #[test]
    fn codex_rpc_control_cached_snapshot_reuses_history_and_live_item_ids_in_order() {
        let mut normalizer = Normalizer::default();
        for (thread, id) in [("own", "first"), ("foreign", "other"), ("own", "second")] {
            normalize_entry(
                &mut normalizer,
                thread,
                &json!({"item":{"type":"agentMessage","id":id,"text":id}}),
            )
            .unwrap();
        }
        let snapshot = normalizer.snapshot("own");
        assert_eq!(
            snapshot
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(normalizer.snapshot("own").len(), 2);
    }
}
