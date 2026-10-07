use serde_json::Value;
use tokio::sync::watch;
use tokio::time::{timeout, timeout_at, Instant};

use super::recovery::{Frames, Recovery, CONNECT_DEADLINE, RECONCILE_DEADLINE};
use super::{handle_event, reconcile, ChatStreamState, ServiceContext};

pub(super) async fn run(
    svc: ServiceContext,
    tab_id: String,
    base_url: String,
    cwd: String,
    mut signal: watch::Receiver<u64>,
) {
    // Async reqwest has no total timeout; each event wait has a silence deadline instead.
    let client = match reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(CONNECT_DEADLINE)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            transport_state(&svc, &tab_id, "failed", &error.to_string());
            return;
        }
    };
    let mut state = ChatStreamState::default();
    let mut recovery = Recovery::default();
    loop {
        if signal.has_changed().is_err() || super::tab_gone(&svc, &tab_id) {
            return;
        }
        let result = follow(
            &svc,
            &tab_id,
            &base_url,
            &cwd,
            &client,
            &mut signal,
            &mut state,
            &mut recovery,
        )
        .await;
        if signal.has_changed().is_err() {
            return;
        }
        let error = result.err().unwrap_or_else(|| "event stream ended".into());
        transport_state(&svc, &tab_id, "reconnecting", &error);
        if recovery.failed() {
            transport_state(
                &svc,
                &tab_id,
                "failed",
                "Reconnect budget exhausted; waiting for provider output or binding evidence.",
            );
            if signal.changed().await.is_err() {
                return;
            }
            recovery.healthy();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn follow(
    svc: &ServiceContext,
    tab_id: &str,
    base_url: &str,
    cwd: &str,
    client: &reqwest::Client,
    signal: &mut watch::Receiver<u64>,
    state: &mut ChatStreamState,
    recovery: &mut Recovery,
) -> Result<(), String> {
    let request = client
        .get(format!("{base_url}/event"))
        .query(&[("directory", cwd)])
        .header("Accept", "text/event-stream")
        .send();
    let mut response = tokio::select! {
        result = timeout(CONNECT_DEADLINE, request) => result.map_err(|_| "event connect deadline")?.map_err(|error| error.to_string())?.error_for_status().map_err(|error| error.to_string())?,
        _ = signal.wait_for(|_| false) => return Ok(()),
    };
    let snapshot = reconcile::reconcile(svc, tab_id, client, base_url, cwd, state);
    tokio::select! {
        result = timeout(RECONCILE_DEADLINE, snapshot) => result.map_err(|_| "reconciliation deadline")??,
        _ = signal.wait_for(|_| false) => return Ok(()),
    }
    let mut frames = Frames::default();
    let mut deadline = super::recovery::heartbeat_deadline(Instant::now());
    let mut healthy = false;
    loop {
        tokio::select! {
            changed = signal.changed() => {
                changed.map_err(|_| "subscriber cancelled")?;
                let bound = svc.mux.read().get_tab(tab_id).and_then(|tab| tab.agent_session_id.clone());
                if bound != state.session_id {
                    timeout(RECONCILE_DEADLINE, reconcile::reconcile(svc, tab_id, client, base_url, cwd, state))
                        .await.map_err(|_| "binding reconciliation deadline")??;
                }
            }
            chunk = timeout_at(deadline, response.chunk()) => {
                let chunk = chunk.map_err(|_| "event heartbeat silence deadline")?
                    .map_err(|error| error.to_string())?.ok_or("event stream ended")?;
                for event in frames.push(&chunk)? {
                    if signal.has_changed().is_err() { return Ok(()); }
                    deadline = super::recovery::heartbeat_deadline(Instant::now());
                    if !healthy {
                        recovery.healthy();
                        healthy = true;
                        transport_state(svc, tab_id, "connected", "Event stream reconciled.");
                    }
                    tokio::select! {
                        result = apply_event(svc, tab_id, client, base_url, cwd, &event, state) => result?,
                        _ = signal.wait_for(|_| false) => return Ok(()),
                    }
                }
            }
        }
    }
}

async fn apply_event(
    svc: &ServiceContext,
    tab_id: &str,
    client: &reqwest::Client,
    base_url: &str,
    cwd: &str,
    event: &Value,
    state: &mut ChatStreamState,
) -> Result<(), String> {
    if let super::ParsedEvent::PartUpdated { update } = super::parse_event(event) {
        if state.recovered_parts.contains(&update.part_id) {
            if !super::owns_session(svc, tab_id, &update.session_id) {
                return Ok(());
            }
            // Queued deltas have no sequence number relative to a snapshot: re-read absolute parts.
            let message = reconcile::read(
                client,
                base_url,
                cwd,
                &format!(
                    "/session/{}/message/{}",
                    update.session_id, update.message_id
                ),
            )
            .await?;
            for event in
                reconcile::snapshot_events(&update.session_id, &serde_json::json!([message]))?
            {
                if event["type"] == "message.part.updated" {
                    reconcile::emit_snapshot_part(svc, tab_id, &event, state);
                } else {
                    handle_event(svc, tab_id, &event, state);
                }
            }
            return Ok(());
        }
    }
    handle_event(svc, tab_id, event, state);
    Ok(())
}

fn transport_state(svc: &ServiceContext, tab_id: &str, status: &str, detail: &str) {
    log::info!("[structured-channel] tab={tab_id} status={status} detail={detail}");
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "structured-channel-state",
        serde_json::json!({
            "tab_id":tab_id, "status":status, "detail":detail
        }),
    );
    if status == "failed" {
        svc.create_notification_ext(
            "Structured channel unavailable".into(),
            Some(format!("Tab {tab_id}: {detail}")),
            Some("error"),
            None,
            Some(true),
            crate::service::notification::NotificationOrigin::local_tab(tab_id.into()),
            crate::service::notification::PushIntent {
                level: Some(crate::notification::push::NotifyLevel::Failure),
                event_key: Some(format!("structured-channel:{tab_id}")),
            },
            None,
        );
    }
}
