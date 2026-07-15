use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const LOOP_WINDOW_SECS: i64 = 300;
const LOOP_THRESHOLD: u32 = 3;
const MAX_BACKOFF_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownReason {
    UserExit,
    Update,
    Restart,
    Signal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrashClassification {
    AbnormalExit,
    FastFail,
    PowerLossOrKill,
    CleanShutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessEpoch {
    pub epoch: String,
    pub pid: u32,
    pub started_at: i64,
    pub last_healthy_at: i64,
    pub build_hash: String,
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashRecord {
    pub classification: CrashClassification,
    pub previous: ProcessEpoch,
    pub detected_at: i64,
    pub new_epoch: String,
    pub supervisor_action: SupervisorAction,
    pub work_policy: WorkRecoveryPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCrashRecord {
    pub classification: CrashClassification,
    pub previous: ProcessEpoch,
    pub detected_at: i64,
    pub supervisor_action: SupervisorAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorAction {
    pub external_restart_required: bool,
    pub backoff_ms: u64,
    pub loop_protected: bool,
    pub crash_count_in_window: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkRecoveryPolicy {
    pub restore_session_snapshot: bool,
    pub resume_safe_idempotent: bool,
    pub ambiguous_execution: String,
    pub duplicate_worker_guard: String,
    pub report_route_guard: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanShutdownRecord {
    pub reason: ShutdownReason,
    pub epoch: Option<String>,
    pub pid: u32,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryState {
    pub current: ProcessEpoch,
    pub last_crash: Option<CrashRecord>,
    pub last_clean_shutdown: Option<CleanShutdownRecord>,
}

pub fn record_startup() -> RecoveryState {
    let now = now_ts();
    let previous = read_json::<ProcessEpoch>(&current_path());
    let clean = read_json::<CleanShutdownRecord>(&last_clean_path());
    let current = ProcessEpoch {
        epoch: uuid::Uuid::new_v4().to_string(),
        pid: std::process::id(),
        started_at: now,
        last_healthy_at: now,
        build_hash: env!("BUILD_HASH").to_string(),
        platform: platform_name().to_string(),
    };

    let pending = read_json::<PendingCrashRecord>(&pending_crash_path());
    let last_crash = pending
        .map(|pending| finalize_pending(pending, &current))
        .or_else(|| {
            previous.and_then(|prev| classify_previous_if_unclean(prev, &clean, &current, now))
        });

    write_json(&current_path(), &current);
    if let Some(crash) = &last_crash {
        write_json(&last_crash_path(), crash);
        append_crash_history(crash);
    } else {
        let _ = std::fs::remove_file(last_crash_path());
    }
    let _ = std::fs::remove_file(pending_crash_path());

    RecoveryState {
        current,
        last_crash,
        last_clean_shutdown: clean,
    }
}

pub fn heartbeat() {
    if let Some(mut current) = read_json::<ProcessEpoch>(&current_path()) {
        current.last_healthy_at = now_ts();
        write_json(&current_path(), &current);
    }
}

pub fn record_clean_shutdown(reason: ShutdownReason) {
    let current = read_json::<ProcessEpoch>(&current_path());
    let record = CleanShutdownRecord {
        reason,
        epoch: current.as_ref().map(|p| p.epoch.clone()),
        pid: std::process::id(),
        at: now_ts(),
    };
    write_json(&last_clean_path(), &record);
}

pub fn recovery_state() -> RecoveryState {
    let now = now_ts();
    let current = read_json::<ProcessEpoch>(&current_path()).unwrap_or(ProcessEpoch {
        epoch: "unknown".into(),
        pid: std::process::id(),
        started_at: now,
        last_healthy_at: now,
        build_hash: env!("BUILD_HASH").to_string(),
        platform: platform_name().to_string(),
    });
    RecoveryState {
        current,
        last_crash: read_json(&last_crash_path()),
        last_clean_shutdown: read_json(&last_clean_path()),
    }
}

fn classify_previous(previous: ProcessEpoch, current: &ProcessEpoch, now: i64) -> CrashRecord {
    let age = now.saturating_sub(previous.last_healthy_at);
    let classification = if age > LOOP_WINDOW_SECS {
        CrashClassification::PowerLossOrKill
    } else if now.saturating_sub(previous.started_at) < 30 {
        CrashClassification::FastFail
    } else {
        CrashClassification::AbnormalExit
    };
    let crash_count = crash_count_in_window(now) + 1;
    CrashRecord {
        classification,
        previous,
        detected_at: now,
        new_epoch: current.epoch.clone(),
        supervisor_action: SupervisorAction {
            external_restart_required: true,
            backoff_ms: bounded_backoff_ms(crash_count),
            loop_protected: crash_count >= LOOP_THRESHOLD,
            crash_count_in_window: crash_count,
        },
        work_policy: work_recovery_policy(),
    }
}

fn classify_previous_if_unclean(
    previous: ProcessEpoch,
    clean: &Option<CleanShutdownRecord>,
    current: &ProcessEpoch,
    now: i64,
) -> Option<CrashRecord> {
    (!clean_matches(clean, &previous)).then(|| classify_previous(previous, current, now))
}

fn finalize_pending(pending: PendingCrashRecord, current: &ProcessEpoch) -> CrashRecord {
    CrashRecord {
        classification: pending.classification,
        previous: pending.previous,
        detected_at: pending.detected_at,
        new_epoch: current.epoch.clone(),
        supervisor_action: pending.supervisor_action,
        work_policy: work_recovery_policy(),
    }
}

fn work_recovery_policy() -> WorkRecoveryPolicy {
    WorkRecoveryPolicy {
        restore_session_snapshot: true,
        resume_safe_idempotent: true,
        ambiguous_execution: "needs-review/reassign".into(),
        duplicate_worker_guard:
            "restore bindings before spawn; never spawn for an existing live attempt".into(),
        report_route_guard: "route reports through restored owner/planner bindings".into(),
    }
}

fn clean_matches(clean: &Option<CleanShutdownRecord>, previous: &ProcessEpoch) -> bool {
    clean.as_ref().is_some_and(|c| {
        c.epoch.as_deref() == Some(previous.epoch.as_str())
            && c.pid == previous.pid
            && c.at >= previous.started_at
            && c.at >= previous.last_healthy_at
    })
}

fn bounded_backoff_ms(crash_count: u32) -> u64 {
    let shift = crash_count.saturating_sub(1).min(5);
    (1_000u64 << shift).min(MAX_BACKOFF_SECS * 1_000)
}

fn crash_count_in_window(now: i64) -> u32 {
    read_history()
        .into_iter()
        .filter(|record| now.saturating_sub(record.detected_at) <= LOOP_WINDOW_SECS)
        .count() as u32
}

fn append_crash_history(record: &CrashRecord) {
    let mut records = read_history();
    records.push(record.clone());
    let cutoff = record.detected_at.saturating_sub(LOOP_WINDOW_SECS);
    records.retain(|r| r.detected_at >= cutoff);
    write_json(&history_path(), &records);
}

fn read_history() -> Vec<CrashRecord> {
    read_json(&history_path()).unwrap_or_default()
}

fn state_dir() -> PathBuf {
    let dir = crate::paths::data_dir().join("state").join("recovery");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn current_path() -> PathBuf {
    state_dir().join("current.json")
}

fn last_clean_path() -> PathBuf {
    state_dir().join("last_clean.json")
}

fn last_crash_path() -> PathBuf {
    state_dir().join("last_crash.json")
}

fn pending_crash_path() -> PathBuf {
    state_dir().join("pending_crash.json")
}

fn history_path() -> PathBuf {
    state_dir().join("crash_history.json")
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::error!("[recovery] create {}: {}", parent.display(), error);
            return;
        }
    }
    if let Ok(raw) = serde_json::to_string_pretty(value) {
        let result = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .and_then(|mut file| {
                file.write_all(raw.as_bytes())?;
                file.sync_data()
            });
        if let Err(error) = result {
            log::error!("[recovery] write {}: {}", path.display(), error);
        }
    }
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_shutdown_matching_epoch_is_not_crash() {
        let previous = ProcessEpoch {
            epoch: "e1".into(),
            pid: 1,
            started_at: 10,
            last_healthy_at: 20,
            build_hash: "h".into(),
            platform: "windows".into(),
        };
        let clean = Some(CleanShutdownRecord {
            reason: ShutdownReason::UserExit,
            epoch: Some("e1".into()),
            pid: 1,
            at: 30,
        });
        assert!(clean_matches(&clean, &previous));
    }

    #[test]
    fn clean_kill_sequence_writes_clean_record() {
        let previous = ProcessEpoch {
            epoch: "dev-run".into(),
            pid: 41,
            started_at: 10,
            last_healthy_at: 20,
            build_hash: "old".into(),
            platform: "windows".into(),
        };
        let clean = Some(CleanShutdownRecord {
            reason: ShutdownReason::Restart,
            epoch: Some("dev-run".into()),
            pid: 41,
            at: 30,
        });
        let current = ProcessEpoch {
            epoch: "rebuilt-run".into(),
            pid: 42,
            started_at: 1_000,
            last_healthy_at: 1_000,
            build_hash: "new".into(),
            platform: "windows".into(),
        };

        assert!(classify_previous_if_unclean(previous, &clean, &current, 1_000).is_none());
    }

    #[test]
    fn abnormal_termination_records_exactly_once() {
        let previous = ProcessEpoch {
            epoch: "unclean".into(),
            pid: 41,
            started_at: 10,
            last_healthy_at: 20,
            build_hash: "old".into(),
            platform: "windows".into(),
        };
        let current = ProcessEpoch {
            epoch: "first-restart".into(),
            pid: 42,
            started_at: 100,
            last_healthy_at: 100,
            build_hash: "new".into(),
            platform: "windows".into(),
        };

        let crash = classify_previous_if_unclean(previous, &None, &current, 100)
            .expect("unclean previous epoch must be recorded");
        assert_eq!(crash.previous.pid, 41);
        assert!((1_000..=MAX_BACKOFF_SECS * 1_000).contains(&crash.supervisor_action.backoff_ms));

        let clean = Some(CleanShutdownRecord {
            reason: ShutdownReason::Restart,
            epoch: Some(current.epoch.clone()),
            pid: current.pid,
            at: 110,
        });
        let next = ProcessEpoch {
            epoch: "second-restart".into(),
            pid: 43,
            started_at: 120,
            last_healthy_at: 120,
            build_hash: "new".into(),
            platform: "windows".into(),
        };
        assert!(classify_previous_if_unclean(current, &clean, &next, 120).is_none());
    }

    #[test]
    fn marker_older_than_last_heartbeat_does_not_hide_real_crash() {
        let previous = ProcessEpoch {
            epoch: "dev-run".into(),
            pid: 41,
            started_at: 10,
            last_healthy_at: 40,
            build_hash: "old".into(),
            platform: "windows".into(),
        };
        let clean = Some(CleanShutdownRecord {
            reason: ShutdownReason::Restart,
            epoch: Some("dev-run".into()),
            pid: 41,
            at: 30,
        });
        let current = ProcessEpoch {
            epoch: "next-run".into(),
            pid: 42,
            started_at: 1_000,
            last_healthy_at: 1_000,
            build_hash: "new".into(),
            platform: "windows".into(),
        };

        let crash = classify_previous_if_unclean(previous, &clean, &current, 1_000)
            .expect("activity after the marker must remain a crash");
        assert_eq!(crash.classification, CrashClassification::PowerLossOrKill);
    }

    #[test]
    fn backoff_is_bounded() {
        assert_eq!(bounded_backoff_ms(1), 1_000);
        assert_eq!(bounded_backoff_ms(6), 30_000);
        assert_eq!(bounded_backoff_ms(99), 30_000);
    }

    #[test]
    fn stale_epoch_clean_shutdown_does_not_match() {
        let previous = ProcessEpoch {
            epoch: "newer".into(),
            pid: 2,
            started_at: 20,
            last_healthy_at: 25,
            build_hash: "h".into(),
            platform: "windows".into(),
        };
        let clean = Some(CleanShutdownRecord {
            reason: ShutdownReason::Update,
            epoch: Some("older".into()),
            pid: 1,
            at: 30,
        });
        assert!(!clean_matches(&clean, &previous));
    }

    #[test]
    fn classifies_fast_fail_when_previous_epoch_died_immediately() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 100,
            last_healthy_at: 100,
            build_hash: "h2".into(),
            platform: "windows".into(),
        };
        let previous = ProcessEpoch {
            epoch: "old".into(),
            pid: 1,
            started_at: 90,
            last_healthy_at: 95,
            build_hash: "h1".into(),
            platform: "windows".into(),
        };
        let record = classify_previous(previous, &current, 100);
        assert_eq!(record.classification, CrashClassification::FastFail);
        assert_eq!(record.new_epoch, "new");
    }

    #[test]
    fn classifies_power_loss_or_kill_when_health_is_stale() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 1_000,
            last_healthy_at: 1_000,
            build_hash: "h2".into(),
            platform: "linux".into(),
        };
        let previous = ProcessEpoch {
            epoch: "old".into(),
            pid: 1,
            started_at: 10,
            last_healthy_at: 20,
            build_hash: "h1".into(),
            platform: "linux".into(),
        };
        let record = classify_previous(previous, &current, 1_000);
        assert_eq!(record.classification, CrashClassification::PowerLossOrKill);
    }

    #[test]
    fn classifies_abnormal_exit_for_recent_non_fast_failure() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 100,
            last_healthy_at: 100,
            build_hash: "h2".into(),
            platform: "macos".into(),
        };
        let previous = ProcessEpoch {
            epoch: "old".into(),
            pid: 1,
            started_at: 50,
            last_healthy_at: 95,
            build_hash: "h1".into(),
            platform: "macos".into(),
        };
        let record = classify_previous(previous, &current, 100);
        assert_eq!(record.classification, CrashClassification::AbnormalExit);
    }

    #[test]
    fn pending_external_classification_keeps_action_and_binds_new_epoch() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 100,
            last_healthy_at: 100,
            build_hash: "h2".into(),
            platform: "windows".into(),
        };
        let pending = PendingCrashRecord {
            classification: CrashClassification::FastFail,
            previous: ProcessEpoch {
                epoch: "external-launch".into(),
                pid: 1,
                started_at: 90,
                last_healthy_at: 90,
                build_hash: "unknown".into(),
                platform: "windows".into(),
            },
            detected_at: 95,
            supervisor_action: SupervisorAction {
                external_restart_required: true,
                backoff_ms: 2_000,
                loop_protected: false,
                crash_count_in_window: 2,
            },
        };
        let record = finalize_pending(pending, &current);
        assert_eq!(record.classification, CrashClassification::FastFail);
        assert_eq!(record.new_epoch, "new");
        assert_eq!(record.supervisor_action.crash_count_in_window, 2);
    }
}
