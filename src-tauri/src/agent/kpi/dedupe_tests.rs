use std::path::PathBuf;
use std::sync::Arc;

use super::attribution::KpiAttribution;
use super::recorder::KpiRecorder;
use super::usage::UsageSource;
use crate::agent::kpi_ledger::{KpiEventInput, KpiEventKind, KpiLedger, TokenCounts};
use crate::multiplexer::types::AgentRole;

const CLAUDE_STREAMED: &str = include_str!("usage/fixtures/claude-streamed-responses.jsonl");
const CODEX_REPEATED: &str = include_str!("usage/fixtures/codex-repeated-token-count.jsonl");

const CLAUDE_TRUTH: (u64, u64, u64, u64) = (2, 78_775, 4_221, 607);
const CODEX_TRUTH: (u64, u64, u64, u64) = (3, 50_000, 10_000, 700);

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("kpi-dedupe-ledger-{}-{}", name, ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn first_lines(body: &str, count: usize) -> String {
    body.split_inclusive('\n').take(count).collect()
}

fn worker(tab: &str, provider: &str) -> KpiAttribution {
    KpiAttribution::for_tab(tab)
        .plan(Some("pl_dedupe".into()), Some("task_dedupe".into()))
        .role(Some(AgentRole::Worker), None)
        .provider(Some(provider.into()))
        .model(Some("model-x".into()))
}

/// `(turns, input, uncached input, output)` over every token row in the ledger.
fn token_totals(ledger: &KpiLedger) -> (u64, u64, u64, u64) {
    let rows = ledger.list_all().unwrap();
    let tokens: Vec<TokenCounts> = rows
        .iter()
        .filter(|row| row.kind == KpiEventKind::TokenUsage)
        .filter_map(|row| row.tokens)
        .collect();
    (
        tokens.len() as u64,
        tokens.iter().map(|t| t.input).sum(),
        tokens.iter().map(|t| t.uncached_input).sum(),
        tokens.iter().map(|t| t.output).sum(),
    )
}

#[test]
fn kpi_usage_dedupe_response_split_across_cursor_passes_is_one_row() {
    let dir = scratch("split");
    let session = dir.join("session.jsonl");
    std::fs::write(&session, first_lines(CLAUDE_STREAMED, 2)).unwrap();
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());

    recorder.request_ingest(UsageSource::new(&session), worker("tab-claude", "claude"));
    recorder.drain();
    assert_eq!(token_totals(&ledger), (1, 38_233, 1_904, 1));

    std::fs::write(&session, CLAUDE_STREAMED).unwrap();
    recorder.request_ingest(UsageSource::new(&session), worker("tab-claude", "claude"));
    recorder.drain();
    assert_eq!(token_totals(&ledger), CLAUDE_TRUTH);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_main_and_subagent_files_count_a_shared_response_once() {
    let dir = scratch("main-and-subagent");
    let session = dir.join("session.jsonl");
    std::fs::write(&session, CLAUDE_STREAMED).unwrap();
    let sidechain = dir.join("session").join("subagents").join("agent-a.jsonl");
    std::fs::create_dir_all(sidechain.parent().unwrap()).unwrap();
    let shared: String = CLAUDE_STREAMED
        .split_inclusive('\n')
        .skip(2)
        .take(2)
        .collect();
    std::fs::write(&sidechain, shared).unwrap();
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());

    recorder.request_ingest(UsageSource::new(&session), worker("tab-claude", "claude"));
    recorder.request_ingest(UsageSource::new(&sidechain), worker("tab-claude", "claude"));
    recorder.drain();

    assert_eq!(token_totals(&ledger), CLAUDE_TRUTH);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_rereading_a_rewound_range_is_idempotent() {
    let dir = scratch("rewound");
    let session = dir.join("session.jsonl");
    std::fs::write(&session, CLAUDE_STREAMED).unwrap();
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());
    recorder.request_ingest(UsageSource::new(&session), worker("tab-claude", "claude"));
    recorder.drain();
    let recorded = ledger.list_all().unwrap();

    // Shorter than the cursor, so the next pass re-reads from byte zero.
    std::fs::write(&session, first_lines(CLAUDE_STREAMED, 4)).unwrap();
    recorder.request_ingest(UsageSource::new(&session), worker("tab-claude", "claude"));
    recorder.drain();

    assert_eq!(ledger.list_all().unwrap(), recorded);
    assert_eq!(token_totals(&ledger), CLAUDE_TRUTH);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_ledger_merges_a_repeated_key_and_rejects_a_missing_one() {
    let dir = scratch("ledger-key");
    let ledger = KpiLedger::open(&dir).unwrap();
    let attribution = worker("tab-claude", "claude");
    let key = "usage:response:msg_a:req_a".to_string();
    let first_line = attribution.token_event_at(
        TokenCounts {
            input: 38_233,
            uncached_input: 1_904,
            output: 1,
        },
        1_789_000_000,
        key.clone(),
    );
    let last_line = attribution.token_event_at(
        TokenCounts {
            input: 38_233,
            uncached_input: 1_904,
            output: 160,
        },
        1_789_000_001,
        key.clone(),
    );

    ledger
        .insert_many(&[first_line.clone(), last_line])
        .unwrap();
    ledger.insert(&first_line).unwrap();

    let rows = ledger.list_all().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].dedup_key.as_deref(), Some(key.as_str()));
    assert_eq!(rows[0].tokens.map(|t| t.output), Some(160));
    assert_eq!(rows[0].bytes, 38_233 + 160);
    assert_eq!(rows[0].timestamp, 1_789_000_000);

    let keyless = KpiEventInput {
        dedup_key: None,
        ..first_line
    };
    assert!(ledger.insert(&keyless).is_err());
    assert_eq!(ledger.list_all().unwrap().len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_turns_count_responses_not_lines() {
    let dir = scratch("turns");
    let claude = dir.join("session.jsonl");
    std::fs::write(&claude, CLAUDE_STREAMED).unwrap();
    let codex = dir.join("rollout.jsonl");
    std::fs::write(&codex, CODEX_REPEATED).unwrap();
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());

    recorder.request_ingest(UsageSource::new(&claude), worker("tab-claude", "claude"));
    recorder.request_ingest(UsageSource::new(&codex), worker("tab-codex", "codex"));
    recorder.drain();

    let by_provider = ledger.aggregate_token_usage_by_provider_model().unwrap();
    let figure = |provider: &str| {
        let row = by_provider.iter().find(|r| r.provider == provider).unwrap();
        (
            row.turns,
            row.input_tokens,
            row.uncached_input_tokens,
            row.output_tokens,
        )
    };
    assert_eq!(figure("claude"), CLAUDE_TRUTH);
    assert_eq!(figure("codex"), CODEX_TRUTH);
    let plan = ledger
        .aggregate_by_plan()
        .unwrap()
        .into_iter()
        .find(|row| row.plan_id.as_deref() == Some("pl_dedupe"))
        .unwrap();
    assert_eq!(plan.turns, CLAUDE_TRUTH.0 + CODEX_TRUTH.0);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_restarted_recorder_resumes_codex_totals() {
    let dir = scratch("codex-restart");
    let rollout = dir.join("rollout.jsonl");
    std::fs::write(&rollout, first_lines(CODEX_REPEATED, 6)).unwrap();
    let ledger = Arc::new(KpiLedger::open(&dir).unwrap());

    let first_run = KpiRecorder::with_ledger(ledger.clone());
    first_run.request_ingest(UsageSource::new(&rollout), worker("tab-codex", "codex"));
    first_run.drain();
    assert_eq!(token_totals(&ledger), (2, 38_000, 8_000, 500));

    std::fs::write(&rollout, CODEX_REPEATED).unwrap();
    let second_run = KpiRecorder::with_ledger(ledger.clone());
    second_run.request_ingest(UsageSource::new(&rollout), worker("tab-codex", "codex"));
    second_run.drain();

    assert_eq!(token_totals(&ledger), CODEX_TRUTH);

    std::fs::remove_dir_all(&dir).ok();
}
