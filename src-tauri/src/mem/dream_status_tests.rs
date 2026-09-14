//! Execution-ended and diary-finalization are separate, identity-scoped facts.
//! Fixtures reproduce the real records from issue #478 so a reported run over
//! an unfinished diary can never read as a verified completion.

use std::path::PathBuf;

use codeterm_mem::dream_status::{
    classify_diary_record, diary_evidence, reconcile_finalization, DiaryFinalization,
    DiaryRecordState, DreamRunStatus, ExecutionEnd, FinalizationClaim,
};
use codeterm_mem::workers::{make_info, GraphWorkerInfo, GraphWorkerRegistry, WorkerStatus};

/// gw_7b948eb1 / daily_dream_2026_09_11_08_30 — the run whose final diary
/// update and self-link timed out after worker reporting succeeded.
const TIMED_OUT_WORKER: &str = "gw_7b948eb1";
const TIMED_OUT_DIARY: &str = "daily_dream_2026_09_11_08_30";
/// gw_6636edc5 / daily_dream_2026_09_12_03_20 — the run with a real diary.
const COMPLETE_WORKER: &str = "gw_6636edc5";
const COMPLETE_DIARY: &str = "daily_dream_2026_09_12_03_20";
/// The cron wrapper and REM worker finish times observed on the same night.
const CRON_JOB_ID: &str = "c9a1d130-ddc3-4450-a9bb-496bf6efe979";
const CRON_FINISHED_AT: u64 = 1_789_183_230_330;
const REM_FINISHED_AT: u64 = 1_789_184_113_893;

/// The body still stored under daily_dream_2026_09_11_08_30: skeleton insight
/// plus pending stage fields.
const UNFINISHED_DIARY_BODY: &str = "<insight>WIP: dreaming...</insight>\n\
     <log>\n\
     STARTED: 2026-09-11T08:30:00Z\n\
     PATTERN: pending\n\
     HUBS: pending\n\
     CALIBRATION: pending\n\
     ACTION: pending\n\
     </log>";

const FINISHED_DIARY_BODY: &str = "<insight>\n\
     The graph keeps rebuilding the same three bridges every night.\n\
     </insight>\n\
     <log>\n\
     PATTERN: repeat bridging between mem and mesh clusters\n\
     HUBS_BEFORE: 6\n\
     HUBS_AFTER: 1\n\
     ORPHANS: bridged=18 deleted=4\n\
     CALIBRATION: swept 412 items\n\
     ACTION: codeterm mem update fact_x42 --area auth\n\
     </log>";

fn registry(name: &str) -> std::sync::Arc<GraphWorkerRegistry> {
    let path: PathBuf = std::env::temp_dir().join(format!(
        "dream-status-{}-{}.json",
        name,
        uuid::Uuid::new_v4()
    ));
    GraphWorkerRegistry::load(path)
}

fn rem_worker(worker_id: &str, session_id: &str) -> GraphWorkerInfo {
    let mut info = make_info(
        worker_id.into(),
        format!("tab-{worker_id}"),
        "dreaming:rem".into(),
        "system:dreaming".into(),
        "hsl(0,0%,0%)".into(),
    );
    info.session_id = Some(session_id.into());
    info
}

