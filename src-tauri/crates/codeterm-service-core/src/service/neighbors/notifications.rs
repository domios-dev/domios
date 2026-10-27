use super::{
    history::Entry,
    storage::{Metadata, Settings},
};
use crate::service::{
    notification::{NotificationOrigin, PushIntent},
    ServiceContext,
};
use codeterm_neighbors::{
    wire::{hex_id, Kind},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NeighborNotify {
    Off,
    #[default]
    InApp,
    InAppAndBridges,
}

pub(super) struct Arrival {
    message_id: String,
    kind: Kind,
    preview: String,
}

pub(super) struct AgentNotice {
    agent: String,
    reply: bool,
}

impl AgentNotice {
    pub(super) fn for_author(author: &super::history::Author, reply: bool) -> Option<Self> {
        match author {
            super::history::Author::Agent { agent, .. } => Some(Self {
                agent: agent.clone(),
                reply,
            }),
            _ => None,
        }
    }
}

enum Notice {
    Arrival(Arrival),
    Rejected(AgentNotice, super::delivery::Rejection),
}

fn short_line(value: &str, limit: usize) -> String {
    let line = value.lines().next().unwrap_or_default();
    let mut chars = line.chars().filter(|ch| !ch.is_control());
    let mut text: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        text.push('…');
    }
    text
}

impl Arrival {
    pub(super) fn from_entry(entry: &Entry) -> Option<Self> {
        (entry.direction == "in").then(|| Self {
            message_id: entry.message_id.clone(),
            kind: entry.kind,
            preview: short_line(&entry.body, 120),
        })
    }
}

impl Settings {
    pub(super) fn neighbor_notify(&self, friendship: &str) -> NeighborNotify {
        self.notifications
            .get(friendship)
            .copied()
            .unwrap_or_default()
    }
}

impl ServiceContext {
    pub(super) fn neighbor_notification_preferences(settings: &Settings, friends: &Value) -> Value {
        let mut preferences = serde_json::Map::new();
        if let Some(friends) = friends["friends"].as_array() {
            for friend in friends {
                if let Some(id) = friend["friendship_id"].as_str() {
                    preferences.insert(id.into(), json!(settings.neighbor_notify(id)));
                }
            }
        }
        Value::Object(preferences)
    }

    pub(super) fn remember_neighbor_names(&self, friends: &Value) {
        let mut names = std::collections::HashMap::new();
        if let Some(friends) = friends["friends"].as_array() {
            for friend in friends.iter().take(200) {
                if let Some(id) = friend["friendship_id"].as_str().filter(|id| hex_id(id, 64)) {
                    let name = friend["displayName"]
                        .as_str()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or("Neighbor");
                    names.insert(id.to_owned(), short_line(name, 48));
                }
            }
        }
        *self.neighbor_cell_names() = names;
    }

    pub(super) async fn notify_neighbor_arrival(
        &self,
        metadata: &Metadata,
        friendship: &str,
        arrival: Arrival,
        generation: u64,
    ) -> Result<()> {
        self.notify_neighbor(metadata, friendship, Notice::Arrival(arrival), generation)
            .await
    }

    pub(super) async fn notify_neighbor_rejection(
        &self,
        metadata: &Metadata,
        friendship: &str,
        agent: AgentNotice,
        rejection: super::delivery::Rejection,
        generation: u64,
    ) -> Result<()> {
        self.notify_neighbor(
            metadata,
            friendship,
            Notice::Rejected(agent, rejection),
            generation,
        )
        .await
    }

    async fn notify_neighbor(
        &self,
        metadata: &Metadata,
        friendship: &str,
        notice: Notice,
        generation: u64,
    ) -> Result<()> {
        self.require_neighbor_history_generation(generation)?;
        let settings = metadata.settings().await?;
        let preference = settings.neighbor_notify(friendship);
        if preference == NeighborNotify::Off {
            return Ok(());
        }
        let name = self
            .neighbor_cell_names()
            .get(friendship)
            .cloned()
            .unwrap_or_else(|| "Neighbor".into());
        let (title, gateway, event_key) = match notice {
            Notice::Arrival(arrival) => {
                let verb = match arrival.kind {
                    Kind::Answer => "replied",
                    Kind::Refusal => "refused",
                    _ => "asked",
                };
                (
                    format!("{name} {verb}: {}", arrival.preview),
                    settings.bindings.get(friendship).cloned(),
                    format!("neighbor-arrival:{friendship}:{}", arrival.message_id),
                )
            }
            Notice::Rejected(agent, rejection) => {
                let subject = if agent.reply {
                    "your concierge's reply"
                } else {
                    "your agent's message"
                };
                (
                    format!(
                        "{name}: {subject} was not delivered ({})",
                        rejection.description()
                    ),
                    Some(agent.agent),
                    format!("neighbor-rejected:{friendship}:{}", uuid::Uuid::new_v4()),
                )
            }
        };
        let friendship = friendship.to_owned();
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            service.require_neighbor_history_generation(generation)?;
            service.create_in_app_notice(
                title,
                vec![
                    codeterm_notification_core::contract::NotificationAction::Url {
                        label: "Open Neighbor".into(),
                        value: format!("domios://neighbor/{friendship}"),
                    },
                ],
                NotificationOrigin::claimed(gateway),
                PushIntent {
                    level: Some(crate::notification::push::NotifyLevel::Verbose),
                    event_key: Some(event_key),
                },
                preference == NeighborNotify::InAppAndBridges,
            );
            Ok(())
        })
        .await
        .map_err(|_| Error::Gateway)?
    }
}

#[cfg(test)]
mod tests;
