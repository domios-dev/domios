use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

const LOOP_WINDOW_SECS: i64 = 300;
const LOOP_THRESHOLD: u32 = 3;
const MAX_BACKOFF_SECS: u64 = 30;
const FAST_FAIL_SECS: i64 = 30;
static RECOVERY_HEAL_PENDING: AtomicBool = AtomicBool::new(false);

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
    /// Set once the new epoch restored every binding the dead one held. The
    /// record survives as audit; the warning does not.
    #[serde(default)]
    pub healed_at: Option<i64>,
    #[serde(default)]
    pub bindings: Option<BindingEvidence>,
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
    /// `None` until restored bindings prove work was in flight; absence of
    /// evidence never becomes an ambiguous-work claim.
    pub ambiguous_execution: Option<AmbiguousWork>,
    pub duplicate_worker_guard: String,
    pub report_route_guard: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AmbiguousWork {
    pub missing_bindings: u32,
    pub disposition: String,
}

/// What the dead epoch had bound versus what actually came back. This is the
/// only input to ambiguity and health — never elapsed time.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingEvidence {
    pub bound_at_shutdown: u32,
    pub restored: u32,
    #[serde(default)]
    pub bound_identities: Vec<BindingIdentity>,
    #[serde(default)]
    pub restored_identities: Vec<BindingIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct BindingIdentity {
    pub tab_id: String,
    pub agent_session_id: Option<String>,
}

impl BindingIdentity {
    pub fn healthy_lazy_restored(
        tab_id: &str,
        agent_session_id: Option<&str>,
        agent_state_seeded: bool,
    ) -> Option<Self> {
        agent_state_seeded.then(|| Self {
            tab_id: tab_id.to_string(),
            agent_session_id: agent_session_id.map(str::to_string),
        })
    }
}

impl BindingEvidence {
    pub fn missing(&self) -> u32 {
        if !self.bound_identities.is_empty() {
            return self
                .bound_identities
                .iter()
                .filter(|binding| !self.restored_identities.contains(binding))
                .count() as u32;
        }
        self.bound_at_shutdown.saturating_sub(self.restored)
    }

    pub fn fully_restored(&self) -> bool {
        self.missing() == 0
    }

