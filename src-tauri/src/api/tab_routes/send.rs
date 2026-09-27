use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_tab};
use crate::service::delivery_error::{DeliveryError, PayloadTooLarge, SendOrigin};

use super::AppState;

pub async fn chat_send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SendTextBody>,
) -> Result<Json<crate::service::agent_pad::ChatSendResult>, SendRejection> {
    check_auth(&state, &headers).map_err(SendRejection::from)?;
    let tab_id = resolve_tab(&state, &id).map_err(SendRejection::from)?;
    let has_agent_identity = state.svc.tab_has_agent_identity(&tab_id);
    if let Some(error) = chat_send_target_refusal(
        &tab_id,
        has_agent_identity,
        state.svc.tab_chat_backing(&tab_id).is_some(),
    ) {
        return Err(error.into());
    }
    let (text, pad_file) = state
        .svc
        .substitute_paste_limited_body(&tab_id, body.text.unwrap_or_default())
        .map_err(|detail| pad_write_refusal(&tab_id, detail))?;
    let run_stages = crate::chatpipeline::is_user_turn_send(false, false, has_agent_identity);
    let text = if run_stages && pad_file.is_none() {
        state.svc.run_outbound_stages(&tab_id, text)
    } else {
        text
    };
    let submit_claim = state.svc.claim_chat_submit_cycle(&tab_id);
    let client_token = body.client_token;
    let svc = state.svc.clone();
    let send_tab = tab_id.clone();
    // Anchor capture reads the session file and may enter a plugin VM.
    let status = tokio::task::spawn_blocking(move || {
        svc.start_chat_send(&send_tab, text, submit_claim, client_token)
    })
    .await
    .map_err(|_| SendRejection::from((StatusCode::INTERNAL_SERVER_ERROR, "chat send task failed")))?
    .map_err(|detail| {
        DeliveryError::TabBusyRefused {
            tab: tab_id,
            detail,
        }
        .into()
    })?;
    Ok(Json(crate::service::agent_pad::ChatSendResult::new(
        status, pad_file,
    )))
}

fn chat_send_target_refusal(
    tab_id: &str,
    has_agent_identity: bool,
    has_chat_backend: bool,
) -> Option<DeliveryError> {
    if has_agent_identity || has_chat_backend {
        return None;
    }
    Some(DeliveryError::TabBusyRefused {
        tab: tab_id.to_string(),
        detail: "tab has no agent identity; refusing to send prose to a shell".into(),
    })
}

pub async fn chat_send_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::service::chat_send_transaction::ChatSendStatus>>, SendRejection> {
    check_auth(&state, &headers).map_err(SendRejection::from)?;
    let tab_id = resolve_tab(&state, &id).map_err(SendRejection::from)?;
    let svc = state.svc.clone();
    // Ledger read doubles as the closed-chat reconciliation point: scan each
    // unconfirmed send's own session append window before reporting state.
    let statuses = tokio::task::spawn_blocking(move || {
        svc.reconcile_chat_send_evidence(&tab_id);
        svc.chat_send_ledger.reconcile(&tab_id)
    })
    .await
    .map_err(|_| {
        SendRejection::from((StatusCode::INTERNAL_SERVER_ERROR, "chat status task failed"))
    })?;
    Ok(Json(statuses))
}

/// Classify a failed PTY delivery for the caller (local or mesh relay). The tab
/// resolved before any write was attempted, so a failure here is never "missing".
fn classify_delivery_failure(tab: &str, err: &str) -> DeliveryError {
    DeliveryError::from_opaque(tab, err)
}

fn pad_write_refusal(tab: &str, detail: String) -> SendRejection {
    DeliveryError::TabBusyRefused {
        tab: tab.to_string(),
        detail: format!(
            "long message was not sent because recipient AgentPad write failed: {detail}"
        ),
    }
    .into()
}

/// Error surface for `/send`. Delivery failures serialize the structured
/// taxonomy so the mesh relay can forward `code` + `bytes_written` verbatim
/// instead of collapsing them into a bare status line.
pub struct SendRejection {
    status: StatusCode,
    body: serde_json::Value,
}

impl IntoResponse for SendRejection {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

impl From<(StatusCode, &'static str)> for SendRejection {
    fn from((status, message): (StatusCode, &'static str)) -> Self {
        Self {
            status,
            body: serde_json::json!({
                "ok": false,
                "error": {"code": "bad_request", "message": message},
            }),
        }
    }
}

impl From<DeliveryError> for SendRejection {
    fn from(err: DeliveryError) -> Self {
        Self {
            status: err.status(),
            body: err.to_json(),
        }
    }
}

impl From<PayloadTooLarge> for SendRejection {
    fn from(err: PayloadTooLarge) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            body: err.to_json(),
        }
    }
}

