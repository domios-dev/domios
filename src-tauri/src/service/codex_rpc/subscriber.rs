use super::{
    lifecycle::Sidecar, normalize::Normalizer, protocol::Frame, routing::verified_user_thread,
};
use crate::service::ServiceContext;
use serde_json::{json, Value};
use std::sync::{atomic::Ordering, Arc};

pub fn start(svc: ServiceContext, tab: String, sidecar: &Arc<Sidecar>) {
    let weak = Arc::downgrade(sidecar);
    let mut events = sidecar.client.subscribe();
    let mut observations = sidecar.client.observe();
    let mut death = sidecar.client.death();
    let mut process_death = sidecar.process.death.clone();
    sidecar.client.release_thread(&sidecar.thread.lock());
    tauri::async_runtime::spawn(async move {
        let mut normalizer = Normalizer::default();
        loop {
            let (frame, discovery) = tokio::select! {
                _ = death.changed() => break,
                _ = process_death.changed() => break,
                frame = events.recv() => (frame, false),
                frame = observations.recv() => (frame, true),
            };
            let Some(sidecar) = weak.upgrade() else { break };
            let Ok(frame) = frame else {
                sidecar.client.close("RPC event subscriber lost continuity");
                break;
            };
            let current = sidecar.thread.lock().clone();
            if discovery {
                discover(&svc, &tab, &sidecar, &current, &frame).await;
                continue;
            }
            if let Frame::ServerRequest { id, method, params } = frame {
                if params.get("threadId").and_then(Value::as_str) == Some(&current) {
                    super::approvals::receive(&svc, &tab, id, method, params);
                    if super::approvals::pending(&tab).is_some() {
                        let pty = svc
                            .mux
                            .read()
                            .get_tab(&tab)
                            .and_then(|tab| tab.pty_id.clone());
                        if let Some(pty) = pty {
                            svc.agent_tracker.lock().set_waiting(&pty);
                        }
                        crate::agent::state_verdict::observe_declared_event(
                            &svc,
                            &tab,
                            &current,
                            "serverRequest/pending",
                        );
                    }
                } else {
                    let _ = sidecar.client.reply(
                        &id,
                        Err(super::protocol::RpcError {
                            code: -32601,
                            message: "Thread is not controlled by this tab".into(),
                            data: None,
                        }),
                    );
                }
                continue;
            }
            let Frame::Notification {
                ref method,
                ref params,
            } = frame
            else {
                continue;
            };
            if method == "account/login/completed" {
                if let Ok(response) = sidecar
                    .client
                    .request("account/read", json!({"refreshToken":false}))
                    .await
                {
                    let auth = super::auth::account_state(&response);
                    *sidecar.auth.lock() = auth.clone();
                    svc.notify_event("codex-rpc-auth", json!({"tab_id":tab,"auth":auth}));
                }
            }
            if params.get("threadId").and_then(Value::as_str) != Some(&current) {
                continue;
            }
            if let Some(message) = normalizer.apply(&frame) {
                super::transcript::emit(&svc, &tab, &current, message);
            }
            if method == "turn/started" {
                if let Some(turn) = super::control::turn_id(params) {
                    sidecar.control.lock().started(&current, turn);
                }
                sidecar.busy.store(true, Ordering::Release);
                svc.mark_agent_turn_started(&tab);
                crate::agent::state_verdict::observe_declared_event(&svc, &tab, &current, method);
            }
            if method == "turn/completed" {
                let first = super::control::turn_id(params)
                    .is_some_and(|turn| sidecar.control.lock().completed(&current, turn));
                if first {
                    sidecar.busy.store(false, Ordering::Release);
                    svc.mark_agent_turn_complete(&tab);
                    sidecar
                        .turn_settled
                        .send_modify(|generation| *generation = generation.wrapping_add(1));
                    crate::agent::state_verdict::observe_declared_event(
                        &svc, &tab, &current, method,
                    );
                    let interrupted = super::control::turn_id(params)
                        .is_some_and(|turn| sidecar.control.lock().was_interrupted(&current, turn))
                        || params.pointer("/turn/status").and_then(Value::as_str)
                            == Some("interrupted");
                    if interrupted {
                        let pty = svc
                            .mux
                            .read()
                            .get_tab(&tab)
                            .and_then(|tab| tab.pty_id.clone());
                        if let Some(pty) = pty {
                            svc.mark_agent_interrupted(&pty);
                        }
                    }
                    super::control::settled(&svc, &tab);
                    crate::service::provider_update::tab_became_idle(&svc, &tab);
                }
            }
            if method == "serverRequest/resolved" {
                if let Some(id) = params
                    .get("requestId")
                    .and_then(|id| serde_json::from_value(id.clone()).ok())
                {
                    super::approvals::resolved(&svc, &tab, &id);
                    let event = if super::approvals::pending(&tab).is_some() {
                        "serverRequest/pending"
                    } else if sidecar.busy.load(Ordering::Acquire) {
                        "turn/started"
                    } else {
                        "turn/completed"
                    };
                    crate::agent::state_verdict::observe_declared_event(
                        &svc, &tab, &current, event,
                    );
                }
            }
            if method == "thread/settings/updated" {
                super::model::observe(&svc, &tab, &current, params);
            }
            if method == "item/completed" {
                let injection = params.get("item").and_then(|item| {
                    sidecar.compaction.lock().injection(
                        &current,
                        item,
                        sidecar
                            .thread_settings
                            .get("developerInstructions")
                            .and_then(Value::as_str),
                    )
                });
                if let Some(injection) = injection {
                    if let Err(error) = sidecar
                        .client
                        .request("thread/inject_items", injection)
                        .await
                    {
                        sidecar
                            .client
                            .close(&format!("Contract reinjection failed: {error:?}"));
                    }
                }
            }
        }
        if weak.upgrade().is_some_and(|sidecar| {
            super::lifecycle::for_tab(&tab).is_some_and(|current| Arc::ptr_eq(&sidecar, &current))
        }) {
            super::lifecycle::clear_tab(&tab);
            svc.push_structured_prompt(&tab, None);
            if let Some(tab) = svc.mux.write().get_tab_mut(&tab) {
                tab.structured_control_active = false;
            }
            svc.emit_state_changed();
        }
    });
}

