use serde_json::{json, Value};

use super::recovery::RECONCILE_DEADLINE;
use super::{handle_event, owns_session, ChatStreamState, ServiceContext, ACTIVE};

pub(super) fn snapshot_events(session_id: &str, messages: &Value) -> Result<Vec<Value>, String> {
    let messages = messages.as_array().ok_or("invalid message snapshot")?;
    let mut events = Vec::new();
    for message in messages {
        let info = message.get("info").ok_or("message snapshot missing info")?;
        if info.get("sessionID").and_then(Value::as_str) != Some(session_id) {
            continue;
        }
        events.push(json!({"type":"message.updated", "properties":{"info":info}}));
        for part in message
            .get("parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            events.push(json!({"type":"message.part.updated", "properties":{"part":part}}));
        }
    }
    Ok(events)
}

pub(super) fn pending_events(
    session_id: &str,
    permissions: &Value,
    questions: &Value,
) -> Result<Vec<Value>, String> {
    let mut events = Vec::new();
    for (kind, requests) in [
        ("permission.asked", permissions),
        ("question.asked", questions),
    ] {
        for request in requests
            .as_array()
            .ok_or("invalid pending request snapshot")?
        {
            if request.get("sessionID").and_then(Value::as_str) == Some(session_id) {
                events.push(json!({"type":kind, "properties":request}));
            }
        }
    }
    Ok(events)
}

pub(super) fn status_event(session_id: &str, statuses: &Value) -> Result<Value, String> {
    let statuses = statuses
        .as_object()
        .ok_or("invalid session status snapshot")?;
    let status = statuses
        .get(session_id)
        .cloned()
        .unwrap_or_else(|| json!({"type":"idle"}));
    let kind = status
        .get("type")
        .and_then(Value::as_str)
        .ok_or("invalid session status")?;
    match kind {
        "idle" => Ok(json!({"type":"session.idle", "properties":{"sessionID":session_id}})),
        "busy" | "retry" => Ok(
            json!({"type":"session.status", "properties":{"sessionID":session_id,"status":status}}),
        ),
        _ => Err("unknown session status".into()),
    }
}

pub(super) async fn read(
    client: &reqwest::Client,
    base_url: &str,
    cwd: &str,
    path: &str,
) -> Result<Value, String> {
    client
        .get(format!("{base_url}{path}"))
        .query(&[("directory", cwd)])
        .timeout(RECONCILE_DEADLINE)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())
}

