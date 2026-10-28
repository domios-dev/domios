use super::super::ServiceContext;
use codeterm_account::account_mesh::callable::{result, CallableTransport, HttpCallable};
use codeterm_analytics::{Consent, ConsentTicket, Status, Worker};
use serde_json::{json, Value};

#[derive(Default)]
pub(crate) struct Bridge(tokio::sync::Mutex<()>);

impl ServiceContext {
    pub async fn set_product_analytics_consent(&self, consent: Consent) -> Result<Status, String> {
        let worker = self.analytics_worker().await?;
        let mut ticket = worker.begin_consent(Some(consent)).await?;
        let account = self.account().await?;
        if let Some(session) = account
            .current_session()
            .await
            .map_err(|_| "Analytics sign-in unavailable")?
        {
            if consent == Consent::Off && ticket.uid.is_none() {
                let latest = worker.consent_ticket().await?;
                if latest.consent == Consent::Off
                    && latest.uid.as_deref() == Some(session.identity.uid.as_str())
                {
                    ticket = worker.begin_consent(Some(Consent::Off)).await?;
                }
            }
            self.analytics_bridge(&session, ticket, Some(consent))
                .await?;
        } else if consent == Consent::On {
            if ticket.uid.is_some() {
                return Err("Analytics account unavailable".into());
            }
            worker.approve_consent(ticket).await?;
        }
        worker.status().await
    }

    async fn analytics_bridge(
        &self,
        session: &codeterm_account::Session,
        ticket: ConsentTicket,
        intent: Option<Consent>,
    ) -> Result<(), String> {
        let worker = self.analytics_worker().await?;
        if session.identity.provider != "firebase" {
            if ticket.uid.as_deref() == Some(session.identity.uid.as_str()) {
                worker.approve_consent(ticket).await?;
            }
            return Ok(());
        }
        let endpoint = codeterm_account::firebase_endpoints::current().functions;
        let transport = HttpCallable::new(endpoint).map_err(|_| "Analytics bridge unavailable")?;
        synchronize(
            self.extensions.get_or_init::<Bridge>(Default::default),
            worker,
            &transport,
            session.id_token.expose(),
            &session.identity.uid,
            ticket,
            intent,
        )
        .await?;
        if let Some(auth_provider) =
            codeterm_analytics::AuthProvider::from_catalog(&session.identity.provider)
        {
            let _ = worker
                .record(codeterm_analytics::Event::SignedIn { auth_provider })
                .await;
        }
        Ok(())
    }

    pub(crate) async fn sync_product_analytics(
        &self,
        session: &codeterm_account::Session,
        intent: Option<Consent>,
    ) -> Result<(), String> {
        let ticket = self.analytics_worker().await?.begin_consent(intent).await?;
        self.analytics_bridge(session, ticket, intent).await
    }
}

async fn call(
    transport: &impl CallableTransport,
    token: &str,
    route: &str,
    data: Value,
) -> Result<Value, String> {
    let body = transport
        .call(route, token, data)
        .await
        .map_err(|_| "Analytics bridge unavailable")?;
    result(body).map_err(|_| "Analytics bridge refused".into())
}

async fn current(worker: &Worker, ticket: &ConsentTicket, uid: &str) -> Result<(), String> {
    let latest = worker.consent_ticket().await?;
    if latest.sequence != ticket.sequence
        || latest.uid != ticket.uid
        || ticket.uid.as_deref() != Some(uid)
    {
        return Err("Analytics intent superseded".into());
    }
    Ok(())
}

async fn synchronize(
    bridge: &Bridge,
    worker: &Worker,
    transport: &impl CallableTransport,
    token: &str,
    uid: &str,
    ticket: ConsentTicket,
    intent: Option<Consent>,
) -> Result<(), String> {
    let _guard = bridge.0.lock().await;
    for withdrawal in worker.consent_ticket().await?.withdrawals {
        if withdrawal.uid != uid {
            continue;
        }
        let applied = call(
            transport,
            token,
            "analyticsConsent",
            json!({
                "state":"off", "change_id":withdrawal.change_id,
            }),
        )
        .await?;
        if applied.get("status").and_then(Value::as_str) != Some("applied") {
            return Err("Analytics withdrawal unconfirmed".into());
        }
        worker.acknowledge_withdrawal(withdrawal.change_id).await?;
    }
    current(worker, &ticket, uid).await?;
    if ticket.consent == Consent::Off {
        return Ok(());
    }
    if !worker.consent_ticket().await?.withdrawals.is_empty() {
        return Err("Analytics withdrawal pending for another account".into());
    }
    let outcome = async {
        if intent == Some(Consent::On) {
            let response = call(
                transport,
                token,
                "analyticsConsent",
                json!({"action":"read"}),
            )
            .await?;
            current(worker, &ticket, uid).await?;
            let revision = response
                .get("revision")
                .and_then(Value::as_u64)
                .ok_or("Analytics revision unavailable")?;
            let applied = call(
                transport,
                token,
                "analyticsConsent",
                json!({
                    "state":"on", "base_revision":revision, "change_id":uuid::Uuid::new_v4(),
                }),
            )
            .await?;
            current(worker, &ticket, uid).await?;
            if applied.get("status").and_then(Value::as_str) != Some("applied") {
                return Err("Analytics consent changed; choose again".into());
            }
        }
        current(worker, &ticket, uid).await?;
        let device = ticket.device_id.ok_or("Analytics device unavailable")?;
        let response = call(
            transport,
            token,
            "analyticsSignIn",
            json!({
                "device_id":device, "change_id":uuid::Uuid::new_v4(),
            }),
        )
        .await?;
        current(worker, &ticket, uid).await?;
        if response.get("status").and_then(Value::as_str) != Some("recorded") {
            return Err("Analytics identity bridge refused".into());
        }
        worker.approve_consent(ticket.clone()).await?;
        if intent == Some(Consent::On) {
            let _ = worker
                .record(codeterm_analytics::Event::AnalyticsOptedIn {})
                .await;
        }
        Ok(())
    }
    .await;
    if outcome.is_err() {
        let _ = worker.refuse_consent(ticket).await;
    }
    outcome
}

#[cfg(test)]
mod tests;
