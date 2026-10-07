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
                    let current = sidecar.thread.lock().clone();
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
                            let resumed = sidecar.client.request("thread/resume", json!({"threadId":candidate,"excludeTurns":true})).await;
                            if resumed.is_ok() {
                                *sidecar.thread.lock() = candidate.clone();
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
