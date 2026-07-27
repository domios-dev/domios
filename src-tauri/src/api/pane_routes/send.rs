use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_pane};
use crate::service::delivery_error::DeliveryError;

use super::AppState;

pub async fn chat_send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SendTextBody>,
) -> Result<Json<crate::service::chat_send_transaction::ChatSendStatus>, SendRejection> {
    check_auth(&state, &headers).map_err(SendRejection::from)?;
    let pane_id = resolve_pane(&state, &id).map_err(SendRejection::from)?;
    let text = body.text.unwrap_or_default();
    let run_stages =
        crate::chatpipeline::is_user_turn_send(false, false, state.svc.pane_has_agent(&pane_id));
    let text = if run_stages {
        state.svc.run_outbound_stages(&pane_id, text)
    } else {
        text
    };
    state
        .svc
        .start_chat_send(&pane_id, text)
        .map(Json)
        .map_err(|detail| {
            DeliveryError::PaneBusyRefused {
                pane: pane_id,
                detail,
            }
            .into()
        })
}

pub async fn chat_send_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::service::chat_send_transaction::ChatSendStatus>>, SendRejection> {
    check_auth(&state, &headers).map_err(SendRejection::from)?;
    let pane_id = resolve_pane(&state, &id).map_err(SendRejection::from)?;
    Ok(Json(state.svc.chat_send_ledger.reconcile(&pane_id)))
}