/// Route-level origin from the existing request markers: a service push token,
/// a sender tab, or a mesh peer classifies the send as programmatic.
fn route_send_origin(
    is_service_push: bool,
    has_source_tab: bool,
    has_source_peer: bool,
) -> SendOrigin {
    if is_service_push || has_source_tab || has_source_peer {
        SendOrigin::Programmatic
    } else {
        SendOrigin::User
    }
}

/// Byte length of everything this send would write: plain/raw text plus batch
/// bodies. Length only — the content is never inspected.
fn route_payload_bytes(text: Option<&str>, batch: Option<&[BatchCommand]>) -> usize {
    text.map_or(0, str::len)
        + batch.map_or(0, |items| items.iter().map(|c| c.text.len()).sum::<usize>())
}

pub(super) use self::await_pty_link as wait_for_pty_link;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabInputTransport {
    Pty,
    ChatBackend,
}

/// Whether to inject the sender return-address marker into the PTY text. The
/// marker is meaningful only to an AGENT reading its input; a bare shell runs
/// the bytes verbatim and a chatBackend carries identity structurally.
pub(crate) fn should_prepend_source(
    has_source: bool,
    target_has_agent: bool,
    transport: TabInputTransport,
) -> bool {
    has_source && target_has_agent && transport == TabInputTransport::Pty
}

/// A return address describes a message, and a keystroke-only send carries no
/// message. Wrapping its empty body produced a marker with nothing to introduce,
/// which the target then received as the payload of the keystroke itself.
pub(crate) fn prefixes_return_address(body: &str, prefix_source: bool) -> bool {
    prefix_source && !body.is_empty()
}