pub(super) async fn reconcile(
    svc: &ServiceContext,
    tab_id: &str,
    client: &reqwest::Client,
    base_url: &str,
    cwd: &str,
    state: &mut ChatStreamState,
    live: &parking_lot::Mutex<bool>,
) -> Result<(), String> {
    let binding = {
        let mux = svc.mux.read();
        mux.get_tab(tab_id).and_then(|tab| {
            tab.agent_session_id
                .clone()
                .map(|id| (id, tab.session_binding_nonce.clone()))
        })
    };
    let Some((session_id, nonce)) = binding else {
        return Ok(());
    };
    let messages = read(
        client,
        base_url,
        cwd,
        &format!("/session/{session_id}/message"),
    )
    .await?;
    let permissions = read(client, base_url, cwd, "/permission").await?;
    let questions = read(client, base_url, cwd, "/question").await?;
    let permissions_v2 = read(
        client,
        base_url,
        cwd,
        &format!("/api/session/{session_id}/permission"),
    )
    .await?;
    let questions_v2 = read(
        client,
        base_url,
        cwd,
        &format!("/api/session/{session_id}/question"),
    )
    .await?;
    let statuses = read(client, base_url, cwd, "/session/status").await?;
    let generation = live.lock();
    if !*generation {
        return Err("subscriber generation ended".into());
    }
    let current = svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
        tab.agent_session_id.as_deref() == Some(&session_id) && tab.session_binding_nonce == nonce
    });
    if !current {
        return Err("session binding changed during reconciliation".into());
    }
    state.reset_session(&session_id);
    let events = snapshot_events(&session_id, &messages)?;
    let mut pending = pending_events(&session_id, &permissions, &questions)?;
    let v2 = pending_events(
        &session_id,
        permissions_v2
            .get("data")
            .ok_or("invalid v2 permission snapshot")?,
        questions_v2
            .get("data")
            .ok_or("invalid v2 question snapshot")?,
    )?;
    for mut event in v2 {
        event["type"] = json!(if event["type"] == "permission.asked" {
            "permission.v2.asked"
        } else {
            "question.v2.asked"
        });
        pending.push(event);
    }
    let status = status_event(&session_id, &statuses)?;
    for event in events {
        // Historical user messages must never confirm a newly written send.
        if event.get("type").and_then(Value::as_str) == Some("message.part.updated") {
            emit_snapshot_part(svc, tab_id, &event, state);
        } else {
            handle_event(svc, tab_id, &event, state);
        }
    }
    let listed: std::collections::HashSet<_> = pending
        .iter()
        .filter_map(|event| event.pointer("/properties/id").and_then(Value::as_str))
        .collect();
    if let Some(queued) = super::QUEUED.lock().get_mut(tab_id) {
        queued.retain(|entry| listed.contains(entry.route.request_id.as_str()));
    }
    {
        let mut active = ACTIVE.lock();
        if active
            .get(tab_id)
            .is_some_and(|entry| !listed.contains(entry.route.request_id.as_str()))
        {
            active.remove(tab_id);
        }
    }
    svc.push_structured_prompt(tab_id, super::promote_prompt(tab_id));
    for event in &pending {
        handle_event(svc, tab_id, event, state);
    }
    handle_event(svc, tab_id, &status, state);
    if status["type"] == "session.idle" {
        svc.on_agent_turn_settled(tab_id);
    }
    Ok(())
}

pub(super) fn emit_snapshot_part(
    svc: &ServiceContext,
    tab_id: &str,
    event: &Value,
    state: &mut ChatStreamState,
) {
    let super::ParsedEvent::PartUpdated { update } = super::parse_event(event) else {
        return;
    };
    if !owns_session(svc, tab_id, &update.session_id) {
        return;
    }
    let Some(message) = state.messages.get(&update.message_id).cloned() else {
        return;
    };
    if let Some(mut part) = state.prepare_chat_part(&update, &message) {
        super::tool_diffs::apply(svc, tab_id, &update.part, &mut part);
        let nonce = svc
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone());
        crate::service::tab_chat::structured_sink::emit_chat_msgs(
            svc.ui.as_ref(),
            tab_id,
            &[part],
            None,
            Some(&update.session_id),
            nonce.as_deref(),
        );
        state.recovered_parts.insert(update.part_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_keep_native_ids_and_exclude_foreign_sessions() {
        let events = snapshot_events("own", &json!([
            {"info":{"id":"m1","sessionID":"own","role":"assistant"},"parts":[{"id":"p1","sessionID":"own","messageID":"m1","type":"text","text":"backfill"}]},
            {"info":{"id":"m2","sessionID":"foreign"},"parts":[]}
        ])).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["properties"]["part"]["id"], "p1");
        assert!(snapshot_events("own", &Value::Null).is_err());
    }

    #[test]
    fn pending_snapshot_is_authoritative_and_session_scoped() {
        let events = pending_events(
            "own",
            &json!([
                {"id":"p1","sessionID":"own"},{"id":"p2","sessionID":"foreign"}
            ]),
            &json!([{"id":"q1","sessionID":"own","questions":[]}]),
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "permission.asked");
        assert_eq!(events[1]["properties"]["id"], "q1");
        assert!(pending_events("own", &Value::Null, &json!([])).is_err());
    }

    #[test]
    fn status_snapshot_settles_missing_and_idle_sessions_but_preserves_busy() {
        assert_eq!(
            status_event("own", &json!({})).unwrap()["type"],
            "session.idle"
        );
        assert_eq!(
            status_event("own", &json!({"own":{"type":"busy"}})).unwrap()["type"],
            "session.status"
        );
        assert!(status_event("own", &Value::Null).is_err());
    }
}
