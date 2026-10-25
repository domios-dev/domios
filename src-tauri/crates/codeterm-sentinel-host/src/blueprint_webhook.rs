mod limits;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use codeterm_blueprint_types::blueprint_journal::JournalMarker;
use codeterm_blueprint_types::blueprint_pipeline::Trigger;
use codeterm_plugin::graph_events::EventEnvelope;
use codeterm_service_core::secrets::{SaveRequest, SecretError, SecretString};
use codeterm_service_core::service::{ServiceContext, ServiceState};
use codeterm_storage::BlueprintJournalWrite;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{scheduler::Handle, service_adapter::SentinelCaller, SentinelExt};

pub const MAX_BODY_BYTES: usize = 1024 * 1024;
const NOT_FOUND: &str = "webhook unavailable";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Unavailable,
    Unauthorized,
    InvalidPayload,
    RateLimited,
    Busy,
}

#[derive(Default)]
pub struct Authentication {
    pub bearer: Option<String>,
    pub signature: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Default)]
pub struct Webhooks {
    gate: tokio::sync::Mutex<()>,
    limits: parking_lot::Mutex<limits::Limits>,
}

pub fn service(svc: &ServiceContext) -> Result<Arc<Webhooks>, Refusal> {
    if let Some(service) = svc.host_extension::<Webhooks>() {
        return Ok(service);
    }
    let service = Arc::new(Webhooks::default());
    match svc.register_host_extension(service.clone()) {
        Ok(()) => Ok(service),
        Err(_) => svc.host_extension::<Webhooks>().ok_or(Refusal::Unavailable),
    }
}

fn secret_name(id: &str) -> String {
    format!("blueprint-webhook-{id}")
}

fn handle(svc: &ServiceContext) -> Result<Arc<Handle>, Refusal> {
    svc.host_extension::<Handle>().ok_or(Refusal::Unavailable)
}

fn active(
    svc: &ServiceContext,
    handle: &Handle,
    id: &str,
) -> Result<crate::store::Sentinel, Refusal> {
    let store = handle.store.lock();
    store.ensure_available().map_err(|_| Refusal::Unavailable)?;
    let job = store.get(id).ok_or(Refusal::Unavailable)?.clone();
    let now = store.clock.now_ms();
    drop(store);
    if job.machine.stopped
        || job.machine.paused
        || job.machine.held.is_some()
        || !job.admission.permits_steps()
        || job.expires_at.is_some_and(|at| now >= at)
        || !matches!(
            job.pipeline().map(|p| &p.trigger),
            Some(Trigger::Webhook { .. })
        )
        || crate::plan_lock::locked(&svc.locks(), &job)
    {
        return Err(Refusal::Unavailable);
    }
    Ok(job)
}

impl Webhooks {
    pub fn before_read(&self, now: u64) -> Result<(), Refusal> {
        self.limits.lock().attempt(now)
    }