#[derive(Deserialize)]
pub(crate) struct SendTextBody {
    pub(crate) text: Option<String>,
    /// Terminal key/action sequence. Enter submits optional text through the
    /// acknowledged transaction; navigation keys and delays remain ordered.
    /// A plain text send always submits — use `actions` (no Enter) or `raw` to
    /// type without submitting.
    pub(crate) actions: Option<Vec<SendAction>>,
    /// Batch: send multiple commands sequentially with optional delays
    pub(crate) batch: Option<Vec<BatchCommand>>,
    /// Raw mode: bypass bracket paste, send bytes directly to PTY.
    /// Use for keystrokes that need to be interpreted by TUI (e.g. vim j/k navigation).
    #[serde(default)]
    pub(crate) raw: bool,
    /// Sender's tab id when CLI is invoked from inside CodeTerm. Daemon
    /// prepends a CodeTerm marker so the recipient knows the return-address.
    /// Mesh hops and hierarchy reports use the same marker family.
    #[serde(default)]
    pub(crate) source_tab: Option<String>,
    /// Originating peer name, injected by the mesh relay (authenticated from
    /// the signed token — peers cannot spoof it). Present ⇒ this delivery
    /// crossed the mesh; the label is peer-qualified `label@peer`.
    #[serde(default)]
    pub(crate) source_peer: Option<String>,
    /// Source agent label + model, threaded from the originating peer (the
    /// source tab is foreign and cannot be resolved against the local mux).
    #[serde(default)]
    pub(crate) source_label: Option<String>,
    #[serde(default)]
    pub(crate) source_model: Option<String>,
    /// Delivery framing for a mesh-relayed message. `"report"` renders as
    /// `[Report from <label>@<peer>]` (a remote worker's `agent report`); any
    /// other / absent value is a plain tab message. Only honoured alongside
    /// `source_peer` so a local send can't forge report framing.
    #[serde(default)]
    pub(crate) kind: Option<String>,
    /// Opaque client correlation token. Echoed verbatim onto the committed
    /// user turn's ChatSendStatus (`correlationId`) so the optimistic bubble
    /// reconciles by identity. Never interpreted; never gates delivery.
    #[serde(default)]
    pub(crate) client_token: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BatchCommand {
    pub(crate) text: String,
    pub(crate) delay_ms: Option<u64>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SendAction {
    Key { key: String },
    Delay { ms: u64 },
}

pub(crate) use crate::agent::keys::action_key_bytes as send_action_bytes;

fn action_preview(actions: &[SendAction]) -> String {
    actions
        .iter()
        .map(|a| match a {
            SendAction::Key { key } => key.clone(),
            SendAction::Delay { ms } => format!("delay:{ms}ms"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) async fn await_pty_link(state: &AppState, tab_id: &str) -> bool {
    if state.svc.tab_chat_backing(tab_id).is_some() {
        return true;
    }
    let svc = state.svc.clone();
    let tab = tab_id.to_string();
    tokio::task::spawn_blocking(move || {
        svc.await_pty_link_blocking(&tab, std::time::Duration::from_secs(15))
            .is_ok()
    })
    .await
    .unwrap_or(false)
}

fn record_push_activity(state: &AppState, svc_id: &Option<String>, tab_id: &str, text: &str) {
    let Some(svc_id) = svc_id else { return };
    let writer = crate::service::activity::ActivityWriter::new(state.svc.services_store.conn());
    let preview = text.chars().take(200).collect::<String>();
    let _ = writer.record(crate::service::activity::ActivityEvent::push(
        svc_id, tab_id, &preview,
    ));
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "service-pushed",
        serde_json::json!({"source_id": svc_id, "target": tab_id}),
    );
}

pub async fn send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<SendTextBody>,
) -> Result<Json<serde_json::Value>, SendRejection> {
    let svc_token_match = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            crate::service::push::validate_push(v, &state.svc.tokens, &state.svc.services_store)
                .ok()
                .flatten()
        });

    if svc_token_match.is_none() {
        check_auth(&state, &headers)?;
    }
    // Reachability is the FIRST fallible step and the only one allowed to report
    // a missing tab — everything past it has resolved a real target.
    let tab_id =
        resolve_tab(&state, &raw_tab_id).map_err(|_| DeliveryError::TabNotReachableOnPeer {
            tab: raw_tab_id.clone(),
        })?;

    // Refuse over-bound programmatic payloads (16MB bound) before any delivery
    // machinery runs; user-origin sends keep today's behavior.
    let origin = route_send_origin(
        svc_token_match.is_some(),
        body.source_tab.is_some(),
        body.source_peer.as_deref().is_some_and(|p| !p.is_empty()),
    );
    crate::service::delivery_error::preflight_payload_size(
        &tab_id,
        origin,
        route_payload_bytes(body.text.as_deref(), body.batch.as_deref()),
    )?;

    let transport = if state.svc.tab_chat_backing(&tab_id).is_some() {
        TabInputTransport::ChatBackend
    } else {
        TabInputTransport::Pty
    };
    // The return-address marker is meaningful only to an AGENT reading its input.
    // A bare shell PTY must run the injected bytes verbatim — never prefix it.
    let target_has_agent = state.svc.tab_has_agent(&tab_id);

    if body.kind.as_deref() == Some("report") {
        if let (Some(peer), Some(src)) = (
            body.source_peer.as_deref().filter(|s| !s.is_empty()),
            body.source_tab.as_deref().filter(|s| !s.is_empty()),
        ) {
            let label = body
                .source_label
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(peer);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            state.svc.remote_workers.record_report(
                &tab_id,
                peer,
                src,
                label,
                body.source_model.clone(),
                body.text.as_deref().unwrap_or(""),
                now_ms,
            );
            state.svc.emit_remote_workers_changed(&tab_id);
        }
    }

    let requires_immediate_pty = body.raw || body.actions.is_some() || body.batch.is_some();
    if transport == TabInputTransport::Pty && requires_immediate_pty {
        state
            .svc
            .ensure_tab_pty_for_send(&tab_id)
            .map_err(|e| DeliveryError::PtyUnavailable {
                tab: tab_id.clone(),
                detail: e.to_string(),
            })?;

        if !await_pty_link(&state, &tab_id).await {
            return Err(DeliveryError::PtyUnavailable {
                tab: tab_id.clone(),
                detail: "PTY did not link within 15s".into(),
            }
            .into());
        }
    }

    let source_envelope: Option<crate::service::messenger::MessageEnvelope> =
        body.source_tab.as_deref().and_then(|src| {
            match body.source_peer.as_deref().filter(|p| !p.is_empty()) {
                // the body (resolved on the originating peer), peer-qualified here.
                Some(peer) => {
                    let label = body
                        .source_label
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or(peer);
                    let envelope = if body.kind.as_deref() == Some("report") {
                        crate::service::messenger::MessageEnvelope::report(label, src)
                    } else {
                        crate::service::messenger::MessageEnvelope::tab(label, src)
                    };
                    Some(
                        envelope
                            .model(body.source_model.clone())
                            .peer(Some(peer.to_string()))
                            .mesh(Some(peer.to_string())),
                    )
                }
                // Local delivery: resolve the label/model from the local mux.
                None => {
                    let mux = state.svc.mux.write();
                    let resolved = mux.resolve_tab_id(src)?;
                    let from_label = mux.display_label_for_tab(&resolved);
                    let model = mux
                        .get_tab(&resolved)
                        .and_then(|p| p.agent_session_model.clone());
                    Some(
                        crate::service::messenger::MessageEnvelope::tab(from_label, resolved)
                            .model(model),
                    )
                }
            }
        });
    let prefix_source =
        should_prepend_source(source_envelope.is_some(), target_has_agent, transport);
    let prepend = |s: &str| -> String {
        match &source_envelope {
            Some(envelope) if prefixes_return_address(s, prefix_source) => {
                crate::service::messenger::Messenger::from(envelope.clone()).wrap(s)
            }
            _ => s.to_string(),
        }
    };

    // send_text is also used by system senders and stays transport-clean.
    let run_stages = crate::chatpipeline::is_user_turn_send(
        svc_token_match.is_some(),
        body.source_tab.is_some(),
        target_has_agent,
    );
    let stage = |s: String| -> String {
        if run_stages {
            state.svc.run_outbound_stages(&tab_id, s)
        } else {
            s
        }
    };

    if let Some(actions) = body.actions {
        if actions.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "actions cannot be empty").into());
        }
        let mut ordered = Vec::with_capacity(actions.len());
        for action in &actions {
            match action {
                SendAction::Delay { ms } => {
                    ordered.push(crate::service::pty_io::OrderedPtyAction::Delay(
                        std::time::Duration::from_millis(*ms),
                    ))
                }
                SendAction::Key { key } => {
                    let Some(bytes) = send_action_bytes(key) else {
                        return Err((StatusCode::BAD_REQUEST, "unknown action key").into());
                    };
                    if bytes == b"\r" {
                        ordered.push(crate::service::pty_io::OrderedPtyAction::Submit);
                    } else {
                        ordered.push(crate::service::pty_io::OrderedPtyAction::Raw(bytes));
                    }
                }
            }
        }

        let raw_text = body.text.clone().unwrap_or_default();
        let (raw_text, pad_file) = state
            .svc
            .substitute_paste_limited_body(&tab_id, raw_text)
            .map_err(|detail| pad_write_refusal(&tab_id, detail))?;
        let raw_text = if pad_file.is_some() {
            raw_text
        } else {
            stage(raw_text)
        };
        let text = prepend(&raw_text);
        if transport == TabInputTransport::ChatBackend {
            if !raw_text.is_empty() {
                state
                    .svc
                    .stage_text_when_ready(&tab_id, &text)
                    .await
                    .map_err(|e| classify_delivery_failure(&tab_id, &e))?;
            }
        } else {
            let svc = state.svc.clone();
            let action_tab = tab_id.clone();
            let bytes_written = std::cell::Cell::new(false);
            let action_result = tokio::task::spawn_blocking(move || {
                let provider_action_refusal = || {
                    let target = {
                        let mux = svc.mux.read();
                        mux.get_tab(&action_tab).and_then(|tab| {
                            let pty_id = tab.pty_id.as_deref()?;
                            if mux.tab_id_for_pty(pty_id)? != action_tab.as_str() {
                                return None;
                            }
                            Some((
                                pty_id.to_string(),
                                tab.agent_type.clone()?,
                                tab.agent_session_id.clone(),
                                tab.cwd.clone().or_else(|| tab.provisioned_cwd.clone()),
                            ))
                        })
                    };
                    target.and_then(|(pty_id, agent_type, session_id, cwd)| {
                        let screen = svc.screen_state.lock().get_screen_text(&pty_id)?;
                        svc.agent_registry
                            .with_provider(&agent_type, |provider| {
                                use codeterm_plugin::tab_state::TabState;

                                let snapshot = codeterm_plugin::tab_state::TabStateSnapshot {
                                    tab_id: action_tab.clone(),
                                    agent_type: agent_type.clone(),
                                    session_id,
                                    cwd,
                                    screen_text: screen.clone(),
                                };
                                let classification = provider.classify_tab_state(&snapshot);
                                let high_confidence =
                                    classification.as_ref().is_some_and(|value| {
                                        value.confidence
                                            >= crate::agent::state_reactor::GLOBAL_THRESHOLD
                                    });
                                let structured_choice = high_confidence
                                    && classification.as_ref().is_some_and(|value| {
                                        value.state == TabState::ClarifyingQuestion
                                    })
                                    && provider.parse_prompt(&screen).is_some();
                                let update_dialog = high_confidence
                                    && classification
                                        .as_ref()
                                        .is_some_and(|value| value.state == TabState::UpdatePrompt);
                                if provider.directory_trust_prompt_response(&screen).is_some() {
                                    Some("provider trust dialog is visible".to_string())
                                } else if update_dialog {
                                    Some("provider update dialog is visible".to_string())
                                } else if structured_choice {
                                    Some("provider structured-choice dialog is visible".to_string())
                                } else {
                                    None
                                }
                            })
                            .flatten()
                    })
                };
                let action_refusal = || {
                    let detail = provider_action_refusal()?;
                    log::info!(
                        "[send-actions] refused tab={} detail={}",
                        action_tab,
                        detail
                    );
                    Some(if bytes_written.get() {
                        DeliveryError::SubmitUnconfirmed {
                            tab: action_tab.clone(),
                            detail,
                        }
                    } else {
                        DeliveryError::TabBusyRefused {
                            tab: action_tab.clone(),
                            detail,
                        }
                    })
                };
                if let Some(error) = action_refusal() {
                    return Err(error);
                }
                crate::service::pty_io::execute_ordered_actions(
                    &text,
                    &ordered,
                    |body, enter| {
                        if enter {
                            if let Some(error) = action_refusal() {
                                return Err(error);
                            }
                        }
                        match svc.send_text_input_delivery(&action_tab, body, enter, None) {
                            Ok(()) => {
                                bytes_written.set(bytes_written.get() || !body.is_empty() || enter);
                                Ok(())
                            }
                            Err(error) if enter && !error.bytes_written() => {
                                if let Some(action_error) = action_refusal() {
                                    Err(action_error)
                                } else if bytes_written.get() {
                                    Err(DeliveryError::SubmitUnconfirmed {
                                        tab: action_tab.clone(),
                                        detail: error.to_string(),
                                    })
                                } else {
                                    Err(error)
                                }
                            }
                            Err(error) if bytes_written.get() && !error.bytes_written() => {
                                Err(DeliveryError::SubmitUnconfirmed {
                                    tab: action_tab.clone(),
                                    detail: error.to_string(),
                                })
                            }
                            Err(error) => Err(error),
                        }
                    },
                    |bytes| {
                        if let Some(error) = action_refusal() {
                            return Err(error);
                        }
                        svc.write_raw(&action_tab, bytes)
                            .map_err(|error| {
                                if bytes_written.get() {
                                    DeliveryError::SubmitUnconfirmed {
                                        tab: action_tab.clone(),
                                        detail: error,
                                    }
                                } else {
                                    DeliveryError::from_opaque(&action_tab, error)
                                }
                            })
                            .map(|()| bytes_written.set(bytes_written.get() || !bytes.is_empty()))
                    },
                    crate::service::pty_io::await_action_deadline,
                )
            })
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "action delivery task failed",
                )
            })?;
            action_result?;
        }

        let preview = if raw_text.is_empty() {
            format!("<actions:{}>", action_preview(&actions))
        } else {
            format!("{} <actions:{}>", raw_text, action_preview(&actions))
        };
        record_push_activity(&state, &svc_token_match, &tab_id, &preview);
        return Ok(Json(serde_json::json!({
            "ok": true,
            "actions": true,
            "delivery": "sent",
            "pad_file": pad_file.map(|path| path.display().to_string())
        })));
    }

    if let Some(batch) = body.batch {
        let preview = batch
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ; ");
        let mut pad_files = Vec::new();
        let mut items = Vec::with_capacity(batch.len());
        for chunk in batch {
            let (text, pad_file) = state
                .svc
                .substitute_paste_limited_body(&tab_id, chunk.text)
                .map_err(|detail| pad_write_refusal(&tab_id, detail))?;
            if let Some(path) = pad_file {
                pad_files.push(path.display().to_string());
                items.push(crate::service::pty_io::SendBatchItem {
                    text: prepend(&text),
                    delay_ms: chunk.delay_ms,
                });
            } else {
                items.push(crate::service::pty_io::SendBatchItem {
                    text: prepend(&stage(text)),
                    delay_ms: chunk.delay_ms,
                });
            }
        }
        let svc = state.svc.clone();
        let batch_tab = tab_id.clone();
        tokio::task::spawn_blocking(move || svc.send_text_batch(&batch_tab, items))
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "batch delivery task failed",
                )
            })?
            .map_err(|e| classify_delivery_failure(&tab_id, &e))?;
        record_push_activity(&state, &svc_token_match, &tab_id, &preview);
        return Ok(Json(serde_json::json!({
            "ok": true,
            "batch": true,
            "delivery": "sent",
            "pad_files": pad_files
        })));
    }

    if body.raw {
        // Byte-for-byte to the PTY (bracket paste + sender prefix skipped, for
        // TUI keystrokes like vim j/k). The caller includes \r to submit.
        let raw_text = body.text.clone().unwrap_or_default();
        state
            .svc
            .write_raw(&tab_id, raw_text.as_bytes())
            .map_err(|e| DeliveryError::PtyUnavailable {
                tab: tab_id.clone(),
                detail: e.to_string(),
            })?;
        record_push_activity(&state, &svc_token_match, &tab_id, &raw_text);
        return Ok(Json(serde_json::json!({
            "ok": true,
            "delivery": "sent"
        })));
    }

    let raw_text = body.text.unwrap_or_default();
    if source_envelope.is_some()
        && body
            .source_peer
            .as_deref()
            .is_some_and(|peer| !peer.is_empty())
        && transport == TabInputTransport::Pty
    {
        let (raw_text, pad_file) = state
            .svc
            .substitute_paste_limited_body(&tab_id, raw_text)
            .map_err(|detail| pad_write_refusal(&tab_id, detail))?;
        let text = prepend(&raw_text);
        state.svc.send_text_when_ready(&tab_id, &text).await?;
        record_push_activity(&state, &svc_token_match, &tab_id, &raw_text);
        return Ok(Json(serde_json::json!({
            "ok": true,
            "mesh": true,
            "delivery": "sent",
            "pad_file": pad_file.map(|path| path.display().to_string())
        })));
    }
    if let Some(envelope) = source_envelope {
        let (raw_text, pad_file) = state
            .svc
            .substitute_paste_limited_body(&tab_id, raw_text)
            .map_err(|detail| pad_write_refusal(&tab_id, detail))?;
        crate::service::messenger::Messenger::deliver(&state.svc, &tab_id, envelope, &raw_text)
            .map_err(|e| classify_delivery_failure(&tab_id, &e))?;
        record_push_activity(&state, &svc_token_match, &tab_id, &raw_text);
        return Ok(Json(serde_json::json!({
            "ok": true,
            "delivery": "pending",
            "pad_file": pad_file.map(|path| path.display().to_string())
        })));
    }

    let (raw_text, pad_file) = state
        .svc
        .substitute_paste_limited_body(&tab_id, raw_text)
        .map_err(|detail| pad_write_refusal(&tab_id, detail))?;
    let raw_text = if pad_file.is_some() {
        raw_text
    } else {
        stage(raw_text)
    };
    let text = prepend(&raw_text);
    // A plain text send always submits — no-Enter/keystroke cases go through
    // `actions` or `raw`.
    state.svc.send_text_when_ready(&tab_id, &text).await?;
    record_push_activity(&state, &svc_token_match, &tab_id, &text);
    Ok(Json(serde_json::json!({
        "ok": true,
        "delivery": "sent",
        "pad_file": pad_file.map(|path| path.display().to_string())
    })))
}