async fn discover(
    svc: &ServiceContext,
    tab: &str,
    sidecar: &Sidecar,
    current: &str,
    frame: &Frame,
) {
    let Frame::Notification { method, params } = frame else {
        return;
    };
    let candidate = if method == "thread/started" {
        params.pointer("/thread/id").and_then(Value::as_str)
    } else {
        params.get("threadId").and_then(Value::as_str)
    };
    let Some(candidate) = candidate.filter(|id| *id != current) else {
        return;
    };
    if sidecar.control.lock().parents.contains_key(candidate) {
        if let Some(turn) = super::control::turn_id(params) {
            if method == "turn/started" {
                sidecar.control.lock().started(candidate, turn);
            }
            if method == "turn/completed" {
                sidecar.control.lock().completed(candidate, turn);
                sidecar
                    .turn_settled
                    .send_modify(|generation| *generation = generation.wrapping_add(1));
            }
        }
        return;
    }
    let Ok(read) = sidecar
        .client
        .request(
            "thread/read",
            json!({"threadId":candidate,"includeTurns":false}),
        )
        .await
    else {
        return;
    };
    if !verified_user_thread(candidate, &read) {
        if let Some(parent) = read
            .pointer("/thread/parentThreadId")
            .and_then(Value::as_str)
        {
            sidecar
                .control
                .lock()
                .parents
                .insert(candidate.into(), parent.into());
        }
        if method == "turn/started" {
            if let Some(turn) = super::control::turn_id(params) {
                sidecar.control.lock().started(candidate, turn);
            }
        }
        sidecar.client.release_thread(candidate);
        return;
    }
    sidecar.verified.store(false, Ordering::Release);
    let mut settings = sidecar.thread_settings.clone();
    settings["threadId"] = json!(candidate);
    settings["excludeTurns"] = json!(true);
    match sidecar.client.request("thread/resume", settings).await {
        Ok(_) => {
            super::approvals::clear_tab(tab);
            svc.push_structured_prompt(tab, None);
            *sidecar.thread.lock() = candidate.into();
            sidecar.busy.store(false, Ordering::Release);
            super::lifecycle::bind(svc, tab, candidate.into());
            sidecar.client.release_thread(candidate);
            sidecar.verified.store(true, Ordering::Release);
            svc.wake_parked_user_send(tab);
            svc.wake_agent_deliveries_on_state_change(tab);
        }
        Err(error) => sidecar
            .client
            .close(&format!("Thread subscription failed: {error:?}")),
    }
}