    pub fn all_bound_are_healthy(&self, healthy: &[BindingIdentity]) -> bool {
        !self.bound_identities.is_empty()
            && self
                .bound_identities
                .iter()
                .all(|binding| healthy.contains(binding))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrashPresentation {
    Active,
    Settled,
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
    /// Audit history stays in `last_crash`; this drives presentation only.
    pub crash_presentation: CrashPresentation,
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

    let presentation = crash_presentation(&current, last_crash.as_ref());
    RECOVERY_HEAL_PENDING.store(presentation == CrashPresentation::Active, Ordering::Release);
    RecoveryState {
        crash_presentation: presentation,
        current,
        last_crash,
        last_clean_shutdown: clean,
    }
}

/// Record what the dead epoch had bound versus what came back. Ambiguity is the
/// shortfall; a full restore leaves no claim regardless of how long we were down.
pub fn note_restored_bindings(evidence: BindingEvidence) {
    update_current_crash(|crash| {
        let ambiguous = ambiguous_work(&evidence);
        crash.bindings = Some(evidence);
        crash.work_policy.ambiguous_execution = ambiguous;
        Some(())
    });
}

/// End the warning's life at proven health: every binding the dead epoch held is
/// back and its agent is ready. Durable, so the refresh path cannot resurrect it.
pub fn heal_current_epoch(healthy_bindings: &[BindingIdentity]) -> bool {
    let healed = update_current_crash(|crash| {
        heal_crash_record(crash, healthy_bindings, now_ts()).then_some(true)
    })
    .unwrap_or(false);
    if healed {
        RECOVERY_HEAL_PENDING.store(false, Ordering::Release);
    }
    healed
}

pub fn recovery_heal_pending() -> bool {
    RECOVERY_HEAL_PENDING.load(Ordering::Acquire)
}

fn heal_crash_record(
    crash: &mut CrashRecord,
    healthy_bindings: &[BindingIdentity],
    healed_at: i64,
) -> bool {
    let Some(evidence) = crash.bindings.as_ref() else {
        return false;
    };
    if !evidence.all_bound_are_healthy(healthy_bindings) {
        return false;
    }
    crash.work_policy.ambiguous_execution = None;
    crash.healed_at = Some(healed_at);
    true
}

/// Mutate the crash record bound to THIS epoch, then persist it. A record from
/// an earlier epoch is audit and is never rewritten.
fn update_current_crash<T>(mutate: impl FnOnce(&mut CrashRecord) -> Option<T>) -> Option<T> {
    let Some(current) = read_json::<ProcessEpoch>(&current_path()) else {
        return None;
    };
    let Some(mut crash) = read_json::<CrashRecord>(&last_crash_path()) else {
        return None;
    };
    if crash.new_epoch != current.epoch || crash.healed_at.is_some() {
        return None;
    }
    let result = mutate(&mut crash)?;
    write_json(&last_crash_path(), &crash);
    Some(result)
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
    let last_crash: Option<CrashRecord> = read_json(&last_crash_path());
    RecoveryState {
        crash_presentation: crash_presentation(&current, last_crash.as_ref()),
        current,
        last_crash,
        last_clean_shutdown: read_json(&last_clean_path()),
    }
}

/// Presentation reads persisted state only — no clock. A healed record stays
/// healed across every re-read, which is what makes the refresh path safe.
fn crash_presentation(
    current: &ProcessEpoch,
    last_crash: Option<&CrashRecord>,
) -> CrashPresentation {
    let Some(crash) = last_crash else {
        return CrashPresentation::Settled;
    };
    if crash.new_epoch != current.epoch || crash.healed_at.is_some() {
        CrashPresentation::Settled
    } else {
        CrashPresentation::Active
    }
}

/// Classification uses only what the dead epoch itself recorded. Time since the
/// restart is downtime, not evidence — `PowerLossOrKill` comes solely from a
/// supervisor that observed the kill (`pending_crash.json`).
fn classify_previous(previous: ProcessEpoch, current: &ProcessEpoch, now: i64) -> CrashRecord {
    let healthy_lifetime = previous
        .last_healthy_at
        .saturating_sub(previous.started_at)
        .max(0);
    let classification = if healthy_lifetime < FAST_FAIL_SECS {
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
        healed_at: None,
        bindings: None,
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
        healed_at: None,
        bindings: None,
    }
}

/// Only a binding that did NOT come back is ambiguous work.
fn ambiguous_work(evidence: &BindingEvidence) -> Option<AmbiguousWork> {
    (!evidence.fully_restored()).then(|| AmbiguousWork {
        missing_bindings: evidence.missing(),
        disposition: "needs-review/reassign".into(),
    })
}

fn work_recovery_policy() -> WorkRecoveryPolicy {
    WorkRecoveryPolicy {
        restore_session_snapshot: true,
        resume_safe_idempotent: true,
        ambiguous_execution: None,
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
        assert_eq!(crash.classification, CrashClassification::AbnormalExit);
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
    fn long_downtime_alone_is_not_power_loss_or_kill() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 900_000,
            last_healthy_at: 900_000,
            build_hash: "h2".into(),
            platform: "linux".into(),
        };
        let previous = ProcessEpoch {
            epoch: "old".into(),
            pid: 1,
            started_at: 10,
            last_healthy_at: 5_000,
            build_hash: "h1".into(),
            platform: "linux".into(),
        };
        let record = classify_previous(previous, &current, 900_000);
        assert_eq!(record.classification, CrashClassification::AbnormalExit);
    }

    #[test]
    fn short_lived_previous_epoch_is_fast_fail_regardless_of_downtime() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 900_000,
            last_healthy_at: 900_000,
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
        let record = classify_previous(previous, &current, 900_000);
        assert_eq!(record.classification, CrashClassification::FastFail);
    }