#[cfg(test)]
mod delivery_classification_tests {
    use super::{chat_send_target_refusal, classify_delivery_failure, SendRejection};
    use crate::service::delivery_error::DeliveryError;
    use axum::http::StatusCode;

    /// #240: a submit that failed AFTER the paste reached the PTY must never be
    /// reported as a missing tab — the caller would retry onto parked text.
    #[test]
    fn submit_failure_is_not_reported_as_missing_tab() {
        let err = classify_delivery_failure(
            "53854ac8",
            "submit not confirmed after 3s; screen_tail: \"> parked body\"",
        );
        assert_eq!(err.code(), "submit_unconfirmed");
        assert!(err.bytes_written());
    }

    #[test]
    fn booting_tab_refusal_is_a_zero_byte_busy_error() {
        let typed = DeliveryError::TabBusyRefused {
            tab: "53854ac8".into(),
            detail: "Agent in Tab 53854ac8 is still booting — input would be lost".into(),
        };
        let err = classify_delivery_failure("53854ac8", &typed.ledger_message());
        assert_eq!(err.code(), "tab_busy_refused");
        assert!(!err.bytes_written());
    }

    #[test]
    fn pre_write_refusal_is_never_submit_unconfirmed() {
        let typed = DeliveryError::TabBusyRefused {
            tab: "53854ac8".into(),
            detail: "provider-classified update dialog is visible".into(),
        };
        let err = classify_delivery_failure("53854ac8", &typed.ledger_message());

        assert_eq!(err.code(), "tab_busy_refused");
        assert_eq!(err.status(), StatusCode::CONFLICT);
        assert!(!err.bytes_written());
        let rejection = SendRejection::from(err);
        assert_eq!(rejection.status, StatusCode::CONFLICT);
        assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
        assert_eq!(rejection.body["error"]["bytes_written"], false);
        assert!(rejection.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("nothing written"));
    }

