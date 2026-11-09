use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum BillingMode {
    Subscription,
    Api,
    #[default]
    Unknown,
}

impl BillingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subscription => "subscription",
            Self::Api => "api",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct UsageSnapshotRow {
    pub id: i64,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub usage_details: Option<serde_json::Value>,
    pub provider: String,
    #[serde(default)]
    pub billing_mode: BillingMode,
    #[serde(default)]
    pub limits_reported: bool,
    pub account_id: Option<String>,
    pub captured_at: i64,
    pub parsed_ok: bool,
    pub session_pct: Option<f32>,
    pub session_resets_at: Option<i64>,
    pub weekly_pct: Option<f32>,
    pub weekly_resets_at: Option<i64>,
    #[serde(default)]
    pub spend_cents: Option<f64>,
    #[serde(default)]
    pub spend_limit_cents: Option<f64>,
    #[serde(default)]
    pub spend_remaining_cents: Option<f64>,
    #[serde(default)]
    pub spend_resets_at: Option<i64>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub probe_command: Option<String>,
    #[serde(default)]
    pub raw_excerpt: Option<String>,
    #[serde(default)]
    pub debug_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum UsageSnapshotEvent<'a> {
    Snapshot {
        provider: &'a str,
        account_id: Option<&'a str>,
        snapshot_id: i64,
    },
    Runtime {
        provider: &'a str,
        account_id: Option<&'a str>,
    },
    Billing {
        provider: &'a str,
        account_id: Option<&'a str>,
        tab_id: &'a str,
        captured_at: u64,
    },
}

#[cfg(test)]
mod tests;
