use super::{
    history::{Entry, HistoryCell},
    storage::{Metadata, Settings},
    NeighborNotify,
};
use crate::service::{
    notification::{NotificationOrigin, PushIntent},
    ServiceContext,
};
use codeterm_neighbors::wire::hex_id;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DENIED: &str = "neighbor_forbidden: This caller cannot flag this incoming Neighbor message";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlagCategory {
    SuspiciousInstructions,
    PrivateDataRequest,
    MaliciousLink,
    Spam,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum FlagBy {
    Concierge,
    Tab,
    Owner,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct MessageFlag {
    pub category: FlagCategory,
    pub reason: String,
    pub by: FlagBy,
    pub at: u64,
}

pub(super) struct FlagRequest {
    pub friendship: String,
    pub message: String,
    pub reason: String,
    pub category: FlagCategory,
}

struct Actor {
    key: String,
    by: FlagBy,
    label: String,
    tab: Option<String>,
}

fn reason(raw: &str, body: &str) -> Result<String, String> {
    if raw.chars().count() > 280 || raw.chars().any(char::is_control) {
        return Err(
            "invalid_flag: reason must be a short summary of 1 through 280 characters".into(),
        );
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(
            "invalid_flag: reason must be a short summary of 1 through 280 characters".into(),
        );
    }
    let body = body.trim();
    let summary = if !body.is_empty() && raw == body {
        "[message text omitted]".to_string()
    } else if body.chars().count() >= 8 {
        raw.replace(body, "[message text omitted]")
    } else {
        raw.to_string()
    };
    Ok(codeterm_util::redact::text(&summary)
        .chars()
        .take(280)
        .collect())
}

impl ServiceContext {
    fn flag_actor(
        &self,
        request: &FlagRequest,
        entry: &Entry,
        settings: &Settings,
        caller: Option<&str>,
        nonce: Option<&str>,
        uid: &str,
    ) -> Result<Actor, String> {
        let Some(caller) = caller else {
            return Ok(Actor {
                key: "owner".into(),
                by: FlagBy::Owner,
                label: "Owner".into(),
                tab: None,
            });
        };
        let mux = self.mux.read();
        let tab = mux.get_tab(caller).ok_or(DENIED)?;
        if nonce.is_none()
            || tab.session_binding_nonce.as_deref() != nonce
            || !tab.launch_session_authenticated()
        {
            return Err(DENIED.into());
        }
        let label = codeterm_util::redact::text(&tab.title)
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        let mut actor = Actor {
            key: format!("tab:{caller}"),
            by: FlagBy::Tab,
            label,
            tab: Some(caller.into()),
        };
        if crate::service::general_agent::is_general_agent(tab) && tab.neighbor_authority.is_none()
        {
            actor.by = FlagBy::Owner;
            return Ok(actor);
        }
        if tab
            .neighbor_authority
            .as_ref()
            .is_some_and(|a| a.uid != uid || a.friendship_id != request.friendship)
            || settings
                .bindings
                .iter()
                .any(|(friend, tab)| tab == caller && friend != &request.friendship)
        {
            return Err(DENIED.into());
        }
        if settings
            .bindings
            .get(&request.friendship)
            .is_some_and(|bound| bound == caller)
        {
            actor.by = FlagBy::Concierge;
            return Ok(actor);
        }
        let received = entry.received_by.as_ref().ok_or(DENIED)?;
        if received.tab_id != caller
            || tab.agent_session_id.as_deref() != Some(received.session_id.as_str())
            || nonce != Some(received.binding_nonce.as_str())
        {
            return Err(DENIED.into());
        }
        Ok(actor)
    }

    pub(super) async fn flag_neighbor_message(
        &self,
        friendship: &str,
        message: &str,
        raw_reason: &str,
        category: FlagCategory,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<Value, String> {
        if !hex_id(friendship, 64) || !hex_id(message, 32) {
            return Err("invalid_flag: invalid friendship or message id".into());
        }
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned()
            .ok_or("Sign in with a hosted account to use Neighbors")?;
        let session = account
            .session()
            .await
            .filter(|s| s.identity.provider != "local")
            .ok_or("Sign in with a hosted account to use Neighbors")?;
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        let generation = self.neighbor_history_generation();
        let request = FlagRequest {
            friendship: friendship.into(),
            message: message.into(),
            reason: raw_reason.into(),
            category,
        };
        let caller = caller.map(str::to_string);
        let nonce = nonce.map(str::to_string);
        let service = self.clone();
        tokio::spawn(async move {
            if !super::same_session(account.session().await.as_ref(), &session) {
                return Err("Neighbor account changed".into());
            }
            service
                .flag_neighbor_metadata(
                    &metadata,
                    &request,
                    caller.as_deref(),
                    nonce.as_deref(),
                    &session.identity.uid,
                    generation,
                )
                .await
        })
        .await
        .map_err(|_| "neighbor_flag_failed: local flag task stopped".to_string())?
    }

    pub(super) async fn flag_neighbor_metadata(
        &self,
        metadata: &Metadata,
        request: &FlagRequest,
        caller: Option<&str>,
        nonce: Option<&str>,
        uid: &str,
        generation: u64,
    ) -> Result<Value, String> {
        let _guard = self
            .extensions
            .get_or_init(HistoryCell::default)
            .0
            .lock()
            .await;
        self.require_neighbor_generation(generation)?;
        let settings = metadata.settings().await.map_err(|e| e.to_string())?;
        let mut history = metadata
            .history(&request.friendship)
            .await
            .map_err(|e| e.to_string())?;
        let entry = history
            .entries
            .iter_mut()
            .find(|e| e.direction == "in" && e.message_id == request.message)
            .ok_or(DENIED)?;
        let actor = self.flag_actor(request, entry, &settings, caller, nonce, uid)?;
        if let Some(index) = entry.flag_callers.iter().position(|key| key == &actor.key) {
            let flag = entry
                .flags
                .get(index)
                .ok_or("neighbor_flag_failed: invalid local flag metadata")?;
            return Ok(
                json!({"friendship_id":request.friendship,"message_id":request.message,"flag":flag,"created":false}),
            );
        }
        if entry.flags.len() >= 32 {
            return Err(
                "neighbor_flag_limit: this message has reached its local flag limit".into(),
            );
        }
        let flag = MessageFlag {
            category: request.category,
            reason: reason(&request.reason, &entry.body)?,
            by: actor.by,
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        entry.flags.push(flag.clone());
        entry.flag_callers.push(actor.key.clone());
        self.require_neighbor_generation(generation)?;
        metadata
            .write(format!("history-{}.json", request.friendship), history)
            .await
            .map_err(|e| e.to_string())?;
        self.ui.emit(
            "neighbor-history-changed",
            json!({"friendship_id":request.friendship}),
        );
        if settings.neighbor_notify(&request.friendship) != NeighborNotify::Off
            && self.configuration().neighbors.enabled
        {
            let friend = self
                .neighbor_cell_names()
                .get(&request.friendship)
                .cloned()
                .unwrap_or_else(|| "Neighbor".into());
            let service = self.clone();
            let friendship = request.friendship.clone();
            let message = request.message.clone();
            tokio::task::spawn_blocking(move || {
                service.require_neighbor_generation(generation)?;
                service.create_in_app_notice(
                    format!("{} flagged a message from {friend}", actor.label),
                    vec![
                        codeterm_notification_core::contract::NotificationAction::Url {
                            label: "Open Neighbor".into(),
                            value: format!("domios://neighbor/{friendship}"),
                        },
                    ],
                    NotificationOrigin::claimed(actor.tab),
                    PushIntent {
                        level: Some(crate::notification::push::NotifyLevel::Verbose),
                        event_key: Some(format!(
                            "neighbor-flag:{friendship}:{message}:{}",
                            actor.key
                        )),
                    },
                    false,
                );
                Ok::<_, String>(())
            })
            .await
            .map_err(|_| "neighbor_flag_failed: owner notification task stopped")??;
        }
        Ok(
            json!({"friendship_id":request.friendship,"message_id":request.message,"flag":flag,"created":true}),
        )
    }
}

#[cfg(test)]
mod tests;