    #[test]
    fn written_payload_failure_reports_submit_unconfirmed_with_bytes_written() {
        let rejection = SendRejection::from(DeliveryError::SubmitUnconfirmed {
            tab: "53854ac8".into(),
            detail: "provider modal appeared before recovery Enter".into(),
        });

        assert_eq!(rejection.status, StatusCode::BAD_GATEWAY);
        assert_eq!(rejection.body["error"]["code"], "submit_unconfirmed");
        assert_eq!(rejection.body["error"]["bytes_written"], true);
    }

    #[test]
    fn chat_send_to_shell_without_agent_identity_refuses_without_writing() {
        let error = chat_send_target_refusal("shell-tab", false, false).unwrap();
        let rejection = SendRejection::from(error);

        assert_eq!(rejection.status, StatusCode::CONFLICT);
        assert_eq!(rejection.body["ok"], false);
        assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
        assert_eq!(rejection.body["error"]["bytes_written"], false);
        assert!(rejection.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("nothing written"));
    }

    #[test]
    fn chat_send_target_guard_allows_booting_agent_identity_or_chat_backend() {
        assert!(chat_send_target_refusal("booting-agent-tab", true, false).is_none());
        assert!(chat_send_target_refusal("backend-tab", false, true).is_none());
    }
}

