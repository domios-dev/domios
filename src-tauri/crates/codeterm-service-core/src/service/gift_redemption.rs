use super::ServiceContext;
use codeterm_account::account_mesh::callable::{CallableFailure, CallableTransport, HttpCallable};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GiftRedemption {
    key: String,
    expected_uid: String,
}

impl GiftRedemption {
    fn valid_key(&self) -> bool {
        self.key.len() == 46
            && self.key.starts_with("g1_")
            && self.key.as_bytes()[3..]
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
    }
}

fn receipt(value: Value) -> Result<Value, String> {
    let fields = value.as_object().ok_or("invalid_response")?;
    if fields.len() != 5
        || value["schema_version"] != 1
        || value["status"] != "redeemed"
        || !matches!(value["plan"].as_str(), Some("pro" | "max" | "sponsor"))
    {
        return Err("invalid_response".into());
    }
    let lifetime = match value["duration"].as_str() {
        Some("lifetime") => true,
        Some("1m" | "3m" | "1y" | "3y") => false,
        _ => return Err("invalid_response".into()),
    };
    match fields.get("expires_at") {
        Some(Value::Null) if lifetime => Ok(value),
        Some(end)
            if !lifetime
                && end
                    .as_u64()
                    .is_some_and(|end| end > 0 && end <= 8_640_000_000_000) =>
        {
            Ok(value)
        }
        _ => Err("invalid_response".into()),
    }
}

fn refusal(body: &Value) -> &'static str {
    match body
        .pointer("/error/details/reason")
        .and_then(Value::as_str)
    {
        Some("invalid_key") => "invalid_key",
        Some("already_lifetime") => "already_lifetime",
        Some("rate_limited") => "rate_limited",
        _ => "unavailable",
    }
}

fn same_session(
    current: Option<&codeterm_account::Session>,
    expected: &codeterm_account::Session,
) -> bool {
    current.is_some_and(|current| {
        current.identity.uid == expected.identity.uid
            && current.login_generation == expected.login_generation
    })
}

impl ServiceContext {
    pub async fn redeem_gift_key(&self, request: GiftRedemption) -> Result<Value, String> {
        if !request.valid_key() {
            return Err("invalid_key".into());
        }
        let account = self.account().await.map_err(|_| "unavailable")?;
        let session = account
            .current_session()
            .await
            .map_err(|_| "sign_in_required")?
            .ok_or("sign_in_required")?;
        if session.identity.uid != request.expected_uid {
            return Err("account_changed".into());
        }
        let transport =
            HttpCallable::new(codeterm_account::firebase_endpoints::current().functions)
                .map_err(|_| "unavailable")?;
        let body = match transport
            .call(
                "redeemGiftKey",
                session.id_token.expose(),
                json!({"key": request.key}),
            )
            .await
        {
            Ok(body) => body,
            Err(CallableFailure::Status {
                body: Some(body), ..
            }) => {
                if codeterm_account::session_revocation::is_session_revoked(&body) {
                    self.session_revocations().revoked(&session).await;
                }
                return Err(refusal(&body).into());
            }
            Err(_) => return Err("unavailable".into()),
        };
        if !same_session(account.session().await.as_ref(), &session) {
            return Err("account_changed".into());
        }
        let result = receipt(body.get("result").cloned().ok_or("invalid_response")?)?;
        self.refresh_billing(codeterm_billing::RefreshEvent::PlanChanged)
            .await
            .map_err(|_| "refresh_unavailable")?;
        if !same_session(account.session().await.as_ref(), &session) {
            return Err("account_changed".into());
        }
        self.announce_limit_gate();
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
