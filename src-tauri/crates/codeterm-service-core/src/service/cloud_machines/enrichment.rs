use std::collections::HashMap;

use super::controller::CloudMachineController;
use super::types::{CloudLifecycleState, CloudMachineRecord, CloudReceipt};

pub fn extract_installation_instance_bindings(value: &serde_json::Value) -> Vec<(String, String)> {
    let Some(installations) = value
        .get("installations")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    installations
        .iter()
        .filter(|row| row.get("active").and_then(serde_json::Value::as_bool) != Some(false))
        .filter_map(|row| {
            let inst = row
                .get("installation_id")
                .and_then(serde_json::Value::as_str)?
                .trim();
            let peer = row
                .get("instance_id")
                .and_then(serde_json::Value::as_str)?
                .trim();
            (!inst.is_empty() && !peer.is_empty()).then(|| (inst.to_owned(), peer.to_owned()))
        })
        .collect()
}

pub fn enrich_peers_json(
    mut value: serde_json::Value,
    records: &[CloudMachineRecord],
    receipts: &[CloudReceipt],
    now_ms: u64,
) -> serde_json::Value {
    if receipts.is_empty() && records.is_empty() {
        return value;
    }
    let instance_to_installation: HashMap<String, String> =
        extract_installation_instance_bindings(&value)
            .into_iter()
            .map(|(inst, peer)| (peer, inst))
            .collect();
    let lookup = |id: &str| {
        records
            .iter()
            .find(|r| r.matches_selector(id))
            .map(|r| r.cost_summary(now_ms))
    };
    if let Some(installations) = value
        .get_mut("installations")
        .and_then(serde_json::Value::as_array_mut)
    {
        for row in installations {
            let inst_key = row
                .get("installation_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let peer_key = row
                .get("instance_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let summary = inst_key
                .as_deref()
                .and_then(&lookup)
                .or_else(|| peer_key.as_deref().and_then(&lookup));
            if let Some(summary) = summary {
                if let Some(obj) = row.as_object_mut() {
                    if !summary.name.trim().is_empty() {
                        let existing_name = obj.get("name").and_then(serde_json::Value::as_str);
                        let missing_name = crate::mesh::paired_resolve::clean_machine_name(
                            existing_name,
                            inst_key.as_deref().or(peer_key.as_deref()),
                        )
                        .is_none();
                        if missing_name {
                            obj.insert("name".into(), serde_json::json!(summary.name));
                        }
                    }
                    obj.insert("kind".into(), serde_json::json!("cloud"));
                    obj.insert(
                        "cloud".into(),
                        serde_json::to_value(summary).unwrap_or_default(),
                    );
                }
            }
        }
    }
    if let Some(paired) = value
        .get_mut("paired")
        .and_then(serde_json::Value::as_array_mut)
    {
        for peer in paired {
            let instance_id = peer
                .get("instance_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let instance_name = peer
                .get("instance_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let mapped_installation = instance_id
                .as_deref()
                .and_then(|id| instance_to_installation.get(id))
                .map(String::as_str);
            let summary = instance_id
                .as_deref()
                .and_then(&lookup)
                .or_else(|| mapped_installation.and_then(&lookup))
                .or_else(|| instance_name.as_deref().and_then(&lookup));
            if let Some(summary) = summary {
                if let Some(obj) = peer.as_object_mut() {
                    if !summary.name.trim().is_empty() {
                        obj.insert("instance_name".into(), serde_json::json!(summary.name));
                    }
                    obj.insert("kind".into(), serde_json::json!("cloud"));
                    obj.insert(
                        "cloud".into(),
                        serde_json::to_value(summary).unwrap_or_default(),
                    );
                }
            }
        }
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "cloud_machines".into(),
            serde_json::to_value(receipts).unwrap_or_default(),
        );
    }
    value
}

impl CloudMachineController {
    /// Backfills `peer_instance_id` on active cloud machine records from observed
    /// verified `installation_id -> instance_id` mesh bindings.
    pub fn observe_peer_instance_ids<'a>(
        &self,
        bindings: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> usize {
        let map: HashMap<&str, &str> = bindings
            .into_iter()
            .filter(|(inst, peer)| !inst.trim().is_empty() && !peer.trim().is_empty())
            .collect();
        if map.is_empty() {
            return 0;
        }
        let mut machines = self.machines.lock();
        let mut updated = 0usize;
        for rec in machines.iter_mut() {
            if rec.state == CloudLifecycleState::Deleted {
                continue;
            }
            let Some(inst_id) = rec.installation_id.as_deref() else {
                continue;
            };
            let Some(&peer_id) = map.get(inst_id) else {
                continue;
            };
            if rec.peer_instance_id.as_deref() != Some(peer_id) {
                rec.peer_instance_id = Some(peer_id.to_owned());
                updated += 1;
            }
        }
        if updated > 0 {
            if let Err(error) = self.persist_locked(&machines) {
                log::warn!("[cloud] failed to persist backfilled peer_instance_id: {error}");
            }
            drop(machines);
            self.notify_lifecycle();
        }
        updated
    }

    pub fn enrich_peers_value(&self, value: serde_json::Value) -> serde_json::Value {
        let bindings = extract_installation_instance_bindings(&value);
        if !bindings.is_empty() {
            self.observe_peer_instance_ids(
                bindings
                    .iter()
                    .map(|(inst, peer)| (inst.as_str(), peer.as_str())),
            );
        }
        let receipts = self.list();
        if receipts.is_empty() {
            return value;
        }
        let now_ms = self.now_ms();
        let records = self.list_records();
        enrich_peers_json(value, &records, &receipts, now_ms)
    }

    pub fn enrich_tab_list(&self, tabs: &mut [serde_json::Value]) {
        let records = self.list_records();
        if records.is_empty() {
            return;
        }
        let now_ms = self.now_ms();
        for tab in tabs {
            let peer = tab
                .get("remote_peer")
                .and_then(serde_json::Value::as_str)
                .or_else(|| tab.get("peer_id").and_then(serde_json::Value::as_str))
                .map(str::to_owned);
            let Some(summary) = peer
                .as_deref()
                .and_then(|target| records.iter().find(|r| r.matches_selector(target)))
                .map(|r| r.cost_summary(now_ms))
            else {
                continue;
            };
            if let Some(obj) = tab.as_object_mut() {
                obj.insert(
                    "cloud".into(),
                    serde_json::to_value(summary).unwrap_or_default(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