#[cfg(test)]
mod action_delivery_tests {
    use super::SendRejection;
    use crate::service::delivery_error::DeliveryError;
    use crate::service::inject_verify::InjectionError;
    use crate::service::pty_io::{execute_ordered_actions, OrderedPtyAction};
    use axum::http::StatusCode;

    #[test]
    fn send_actions_enter_into_provider_modal_writes_zero_bytes() {
        let mut bytes_written = 0usize;
        let result = execute_ordered_actions(
            "",
            &[OrderedPtyAction::Submit],
            |body, enter| {
                assert!(body.is_empty());
                assert!(enter);
                Err::<(), _>(DeliveryError::TabBusyRefused {
                    tab: "53854ac8".into(),
                    detail: "provider update dialog is visible".into(),
                })
            },
            |bytes| {
                bytes_written += bytes.len();
                Ok(())
            },
            |_| {},
        );
        let rejection = SendRejection::from(result.unwrap_err());

        assert_eq!(bytes_written, 0);
        assert_eq!(rejection.status, StatusCode::CONFLICT);
        assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
        assert_eq!(rejection.body["error"]["bytes_written"], false);
        assert_eq!(
            rejection.body["error"]["detail"],
            "provider update dialog is visible"
        );
    }

    #[test]
    fn send_actions_raw_key_into_provider_modal_writes_zero_bytes() {
        let mut raw_writes = Vec::new();
        let result = execute_ordered_actions(
            "",
            &[OrderedPtyAction::Raw(b"up".to_vec())],
            |_, _| panic!("raw action must not wait for public agent readiness"),
            |bytes| {
                if bytes == b"up" {
                    Err(DeliveryError::TabBusyRefused {
                        tab: "53854ac8".into(),
                        detail: "provider trust dialog is visible".into(),
                    })
                } else {
                    raw_writes.push(bytes.to_vec());
                    Ok(())
                }
            },
            |_| {},
        );
        let rejection = SendRejection::from(result.unwrap_err());

        assert!(raw_writes.is_empty());
        assert_eq!(rejection.status, StatusCode::CONFLICT);
        assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
        assert_eq!(rejection.body["error"]["bytes_written"], false);
        assert_eq!(
            rejection.body["error"]["detail"],
            "provider trust dialog is visible"
        );
    }