#[test]
fn diary_finalization_timeout_is_not_done() {
    let registry = registry("timeout");
    registry.register(rem_worker(TIMED_OUT_WORKER, "sess-09-11"));

    // The run reported success, and its execution really did end.
    let evidence = diary_evidence(TIMED_OUT_DIARY, UNFINISHED_DIARY_BODY);
    let finalization = reconcile_finalization(
        Some(FinalizationClaim::Succeeded),
        Some("final diary update/self-link timed out"),
        Some(&evidence),
    );
    registry
        .record_finalization(TIMED_OUT_WORKER, Some(TIMED_OUT_DIARY.into()), finalization)
        .unwrap();
    let reported = registry
        .complete(TIMED_OUT_WORKER, "Diary saved. Hubs decomposed.".into())
        .unwrap();

    // Execution ended, by explicit report, and is tied to worker/run identity.
    assert_eq!(reported.worker_id, TIMED_OUT_WORKER);
    assert_eq!(reported.session_id.as_deref(), Some("sess-09-11"));
    assert_eq!(reported.execution_end, Some(ExecutionEnd::Reported));
    assert_eq!(reported.status, WorkerStatus::Done);

    // Finalization is a separate fact, and it did not succeed.
    assert!(
        !reported.finalization.is_verified(),
        "a timed-out finalization must never read as verified: {:?}",
        reported.finalization
    );
    assert_eq!(reported.diary_id.as_deref(), Some(TIMED_OUT_DIARY));
    assert_eq!(
        reported.finalization.diary_id(),
        Some(TIMED_OUT_DIARY),
        "the partial state must name the diary it belongs to"
    );
    let reason = reported
        .finalization
        .reason()
        .expect("a partial finalization must carry its concrete reason");
    assert!(
        reason.contains(TIMED_OUT_DIARY),
        "reason must name the diary: {reason}"
    );
    for pending in ["PATTERN", "HUBS", "CALIBRATION", "ACTION"] {
        assert!(
            reason.contains(pending),
            "reason must name pending field {pending}: {reason}"
        );
    }

    match reported.dream_run_status() {
        DreamRunStatus::PartiallyFinalized { reason } => {
            assert!(reason.contains("placeholder"), "{reason}");
        }
        other => panic!("expected a visible partial status, got {other:?}"),
    }

    // Zero-metric placeholders are not promoted: the evidence itself says so.
    assert!(!evidence.is_complete());
    assert_eq!(
        evidence.pending_fields,
        vec!["PATTERN", "HUBS", "CALIBRATION", "ACTION"]
    );
}

#[test]
fn rem_completion_links_own_diary_without_cron() {
    use crate::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};

    let registry = registry("rem-link");
    // The REM run ended on its own clock, with its own worker/run identity.
    let mut worker = rem_worker(COMPLETE_WORKER, "sess-09-12");
    worker.status = WorkerStatus::Done;
    worker.execution_end = Some(ExecutionEnd::Reported);
    worker.report = Some("Diary saved.".into());
    worker.done_at = Some(REM_FINISHED_AT);
    registry.register(worker);

    let evidence = diary_evidence(COMPLETE_DIARY, FINISHED_DIARY_BODY);
    assert!(evidence.is_complete(), "fixture diary must be complete");
    let finalization =
        reconcile_finalization(Some(FinalizationClaim::Succeeded), None, Some(&evidence));
    registry
        .record_finalization(COMPLETE_WORKER, Some(COMPLETE_DIARY.into()), finalization)
        .unwrap();

    // A cron wrapper exists for an unrelated job and finishes at its own time;
    // the dreaming wrapper is absent entirely.
    let dir = tempfile::tempdir().unwrap();
    let mut crons = CronStore::new_with_path(dir.path().join("crons.json"));
    let params: CronCreateParams = serde_json::from_value(serde_json::json!({
        "name": "unrelated-nightly",
        "workspace_id": "",
        "schedule": "0 6 * * *",
        "task": "echo hi",
    }))
    .unwrap();
    let job = crons.create(params).unwrap();
    crons
        .record_run(
            &job.id,
            CronRunInfo {
                run_id: CRON_JOB_ID.into(),
                started_at: CRON_FINISHED_AT - 60_000,
                finished_at: Some(CRON_FINISHED_AT),
                status: CronRunStatus::Success,
                tab_id: None,
                session_id: None,
                cwd: None,
            },
        )
        .unwrap();
    assert!(
        crons
            .list()
            .iter()
            .all(|j| j.name != codeterm_mem::dreaming::DREAMING_CRON_NAME),
        "this run must stand on its own without a dreaming cron wrapper"
    );

    let rem = registry.get(COMPLETE_WORKER).unwrap();
    assert_eq!(
        rem.dream_run_status(),
        DreamRunStatus::Finalized {
            diary_id: COMPLETE_DIARY.into()
        },
        "a successful REM run links its own diary by identity"
    );
    assert_eq!(rem.diary_id.as_deref(), Some(COMPLETE_DIARY));
    assert_eq!(rem.session_id.as_deref(), Some("sess-09-12"));
    assert_eq!(
        rem.done_at,
        Some(REM_FINISHED_AT),
        "cron completion must not overwrite the REM finish time"
    );
    assert_ne!(
        rem.done_at,
        Some(CRON_FINISHED_AT),
        "the two completions are separate facts"
    );

    // The other direction: the cron run keeps its own finish time too.
    let history = crons.get_history(&job.id);
    let cron_run = history
        .iter()
        .find(|r| r.run_id == CRON_JOB_ID)
        .expect("cron run recorded");
    assert_eq!(cron_run.finished_at, Some(CRON_FINISHED_AT));
    assert_eq!(cron_run.status, CronRunStatus::Success);
}

