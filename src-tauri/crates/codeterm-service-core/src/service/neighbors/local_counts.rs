use super::storage::Metadata;
use crate::service::ServiceContext;
use codeterm_neighbors::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default, Deserialize, Serialize)]
struct Counts {
    day: u64,
    friends: BTreeMap<String, u64>,
}
impl Counts {
    fn record(&mut self, friendship: &str, now: u64) -> Result<()> {
        let day = now / 86400;
        if self.day != day {
            self.day = day;
            self.friends.clear();
        }
        if self.friends.len() >= 200 && !self.friends.contains_key(friendship) {
            return Err(Error::CursorStore);
        }
        let count = self.friends.entry(friendship.to_owned()).or_default();
        *count = count.saturating_add(1);
        Ok(())
    }
    fn project(&self, list: &mut Value, now: u64) {
        if let Some(friends) = list["friends"].as_array_mut() {
            for friend in friends {
                let count = if self.day == now / 86400 {
                    friend["friendship_id"]
                        .as_str()
                        .and_then(|id| self.friends.get(id))
                        .copied()
                        .unwrap_or(0)
                } else {
                    0
                };
                friend["sent_today"] = json!(count);
            }
        }
    }
}

impl ServiceContext {
    pub(super) async fn project_neighbor_send_counts(
        &self,
        metadata: &Metadata,
        list: &mut Value,
    ) -> Result<()> {
        let counts: Counts = metadata
            .read("sent-today.json".into())
            .await?
            .unwrap_or_default();
        counts.project(list, super::projection::now_seconds());
        Ok(())
    }
    pub(super) async fn record_neighbor_send(
        &self,
        metadata: &Metadata,
        uid: &str,
        friendship: &str,
        generation: u64,
    ) -> Result<()> {
        self.require_neighbor_generation(generation)
            .map_err(|_| Error::Revoked)?;
        let mut counts: Counts = metadata
            .read("sent-today.json".into())
            .await?
            .unwrap_or_default();
        counts.record(friendship, super::projection::now_seconds())?;
        metadata.write("sent-today.json".into(), counts).await?;
        self.require_neighbor_generation(generation)
            .map_err(|_| Error::Revoked)?;
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .ok_or(Error::Revoked)?;
        let session = account.session().await.ok_or(Error::Revoked)?;
        let installation = self
            .installation_id()
            .await
            .map_err(|_| Error::Transport)?
            .as_str()
            .to_owned();
        if let Some(mut list) = self.peek_neighbor_projection(
            uid,
            session.login_generation,
            "friendsList",
            &installation,
        ) {
            let settings = metadata.settings().await?;
            super::annotate_list(&mut list, &settings).map_err(|_| Error::InvalidEnvelope)?;
            self.project_neighbor_pauses(&mut list, &settings);
            self.project_neighbor_send_counts(metadata, &mut list)
                .await?;
            self.ui
                .emit("neighbors-changed", json!({"uid":uid,"list":list}));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