    #[test]
    fn api_send_actions_text_enter_uses_acknowledged_submit() {
        let mut submits = Vec::new();
        let mut raw_writes = Vec::new();
        let result = execute_ordered_actions(
            "parked body",
            &[OrderedPtyAction::Submit],
            |text, enter| {
                submits.push((text.to_string(), enter));
                Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                    waited: std::time::Duration::ZERO,
                    screen_tail: "> parked body".into(),
                })
            },
            |bytes| {
                raw_writes.push(bytes.to_vec());
                Ok(())
            },
            |_| {},
        );
        assert!(matches!(
            result,
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
        assert_eq!(submits, [("parked body".into(), true)]);
        assert!(raw_writes.is_empty());
    }

    #[test]
    fn api_send_actions_raw_keys_ordered_bounded() {
        let actions = [
            OrderedPtyAction::Raw(b"up".to_vec()),
            OrderedPtyAction::Delay(std::time::Duration::from_millis(25)),
            OrderedPtyAction::Raw(b"esc".to_vec()),
        ];
        let mut raw_writes = Vec::new();
        let mut deadlines = Vec::new();
        let mut preflights = Vec::new();
        execute_ordered_actions(
            "",
            &actions,
            |text, enter| {
                preflights.push((text.to_string(), enter));
                Ok::<_, String>(())
            },
            |bytes| {
                raw_writes.push(bytes.to_vec());
                Ok(())
            },
            |deadline| deadlines.push(deadline),
        )
        .unwrap();
        assert_eq!(raw_writes, [b"up".to_vec(), b"esc".to_vec()]);
        assert_eq!(deadlines, [std::time::Duration::from_millis(25)]);
        assert!(preflights.is_empty());
    }

    #[test]
    fn send_actions_enter_and_ctrl_c_to_shell_write_their_bytes() {
        let written = std::cell::RefCell::new(Vec::new());
        let result = execute_ordered_actions(
            "",
            &[OrderedPtyAction::Submit, OrderedPtyAction::Raw(vec![0x03])],
            |text, enter| {
                assert!(text.is_empty());
                assert!(enter);
                written.borrow_mut().extend_from_slice(b"\r");
                Ok::<_, DeliveryError>(())
            },
            |bytes| {
                written.borrow_mut().extend_from_slice(bytes);
                Ok(())
            },
            |_| {},
        );

        result.unwrap();
        assert_eq!(written.into_inner(), b"\r\x03");
    }

    #[test]
    fn a_keystroke_only_send_carries_no_return_address() {
        use super::prefixes_return_address;
        assert!(!prefixes_return_address("", true));
        assert!(prefixes_return_address("answer the prompt", true));
        assert!(!prefixes_return_address("answer the prompt", false));
    }
}

