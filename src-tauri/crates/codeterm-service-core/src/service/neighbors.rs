use super::ServiceContext;
use codeterm_account::account_mesh::callable::{CallableFailure, CallableTransport, HttpCallable};
use serde::Deserialize;
use serde_json::{json, Value};

mod referral;
pub(crate) mod runtime;
mod storage;
pub use runtime::{NeighborRuntimeCell, NeighborRuntimeCommand};

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NeighborCommand {
    List {},
    ClaimReferralTrial {},
    Request {
        email: String,
    },
    Respond {
        request_id: String,
        accept: bool,
    },
    Remove {
        friendship_id: String,
    },
    SetLimit {
        friendship_id: String,
        per_hour: u16,
    },
}

impl NeighborCommand {
    fn callable(self) -> (&'static str, Value) {
        match self {
            Self::List {} => ("friendsList", json!({})),
            Self::ClaimReferralTrial {} => ("claimReferralTrial", json!({})),
            Self::Request { email } => ("friendsRequest", json!({"email": email})),
            Self::Respond { request_id, accept } => (
                "friendsRespond",
                json!({"request_id": request_id, "accept": accept}),
            ),
            Self::Remove { friendship_id } => {
                ("friendsRemove", json!({"friendship_id": friendship_id}))
            }
            Self::SetLimit {
                friendship_id,
                per_hour,
            } => (
                "friendsSetLimit",
                json!({"friendship_id": friendship_id, "per_hour": per_hour}),
            ),
        }
    }
}

impl ServiceContext {
    pub async fn neighbor_registry(&self, command: NeighborCommand) -> Result<Value, String> {
        let account = self.account().await?;
        let session = account
            .fresh_session()
            .await
            .map_err(|_| "Sign in to manage Neighbors")?
            .ok_or("Sign in to manage Neighbors")?;
        let base = codeterm_account::firebase_endpoints::current().functions;
        let transport = HttpCallable::new(base).map_err(|_| "Neighbors unavailable")?;
        let (name, data) = command.callable();
        let response = transport.call(name, session.id_token.expose(), data).await;
        let body = match response {
            Ok(body) => body,
            Err(CallableFailure::Status {
                body: Some(body), ..
            }) => {
                if codeterm_account::session_revocation::is_session_revoked(&body) {
                    self.session_revocations().revoked(&session).await;
                }
                return Err(neighbor_refusal(&body));
            }
            Err(_) => return Err("Neighbors unavailable".into()),
        };
        if !same_session(account.session().await.as_ref(), &session) {
            return Err("Account changed while managing Neighbors".into());
        }
        let mut result = body
            .get("result")
            .cloned()
            .ok_or_else(|| "Invalid Neighbors response".to_owned())?;
        if name == "friendsList" {
            let metadata = storage::Metadata::new(self.account_data_root()?, &session.identity.uid);
            let settings = metadata.settings().await.map_err(|e| e.to_string())?;
            annotate_list(&mut result, &settings)?;
            if !same_session(account.session().await.as_ref(), &session) {
                return Err("Account changed while listing Neighbors".into());
            }
        }
        if name == "claimReferralTrial" && referral::refresh_required(&result)? {
            self.refresh_billing(codeterm_billing::RefreshEvent::Explicit)
                .await?;
            if !same_session(account.session().await.as_ref(), &session) {
                return Err("Account changed while checking the referral".into());
            }
        }
        Ok(result)
    }
}

fn annotate_list(result: &mut Value, settings: &storage::Settings) -> Result<(), String> {
    let status = if result["access"] == "paid" {
        "connected"
    } else {
        "paused"
    };
    let friends = result["friends"]
        .as_array_mut()
        .filter(|friends| friends.len() <= 200)
        .ok_or("Invalid Neighbors response")?;
    for friend in friends {
        let id = friend["friendship_id"]
            .as_str()
            .ok_or("Invalid Neighbors response")?;
        let permission = settings.permissions.get(id).cloned().unwrap_or_default();
        friend["permission"] = json!(permission);
        friend["status"] = json!(status);
    }
    Ok(())
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

fn neighbor_refusal(body: &Value) -> String {
    match body
        .pointer("/error/details/reason")
        .and_then(Value::as_str)
    {
        Some("friends_locked") => "friends_locked: Upgrade to use your Neighbors channel".into(),
        Some("invite_requires_paid_plan") => {
            "invite_requires_paid_plan: Inviting Neighbors requires an active paid plan.".into()
        }
        Some("neighbor_limit") => "neighbor_limit: Your plan's Neighbor allowance is full".into(),
        Some("duplicate_request") => "A request or invitation has already been sent".into(),
        Some("self_request") => "Choose another person's email address".into(),
        Some("limit_out_of_range") => "Choose a limit from 1 to 200 requests per hour".into(),
        _ => "The Neighbors request could not be completed".into(),
    }
}

#[cfg(test)]
mod tests;