    pub async fn configure(
        &self,
        svc: &ServiceContext,
        caller: &SentinelCaller,
        id: &str,
        action: &str,
        reveal: bool,
    ) -> Result<Value, String> {
        let _gate = self.gate.lock().await;
        let job = svc.sentinel_show(caller, id)?;
        if !matches!(
            job.pipeline().map(|p| &p.trigger),
            Some(Trigger::Webhook { .. })
        ) {
            return Err(NOT_FOUND.into());
        }
        svc.authorize_role_blueprint(
            &caller.tab,
            Some(id),
            codeterm_service_core::service::role_policy::BlueprintAction::Input,
        )?;
        let store = svc.secrets.read().await.clone();
        let name = secret_name(id);
        match action {
            "enable" | "rotate" => {
                let secret = SecretString::new(format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                ));
                store
                    .save(SaveRequest {
                        name,
                        value: secret.clone(),
                        notes: None,
                        folder: None,
                        organization: None,
                        collection: None,
                        overwrite: action == "rotate",
                    })
                    .await
                    .map_err(|_| "webhook secret could not be saved")?;
                let mut result =
                    json!({"enabled":true,"path":format!("/api/blueprints/{id}/webhook")});
                if reveal {
                    result["secret"] = json!(secret.expose_secret());
                }
                Ok(result)
            }
            "disable" => {
                match store.delete(&name).await {
                    Ok(()) | Err(SecretError::NotFound) => {}
                    Err(_) => return Err("webhook secret could not be removed".into()),
                }
                Ok(json!({"enabled":false}))
            }
            "show-url" => Ok(json!({"path":format!("/api/blueprints/{id}/webhook")})),
            _ => Err("unknown webhook operation".into()),
        }
    }

    pub async fn receive(
        self: &Arc<Self>,
        svc: ServiceState,
        id: String,
        auth: Authentication,
        delivery: Option<String>,
        bytes: Vec<u8>,
    ) -> Result<Value, Refusal> {
        let _gate = self.gate.try_lock().map_err(|_| Refusal::Busy)?;
        let handle = handle(&svc)?;
        active(&svc, &handle, &id)?;
        let store = svc.secrets.read().await.clone();
        let secret = store
            .get(&secret_name(&id))
            .await
            .map_err(|_| Refusal::Unavailable)?;
        authenticate(
            secret.value.expose_secret(),
            &auth,
            &bytes,
            chrono::Utc::now().timestamp(),
        )?;
        let payload = tokio::task::spawn_blocking(move || parse_payload(&bytes))
            .await
            .map_err(|_| Refusal::InvalidPayload)??;
        let job = active(&svc, &handle, &id)?;
        let Trigger::Webhook { schema } = &job.pipeline().ok_or(Refusal::Unavailable)?.trigger
        else {
            return Err(Refusal::Unavailable);
        };
        let definition = codeterm_blueprint_types::blueprint_document::Document::from_definition(
            job.document.as_ref().ok_or(Refusal::Unavailable)?,
        )
        .map_err(|_| Refusal::Unavailable)?;
        if schema
            .as_ref()
            .is_some_and(|schema| !schema.accepts_with_types(&payload, &definition.types))
        {
            return Err(Refusal::InvalidPayload);
        }
        let now = handle.store.lock().clock.now_ms();
        let run_id = uuid::Uuid::new_v4().simple().to_string();
        let event = EventEnvelope {
            subscription_id: String::new(),
            pipeline_id: id.clone(),
            kind: "webhook".into(),
            event_id: run_id.clone(),
            payload,
            admitted_at_ms: now,
        };
        let lease = {
            let mut limits = self.limits.lock();
            if let Some(run_id) = limits.duplicate(&id, delivery.as_deref(), now)? {
                return Ok(json!({"ok":true,"duplicate":true,"run_id":run_id}));
            }
            limits.admit(&id, now)?;
            let lease = {
                let mut store = handle.store.lock();
                if store.active.len() >= 8 {
                    return Err(Refusal::Busy);
                }
                store.stage_event_lease(event).ok_or(Refusal::Busy)?
            };
            if lease.job.revision != job.revision
                || lease.job.definition_version != job.definition_version
            {
                handle.store.lock().release(&id);
                return Err(Refusal::Unavailable);
            }
            lease
        };
        let hooks = self.clone();
        let audit_id = run_id.clone();
        let (accepted, receipt) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let audited = crate::pipeline::write_journal(
                &handle,
                &job,
                BlueprintJournalWrite::Decision {
                    run_id: audit_id,
                    marker: JournalMarker::Run,
                    step_name: "$run".into(),
                    outcome: "webhook_authenticated".into(),
                    at_ms: now,
                    result_summary: Some("Authenticated webhook admitted".into()),
                    reason: Some("webhook_authenticated".into()),
                },
            );
            if audited {
                hooks.limits.lock().remember(
                    &id,
                    delivery,
                    &lease
                        .event
                        .as_ref()
                        .map(|e| e.event_id.clone())
                        .unwrap_or_default(),
                    now,
                );
            }
            let _ = accepted.send(audited);
            if audited {
                crate::pipeline::tick(&handle, &svc, &lease);
            }
            handle.store.lock().release(&lease.job.id);
            handle.notify();
        });
        if !receipt.await.unwrap_or(false) {
            return Err(Refusal::Unavailable);
        }
        Ok(json!({"ok":true,"duplicate":false,"run_id":run_id}))
    }
}

fn authenticate(
    secret: &str,
    auth: &Authentication,
    bytes: &[u8],
    now: i64,
) -> Result<(), Refusal> {
    if secret.is_empty() {
        return Err(Refusal::Unauthorized);
    }
    let bearer_valid = auth.bearer.as_deref().is_some_and(|value| {
        codeterm_service_core::service::tokens::constant_time_eq(
            &Sha256::digest(secret.as_bytes()),
            &Sha256::digest(value.as_bytes()),
        )
    });
    let signature_valid = auth
        .signature
        .as_deref()
        .and_then(|value| value.strip_prefix("sha256="))
        .and_then(decode_signature)
        .is_some_and(|signature| {
            let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
                return false;
            };
            if let Some(timestamp) = &auth.timestamp {
                if timestamp.len() > 20
                    || !timestamp.bytes().all(|b| b.is_ascii_digit())
                    || !timestamp
                        .parse::<i64>()
                        .is_ok_and(|at| now.abs_diff(at) <= 300)
                {
                    return false;
                }
                mac.update(timestamp.as_bytes());
                mac.update(b".");
            }
            mac.update(bytes);
            mac.verify_slice(&signature).is_ok()
        });
    (bearer_valid || signature_valid)
        .then_some(())
        .ok_or(Refusal::Unauthorized)
}

fn decode_signature(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..32)
        .map(|i| u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

fn parse_payload(bytes: &[u8]) -> Result<Value, Refusal> {
    if bytes.len() > MAX_BODY_BYTES {
        return Err(Refusal::InvalidPayload);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Refusal::InvalidPayload)?;
    let envelope = codeterm_blueprint_types::blueprint_execution::ExecutionEnvelope {
        media: "json".into(),
        schema_id: None,
        value: value.clone(),
        text: String::new(),
    };
    if serde_json::to_vec(&envelope).map_or(true, |bytes| {
        bytes.len() > codeterm_blueprint_types::blueprint_execution::MAX_STDIN_BYTES
    }) {
        return Err(Refusal::InvalidPayload);
    }
    fn depth(value: &Value, level: usize) -> bool {
        level <= 32
            && match value {
                Value::Array(values) => values.iter().all(|v| depth(v, level + 1)),
                Value::Object(values) => values.values().all(|v| depth(v, level + 1)),
                _ => true,
            }
    }
    depth(&value, 0)
        .then_some(value)
        .ok_or(Refusal::InvalidPayload)
}
