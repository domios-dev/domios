use serde_json::Value;
use tokio::sync::watch;
use tokio::time::{timeout, timeout_at, Instant};

use super::recovery::{Frames, Recovery, CONNECT_DEADLINE, RECONCILE_DEADLINE};
use super::{handle_event, reconcile, ChatStreamState, ServiceContext};

#[derive(Clone, Copy, Default)]
pub(super) struct Wake {
    pub output: u64,
    pub snapshot: u64,
}

pub(super) async fn run(
    svc: ServiceContext,
    tab_id: String,
    binding: super::subscription::Binding,
    mut signal: watch::Receiver<Wake>,
    live: std::sync::Arc<parking_lot::Mutex<bool>>,
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
        if signal.has_changed().is_err()
            || super::subscription::current(&svc, &tab_id).as_ref() != Some(&binding)
        {
            return;
        }
        let result = follow(
            &svc,
            &tab_id,
            &binding,
            &client,
            &mut signal,
            &mut state,
            &mut recovery,
            &live,
        )
        .await;
        if signal.has_changed().is_err()
            || super::subscription::current(&svc, &tab_id).as_ref() != Some(&binding)
        {
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
            recovery.retry_after_event();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn follow(
    svc: &ServiceContext,
    tab_id: &str,
    binding: &super::subscription::Binding,
    client: &reqwest::Client,
    signal: &mut watch::Receiver<Wake>,
    state: &mut ChatStreamState,
    recovery: &mut Recovery,
    live: &parking_lot::Mutex<bool>,
) -> Result<(), String> {
    let base_url = &binding.base_url;
    let cwd = &binding.cwd;
    let request = client
        .get(format!("{base_url}/event"))
        .query(&[("directory", cwd.as_str())])
        .header("Accept", "text/event-stream")
        .send();
    let mut response = tokio::select! {
        result = timeout(CONNECT_DEADLINE, request) => result.map_err(|_| "event connect deadline")?.map_err(|error| error.to_string())?.error_for_status().map_err(|error| error.to_string())?,
        _ = signal.wait_for(|_| false) => return Ok(()),
    };
    let snapshot = reconcile::reconcile(svc, tab_id, client, base_url, cwd, state, live);
    tokio::select! {
        result = timeout(RECONCILE_DEADLINE, snapshot) => result.map_err(|_| "reconciliation deadline")??,
        _ = signal.wait_for(|_| false) => return Ok(()),
    }
    let mut frames = Frames::default();
    let mut deadline = super::recovery::heartbeat_deadline(Instant::now());
    let mut healthy = false;
    let mut snapshot_revision = signal.borrow().snapshot;
    loop {
        if super::subscription::current(svc, tab_id).as_ref() != Some(binding) {
            return Ok(());
        }
        let bound = svc
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.agent_session_id.clone());
        let revision = signal.borrow().snapshot;
        if bound != state.session_id || revision != snapshot_revision {
            snapshot_revision = revision;
            tokio::select! {
                result = timeout(RECONCILE_DEADLINE, reconcile::reconcile(svc, tab_id, client, base_url, cwd, state, live)) => result.map_err(|_| "binding reconciliation deadline")??,
                _ = signal.wait_for(|_| false) => return Ok(()),
            }
        }
        tokio::select! {
            changed = signal.changed() => { changed.map_err(|_| "subscriber cancelled")?; }
            chunk = timeout_at(deadline, response.chunk()) => {
                let chunk = chunk.map_err(|_| "event heartbeat silence deadline")?
                    .map_err(|error| error.to_string())?.ok_or("event stream ended")?;
                for event in frames.push(&chunk)? {
                    if signal.has_changed().is_err() || super::subscription::current(svc, tab_id).as_ref() != Some(binding) { return Ok(()); }
                    deadline = super::recovery::heartbeat_deadline(Instant::now());
                    if !healthy {
                        recovery.healthy();
                        healthy = true;
                        transport_state(svc, tab_id, "connected", "Event stream reconciled.");
                    }
                    tokio::select! {
                        result = apply_event(svc, tab_id, client, base_url, cwd, &event, state, live) => result?,
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
    live: &parking_lot::Mutex<bool>,
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
            let guard = live.lock();
            if !*guard {
                return Ok(());
            }
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
    let guard = live.lock();
    if !*guard {
        return Ok(());
    }
    handle_event(svc, tab_id, event, state);
    Ok(())
}

fn transport_state(svc: &ServiceContext, tab_id: &str, status: &str, detail: &str) {
    log::info!("[structured-channel] tab={tab_id} status={status} detail={detail}");
    svc.ui.emit(
        "structured-channel-state",
        serde_json::to_value(serde_json::json!({
            "tab_id":tab_id, "status":status, "detail":detail
        }))
        .unwrap_or_default(),
    );
    if status == "failed" {
        svc.log_system_notice(
            "Structured channel unavailable".into(),
            Some(format!("Tab {tab_id}: {detail}")),
            Some("error"),
        );
    }
}
