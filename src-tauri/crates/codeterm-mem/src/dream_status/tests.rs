use std::sync::Arc;

use super::deadline::{
    deadline_body, finalize_at_deadline, finalize_at_deadline_with_reason, last_logged_stage,
    APPROVAL_PROMPT_REASON, DEADLINE_REASON, DREAMS_AREA, DREAM_DIARY_TAG,
};
use super::*;
use crate::store::{MemKind, MemScope};
use crate::workers::{make_info, GraphWorkerRegistry};
use crate::{MemManager, NewMemItem};

const DETAIL: &str = "hard timeout after 1800s";
const WORKER: &str = "gw_deadline1";

/// The skeleton a REM run leaves when it stalls mid A.1: two stages logged,
/// the rest still pending.
const SKELETON: &str = "<insight></insight><log>STARTED: 2026-09-29 06:20:11\n\
     HUBS_BEFORE: 12\n\
     HUBS: pending\n\
     ISLANDS_BEFORE: <count>\n\
     CALIBRATION: pending\n\
     </log>";

const FINISHED: &str = "<insight>\nThe graph keeps rebuilding the same bridges.\n</insight>\n\
     <log>\nHUBS_BEFORE: 6\nHUBS_AFTER: 1\nCALIBRATION: skipped=none applied=0 archived=0\n</log>";

fn registry() -> Arc<GraphWorkerRegistry> {
    GraphWorkerRegistry::load(
        std::env::temp_dir().join(format!("dream-deadline-{}.json", uuid::Uuid::new_v4())),
    )
}

fn rem_worker(registry: &GraphWorkerRegistry) {
    registry.register(make_info(
        WORKER.into(),
        "tab-deadline".into(),
        "dreaming:rem".into(),
        "system:dreaming".into(),
        "hsl(0,0%,0%)".into(),
    ));
}

fn save_diary(mem: &MemManager, title: &str, body: &str, area: &str) -> String {
    mem.save(NewMemItem {
        kind: MemKind::Daily,
        scope: MemScope::Global,
        title: title.into(),
        summary: Some("WIP: dreaming".into()),
        area: Some(area.into()),
        tags: vec![DREAM_DIARY_TAG.into()],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    })
    .unwrap()
}

fn time_out(registry: &GraphWorkerRegistry) {
    registry
        .finish_execution(
            WORKER,
            Some(DETAIL.into()),
            ExecutionEnd::TimedOut {
                detail: DETAIL.into(),
            },
        )
        .unwrap();
}

fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

#[test]
fn dream_deadline_finalizes_wip_diary_as_partial() {
    let mem = MemManager::open_in_memory().unwrap();
    let registry = registry();
    rem_worker(&registry);
    let diary = save_diary(&mem, "dream: deadline wip", SKELETON, "codeterm");
    time_out(&registry);

    let outcome = finalize_at_deadline(&mem, &registry, WORKER, DETAIL).unwrap();

    assert_eq!(outcome.diary_id.as_deref(), Some(diary.as_str()));
    assert!(outcome.diary_rewritten && outcome.finalization_recorded);
    let stored = mem.get(&diary).unwrap().unwrap();
    let evidence = diary_evidence(&diary, &stored.body);
    assert!(evidence.is_complete(), "{evidence:?}");
    assert!(evidence.cut_at_deadline);
    assert_eq!(
        classify_diary_record(&evidence, None),
        DiaryRecordState::Complete,
        "a deadline-finalized diary is never recoverable-incomplete"
    );
    assert_eq!(stored.area.as_deref(), Some(DREAMS_AREA));
    assert!(!is_placeholder_text(
        stored.summary.as_deref().unwrap_or("")
    ));

    let fields = log_fields(&stored.body);
    assert_eq!(field(&fields, "STARTED"), Some("2026-09-29 06:20:11"));
    assert_eq!(field(&fields, "HUBS_BEFORE"), Some("12"));
    for stage in ["HUBS", "ISLANDS_BEFORE", "CALIBRATION"] {
        let value = field(&fields, stage).expect("pending stage keeps its field");
        assert!(!is_unfilled_value(value), "{stage}: {value}");
    }

    let worker = registry.get(WORKER).unwrap();
    assert_eq!(worker.diary_id.as_deref(), Some(diary.as_str()));
    assert_eq!(
        worker.finalization,
        DiaryFinalization::Partial {
            diary_id: Some(diary.clone()),
            reason: DEADLINE_REASON.into(),
        }
    );
    assert_eq!(
        worker.dream_run_status(),
        DreamRunStatus::PartiallyFinalized {
            reason: DEADLINE_REASON.into()
        }
    );
}