#[cfg(test)]
mod payload_preflight_tests {
    use super::{route_payload_bytes, route_send_origin, BatchCommand};
    use crate::service::delivery_error::{
        preflight_payload_size, SendOrigin, PROGRAMMATIC_SEND_MAX_BYTES,
    };

    /// The route seam: the preflight decision gates the delivery closure — a
    /// refusal must leave the closure uninvoked (zero delivery-path invocation).
    fn route_gate(origin: SendOrigin, payload_bytes: usize) -> (bool, bool) {
        let mut delivered = false;
        let passed = preflight_payload_size("53854ac8", origin, payload_bytes)
            .map(|()| delivered = true)
            .is_ok();
        (passed, delivered)
    }

    #[test]
    fn api_route_payload_preflight_refuses_over_bound_programmatic_without_delivery() {
        // Every existing programmatic marker: service push, source tab, mesh peer.
        for (push, src, peer) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let origin = route_send_origin(push, src, peer);
            assert_eq!(origin, SendOrigin::Programmatic);
            let (passed, delivered) = route_gate(origin, PROGRAMMATIC_SEND_MAX_BYTES + 1);
            assert!(!passed);
            assert!(!delivered, "refused send must never reach delivery");
        }
    }

    #[test]
    fn api_route_payload_preflight_refusal_is_the_typed_pre_write_error() {
        let err = preflight_payload_size(
            "53854ac8",
            route_send_origin(false, true, false),
            PROGRAMMATIC_SEND_MAX_BYTES + 1,
        )
        .unwrap_err();
        assert_eq!(err.code(), "payload_too_large");
        assert_eq!(err.tab, "53854ac8");
        assert_eq!(err.payload_bytes, PROGRAMMATIC_SEND_MAX_BYTES + 1);
        assert_eq!(err.max_bytes, PROGRAMMATIC_SEND_MAX_BYTES);
    }

    #[test]
    fn api_route_payload_preflight_passes_at_bound_programmatic_send() {
        let (passed, delivered) = route_gate(
            route_send_origin(false, true, false),
            PROGRAMMATIC_SEND_MAX_BYTES,
        );
        assert!(passed);
        assert!(delivered);
    }

    #[test]
    fn api_route_payload_preflight_exempts_user_origin_over_bound_send() {
        // No marker on the request ⇒ the existing classification is a user turn.
        let origin = route_send_origin(false, false, false);
        assert_eq!(origin, SendOrigin::User);
        let (passed, delivered) = route_gate(origin, PROGRAMMATIC_SEND_MAX_BYTES + 1);
        assert!(passed);
        assert!(delivered);
    }

    #[test]
    fn api_route_payload_preflight_measures_text_and_batch_byte_length() {
        assert_eq!(route_payload_bytes(None, None), 0);
        // Byte length, not char count.
        assert_eq!(route_payload_bytes(Some("абв"), None), 6);
        let batch = [
            BatchCommand {
                text: "12345".into(),
                delay_ms: None,
            },
            BatchCommand {
                text: "678".into(),
                delay_ms: Some(1),
            },
        ];
        assert_eq!(route_payload_bytes(None, Some(&batch)), 8);
        assert_eq!(route_payload_bytes(Some("ab"), Some(&batch)), 10);
    }
}
