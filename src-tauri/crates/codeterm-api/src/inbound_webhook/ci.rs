use super::{
    presented_secret, sanitize_event_text, task_failed, WebhookRejection, WebhookResponse,
};
use axum::{body::Body, extract::State, http::HeaderMap, Json};
use codeterm_service_core::secrets::SecretStore;
use codeterm_service_core::service::message_gateway::{
    DeliveryClass, DeliverySeam, MessageBody, MessageGateway, MessageOrigin, OutboundMessage,
};
use codeterm_service_core::service::messenger::MessageEnvelope;
use codeterm_service_core::service::notification::{NotificationOrigin, PushIntent};
use serde::Deserialize;

use crate::routes::AppState;

pub(crate) const PATH: &str = "/api/ci/webhook";
const SECRET: &str = "ci-webhook";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    token: String,
    repo: String,
    plan: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CiFailure {
    repo: String,
    run: String,
    attempt: String,
    sha: String,
    job: String,
    step: String,
    test: Option<String>,
    exit: Option<i32>,
    log_url: String,
}

impl CiFailure {
    fn validate(&self, repo: &str) -> Result<(), WebhookRejection> {
        let bounded = |value: &str| {
            !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
        };
        let number = |value: &str| {
            value.len() <= 20
                && value.parse::<u64>().is_ok_and(|number| number > 0)
                && value.bytes().all(|byte| byte.is_ascii_digit())
        };
        let expected_url = format!(
            "https://github.com/{}/actions/runs/{}/attempts/{}",
            self.repo, self.run, self.attempt
        );
        if self.repo != repo
            || !number(&self.run)
            || !number(&self.attempt)
            || self.sha.len() != 40
            || !self.sha.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !bounded(&self.job)
            || !bounded(&self.step)
            || self.test.as_deref().is_some_and(|test| !bounded(test))
            || self.exit.is_some_and(|exit| exit == 0)
            || self.log_url != expected_url
        {
            return Err(WebhookRejection::new(
                axum::http::StatusCode::BAD_REQUEST,
                "invalid_ci_event",
                "CI event identity is invalid",
            ));
        }
        Ok(())
    }

    fn key(&self) -> String {
        format!(
            "ci:{}:{}:{}:{}",
            self.repo, self.run, self.attempt, self.job
        )
    }

    fn text(&self) -> String {
        sanitize_event_text(&format!(
            "CI failure: {} run {} attempt {}\nSHA: {}\nJob: {}\nStep: {}\nTest: {}\nExit: {}\n{}",
            self.repo,
            self.run,
            self.attempt,
            self.sha,
            self.job,
            self.step,
            self.test.as_deref().unwrap_or("unavailable"),
            self.exit
                .map_or_else(|| "unavailable".into(), |exit| exit.to_string()),
            self.log_url,
        ))
    }
}

async fn authenticate(
    store: &dyn SecretStore,
    headers: &HeaderMap,
) -> Result<Registration, WebhookRejection> {
    let presented = presented_secret(headers)?;
    let stored = store
        .get(SECRET)
        .await
        .map_err(|_| WebhookRejection::auth_unavailable())?;
    let registration: Registration = serde_json::from_str(stored.value.expose_secret())
        .map_err(|_| WebhookRejection::auth_unavailable())?;
    if registration.token.is_empty()
        || !codeterm_service_core::service::tokens::constant_time_eq(
            presented.as_bytes(),
            registration.token.as_bytes(),
        )
    {
        return Err(WebhookRejection::unauthorized());
    }
    Ok(registration)
}

fn deliver(
    svc: &codeterm_service_core::service::ServiceContext,
    registration: &Registration,
    event: CiFailure,
) -> Result<WebhookResponse, WebhookRejection> {
    event.validate(&registration.repo)?;
    let target = resolve_target(svc, &registration.plan)?;
    deliver_to(&svc.gateway, svc, target.clone(), event, |key, text| {
        svc.create_agent_notification_reported(
            "CI test failure".into(),
            Some(text),
            Some("error"),
            None,
            None,
            NotificationOrigin::local_tab(target),
            PushIntent {
                event_key: Some(key),
                ..Default::default()
            },
            None,
        )
        .map(|_| ())
        .map_err(|_| WebhookRejection::delivery_failed())
    })
}

fn resolve_target(
    svc: &codeterm_service_core::service::ServiceContext,
    plan_id: &str,
) -> Result<String, WebhookRejection> {
    let plan = svc
        .plan_store
        .get(plan_id)
        .ok_or_else(WebhookRejection::delivery_failed)?;
    let target = plan
        .owner
        .tab_id
        .ok_or_else(WebhookRejection::delivery_failed)?;
    let probe = svc.tab_probe(&target);
    if !probe.live_agent
        || !probe.session_binding_valid
        || probe.role != Some(codeterm_service_core::plan::reconcile::TabRole::Orchestrator)
        || plan.owner.session_id.is_none()
        || probe.agent_session_id != plan.owner.session_id
    {
        return Err(WebhookRejection::delivery_failed());
    }
    Ok(target)
}

fn deliver_to(
    gateway: &MessageGateway,
    seam: &impl DeliverySeam,
    target: String,
    event: CiFailure,
    notify: impl FnOnce(String, String) -> Result<(), WebhookRejection>,
) -> Result<WebhookResponse, WebhookRejection> {
    let key = event.key();
    let text = event.text();
    let receipt = gateway
        .submit(
            seam,
            OutboundMessage {
                target: target.clone(),
                body: MessageBody::Text { body: text.clone() },
                envelope: Some(MessageEnvelope::codeterm_reminder("ci-failure")),
                class: DeliveryClass::SystemNotice,
                origin: MessageOrigin::ServiceToken,
                idempotency_key: Some(key.clone()),
            },
        )
        .map_err(|_| WebhookRejection::delivery_failed())?;
    if receipt.dedup_hit {
        return Ok(WebhookResponse::noop(Some(key)));
    }
    notify(key.clone(), text)?;
    Ok(WebhookResponse::delivered(Some(key)))
}

pub(crate) async fn receive(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<WebhookResponse>, WebhookRejection> {
    let store = state.svc.secrets.read().await.clone();
    let registration = authenticate(store.as_ref(), &headers).await?;
    let bytes = axum::body::to_bytes(body, 16 * 1024)
        .await
        .map_err(|_| WebhookRejection::body_too_large())?;
    let event: CiFailure =
        serde_json::from_slice(&bytes).map_err(|_| WebhookRejection::invalid_json())?;
    let svc = state.svc.clone();
    tokio::task::spawn_blocking(move || deliver(&svc, &registration, event))
        .await
        .map_err(task_failed)?
        .map(Json)
}

#[cfg(test)]
mod tests;