#[test]
fn dream_blocking_prompt_finalizes_wip_diary_with_approval_reason() {
    let mem = MemManager::open_in_memory().unwrap();
    let registry = registry();
    rem_worker(&registry);
    let diary = save_diary(&mem, "dream: approval prompt", SKELETON, "codeterm");
    time_out(&registry);

    let outcome = finalize_at_deadline_with_reason(
        &mem,
        &registry,
        WORKER,
        APPROVAL_PROMPT_REASON,
        APPROVAL_PROMPT_REASON,
    )
    .unwrap();

    assert!(outcome.diary_rewritten && outcome.finalization_recorded);
    let stored = mem.get(&diary).unwrap().unwrap();
    let evidence = diary_evidence(&diary, &stored.body);
    assert!(evidence.is_complete() && evidence.cut_at_deadline);
    let worker = registry.get(WORKER).unwrap();
    assert_eq!(worker.diary_id.as_deref(), Some(diary.as_str()));
    assert_eq!(
        worker.finalization,
        DiaryFinalization::Partial {
            diary_id: Some(diary),
            reason: APPROVAL_PROMPT_REASON.into(),
        }
    );
    assert_eq!(
        worker.dream_run_status(),
        DreamRunStatus::PartiallyFinalized {
            reason: APPROVAL_PROMPT_REASON.into(),
        }
    );
}

#[test]
fn dream_deadline_finalization_is_idempotent_with_agent_report() {
    // The agent finalized first: the host keeps both its diary and its claim.
    let mem = MemManager::open_in_memory().unwrap();
    let registry = registry();
    rem_worker(&registry);
    let diary = save_diary(&mem, "dream: agent finished", FINISHED, DREAMS_AREA);
    let before = mem.get(&diary).unwrap().unwrap().body;
    let claimed = reconcile_finalization(
        Some(FinalizationClaim::Succeeded),
        None,
        Some(&diary_evidence(&diary, &before)),
    );
    registry
        .record_finalization(WORKER, Some(diary.clone()), claimed.clone())
        .unwrap();
    time_out(&registry);

    let outcome = finalize_at_deadline(&mem, &registry, WORKER, DETAIL).unwrap();
    assert!(!outcome.diary_rewritten && !outcome.finalization_recorded);
    assert_eq!(mem.get(&diary).unwrap().unwrap().body, before);
    assert_eq!(registry.get(WORKER).unwrap().finalization, claimed);

    // The host finalized first: a repeat is a no-op, and a late success claim
    // over the cut-short diary still reconciles to the deadline partial.
    let mem = MemManager::open_in_memory().unwrap();
    let registry = self::registry();
    rem_worker(&registry);
    let diary = save_diary(&mem, "dream: host first", SKELETON, DREAMS_AREA);
    time_out(&registry);
    finalize_at_deadline(&mem, &registry, WORKER, DETAIL).unwrap();
    let first = mem.get(&diary).unwrap().unwrap().body;

    let again = finalize_at_deadline(&mem, &registry, WORKER, DETAIL).unwrap();
    assert!(!again.diary_rewritten && !again.finalization_recorded);
    assert_eq!(mem.get(&diary).unwrap().unwrap().body, first);

    let late = reconcile_finalization(
        Some(FinalizationClaim::Succeeded),
        None,
        Some(&diary_evidence(&diary, &first)),
    );
    assert_eq!(
        late,
        DiaryFinalization::Partial {
            diary_id: Some(diary.clone()),
            reason: DEADLINE_REASON.into(),
        }
    );
}

#[test]
fn dream_deadline_names_last_logged_stage() {
    let body = "<insight>WIP</insight><log>STARTED: t0\n\
         STAGE_OPAQUE_ONE: 4\n\
         STAGE_OPAQUE_TWO: bridged=3\n\
         STAGE_OPAQUE_THREE: pending\n\
         </log>";
    assert_eq!(last_logged_stage(body).as_deref(), Some("STAGE_OPAQUE_TWO"));

    let finalized = deadline_body(body, DETAIL).expect("a WIP diary is rewritten");
    let insight = tagged_block(&finalized, "insight").unwrap();
    assert!(insight.contains("STAGE_OPAQUE_TWO"), "{insight}");
    assert!(!is_placeholder_text(&insight));
    assert_eq!(
        last_logged_stage(&finalized).as_deref(),
        Some("STAGE_OPAQUE_THREE"),
        "the cut-short stage stays named in the log, after the logged ones"
    );
    assert_eq!(deadline_body(&finalized, DETAIL), None);
    assert_eq!(deadline_body(FINISHED, DETAIL), None);
}

#[test]
fn dream_deadline_without_a_diary_still_records_partial() {
    let mem = MemManager::open_in_memory().unwrap();
    let registry = registry();
    rem_worker(&registry);
    time_out(&registry);

    let outcome = finalize_at_deadline(&mem, &registry, WORKER, DETAIL).unwrap();

    assert_eq!(outcome.diary_id, None);
    assert!(!outcome.diary_rewritten && outcome.finalization_recorded);
    assert_eq!(
        registry.get(WORKER).unwrap().dream_run_status(),
        DreamRunStatus::PartiallyFinalized {
            reason: DEADLINE_REASON.into()
        }
    );
}
