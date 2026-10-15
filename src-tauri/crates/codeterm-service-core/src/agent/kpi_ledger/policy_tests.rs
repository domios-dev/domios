use super::*;
/// The policy this lane settled: recording is on unless someone opts out, and the gate
/// every writer consults follows the config rather than a second switch.
#[test]
fn kpi_policy_default_or_explicit_opt_in() {
    use crate::agent::kpi::KpiRecorder;
    use crate::config::schema::{AppConfig, KpiConfig, KPI_ENABLED_KEY};

    let default_config = AppConfig::default();
    assert!(
        default_config.kpi.enabled,
        "a fresh install records: both preconditions the flag waited on now ship"
    );
    assert!(kpi_ledger_enabled(&default_config));

    let opted_out = AppConfig {
        kpi: KpiConfig {
            enabled: false,
            ..Default::default()
        },
        ..AppConfig::default()
    };
    assert!(
        !kpi_ledger_enabled(&opted_out),
        "the opt-out is honoured by the single gate every writer consults"
    );

    let dir = std::env::temp_dir().join(format!("kpi-policy-{}", ulid::Ulid::new()));
    let recorder = KpiRecorder::from_config(&opted_out, &dir);
    assert!(!recorder.is_enabled());
    assert!(
        !recorder.open_failed(),
        "an opt-out is not a ledger failure, so the surface never blames the ledger"
    );
    assert!(recorder.ledger().is_none());
    assert!(
        !dir.join("kpi_ledger.db").exists(),
        "opting out touches no disk at all"
    );

    let enabled = KpiRecorder::from_config(&AppConfig::default(), &dir);
    assert!(enabled.is_enabled());
    assert!(enabled.ledger().is_some());
    drop(enabled);

    // The key the not-recording surface names must be a real path into this config.
    let as_json = serde_json::to_value(&default_config).unwrap();
    let mut cursor = &as_json;
    for segment in KPI_ENABLED_KEY.split('.') {
        cursor = cursor.get(segment).unwrap_or_else(|| {
            panic!(
                "{} must address a real config field, missing {}",
                KPI_ENABLED_KEY, segment
            )
        });
    }
    assert_eq!(cursor.as_bool(), Some(true));

    std::fs::remove_dir_all(&dir).ok();
}
