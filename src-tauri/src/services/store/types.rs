//! Service entity types — single source of truth for backend + REST + frontend.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceKind {
    Cron,
    Daemon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthMode {
    Http,
    Tcp,
    Process,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Down,
    Unknown,
}

impl Default for HealthStatus {
    fn default() -> Self {
        HealthStatus::Unknown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealthSnapshot {
    pub status: HealthStatus,
    pub probed_at: Option<i64>,
    pub latency_ms: Option<u64>,
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
    #[serde(default)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldMapping {
    pub mappings: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub kind: ServiceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_path: Option<String>,
    pub health_mode: HealthMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_path: Option<String>,
    pub health_interval: u32,
    pub health_ttl: u32,
    pub health: HealthSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_mapping: Option<FieldMapping>,
    pub push_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_status: Option<String>,
    pub registered_at: i64,
    pub updated_at: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceCreate {
    pub name: String,
    pub kind: ServiceKind,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub process_label: Option<String>,
    #[serde(default)]
    pub script_path: Option<String>,
    pub health_mode: HealthMode,
    #[serde(default)]
    pub health_path: Option<String>,
    #[serde(default)]
    pub health_interval: Option<u32>,
    #[serde(default)]
    pub field_mapping: Option<FieldMapping>,
    #[serde(default)]
    pub push_enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServiceUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub process_label: Option<String>,
    #[serde(default)]
    pub script_path: Option<String>,
    #[serde(default)]
    pub health_mode: Option<HealthMode>,
    #[serde(default)]
    pub health_path: Option<String>,
    #[serde(default)]
    pub health_interval: Option<u32>,
    #[serde(default)]
    pub field_mapping: Option<FieldMapping>,
    #[serde(default)]
    pub push_enabled: Option<bool>,
    #[serde(default)]
    pub enabled: Option<bool>,
}