/// Classify a failed PTY delivery for the caller (local or mesh relay). The pane
/// resolved before any write was attempted, so a failure here is never "missing".
fn classify_delivery_failure(pane: &str, err: &str) -> DeliveryError {
    DeliveryError::from_opaque(pane, err)
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

pub(super) use self::await_pty_link as wait_for_pty_link;

pub(crate) fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneInputTransport {
    Pty,
    ChatBackend,
}

/// Whether to inject the sender return-address marker into the PTY text. The
/// marker is meaningful only to an AGENT reading its input; a bare shell runs
/// the bytes verbatim and a chatBackend carries identity structurally.
pub(crate) fn should_prepend_source(
    has_source: bool,
    target_has_agent: bool,
    transport: PaneInputTransport,
) -> bool {
    has_source && target_has_agent && transport == PaneInputTransport::Pty
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
    /// Sender's pane id when CLI is invoked from inside CodeTerm. Daemon
    /// prepends a CodeTerm marker so the recipient knows the return-address.
    /// Mesh hops and hierarchy reports use the same marker family.
    #[serde(default)]
    pub(crate) source_pane: Option<String>,
    /// Originating peer name, injected by the mesh relay (authenticated from
    /// the signed token — peers cannot spoof it). Present ⇒ this delivery
    /// crossed the mesh; the label is peer-qualified `label@peer`.
    #[serde(default)]
    pub(crate) source_peer: Option<String>,
    /// Source agent label + model, threaded from the originating peer (the
    /// source pane is foreign and cannot be resolved against the local mux).
    #[serde(default)]
    pub(crate) source_label: Option<String>,
    #[serde(default)]
    pub(crate) source_model: Option<String>,
    /// Delivery framing for a mesh-relayed message. `"report"` renders as
    /// `[Report from <label>@<peer>]` (a remote worker's `agent report`); any
    /// other / absent value is a plain pane message. Only honoured alongside
    /// `source_peer` so a local send can't forge report framing.
    #[serde(default)]
    pub(crate) kind: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BatchCommand {
    pub(crate) text: String,
    #[serde(default = "default_true")]
    pub(crate) enter: bool,
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

pub(super) async fn await_pty_link(state: &AppState, pane_id: &str) -> bool {
    if state.svc.pane_chat_backing(pane_id).is_some() {
        return true;
    }
    let svc = state.svc.clone();
    let pane = pane_id.to_string();
    tokio::task::spawn_blocking(move || {
        svc.await_pty_link_blocking(&pane, std::time::Duration::from_secs(15))
            .is_ok()
    })
    .await
    .unwrap_or(false)
}

fn record_push_activity(state: &AppState, svc_id: &Option<String>, pane_id: &str, text: &str) {
    let Some(svc_id) = svc_id else { return };
    let writer = crate::service::activity::ActivityWriter::new(state.svc.services_store.conn());
    let preview = text.chars().take(200).collect::<String>();
    let _ = writer.record(crate::service::activity::ActivityEvent::push(
        svc_id, pane_id, &preview,
    ));
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "service-pushed",
        serde_json::json!({"source_id": svc_id, "target": pane_id}),
    );
}

pub async fn send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
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
    // a missing pane — everything past it has resolved a real target.
    let pane_id =
        resolve_pane(&state, &raw_pane_id).map_err(|_| DeliveryError::PaneNotReachableOnPeer {
            pane: raw_pane_id.clone(),
        })?;

    let transport = if state.svc.pane_chat_backing(&pane_id).is_some() {
        PaneInputTransport::ChatBackend
    } else {
        PaneInputTransport::Pty
    };
    // The return-address marker is meaningful only to an AGENT reading its input.
    // A bare shell PTY must run the injected bytes verbatim — never prefix it.
    let target_has_agent = state.svc.pane_has_agent(&pane_id);

    if body.kind.as_deref() == Some("report") {
        if let (Some(peer), Some(src)) = (
            body.source_peer.as_deref().filter(|s| !s.is_empty()),
            body.source_pane.as_deref().filter(|s| !s.is_empty()),
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
                &pane_id,
                peer,
                src,
                label,
                body.source_model.clone(),
                body.text.as_deref().unwrap_or(""),
                now_ms,
            );
            state.svc.emit_remote_workers_changed(&pane_id);
        }
    }

    let requires_immediate_pty = body.raw || body.actions.is_some() || body.batch.is_some();
    if transport == PaneInputTransport::Pty && requires_immediate_pty {
        state.svc.ensure_pane_pty_for_send(&pane_id).map_err(|e| {
            DeliveryError::PtyUnavailable {
                pane: pane_id.clone(),
                detail: e.to_string(),
            }
        })?;

        if !await_pty_link(&state, &pane_id).await {
            return Err(DeliveryError::PtyUnavailable {
                pane: pane_id.clone(),
                detail: "PTY did not link within 15s".into(),
            }
            .into());
        }
    }

    let source_envelope: Option<crate::service::messenger::MessageEnvelope> =
        body.source_pane.as_deref().and_then(|src| {
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
                        crate::service::messenger::MessageEnvelope::pane(label, src)
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
                    let resolved = mux.resolve_pane_id(src)?;
                    let from_label = mux.display_label_for_pane(&resolved);
                    let model = mux
                        .get_pane(&resolved)
                        .and_then(|p| p.agent_session_model.clone());
                    Some(
                        crate::service::messenger::MessageEnvelope::pane(from_label, resolved)
                            .model(model),
                    )
                }
            }
        });
    let prefix_source =
        should_prepend_source(source_envelope.is_some(), target_has_agent, transport);
    let prepend = |s: &str| -> String {
        match &source_envelope {
            Some(envelope) if prefix_source => {
                crate::service::messenger::Messenger::from(envelope.clone()).wrap(s)
            }
            _ => s.to_string(),
        }
    };

    // send_text is also used by system senders and stays transport-clean.
    let run_stages = crate::chatpipeline::is_user_turn_send(
        svc_token_match.is_some(),
        body.source_pane.is_some(),
        target_has_agent,
    );
    let stage = |s: String| -> String {
        if run_stages {
            state.svc.run_outbound_stages(&pane_id, s)
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
        let text = prepend(&stage(raw_text.clone()));
        if transport == PaneInputTransport::ChatBackend {
            if !raw_text.is_empty() {
                state
                    .svc
                    .send_text_when_ready(&pane_id, &text, false)
                    .await
                    .map_err(|e| classify_delivery_failure(&pane_id, &e))?;
            }
        } else {
            let svc = state.svc.clone();
            let action_pane = pane_id.clone();
            tokio::task::spawn_blocking(move || {
                crate::service::pty_io::execute_ordered_actions(
                    &text,
                    &ordered,
                    |body, enter| svc.send_text(&action_pane, body, enter),
                    |bytes| svc.write_raw(&action_pane, bytes),
                    crate::service::pty_io::await_action_deadline,
                )
            })
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "action delivery task failed",
                )
            })?
            .map_err(|e| classify_delivery_failure(&pane_id, &e))?;
        }

        let preview = if raw_text.is_empty() {
            format!("<actions:{}>", action_preview(&actions))
        } else {
            format!("{} <actions:{}>", raw_text, action_preview(&actions))
        };
        record_push_activity(&state, &svc_token_match, &pane_id, &preview);
        if !raw_text.trim().is_empty() {
            state.svc.maybe_send_owned_plan_reminder(&pane_id);
        }
        return Ok(Json(serde_json::json!({"ok": true, "actions": true})));
    }

    if let Some(batch) = body.batch {
        let preview = batch
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ; ");
        let items: Vec<crate::service::pty_io::SendBatchItem> = batch
            .into_iter()
            .map(|c| crate::service::pty_io::SendBatchItem {
                text: prepend(&stage(c.text)),
                enter: c.enter,
                delay_ms: c.delay_ms,
            })
            .collect();
        let svc = state.svc.clone();
        let batch_pane = pane_id.clone();
        tokio::task::spawn_blocking(move || svc.send_text_batch(&batch_pane, items))
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "batch delivery task failed",
                )
            })?
            .map_err(|e| classify_delivery_failure(&pane_id, &e))?;
        record_push_activity(&state, &svc_token_match, &pane_id, &preview);
        if !preview.trim().is_empty() {
            state.svc.maybe_send_owned_plan_reminder(&pane_id);
        }
        return Ok(Json(serde_json::json!({"ok": true, "batch": true})));
    }

    if body.raw {
        // Byte-for-byte to the PTY (bracket paste + sender prefix skipped, for
        // TUI keystrokes like vim j/k). The caller includes \r to submit.
        let raw_text = body.text.clone().unwrap_or_default();
        state
            .svc
            .write_raw(&pane_id, raw_text.as_bytes())
            .map_err(|e| DeliveryError::PtyUnavailable {
                pane: pane_id.clone(),
                detail: e.to_string(),
            })?;
        record_push_activity(&state, &svc_token_match, &pane_id, &raw_text);
        return Ok(Json(serde_json::json!({"ok": true})));
    }

    let raw_text = body.text.unwrap_or_default();
    if source_envelope.is_some()
        && body
            .source_peer
            .as_deref()
            .is_some_and(|peer| !peer.is_empty())
        && transport == PaneInputTransport::Pty
    {
        let text = prepend(&raw_text);
        state
            .svc
            .send_text_when_ready(&pane_id, &text, true)
            .await
            .map_err(|e| classify_delivery_failure(&pane_id, &e))?;
        record_push_activity(&state, &svc_token_match, &pane_id, &raw_text);
        if !raw_text.trim().is_empty() {
            state.svc.maybe_send_owned_plan_reminder(&pane_id);
        }
        return Ok(Json(serde_json::json!({"ok": true, "mesh": true})));
    }
    if let Some(envelope) = source_envelope {
        crate::service::messenger::Messenger::deliver(&state.svc, &pane_id, envelope, &raw_text)
            .map_err(|e| classify_delivery_failure(&pane_id, &e))?;
        record_push_activity(&state, &svc_token_match, &pane_id, &raw_text);
        if !raw_text.trim().is_empty() {
            state.svc.maybe_send_owned_plan_reminder(&pane_id);
        }
        return Ok(Json(serde_json::json!({"ok": true})));
    }

    let has_text = !raw_text.trim().is_empty();
    let text = prepend(&stage(raw_text));
    // A plain text send always submits — no-Enter/keystroke cases go through
    // `actions` or `raw`.
    state
        .svc
        .send_text_when_ready(&pane_id, &text, true)
        .await
        .map_err(|e| classify_delivery_failure(&pane_id, &e))?;
    record_push_activity(&state, &svc_token_match, &pane_id, &text);
    if has_text {
        state.svc.maybe_send_owned_plan_reminder(&pane_id);
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod delivery_classification_tests {
    use super::classify_delivery_failure;

    /// #240: a submit that failed AFTER the paste reached the PTY must never be
    /// reported as a missing pane — the caller would retry onto parked text.
    #[test]
    fn submit_failure_is_not_reported_as_missing_pane() {
        let err = classify_delivery_failure(
            "53854ac8",
            "submit not confirmed after 3s; screen_tail: \"> parked body\"",
        );
        assert_eq!(err.code(), "submit_unconfirmed");
        assert!(err.bytes_written());
    }

    #[test]
    fn booting_pane_refusal_is_a_zero_byte_busy_error() {
        let err = classify_delivery_failure(
            "53854ac8",
            "Agent in Tab 53854ac8 is still booting — input would be lost",
        );
        assert_eq!(err.code(), "pane_busy_refused");
        assert!(!err.bytes_written());
    }
}

#[cfg(test)]
mod action_delivery_tests {
    use crate::service::inject_verify::InjectionError;
    use crate::service::pty_io::{execute_ordered_actions, OrderedPtyAction};

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
        execute_ordered_actions(
            "",
            &actions,
            |_, _| Ok::<_, String>(()),
            |bytes| {
                raw_writes.push(bytes.to_vec());
                Ok(())
            },
            |deadline| deadlines.push(deadline),
        )
        .unwrap();
        assert_eq!(raw_writes, [b"up".to_vec(), b"esc".to_vec()]);
        assert_eq!(deadlines, [std::time::Duration::from_millis(25)]);
    }
}
