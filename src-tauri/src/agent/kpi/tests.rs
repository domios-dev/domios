use std::sync::Arc;

use super::attribution::{deliver_event_kind, KpiAttribution};
use super::recorder::KpiRecorder;
use super::usage::UsageSource;
use crate::agent::kpi_ledger::{KpiEventKind, KpiEventRecord, KpiLedger, TokenCounts};
use crate::multiplexer::types::AgentRole;
use crate::service::messenger::MessageKind;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kpi-wiring-{}-{}", name, ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn orchestrator(tab: &str) -> KpiAttribution {
    KpiAttribution::for_tab(tab)
        .plan(Some("pl_dwojf1".into()), Some("task_xr8mx3".into()))
        .role(Some(AgentRole::Manager), Some("orchestrator"))
        .provider(Some("codex".into()))
        .model(Some("gpt-5.6-terra".into()))
}

fn worker(tab: &str) -> KpiAttribution {
    KpiAttribution::for_tab(tab)
        .plan(Some("pl_dwojf1".into()), Some("task_xr8mx3".into()))
        .role(Some(AgentRole::Worker), None)
        .provider(Some("claude".into()))
        .model(Some("claude-opus-4-8".into()))
}

fn summary(row: &KpiEventRecord) -> (KpiEventKind, String, Option<String>, u64) {
    (row.kind, row.tab_id.clone(), row.role.clone(), row.bytes)
}

