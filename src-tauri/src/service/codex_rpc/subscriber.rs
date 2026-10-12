use super::{lifecycle::Sidecar, protocol::Frame, routing::verified_user_thread};
use crate::service::ServiceContext;
use serde_json::{json, Value};
use std::sync::{atomic::Ordering, Arc};

pub fn start(svc: ServiceContext, tab: String, sidecar: &Arc<Sidecar>) {
    let weak = Arc::downgrade(sidecar);
    let mut events = sidecar.client.subscribe();
    let mut observations = sidecar.client.observe();
    let mut death = sidecar.client.death();
    let mut resync = sidecar.client.resync();
    let mut process_death = sidecar.process.death.clone();
    sidecar.client.release_thread(&sidecar.thread.lock());
    let (discovery_tx, mut discovery_rx) = tokio::sync::mpsc::channel::<Frame>(128);
    let (discovery_svc, discovery_tab, discovery_weak) = (svc.clone(), tab.clone(), weak.clone());
    tauri::async_runtime::spawn(async move {
        while let Some(frame) = discovery_rx.recv().await {
            let Some(sidecar) = discovery_weak.upgrade() else {
                break;
            };
            if !owns_endpoint(&discovery_tab, &sidecar) {
                break;
            }
            let current = sidecar.thread.lock().clone();
            discover(&discovery_svc, &discovery_tab, &sidecar, &current, &frame).await;
        }
    });
    tauri::async_runtime::spawn(async move {
        loop {
            let (frame, discovery) = tokio::select! {
                _ = death.changed() => {
                    if death.borrow().is_none() { continue; }
                    let Some(sidecar) = weak.upgrade() else { break };
                    if !owns_endpoint(&tab, &sidecar) { break; }
                    match super::lifecycle::reconnect(&tab, &sidecar).await {
                        Ok(()) => {
                            resynchronize(&svc, &tab, &sidecar).await;
                            super::delivery::refresh(&svc, &tab);
                            continue;
                        },
                        Err(error) => { log::error!("[codex-rpc] tab={tab} reconnect failed: {error}"); break; }
                    }
                },
                _ = process_death.changed() => break,
                _ = resync.changed() => {
                    let Some(sidecar) = weak.upgrade() else { break };
                    resynchronize(&svc, &tab, &sidecar).await;
                    continue;
                },
                frame = events.recv() => (frame, false),
                frame = observations.recv() => (frame, true),
            };
            let Some(sidecar) = weak.upgrade() else { break };
            if !owns_endpoint(&tab, &sidecar) {
                break;
            }
            let frame = match frame {
                Ok(frame) => frame,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    resynchronize(&svc, &tab, &sidecar).await;
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let current = sidecar.thread.lock().clone();
            if discovery {
                if discovery_tx.try_send(frame).is_err() {
                    resynchronize(&svc, &tab, &sidecar).await;
                }
                continue;
            }
            if let Frame::ServerRequest { id, method, params } = frame {
                let owned = match params.get("threadId").and_then(Value::as_str) {
                    Some(thread) => owned_request(&sidecar, &current, thread).await,
                    None => false,
                };
                if owned {
                    super::approvals::receive(&svc, &tab, id, method, params);
                    if super::approvals::pending(&tab).is_some() {
                        svc.mark_agent_waiting(&tab);
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
            if matches!(
                method.as_str(),
                "turn/started" | "turn/completed" | "thread/status/changed"
            ) {
                log::debug!("[rpc/status] tab={tab} bound_thread={current} notification={method} thread={} turn={:?}",
                    params["threadId"], super::control::turn_id(params));
            }
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
            if method == "serverRequest/resolved"
                && params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .is_some_and(|thread| sidecar.control.lock().owns_thread(&current, thread))
            {
                if let Some(id) = params
                    .get("requestId")
                    .and_then(|id| serde_json::from_value(id.clone()).ok())
                {
                    super::approvals::resolved(&svc, &tab, &id);
                    refresh_request_state(&svc, &tab, &current, &sidecar);
                }
            }
            if params.get("threadId").and_then(Value::as_str) != Some(&current) {
                continue;
            }
            if let Some(observation) = super::auth::session_observation(method, params) {
                let nonce = svc
                    .mux
                    .read()
                    .get_tab(&tab)
                    .and_then(|tab| tab.session_binding_nonce.clone());
                if let Some(nonce) = nonce {
                    crate::service::provider_signals::apply(
                        &svc,
                        &tab,
                        &current,
                        &nonce,
                        &[crate::agent::chat_types::RichMessage {
                            provider_session_id: Some(current.clone()),
                            provider_observations: Some(observation),
                            ..Default::default()
                        }],
                    );
                }
            }
            if method == "thread/status/changed" {
                if let Some(event) = super::status::declared_event(&params["status"]) {
                    super::status::observe(&svc, &tab, &current, &sidecar, event);
                }
            }
            if let Some((thread, client)) = super::send::user_client_id(method, params) {
                if let Some(turn) = params.get("turnId").and_then(Value::as_str) {
                    let target = svc
                        .mux
                        .read()
                        .get_tab(&tab)
                        .filter(|tab| {
                            tab.agent_session_id.as_deref() == Some(thread)
                                && tab.launch_session_authenticated()
                        })
                        .and_then(|tab| {
                            Some((tab.session_binding_nonce.clone()?, tab.agent_type.clone()?))
                        });
                    if let Some((nonce, provider)) = target {
                        svc.chat_send_ledger
                            .trace_rpc_turn(&tab, thread, &nonce, client, turn, &provider);
                    }
                }
            }
            let messages = sidecar.normalizer.lock().apply_all(&frame);
            for message in messages {
                if method == "item/completed" && crate::flow_trace::enabled() {
                    let source = svc
                        .mux
                        .read()
                        .get_tab(&tab)
                        .filter(|tab| {
                            tab.agent_session_id.as_deref() == Some(&current)
                                && tab.launch_session_authenticated()
                        })
                        .and_then(|tab| {
                            Some((tab.agent_type.clone()?, tab.session_binding_nonce.clone()?))
                        });
                    if let Some((provider, nonce)) = source {
                        crate::flow_trace::SourceCursor::rpc(
                            &tab,
                            &provider,
                            &current,
                            Some(&nonce),
                            &message,
                        );
                    }
                }
                super::transcript::emit(&svc, &tab, &current, message);
            }
            if method == "turn/started" {
                let started = super::control::turn_id(params)
                    .is_some_and(|turn| sidecar.control.lock().started(&current, turn));
                if !started {
                    continue;
                }
                super::model::on_turn_started(&svc, &tab, &current);
                super::status::observe(&svc, &tab, &current, &sidecar, method);
            }
            if method == "turn/completed" {
                let first = super::control::turn_id(params)
                    .is_some_and(|turn| sidecar.control.lock().completed(&current, turn));
                if first {
                    super::status::observe(&svc, &tab, &current, &sidecar, method);
                    let nonce = svc
                        .mux
                        .read()
                        .get_tab(&tab)
                        .and_then(|tab| tab.session_binding_nonce.clone());
                    if let Some(nonce) = nonce {
                        svc.staged_model_turn_completed(&tab, &current, &nonce);
                    }
                    sidecar
                        .turn_settled
                        .send_modify(|generation| *generation = generation.wrapping_add(1));
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
                    svc.wake_parked_user_send(&tab);
                    crate::service::provider_update::tab_became_idle(&svc, &tab);
                }
            }
            if method == "thread/settings/updated" {
                super::model::observe(&svc, &tab, &current, params);
            }
            if method == "item/completed" {
                let context = sidecar.client.additional_context(&current);
                sidecar.compaction.lock().additional_context.extend(context);
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
                        sidecar.verified.store(false, Ordering::Release);
                        log::error!("[codex-rpc] tab={tab} contract reinjection failed: {error:?}; sends held");
                    }
                }
            }
        }
        if let Some(sidecar) = weak
            .upgrade()
            .filter(|sidecar| owns_endpoint(&tab, sidecar))
        {
            sidecar.verified.store(false, Ordering::Release);
            super::delivery::refresh(&svc, &tab);
            log::error!(
                "[codex-rpc] tab={tab} control unavailable; server retained and sends held"
            );
            svc.emit_state_changed();
        }
    });
}

async fn resynchronize(svc: &ServiceContext, tab: &str, sidecar: &Sidecar) {
    let thread = sidecar.thread.lock().clone();
    match super::transcript::history(&sidecar.client, &thread).await {
        Ok(history) if owns_endpoint(tab, sidecar) && *sidecar.thread.lock() == thread => {
            *sidecar.normalizer.lock() =
                super::normalize::Normalizer::from_history(&thread, history.messages.clone());
            sidecar.control.lock().restore(
                &thread,
                history.active_turn.as_deref(),
                &history.completed_turns,
            );
            sidecar
                .busy
                .store(history.active_turn.is_some(), Ordering::Release);
            for message in history.messages {
                super::transcript::emit(svc, tab, &thread, message);
            }
            refresh_request_state(svc, tab, &thread, sidecar);
        }
        Err(error) => log::error!("[codex-rpc] tab={tab} resync failed: {error}; server retained"),
        _ => {}
    }
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
    if method != "thread/started" && sidecar.control.lock().retired_roots.contains(candidate) {
        return;
    }
    if sidecar.control.lock().owns_thread(current, candidate) {
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
        let Some(parent) = read
            .pointer("/thread/parentThreadId")
            .and_then(Value::as_str)
            .filter(|parent| sidecar.control.lock().owns_thread(current, parent))
        else {
            sidecar.client.release_thread(candidate);
            return;
        };
        {
            sidecar
                .control
                .lock()
                .parents
                .insert(candidate.into(), parent.into());
        }
        if method == "turn/completed" {
            if let Some(turn) = super::control::turn_id(params) {
                sidecar.control.lock().completed(candidate, turn);
                sidecar
                    .turn_settled
                    .send_modify(|generation| *generation = generation.wrapping_add(1));
            }
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
            if !owns_endpoint(tab, sidecar) {
                return;
            }
            super::approvals::clear_tab(tab);
            svc.push_structured_prompt(tab, None);
            let history = match super::transcript::history(&sidecar.client, candidate).await {
                Ok(history) => history,
                Err(error) => {
                    log::error!("[codex-rpc] tab={tab} discovery history failed: {error}");
                    return;
                }
            };
            if !owns_endpoint(tab, sidecar) {
                return;
            }
            *sidecar.normalizer.lock() =
                super::normalize::Normalizer::from_history(candidate, history.messages);
            sidecar.control.lock().retired_roots.insert(current.into());
            sidecar.compaction.lock().additional_context.clear();
            sidecar.control.lock().restore(
                candidate,
                history.active_turn.as_deref(),
                &history.completed_turns,
            );
            *sidecar.thread.lock() = candidate.into();
            sidecar
                .busy
                .store(history.active_turn.is_some(), Ordering::Release);
            super::lifecycle::bind(svc, tab, candidate.into());
            refresh_request_state(svc, tab, candidate, sidecar);
            sidecar.client.release_thread(candidate);
            sidecar.verified.store(true, Ordering::Release);
            super::delivery::refresh(svc, tab);
        }
        Err(error) => {
            log::error!("[codex-rpc] tab={tab} subscription failed: {error:?}; server retained")
        }
    }
}

async fn owned_request(sidecar: &Sidecar, root: &str, thread: &str) -> bool {
    if sidecar.control.lock().owns_thread(root, thread) {
        return true;
    }
    let Ok(read) = sidecar
        .client
        .request(
            "thread/read",
            json!({"threadId":thread,"includeTurns":false}),
        )
        .await
    else {
        return false;
    };
    let Some(parent) = read
        .pointer("/thread/parentThreadId")
        .and_then(Value::as_str)
    else {
        return false;
    };
    let mut control = sidecar.control.lock();
    if !control.owns_thread(root, parent) {
        return false;
    }
    control.parents.insert(thread.into(), parent.into());
    drop(control);
    sidecar.client.release_thread(thread);
    true
}

fn refresh_request_state(svc: &ServiceContext, tab: &str, root: &str, sidecar: &Sidecar) {
    let pending = super::approvals::pending(tab).is_some();
    let event = if pending {
        svc.mark_agent_waiting(tab);
        "serverRequest/pending"
    } else if sidecar.busy.load(Ordering::Acquire) {
        svc.mark_agent_turn_started(tab);
        "turn/started"
    } else {
        svc.mark_agent_turn_complete(tab);
        "turn/completed"
    };
    crate::agent::state_verdict::observe_declared_event(svc, tab, root, event);
}

fn owns_endpoint(tab: &str, sidecar: &Sidecar) -> bool {
    super::lifecycle::for_tab(tab)
        .is_some_and(|current| std::ptr::eq(Arc::as_ref(&current), sidecar))
}
