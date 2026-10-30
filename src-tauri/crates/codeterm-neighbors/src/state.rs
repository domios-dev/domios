use crate::{wire::{hex_id, key_id, uid, EffectiveReceiver, PeerReceiver, MAX_SAFE_INTEGER}, Error, Result};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Snapshot {
    value: Value,
    pub version: u64,
}

fn number(value: &Value) -> bool { value.as_u64().is_some_and(|value| value <= MAX_SAFE_INTEGER) }
fn text(value: &Value) -> bool { value.as_str().is_some_and(|value| !value.is_empty() && value.len() <= 512) }
fn nullable_text(value: &Value) -> bool { value.is_null() || text(value) }
fn nullable_number(value: &Value) -> bool { value.is_null() || number(value) }
fn profile(row: &Value) -> bool {
    row["uid"].as_str().is_some_and(uid) && nullable_text(&row["displayName"])
        && nullable_text(&row["photoURL"]) && nullable_text(&row["email"])
}
fn rows<'a>(value: &'a Value, field: &str) -> Result<&'a [Value]> {
    value[field].as_array().filter(|rows| rows.len() <= 200).map(Vec::as_slice).ok_or(Error::InvalidEnvelope)
}

impl Snapshot {
    pub fn parse(value: Value) -> Result<Self> {
        if value["schema_version"] != 1 || !number(&value["version"]) || !number(&value["updated_at"])
            || !matches!(value["access"].as_str(), Some("paid" | "locked")) || !value["can_invite"].is_boolean() {
            return Err(Error::InvalidEnvelope);
        }
        for friend in rows(&value, "friends")? {
            let receiver: EffectiveReceiver = serde_json::from_value(friend["my_receiver"].clone()).map_err(|_| Error::InvalidEnvelope)?;
            let peer: PeerReceiver = serde_json::from_value(friend["peer_receiver"].clone()).map_err(|_| Error::InvalidEnvelope)?;
            if !number(&friend["inbound_per_hour"]) || !number(&friend["outbound_per_hour"]) || !profile(friend) || !friend["friendship_id"].as_str().is_some_and(|id| hex_id(id, 64)) || !number(&friend["since"])
                || !friend["receiving"].is_boolean() || friend["receiving"] != friend["peer_receiver"]["receiving"]
                || !friend["my_paused"].is_boolean() || !friend["their_paused"].is_boolean()
                || ["my_daily_limit", "their_daily_limit", "my_file_daily_limit_bytes", "their_file_daily_limit_bytes"].iter().any(|field| !nullable_number(&friend[*field]))
                || receiver.revision > MAX_SAFE_INTEGER || peer.revision > MAX_SAFE_INTEGER
                || receiver.installation_id.as_ref().is_some_and(|id| id.is_empty() || id.len() > 512)
                || receiver.enc_key_id.as_deref().is_some_and(|id| !key_id(id))
                || peer.enc_key_id.as_deref().is_some_and(|id| !key_id(id))
                || receiver.receiving && (receiver.installation_id.is_none() || receiver.enc_key_id.is_none())
                || peer.receiving && peer.enc_key_id.is_none() {
                return Err(Error::InvalidEnvelope);
            }
        }
        for request in rows(&value, "incoming")? {
            if !profile(request) || !request["request_id"].as_str().is_some_and(|id| hex_id(id, 64)) || !number(&request["created_at"]) {
                return Err(Error::InvalidEnvelope);
            }
        }
        for request in rows(&value, "outgoing")? {
            if !request["id"].as_str().is_some_and(|id| hex_id(id, 64)) || !text(&request["typed_email"])
                || !number(&request["created_at"]) || request["state"] != "pending" { return Err(Error::InvalidEnvelope); }
        }
        for key in rows(&value, "own_keys")? {
            if !key["verifier_key_id"].as_str().is_some_and(key_id) || !(key["enc_key_id"].is_null() || key["enc_key_id"].as_str().is_some_and(key_id))
                || !nullable_text(&key["installation_id"]) || !nullable_text(&key["machine_label"]) || !number(&key["created_at"]) {
                return Err(Error::InvalidEnvelope);
            }
        }
        let trial = &value["referral_trial"];
        let invalid_trial_end = if trial["state"] == "granted" {
            !number(&trial["trial_end"]) || trial["trial_end"] == 0
        } else {
            !trial["trial_end"].is_null()
        };
        if !matches!(trial["state"].as_str(), Some("granted" | "already_paid" | "none"))
            || invalid_trial_end
            || !nullable_text(&trial["inviter_uid"])
            || !(trial["friend_request_id"].is_null() || trial["friend_request_id"].as_str().is_some_and(|id| hex_id(id, 64))) {
            return Err(Error::InvalidEnvelope);
        }
        Ok(Self { version: value["version"].as_u64().ok_or(Error::InvalidEnvelope)?, value })
    }

    pub fn list(&self, now: u64) -> Value {
        let mut list = self.value.clone();
        if let Some(fields) = list.as_object_mut() {
            for field in ["own_keys", "updated_at"] { fields.remove(field); }
        }
        if let Some(friends) = list["friends"].as_array_mut() {
            for friend in friends {
                friend["receiving"] = friend["peer_receiver"]["receiving"].clone();
            }
        }
        if let Some(outgoing) = list["outgoing"].as_array_mut() {
            outgoing.retain(|invite| invite["created_at"].as_u64().is_some_and(|created| now < created.saturating_add(30 * 86400)));
        }
        list
    }

    pub fn read(&self, route: &str, installation: &str, now: u64) -> Option<Value> {
        match route {
            "friendsList" => Some(self.list(now)),
            "friendsOwnKeys" => Some(self.value["own_keys"].clone()),
            "friendsReceiverStatus" => Some(json!({"schema_version":1,"receivers":self.value["friends"].as_array()?.iter().map(|friend| {
                let receiver = &friend["my_receiver"];
                json!({"friendship_id":friend["friendship_id"],"machine_label":receiver["machine_label"],
                    "revision":receiver["revision"],"receiving":receiver["receiving"],"is_me":receiver["installation_id"] == installation})
            }).collect::<Vec<_>>()})),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct AccountSnapshot {
    account: Option<(String, u64)>,
    snapshot: Option<Snapshot>,
}

impl AccountSnapshot {
    pub fn select(&mut self, uid: &str, generation: u64) {
        let account = (uid.to_owned(), generation);
        if self.account.as_ref() != Some(&account) { self.account = Some(account); self.snapshot = None; }
    }
    pub fn apply(&mut self, uid: &str, generation: u64, snapshot: Snapshot) -> bool {
        if self.account.as_ref() != Some(&(uid.to_owned(), generation))
            || self.snapshot.as_ref().is_some_and(|old| snapshot.version <= old.version) { return false; }
        self.snapshot = Some(snapshot);
        true
    }
    pub fn read(&self, uid: &str, generation: u64, route: &str, installation: &str, now: u64) -> Option<Value> {
        if self.account.as_ref() != Some(&(uid.to_owned(), generation)) { return None; }
        self.snapshot.as_ref()?.read(route, installation, now)
    }
    pub fn version(&self, uid: &str, generation: u64) -> Option<u64> {
        if self.account.as_ref() != Some(&(uid.to_owned(), generation)) { return None; }
        self.snapshot.as_ref().map(|snapshot| snapshot.version)
    }
}

pub enum StateEvent { Snapshot(Snapshot), Missing }

#[async_trait::async_trait]
pub trait StateListener: Send {
    async fn next(&mut self) -> Result<Option<StateEvent>>;
}

#[cfg(test)]
mod tests;
