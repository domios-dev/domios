use super::ServiceContext;
use codeterm_account::account_mesh::callable::{CallableFailure, CallableTransport, HttpCallable};
use serde::Deserialize;
use serde_json::{json, Value};

mod backend_reads;
pub mod capabilities;
mod confinement;
mod delivery;
mod flags;
mod handling;
mod recovery;
pub use flags::FlagCategory;
pub(crate) mod history;
mod launch_fence;
mod local_counts;
mod notifications;
mod pause;
mod removal;
pub use notifications::NeighborNotify;
mod projection;
mod reception;
pub(crate) mod runtime;
mod storage;
mod toggle;
pub(crate) mod updates;
pub use runtime::{NeighborRuntimeCell, NeighborRuntimeCommand};

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NeighborCommand {
    List {},
    Refresh {},
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
            Self::List {} | Self::Refresh {} => ("friendsList", json!({})),
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
        if matches!(&command, NeighborCommand::Refresh {}) {
            self.rearm_neighbor_reads();
        }
        self.neighbor_registry_owned(command).await
    }

    pub(super) async fn neighbor_registry_owned(
        &self,
        command: NeighborCommand,
    ) -> Result<Value, String> {
        if self.neighbor_access_locked().await
            && !matches!(
                &command,
                NeighborCommand::List {} | NeighborCommand::Refresh {}
            )
        {
            return match command {
                NeighborCommand::List {} | NeighborCommand::Refresh {} => {
                    Ok(backend_reads::locked_list())
                }
                _ => Err(codeterm_neighbors::Error::Locked.to_string()),
            };
        }
        let history_generation = self.neighbor_history_generation();
        let pause_versions = self.neighbor_cell().pauses.snapshot();
        let account = self.account().await?;
        let session = account
            .current_session()
            .await
            .map_err(|_| "Sign in to manage Neighbors")?
            .ok_or("Sign in to manage Neighbors")?;
        let base = codeterm_account::firebase_endpoints::current().functions;
        let transport = HttpCallable::new(base).map_err(|_| "Neighbors unavailable")?;
        let (name, data) = command.callable();
        let removed = if name == "friendsRemove" {
            data["friendship_id"].as_str().map(str::to_owned)
        } else {
            None
        };
        let response = if name == "friendsList" {
            use codeterm_neighbors::adapter::AuthenticatedCall;
            let calls = runtime::callable::Calls {
                service: self.clone(),
                uid: session.identity.uid.clone(),
                generation: session.login_generation,
            };
            let result = calls
                .invoke(name, data)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({"result": result}))
        } else {
            transport
                .call_for_account(
                    &session.identity.uid,
                    name,
                    session.id_token.expose(),
                    data,
                    &super::account::callable_admission::SessionGuard {
                        service: self,
                        expected: &session,
                        neighbors: true,
                    },
                )
                .await
        };
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
        if let Some(friendship) = removed.filter(|_| self.require_neighbors_enabled().is_ok()) {
            let metadata = storage::Metadata::new(self.account_data_root()?, &session.identity.uid);
            self.end_neighbor_friendship(&metadata, &friendship, history_generation)
                .await
                .map_err(|error| error.to_string())?;
        }
        if name == "friendsList" {
            let metadata = storage::Metadata::new(self.account_data_root()?, &session.identity.uid);
            self.reconcile_neighbor_pauses(&session.identity.uid, &result, &pause_versions)
                .map_err(|error| error.to_string())?;
            let settings = metadata.settings().await.map_err(|e| e.to_string())?;
            annotate_list(&mut result, &settings)?;
            self.project_neighbor_pauses(&mut result, &settings);
            self.project_neighbor_send_counts(&metadata, &mut result)
                .await
                .map_err(|error| error.to_string())?;
            self.remember_neighbor_names(&result);
            if self.require_neighbors_enabled().is_ok() {
                self.end_missing_neighbor_friendships(&metadata, &result, history_generation)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            if !same_session(account.session().await.as_ref(), &session) {
                return Err("Account changed while listing Neighbors".into());
            }
            self.require_neighbor_generation(history_generation)?;
            self.remember_neighbor_registry(&result);
        } else {
            self.invalidate_neighbor_list();
            self.forget_neighbor_registry();
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
        let gateway_tab = settings.bindings.get(id).cloned();
        let notify = settings.neighbor_notify(id);
        friend["permission"] = json!(permission);
        friend["gateway_tab"] = json!(gateway_tab);
        friend["notify"] = json!(notify);
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