/// The exchange a real orchestrator/worker pair produces, replayed through the same
/// attribution + kind-mapping the choke points call, against a real ledger.
#[test]
fn scripted_orchestrator_worker_exchange_writes_the_expected_event_sequence() {
    let dir = scratch("exchange");
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());

    let orch = orchestrator("tab-orch");
    let wrk = worker("tab-worker");

    // 1. Orchestrator spawns the worker with a brief.
    let brief = "Implement task_xr8mx3 in this lane";
    recorder.record(wrk.event_at(KpiEventKind::Spawn, brief.len() as u64, 1_700_000_001));

    // 2. Orchestrator sends the worker a follow-up message.
    let message = "check the KPI spec before wiring";
    assert_eq!(
        deliver_event_kind(MessageKind::Message),
        Some(KpiEventKind::Send)
    );
    recorder.record(wrk.event_at(KpiEventKind::Send, message.len() as u64, 1_700_000_002));

    // 3. Scheduler nudges the worker; the reminder delivery itself must not double-count.
    let nudge = "still working? report when the lane is green";
    assert_eq!(
        deliver_event_kind(MessageKind::Reminder),
        None,
        "reminder bytes belong to the nudge writer, not the deliver seam"
    );
    recorder.record(wrk.event_at(KpiEventKind::Nudge, nudge.len() as u64, 1_700_000_003));

    // 4. A heartbeat digest is prepended to the orchestrator's own reminder.
    recorder.record(orch.event_at(KpiEventKind::Heartbeat, 128, 1_700_000_004));

    // 5. Worker reports; the deliver seam stays silent for report envelopes.
    let report = "STATUS: done";
    assert_eq!(deliver_event_kind(MessageKind::Report), None);
    recorder.record(wrk.event_at(KpiEventKind::Report, report.len() as u64, 1_700_000_005));

    // 6. Token usage ingested from the worker's provider session log.
    recorder.record(wrk.token_event_at(
        TokenCounts {
            input: 49_766,
            uncached_input: 24_384,
            output: 204,
        },
        1_700_000_006,
    ));

    recorder.drain();

    let rows = ledger.list_all().unwrap();
    let observed: Vec<_> = rows.iter().map(summary).collect();
    assert_eq!(
        observed,
        vec![
            (
                KpiEventKind::Spawn,
                "tab-worker".to_string(),
                Some("Worker".to_string()),
                brief.len() as u64
            ),
            (
                KpiEventKind::Send,
                "tab-worker".to_string(),
                Some("Worker".to_string()),
                message.len() as u64
            ),
            (
                KpiEventKind::Nudge,
                "tab-worker".to_string(),
                Some("Worker".to_string()),
                nudge.len() as u64
            ),
            (
                KpiEventKind::Heartbeat,
                "tab-orch".to_string(),
                Some("Orchestrator".to_string()),
                128
            ),
            (
                KpiEventKind::Report,
                "tab-worker".to_string(),
                Some("Worker".to_string()),
                report.len() as u64
            ),
            (
                KpiEventKind::TokenUsage,
                "tab-worker".to_string(),
                Some("Worker".to_string()),
                49_970
            ),
        ]
    );

    for row in &rows {
        assert_eq!(row.plan_id.as_deref(), Some("pl_dwojf1"));
        assert_eq!(row.task_id.as_deref(), Some("task_xr8mx3"));
    }
    let providers: Vec<_> = rows.iter().map(|r| r.provider.as_str()).collect();
    assert_eq!(
        providers,
        vec!["claude", "claude", "claude", "codex", "claude", "claude"],
        "each event carries the provider of the tab it is charged to"
    );
    assert_eq!(
        rows[5].tokens,
        Some(TokenCounts {
            input: 49_766,
            uncached_input: 24_384,
            output: 204
        }),
        "token breakdown survives the round trip"
    );
    assert!(
        rows.iter().take(5).all(|r| r.tokens.is_none()),
        "coordination events carry no token breakdown"
    );

    let plan_scoped = ledger.list_for_plan("pl_dwojf1").unwrap();
    assert_eq!(plan_scoped.len(), 6);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ingested_usage_lands_as_attributed_token_and_compaction_events() {
    let dir = scratch("ingest");
    let session = dir.join("session.jsonl");
    std::fs::write(
        &session,
        include_str!("usage/fixtures/claude-session.jsonl"),
    )
    .unwrap();

    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());
    recorder.request_ingest(UsageSource::new(&session), worker("tab-worker"));
    recorder.drain();

    let rows = ledger.list_all().unwrap();
    let kinds: Vec<_> = rows.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![
            KpiEventKind::TokenUsage,
            KpiEventKind::TokenUsage,
            KpiEventKind::Compaction,
            KpiEventKind::TokenUsage,
        ]
    );
    let input: u64 = rows.iter().filter_map(|r| r.tokens).map(|t| t.input).sum();
    let uncached: u64 = rows
        .iter()
        .filter_map(|r| r.tokens)
        .map(|t| t.uncached_input)
        .sum();
    let output: u64 = rows.iter().filter_map(|r| r.tokens).map(|t| t.output).sum();
    assert_eq!((input, uncached, output), (514_028, 25_600, 1_800));
    assert!(rows
        .iter()
        .all(|r| r.tab_id == "tab-worker" && r.provider == "claude"));

    recorder.request_ingest(UsageSource::new(&session), worker("tab-worker"));
    recorder.drain();
    assert_eq!(
        ledger.list_all().unwrap().len(),
        4,
        "a second pass over an unchanged log appends nothing"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// A daemon restart replaces the `KpiRecorder` (and its in-process `UsageIngestor`)
/// but keeps the same `kpi_ledger.db` — the persisted cursor is what must carry the
/// forward-only position across that boundary, or every session gets fully re-ingested
/// (and re-counted) on every restart.
#[test]
fn restarted_recorder_resumes_from_the_persisted_cursor_instead_of_reingesting() {
    let dir = scratch("cursor-persist");
    let session = dir.join("session.jsonl");
    std::fs::write(
        &session,
        include_str!("usage/fixtures/claude-session.jsonl"),
    )
    .unwrap();

    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let first_run = KpiRecorder::with_ledger(ledger.clone());
    first_run.request_ingest(UsageSource::new(&session), worker("tab-worker"));
    first_run.drain();
    assert_eq!(ledger.list_all().unwrap().len(), 4);

    // Simulate a restart: a brand-new recorder (fresh in-memory UsageIngestor, cursor
    // HashMap empty) against the SAME ledger file.
    let second_run = KpiRecorder::with_ledger(ledger.clone());
    second_run.request_ingest(UsageSource::new(&session), worker("tab-worker"));
    second_run.drain();

    assert_eq!(
        ledger.list_all().unwrap().len(),
        4,
        "restart must resume from the persisted watermark, not re-ingest from byte zero"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn disabled_recorder_writes_nothing_and_creates_no_ledger_file() {
    let dir = scratch("flag-off");
    let config = crate::config::schema::AppConfig {
        kpi: crate::config::schema::KpiConfig { enabled: false },
        ..Default::default()
    };
    assert!(!crate::agent::kpi_ledger::kpi_ledger_enabled(&config));

    let recorder = KpiRecorder::from_config(&config, &dir);
    assert!(!recorder.is_enabled());

    let session = dir.join("session.jsonl");
    std::fs::write(
        &session,
        include_str!("usage/fixtures/claude-session.jsonl"),
    )
    .unwrap();
    for kind in KpiEventKind::ALL {
        recorder.record(worker("tab-worker").event_at(kind, 100, 1_700_000_000));
    }
    recorder.request_ingest(UsageSource::new(&session), worker("tab-worker"));
    recorder.drain();

    assert!(
        !dir.join("kpi_ledger.db").exists(),
        "disabled collection never creates the ledger db"
    );
    let stray: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name != "session.jsonl")
        .collect();
    assert!(stray.is_empty(), "disabled collection wrote {:?}", stray);

    std::fs::remove_dir_all(&dir).ok();
}

/// Send-path cost comparison.
///
/// Methodology. Differential timing of two whole loops cannot resolve a 2% target here:
/// every PTY delivery ends in `ServiceContext::send_text_detached`, which spawns an OS
/// thread (`service/pty_io/send.rs`), and thread-spawn latency jitters by more than 2%
/// between batches — a no-op arm measures several percent of "overhead". So each cost is
/// measured on its own stable loop and the ratio is computed from the per-operation
/// minimums, which is the estimator least sensitive to scheduler noise.
///
/// - `spawn`: the detached OS thread every delivery hands its payload to.
/// - `payload`: the real `Messenger` prefix build plus body concatenation.
/// - `hook_on`: what `record_kpi_delivery` adds with the flag on — resolving a fresh
///   attribution and enqueueing the event (the sqlite commit is on the ledger thread).
/// - `hook_off`: what it adds with the flag off — one `Option::is_some`.
///
/// The denominator (`spawn + payload`) excludes the mux locks, poll-streak reset, submit
/// transaction and PTY write that a real delivery also performs, so the reported enabled
/// ratio is an over-estimate of the daemon's.
///
/// The 2% bound is asserted only for the optimized profile, which is what ships; a debug
/// run reports the same numbers but asserts the absolute per-event ceiling alone, because
/// an unoptimized build inflates the pure-Rust hook far more than the kernel-bound spawn.
///
/// Note what the enabled ratio is a share *of*: `record_kpi_delivery` runs after
/// `deliver_inner` has already handed the payload to the detached thread, so it delays
/// the caller's return, never the delivery itself.
#[test]
fn kpi_hook_cost_on_the_send_path_is_bounded() {
    use std::time::{Duration, Instant};

    const ITERATIONS: usize = 200;
    const ROUNDS: usize = 9;
    const BODY: &str = "wire the KPI ledger at the documented daemon choke points";

    let dir = scratch("perf");
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());
    let disabled = KpiRecorder::disabled();
    let envelope = crate::service::messenger::MessageEnvelope::tab("Dumas", "tab-orch");

    fn best<F: FnMut(usize)>(rounds: usize, iterations: usize, mut body: F) -> f64 {
        let mut min = Duration::MAX;
        for _ in 0..rounds {
            let start = Instant::now();
            for i in 0..iterations {
                body(i);
            }
            min = min.min(start.elapsed());
        }
        min.as_nanos() as f64 / iterations as f64
    }

    let mut handles = Vec::with_capacity(ITERATIONS * ROUNDS);
    let spawn_ns = best(ROUNDS, ITERATIONS, |_| {
        handles.push(std::thread::spawn(|| std::hint::black_box(0u8)));
    });
    for handle in handles.drain(..) {
        handle.join().unwrap();
    }

    let payload_ns = best(ROUNDS, ITERATIONS, |i| {
        let text = format!(
            "{}{} #{}",
            crate::service::messenger::Messenger::pty_prefix(&envelope),
            BODY,
            i
        );
        std::hint::black_box(text.len());
    });

    let hook_off_ns = best(ROUNDS, ITERATIONS, |_| {
        if disabled.is_enabled() {
            disabled.record(worker("tab-worker").into_event(KpiEventKind::Send, 64));
        }
    });

    let hook_on_ns = best(ROUNDS, ITERATIONS, |_| {
        recorder.record(worker("tab-worker").into_event(KpiEventKind::Send, 64));
    });
    recorder.drain();

    let send_path_ns = spawn_ns + payload_ns;
    let pct_on = hook_on_ns / send_path_ns * 100.0;
    let pct_off = hook_off_ns / send_path_ns * 100.0;
    let optimized = !cfg!(debug_assertions);
    println!(
        "[kpi-perf] send-path floor={:.0}ns (spawn={:.0}ns + payload={:.0}ns) \
         hook_off={:.0}ns ({:.3}%) hook_on={:.0}ns ({:.3}%) \
         over {} iterations x {} rounds, {} profile",
        send_path_ns,
        spawn_ns,
        payload_ns,
        hook_off_ns,
        pct_off,
        hook_on_ns,
        pct_on,
        ITERATIONS,
        ROUNDS,
        if optimized { "optimized" } else { "debug" }
    );

    assert!(
        pct_off < 2.0,
        "flag-off hook must stay under 2% of the send path: {:.3}% ({:.0}ns)",
        pct_off,
        hook_off_ns
    );
    assert!(
        !optimized || pct_on < 2.0,
        "enabled hook must stay under 2% of the send path in the shipped profile: {:.3}% ({:.0}ns)",
        pct_on,
        hook_on_ns
    );
    assert!(
        !optimized || hook_on_ns < 1_000.0,
        "enabled hook must stay under 1us/event in the shipped profile: {:.0}ns",
        hook_on_ns
    );
    assert_eq!(
        ledger.list_all().unwrap().len(),
        ITERATIONS * ROUNDS,
        "every instrumented send landed in the ledger"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn deliver_kind_mapping_never_double_counts_owned_traffic() {
    assert_eq!(
        deliver_event_kind(MessageKind::Message),
        Some(KpiEventKind::Send)
    );
    assert_eq!(
        deliver_event_kind(MessageKind::Mesh),
        Some(KpiEventKind::Send)
    );
    assert_eq!(
        deliver_event_kind(MessageKind::Telegram),
        Some(KpiEventKind::Send)
    );
    assert_eq!(
        deliver_event_kind(MessageKind::Pebble),
        Some(KpiEventKind::Send)
    );
    assert_eq!(deliver_event_kind(MessageKind::Report), None);
    assert_eq!(deliver_event_kind(MessageKind::Reminder), None);
    assert_eq!(deliver_event_kind(MessageKind::System), None);
}

#[test]
fn unattributed_tab_still_produces_a_workspace_scoped_event() {
    let dir = scratch("unattributed");
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());

    recorder.record(
        KpiAttribution::for_tab("tab-loose")
            .provider(Some("opencode".into()))
            .event_at(KpiEventKind::Send, 12, 1_700_000_000),
    );
    recorder.drain();

    let rows = ledger.list_all().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].plan_id, None,
        "no plan binding rolls up to workspace"
    );
    assert_eq!(rows[0].role, None);
    assert_eq!(rows[0].provider, "opencode");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn recorder_announces_the_plans_of_each_committed_batch() {
    let dir = scratch("commit-listener");
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger);
    let heard: Arc<parking_lot::Mutex<Vec<Vec<String>>>> = Arc::default();
    let sink = Arc::clone(&heard);
    let (parked_tx, parked_rx) = std::sync::mpsc::sync_channel::<()>(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel::<()>(0);
    let gate = parking_lot::Mutex::new(Some((parked_tx, release_rx)));
    recorder.on_commit(move |plans| {
        sink.lock().push(plans);
        let first_call = gate.lock().take();
        if let Some((parked, release)) = first_call {
            parked.send(()).unwrap();
            release.recv().unwrap();
        }
    });

    recorder.drain();
    assert!(heard.lock().is_empty(), "an empty drain commits nothing");

    let unattributed = KpiAttribution::for_tab("tab-loose").provider(Some("codex".into()));
    recorder.record(unattributed.event_at(KpiEventKind::Send, 1, 1_700_000_000));
    parked_rx.recv().unwrap();
    // A batch is whatever is queued when the writer wakes, so both rows are queued
    // while it is parked inside the previous announcement.
    recorder.record(orchestrator("tab-orch").event_at(KpiEventKind::Send, 12, 1_700_000_001));
    recorder.record(unattributed.event_at(KpiEventKind::Send, 7, 1_700_000_002));
    release_tx.send(()).unwrap();
    recorder.drain();
    assert_eq!(
        heard.lock().clone(),
        vec![Vec::new(), vec!["pl_dwojf1".to_string()]],
        "a machine-only row still announces a commit, and one batch announces its plans once"
    );

    recorder.record(unattributed.event_at(KpiEventKind::Send, 3, 1_700_000_003));
    recorder.drain();
    assert_eq!(
        heard.lock().last().cloned(),
        Some(Vec::new()),
        "a machine-only row still announces a commit"
    );
}
