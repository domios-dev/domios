use super::controller::CloudMachineController;

impl CloudMachineController {
    pub fn enrich_peers_value(&self, mut value: serde_json::Value) -> serde_json::Value {
        let receipts = self.list();
        if receipts.is_empty() {
            return value;
        }
        let now_ms = self.now_ms();
        let records = self.list_records();
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
                let key = row
                    .get("installation_id")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| row.get("instance_id").and_then(serde_json::Value::as_str))
                    .map(str::to_owned);
                if let Some(summary) = key.as_deref().and_then(lookup) {
                    if let Some(obj) = row.as_object_mut() {
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
                let key = peer
                    .get("instance_id")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        peer.get("instance_name")
                            .and_then(serde_json::Value::as_str)
                    })
                    .map(str::to_owned);
                if let Some(summary) = key.as_deref().and_then(lookup) {
                    if let Some(obj) = peer.as_object_mut() {
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
