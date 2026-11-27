use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const MAX_VOLUME_SIZE_GB: u32 = 500;
pub const DEFAULT_HOT_RETENTION_DAYS: f64 = 14.0;
pub const DEFAULT_COLD_PURGE_DAYS: f64 = 90.0;
pub const DEFAULT_HOT_GB_MONTH_RATE_CENTS: u64 = 15;
pub const AUX_KEY_RECOVERABLE_ARCHIVE_ID: &str = "recoverable_archive_id";
pub const AUX_KEY_STORAGE_TIER: &str = "storage_tier";
pub const AUX_KEY_PURGE_AT: &str = "purge_at";
pub const AUX_KEY_ALLOCATED_GB: &str = "allocated_gb";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudStorageTier {
    #[default]
    Hot,
    Evicting,
    Cold,
    RecoverableDeleted,
    Purged,
}

impl CloudStorageTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hot => "hot",
            Self::Evicting => "evicting",
            Self::Cold => "cold",
            Self::RecoverableDeleted => "recoverable_deleted",
            Self::Purged => "purged",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudDeleteMode {
    #[default]
    Permanent,
    Recoverable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSnapshotResidual {
    pub provider: String,
    pub snapshots_remaining: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earliest_expiry_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_expiry_at: Option<u64>,
    pub auto_backup_enabled: bool,
    pub individual_delete_supported: bool,
    pub erasure_state: String,
    pub checked_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudMachineStorageUsage {
    pub machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<u64>,
    pub tier: CloudStorageTier,
    pub allocated_gb: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_gb: Option<f64>,
    pub hot_monthly_cost_cents: u64,
    pub hot_accrued_cents: f64,
    pub cold_included: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot_until_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_since_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_bytes: Option<u64>,
    pub archive_generation: u64,
    pub disk_full: bool,
    pub volume_upgrade_max_gb: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_residual: Option<ProviderSnapshotResidual>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudStorageUsageSummary {
    pub hot_gb_month_rate_cents: u64,
    pub hot_days: f64,
    pub purge_days: f64,
    pub purge_warning_days: [f64; 2],
    pub cold_archive_included: bool,
    pub total_allocated_gb: u32,
    pub total_used_bytes: u64,
    pub total_hot_monthly_cost_cents: u64,
    #[serde(default)]
    pub machines: Vec<CloudMachineStorageUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudDeleteReceipt {
    pub machine_id: String,
    pub storage_tier: CloudStorageTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_residual: Option<ProviderSnapshotResidual>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudVolumeResizeReply {
    pub machine_id: String,
    pub allocated_gb: u32,
    pub needs_restart: bool,
    pub hot_monthly_cost_cents: u64,
}

pub struct CloudColdWakeParams {
    pub controller_key: String,
    pub creation_psk: Zeroizing<String>,
    pub archive_id: Option<String>,
}

pub fn validate_volume_upgrade_gb(
    current_gb: Option<u32>,
    requested_gb: u32,
) -> Result<(), String> {
    if requested_gb == 0 || requested_gb > MAX_VOLUME_SIZE_GB {
        return Err(format!(
            "requested volume size must be between 1 and {MAX_VOLUME_SIZE_GB} GB"
        ));
    }
    if let Some(current) = current_gb {
        if requested_gb <= current {
            return Err(format!(
                "requested volume size ({requested_gb} GB) must be larger than current allocation ({current} GB)"
            ));
        }
    }
    Ok(())
}

pub fn is_local_disk_full(used_bytes: Option<u64>, allocated_gb: u32) -> bool {
    let Some(used) = used_bytes else {
        return false;
    };
    let capacity = u64::from(allocated_gb).saturating_mul(1024 * 1024 * 1024);
    if capacity == 0 {
        return false;
    }
    (used as f64) / (capacity as f64) >= 0.98
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_upgrade_validation_is_grow_only_and_bounded() {
        assert!(validate_volume_upgrade_gb(Some(10), 25).is_ok());
        assert!(validate_volume_upgrade_gb(Some(10), 10).is_err());
        assert!(validate_volume_upgrade_gb(Some(10), 5).is_err());
        assert!(validate_volume_upgrade_gb(None, 0).is_err());
        assert!(validate_volume_upgrade_gb(None, 501).is_err());
    }

    #[test]
    fn local_disk_full_threshold_triggers_only_at_98_percent() {
        let ten_gb = 10u64 * 1024 * 1024 * 1024;
        assert!(!is_local_disk_full(Some(ten_gb / 2), 10));
        assert!(is_local_disk_full(Some((ten_gb * 99) / 100), 10));
        assert!(!is_local_disk_full(None, 10));
    }
}
