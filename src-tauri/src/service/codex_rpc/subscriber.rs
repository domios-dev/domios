use super::{
    lifecycle::Sidecar,
    protocol::Frame,
    routing::{selection_for_event, verified_user_thread, Selection},
};
use crate::service::ServiceContext;
use serde_json::json;
use std::sync::{atomic::Ordering, Arc};

pub fn start(svc: ServiceContext, tab: String, sidecar: &Arc<Sidecar>) {
    let weak = Arc::downgrade(sidecar);
    let mut events = sidecar.client.observe();
    let mut death = sidecar.client.death();
    let mut process_death = sidecar.process.death.clone();
    sidecar.client.release_thread(&sidecar.thread.lock());
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = death.changed() => break,
                _ = process_death.changed() => break,
                frame = events.recv() => {
                    let Ok(frame) = frame else { break };
                    let Some(sidecar) = weak.upgrade() else { break };
                    let Frame::Notification { method, params } = frame else { continue };
                    if method == "account/login/completed" {
                        let response = sidecar.client.request("account/read", json!({"refreshToken":false})).await;
                        if let Ok(response) = response {
                            let auth = super::auth::account_state(&response);
                            *sidecar.auth.lock() = auth.clone();
                            svc.notify_event("codex-rpc-auth", json!({"tab_id":tab,"auth":auth}));
                        }
                    }
                    let current = sidecar.thread.lock().clone();
                    if params.get("threadId").and_then(|id| id.as_str()) == Some(&current) {
                        if method == "turn/started" { sidecar.busy.store(true, Ordering::Release); }
                        if method == "turn/completed" {
                            sidecar.busy.store(false, Ordering::Release);
                            crate::service::provider_update::tab_became_idle(&svc, &tab);
                        }
                    }
                    let candidate = if method == "thread/started" {
                        params.get("thread").and_then(|thread| thread.get("id")).and_then(|id| id.as_str()).map(str::to_owned)
                    } else {
                        match selection_for_event(&current, &params) { Selection::Verify(id) => Some(id), _ => None }
                    };
                    let Some(candidate) = candidate.filter(|candidate| candidate != &current) else { continue };
                    sidecar.verified.store(false, Ordering::Release);
                    let read = sidecar.client.request("thread/read", json!({"threadId":candidate,"includeTurns":false})).await;
                    if let Ok(read) = read {
                        if verified_user_thread(&candidate, &read) {
                            let mut settings = sidecar.thread_settings.clone();
                            settings["threadId"] = json!(candidate);
                            settings["excludeTurns"] = json!(true);
                            let resumed = sidecar.client.request("thread/resume", settings).await;
                            if resumed.is_ok() {
                                *sidecar.thread.lock() = candidate.clone();
                                sidecar.busy.store(method == "turn/started", Ordering::Release);
                                super::lifecycle::bind(&svc, &tab, candidate.clone());
                                sidecar.client.release_thread(&candidate);
                                sidecar.verified.store(true, Ordering::Release);
                            }
                        } else {
                            sidecar.verified.store(true, Ordering::Release);
                        }
                    }
                }
            }
        }
        if weak.upgrade().is_some_and(|sidecar| {
            super::lifecycle::for_tab(&tab).is_some_and(|current| Arc::ptr_eq(&sidecar, &current))
        }) {
            super::lifecycle::clear_tab(&tab);
            svc.push_structured_prompt(&tab, None);
        }
    });
}