    #[test]
    fn power_loss_or_kill_survives_only_from_supervisor_observation() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 900_000,
            last_healthy_at: 900_000,
            build_hash: "h2".into(),
            platform: "linux".into(),
        };
        let pending = PendingCrashRecord {
            classification: CrashClassification::PowerLossOrKill,
            previous: ProcessEpoch {
                epoch: "old".into(),
                pid: 1,
                started_at: 10,
                last_healthy_at: 5_000,
                build_hash: "h1".into(),
                platform: "linux".into(),
            },
            detected_at: 5_010,
            supervisor_action: SupervisorAction {
                external_restart_required: true,
                backoff_ms: 1_000,
                loop_protected: false,
                crash_count_in_window: 1,
            },
        };
        let record = finalize_pending(pending, &current);
        assert_eq!(record.classification, CrashClassification::PowerLossOrKill);
    }

    #[test]
    fn fresh_crash_on_current_epoch_is_presented() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 1_000,
            last_healthy_at: 1_010,
            build_hash: "h".into(),
            platform: "windows".into(),
        };
        let crash = classify_previous(previous_epoch(), &current, 1_000);
        assert_eq!(
            crash_presentation(&current, Some(&crash)),
            CrashPresentation::Active
        );
    }

    #[test]
    fn crash_from_an_earlier_epoch_never_presents() {
        let current = ProcessEpoch {
            epoch: "newest".into(),
            pid: 3,
            started_at: 1_000,
            last_healthy_at: 1_001,
            build_hash: "h".into(),
            platform: "windows".into(),
        };
        let superseded = ProcessEpoch {
            epoch: "middle".into(),
            pid: 2,
            started_at: 500,
            last_healthy_at: 600,
            build_hash: "h".into(),
            platform: "windows".into(),
        };
        let crash = classify_previous(previous_epoch(), &superseded, 500);
        assert_eq!(
            crash_presentation(&current, Some(&crash)),
            CrashPresentation::Settled
        );
    }

    #[test]
    fn long_downtime_with_fully_restored_bindings_is_healthy_not_ambiguous() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 900_000,
            last_healthy_at: 900_000,
            build_hash: "h2".into(),
            platform: "linux".into(),
        };
        let mut crash = classify_previous(previous_epoch(), &current, 900_000);
        let evidence = BindingEvidence {
            bound_at_shutdown: 3,
            restored: 3,
            ..Default::default()
        };
        crash.work_policy.ambiguous_execution = ambiguous_work(&evidence);
        crash.bindings = Some(evidence);

        assert_eq!(
            crash.classification,
            CrashClassification::AbnormalExit,
            "15 minutes of downtime is not a kill report"
        );
        assert_eq!(
            crash.work_policy.ambiguous_execution, None,
            "every binding came back, so nothing is ambiguous"
        );
    }

    #[test]
    fn ambiguity_counts_only_bindings_that_never_came_back() {
        assert_eq!(work_recovery_policy().ambiguous_execution, None);
        assert_eq!(
            ambiguous_work(&BindingEvidence {
                bound_at_shutdown: 0,
                restored: 0,
                ..Default::default()
            }),
            None
        );
        let work = ambiguous_work(&BindingEvidence {
            bound_at_shutdown: 3,
            restored: 1,
            ..Default::default()
        })
        .expect("a lost binding is real evidence");
        assert_eq!(work.missing_bindings, 2);
        assert_eq!(work.disposition, "needs-review/reassign");
    }

    #[test]
    fn health_requires_the_exact_restored_binding_identities() {
        let expected = vec![
            BindingIdentity {
                tab_id: "orchestrator".into(),
                agent_session_id: Some("session-a".into()),
            },
            BindingIdentity {
                tab_id: "planner".into(),
                agent_session_id: Some("session-b".into()),
            },
        ];
        let evidence = BindingEvidence {
            bound_at_shutdown: 2,
            restored: 2,
            bound_identities: expected.clone(),
            restored_identities: expected.clone(),
        };
        let wrong_sessions = vec![
            expected[0].clone(),
            BindingIdentity {
                tab_id: "planner".into(),
                agent_session_id: Some("foreign-session".into()),
            },
        ];
        let mismatched = BindingEvidence {
            restored_identities: wrong_sessions.clone(),
            ..evidence.clone()
        };

        assert_eq!(mismatched.missing(), 1);
        assert!(!mismatched.fully_restored());
        assert!(!evidence.all_bound_are_healthy(&wrong_sessions));
        assert!(evidence.all_bound_are_healthy(&expected));
    }

    #[test]
    fn exact_binding_health_durably_settles_the_crash_record() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 1_000,
            last_healthy_at: 1_001,
            build_hash: "h".into(),
            platform: "windows".into(),
        };
        let binding = BindingIdentity {
            tab_id: "orchestrator".into(),
            agent_session_id: Some("session-a".into()),
        };
        let mut crash = classify_previous(previous_epoch(), &current, 1_000);
        crash.bindings = Some(BindingEvidence {
            bound_at_shutdown: 1,
            restored: 1,
            bound_identities: vec![binding.clone()],
            restored_identities: vec![binding.clone()],
        });

        let replacement = BindingIdentity {
            tab_id: "orchestrator".into(),
            agent_session_id: Some("replacement-session".into()),
        };
        assert!(!heal_crash_record(&mut crash, &[replacement], 1_002));
        assert_eq!(crash.healed_at, None);
        assert!(heal_crash_record(&mut crash, &[binding], 1_002));
        assert_eq!(crash.healed_at, Some(1_002));
        assert_eq!(
            crash_presentation(&current, Some(&crash)),
            CrashPresentation::Settled
        );
        assert_eq!(crash.previous.pid, 41);
    }

    #[test]
    fn exact_restored_identity_and_seeded_state_are_sufficient_without_a_live_pty() {
        let identity =
            BindingIdentity::healthy_lazy_restored("orchestrator", Some("restored-session"), true);
        assert_eq!(
            identity,
            Some(BindingIdentity {
                tab_id: "orchestrator".into(),
                agent_session_id: Some("restored-session".into()),
            })
        );
        assert_eq!(
            BindingIdentity::healthy_lazy_restored("orchestrator", Some("restored-session"), false,),
            None
        );
    }

    #[test]
    fn healed_epoch_stays_settled_across_re_reads() {
        let current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 1_000,
            last_healthy_at: 1_001,
            build_hash: "h".into(),
            platform: "windows".into(),
        };
        let mut crash = classify_previous(previous_epoch(), &current, 1_000);
        assert_eq!(
            crash_presentation(&current, Some(&crash)),
            CrashPresentation::Active
        );

        crash.healed_at = Some(1_002);
        for _ in 0..3 {
            assert_eq!(
                crash_presentation(&current, Some(&crash)),
                CrashPresentation::Settled,
                "the refresh path must never resurrect a healed warning"
            );
        }
        assert_eq!(crash.previous.pid, 41, "audit survives the heal");
    }

    #[test]
    fn presentation_ignores_the_clock_entirely() {
        let mut current = ProcessEpoch {
            epoch: "new".into(),
            pid: 2,
            started_at: 1_000,
            last_healthy_at: 1_000,
            build_hash: "h".into(),
            platform: "windows".into(),
        };
        let crash = classify_previous(previous_epoch(), &current, 1_000);
        current.last_healthy_at = current.started_at + 86_400;
        assert_eq!(
            crash_presentation(&current, Some(&crash)),
            CrashPresentation::Active,
            "a day of uptime is not proof; only restored bindings are"
        );
    }

    fn previous_epoch() -> ProcessEpoch {
        ProcessEpoch {
            epoch: "old".into(),
            pid: 41,
            started_at: 10,
            last_healthy_at: 500,
            build_hash: "h1".into(),
            platform: "windows".into(),
        }
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