#[test]
fn incomplete_diary_record_is_recoverable_and_not_live() {
    let registry = registry("recoverable");
    registry.register(rem_worker(TIMED_OUT_WORKER, "sess-09-11"));
    registry
        .record_finalization(
            TIMED_OUT_WORKER,
            Some(TIMED_OUT_DIARY.into()),
            DiaryFinalization::Partial {
                diary_id: Some(TIMED_OUT_DIARY.into()),
                reason: "final diary update/self-link timed out".into(),
            },
        )
        .unwrap();
    registry
        .complete(TIMED_OUT_WORKER, "Diary saved.".into())
        .unwrap();

    // Nothing is executing, so this is history — recoverable, not live work.
    assert_eq!(registry.live_owner_of_diary(TIMED_OUT_DIARY), None);
    let evidence = diary_evidence(TIMED_OUT_DIARY, UNFINISHED_DIARY_BODY);
    let state = classify_diary_record(&evidence, None);
    assert!(!state.is_live());
    match &state {
        DiaryRecordState::IncompleteRecoverable { reason } => {
            assert!(reason.contains(TIMED_OUT_DIARY), "{reason}");
            assert!(reason.contains("PATTERN"), "{reason}");
        }
        other => panic!("expected recoverable history, got {other:?}"),
    }

    // The stored incompleteness is preserved as evidence, not overwritten.
    assert!(evidence.placeholder_insight);
    assert_eq!(evidence.pending_fields.len(), 4);
    assert!(UNFINISHED_DIARY_BODY.contains("WIP: dreaming..."));
    assert!(UNFINISHED_DIARY_BODY.contains("CALIBRATION: pending"));

    // The run itself stays inspectable by identity, and stays unverified.
    let record = registry.get(TIMED_OUT_WORKER).unwrap();
    assert_eq!(record.diary_id.as_deref(), Some(TIMED_OUT_DIARY));
    assert!(!record.finalization.is_verified());
    assert!(record.status.is_terminal());
}

#[test]
fn wip_placeholder_does_not_imply_running() {
    let registry = registry("wip-prose");

    // Same placeholder body, two different registry facts.
    let mut live = rem_worker("gw_live0001", "sess-live");
    live.status = WorkerStatus::Writing;
    live.diary_id = Some(TIMED_OUT_DIARY.into());
    registry.register(live);
    assert_eq!(
        registry.live_owner_of_diary(TIMED_OUT_DIARY).as_deref(),
        Some("gw_live0001"),
        "liveness comes from the registry, not the body"
    );

    let evidence = diary_evidence(TIMED_OUT_DIARY, UNFINISHED_DIARY_BODY);
    let live_owner = registry.live_owner_of_diary(TIMED_OUT_DIARY);
    assert_eq!(
        classify_diary_record(&evidence, live_owner.as_deref()),
        DiaryRecordState::IncompleteLive {
            worker_id: "gw_live0001".into()
        }
    );

    // End that execution: the body is byte-identical, and nothing is live.
    registry
        .finish_execution(
            "gw_live0001",
            Some("WIP: dreaming...".into()),
            ExecutionEnd::ProcessEnded {
                detail: "agent exited to shell".into(),
            },
        )
        .unwrap();
    assert_eq!(registry.live_owner_of_diary(TIMED_OUT_DIARY), None);

    let ended = registry.get("gw_live0001").unwrap();
    assert_eq!(
        ended.status,
        WorkerStatus::Error,
        "process termination must not report done"
    );
    assert_ne!(
        ended.dream_run_status(),
        DreamRunStatus::Running,
        "a WIP report body must not keep the run looking live"
    );
    assert_eq!(
        ended.dream_run_status(),
        DreamRunStatus::FinalizationPending,
        "execution ended with finalization still unevidenced"
    );
    assert!(!ended.finalization.is_verified());
}
