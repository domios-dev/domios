use super::super::ServiceContext;
use codeterm_account::account_mesh::callable::{result, CallableTransport, HttpCallable};
use codeterm_analytics::{Consent, Status};
use serde_json::{json, Value};

impl ServiceContext {
    pub async fn set_product_analytics_consent(&self, consent: Consent) -> Result<Status, String> {
        let worker = self.analytics_worker().await?;
        let status = worker.set_consent(consent).await?;
        let account = self.account().await?;
        if let Some(session) = account
            .current_session()
            .await
            .map_err(|_| "Analytics sign-in unavailable")?
        {
            worker
                .set_user_id(Some(session.identity.uid.clone()))
                .await?;
            self.sync_product_analytics(&session, Some(consent)).await?;
        }
        Ok(status)
    }

    pub(crate) async fn sync_product_analytics(
        &self,
        session: &codeterm_account::Session,
        intent: Option<Consent>,
    ) -> Result<(), String> {
        if session.identity.provider != "firebase" {
            return Ok(());
        }
        let worker = self.analytics_worker().await?;
        let consent = worker.status().await?.consent;
        if intent.is_none() && consent != Consent::On {
            return Ok(());
        }
        let endpoint = codeterm_account::firebase_endpoints::current().functions;
        let transport = HttpCallable::new(endpoint).map_err(|_| "Analytics bridge unavailable")?;
        synchronize(
            &transport,
            session.id_token.expose(),
            consent,
            worker.device_id().await?,
            intent,
        )
        .await
    }
}

async fn synchronize(
    transport: &impl CallableTransport,
    token: &str,
    consent: Consent,
    device: Option<uuid::Uuid>,
    intent: Option<Consent>,
) -> Result<(), String> {
    let call = async |route: &str, data: Value| -> Result<Value, String> {
        let body = transport
            .call(route, token, data)
            .await
            .map_err(|_| "Analytics bridge unavailable")?;
        result(body).map_err(|_| "Analytics bridge refused".into())
    };
    if let Some(intent) = intent {
        let change = uuid::Uuid::new_v4().to_string();
        let data = if intent == Consent::On {
            let current = call("analyticsConsent", json!({"action":"read"})).await?;
            let revision = current
                .get("revision")
                .and_then(Value::as_u64)
                .ok_or("Analytics revision unavailable")?;
            json!({"state":"on","base_revision":revision,"change_id":change})
        } else {
            json!({"state":"off","change_id":change})
        };
        let applied = call("analyticsConsent", data).await?;
        if applied.get("status").and_then(Value::as_str) != Some("applied") {
            return Err("Analytics consent changed; choose again".into());
        }
    }
    if consent == Consent::On {
        if let Some(device) = device {
            let response = call(
                "analyticsSignIn",
                json!({"device_id":device,"change_id":uuid::Uuid::new_v4()}),
            )
            .await?;
            if response.get("status").and_then(Value::as_str) != Some("recorded") {
                return Err("Analytics identity bridge refused".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
