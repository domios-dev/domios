use super::super::local_preview::{artifact_policy, html_preview, references};
use super::super::messenger::{AgentDeliveryPayload, Delivery, MessageEnvelope, Messenger};
use super::ServiceContext;
use codeterm_types::agent::background_work::BackgroundWorkKind;
use std::path::Path;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundWorkTarget {
    pub session_id: String,
    pub binding_nonce: String,
    pub task_id: String,
}

impl BackgroundWorkTarget {
    fn read_tab<T>(
        &self,
        svc: &ServiceContext,
        tab_id: &str,
        read: impl FnOnce(&crate::multiplexer::types::Tab) -> Option<T>,
    ) -> Result<T, String> {
        let mux = svc.mux.read();
        mux.get_tab(tab_id)
            .filter(|tab| {
                tab.agent_session_id.as_deref() == Some(&self.session_id)
                    && tab.session_binding_nonce.as_deref() == Some(&self.binding_nonce)
                    && tab.provider_signals.background.kinds.get(&self.task_id)
                        == Some(&BackgroundWorkKind::Shell)
            })
            .and_then(read)
            .ok_or_else(|| "Background process is unavailable in this session".into())
    }
}

pub fn read_output(
    svc: &ServiceContext,
    tab_id: &str,
    configured: &[String],
    target: &BackgroundWorkTarget,
) -> Result<html_preview::LocalPreviewFile, String> {
    let output_path = || {
        target.read_tab(svc, tab_id, |tab| {
            tab.provider_signals
                .background
                .output_path(&target.task_id)
                .map(str::to_owned)
        })
    };
    let raw = output_path()?;
    let path = html_preview::checked_path(&raw)?;
    let temp = html_preview::checked_path(&std::env::temp_dir().to_string_lossy())?;
    let canonical_temp = temp
        .canonicalize()
        .map_err(|_| "Background output unavailable")?;
    let policy = artifact_policy(
        svc,
        Some(tab_id),
        [references::path_key(&path)].into(),
        configured,
    )?;
    let output = html_preview::read_text_tail_guarded(
        &raw,
        |requested: &Path, canonical: Option<&Path>| {
            if !requested.starts_with(&temp)
                || canonical.is_some_and(|path| !path.starts_with(&canonical_temp))
            {
                return Err("Background output is outside the temp directory".into());
            }
            policy.authorize(requested, canonical)
        },
        32 * 1024,
    )
    .map_err(|_| "Background output unavailable")?;
    if output_path()? != raw {
        return Err("Background output session changed".into());
    }
    Ok(output)
}

pub fn ask_stop(
    svc: &ServiceContext,
    tab_id: &str,
    target: BackgroundWorkTarget,
) -> Result<&'static str, String> {
    if target.task_id.is_empty()
        || target.task_id.len() > 256
        || !target
            .task_id
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-')
    {
        return Err("Invalid background process id".into());
    }
    let context = "background-process-stop";
    svc.require_system_notice_send_allowed(tab_id, None, context)?;
    let pty = target.read_tab(svc, tab_id, |tab| {
        if !tab.launch_session_authenticated()
            || !matches!(tab.backing, crate::multiplexer::types::TabBacking::Pty)
            || !tab
                .provider_signals
                .background
                .running
                .contains_key(&target.task_id)
        {
            return None;
        }
        tab.pty_id.clone()
    })?;
    let messenger = Messenger::from(MessageEnvelope::codeterm_reminder(context));
    let body = format!(
        "Please stop background process {} using your background task controls.",
        target.task_id
    );
    let transport = messenger.framed_delivery(&body);
    let text = transport.to_message();
    let delivery = svc.deliver_agent_payload(
        tab_id,
        AgentDeliveryPayload {
            transport,
            trace_id: None,
            durable_delivery: None,
            observer: None,
            idempotency_key: None,
            expected_pty_id: Some(pty),
            expected_session_id: Some(target.session_id),
            expected_binding_nonce: Some(target.binding_nonce),
        },
        &text,
    )?;
    Ok(match delivery {
        Delivery::Pending { .. } => "pending",
        Delivery::Unconfirmed { .. } => "unconfirmed",
        _ => "written",
    })
}
