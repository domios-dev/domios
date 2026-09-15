use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{params, Connection};

use super::{CumulativeUsage, KpiEventKind, KpiLedger};
use crate::agent::kpi::usage::{UsageIngestor, UsageSource};
use crate::agent::kpi::{KpiAttribution, KpiRecorder};

const CLAUDE_STREAMED: &str = include_str!("../kpi/usage/fixtures/claude-streamed-responses.jsonl");
const CODEX_REPEATED: &str = include_str!("../kpi/usage/fixtures/codex-repeated-token-count.jsonl");
const SUBAGENT_PARENT: &str = include_str!(
    "../../../../plugins/provider-claude/tests/fixtures/subagents/projects/C--Users-User-AppData-Local-Temp-snake-demo/f44205ef-4a6f-4090-9506-1978b4eada1b.jsonl"
);
const SIDECHAIN: &str = include_str!(
    "../../../../plugins/provider-claude/tests/fixtures/subagents/projects/C--Users-User-AppData-Local-Temp-snake-demo/f44205ef-4a6f-4090-9506-1978b4eada1b/subagents/agent-af66d7d8e4a99a6bb.jsonl"
);

struct LegacySource<'a> {
    path: PathBuf,
    tab: &'a str,
    plan: &'a str,
    provider: &'a str,
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kpi-rebuild-{}-{}", name, ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn first_lines(body: &str, count: usize) -> String {
    body.split_inclusive('\n').take(count).collect()
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A v4 ledger as the line-counting ingestor left it: one keyless row per usage line
/// and a cursor at the end of every transcript.
fn legacy_ledger(db: &Path, sources: &[LegacySource<'_>]) {
    let conn = Connection::open(db).unwrap();
    KpiLedger::create_v1_schema(&conn).unwrap();
    KpiLedger::add_v2_token_columns(&conn).unwrap();
    KpiLedger::add_v3_dedup_and_cursors(&conn).unwrap();
    KpiLedger::add_v4_workspace_and_tab_created(&conn).unwrap();
    for source in sources {
        let len = std::fs::metadata(&source.path).unwrap().len();
        let (batch, _) = UsageIngestor::read_through(&UsageSource::new(&source.path), len).unwrap();
        let lines = batch
            .records
            .iter()
            .flat_map(|record| record.observed.iter())
            .chain(batch.superseded.iter());
        for (timestamp, tokens) in lines {
            conn.execute(
                "INSERT INTO kpi_events (schema_version, plan_id, tab_id, role, provider, model,
                     ts, kind, bytes, input_tokens, uncached_input_tokens, output_tokens)
                 VALUES (4, ?1, ?2, 'Worker', ?3, 'model-x', ?4, 'token_usage', ?5, ?6, ?7, ?8)",
                params![
                    source.plan,
                    source.tab,
                    source.provider,
                    *timestamp as i64,
                    tokens.total() as i64,
                    tokens.input as i64,
                    tokens.uncached_input as i64,
                    tokens.output as i64,
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO kpi_usage_cursors (source_path, cursor_offset) VALUES (?1, ?2)",
            params![source.path.to_string_lossy(), len as i64],
        )
        .unwrap();
    }
}

/// `(turns, input, uncached input, output)` for one provider's token rows.
fn provider_usage(ledger: &KpiLedger, provider: &str) -> (u64, u64, u64, u64) {
    ledger
        .aggregate_token_usage_by_provider_model()
        .unwrap()
        .into_iter()
        .filter(|row| row.provider == provider)
        .fold((0, 0, 0, 0), |acc, row| {
            (
                acc.0 + row.turns,
                acc.1 + row.input_tokens,
                acc.2 + row.uncached_input_tokens,
                acc.3 + row.output_tokens,
            )
        })
}

#[test]
fn kpi_usage_rebuild_restores_ground_truth_and_is_idempotent() {
    let dir = scratch("truth");
    let claude = dir.join("claude.jsonl");
    let parent = dir.join("parent.jsonl");
    let sidechain = dir
        .join("parent")
        .join("subagents")
        .join("agent-af66d7d8e4a99a6bb.jsonl");
    let codex = dir.join("rollout.jsonl");
    write(&claude, CLAUDE_STREAMED);
    write(&parent, SUBAGENT_PARENT);
    write(&sidechain, SIDECHAIN);
    write(&codex, CODEX_REPEATED);
    let db = dir.join("kpi_ledger.db");
    let claude_tab = |path: &PathBuf| LegacySource {
        path: path.clone(),
        tab: "tab-claude",
        plan: "pl_claude",
        provider: "claude",
    };
    legacy_ledger(
        &db,
        &[
            claude_tab(&claude),
            claude_tab(&parent),
            claude_tab(&sidechain),
            LegacySource {
                path: codex.clone(),
                tab: "tab-codex",
                plan: "pl_codex",
                provider: "codex",
            },
        ],
    );
    let ledger = Arc::new(KpiLedger::open_at(&db).unwrap());
    assert_eq!(provider_usage(&ledger, "claude").0, 5 + 1 + 7);
    assert_eq!(provider_usage(&ledger, "codex").0, 6);

    let recorder = KpiRecorder::with_ledger(ledger.clone());
    let outcome = recorder.rebuild_usage().unwrap();

    assert_eq!(outcome.sources, 4);
    assert_eq!(outcome.missing_sources, 0);
    assert_eq!(outcome.responses, 2 + 1 + 3 + 3);
    assert_eq!(outcome.legacy_rows_removed, 13 + 6);
    assert_eq!(outcome.unattributed_responses, 0);
    assert_eq!(
        outcome.plans,
        BTreeSet::from(["pl_claude".to_string(), "pl_codex".to_string()])
    );
    // Streamed fixture, sub-agent parent and sidechain, each response's largest line.
    assert_eq!(
        provider_usage(&ledger, "claude"),
        (
            6,
            78_775 + 116_295 + 115_114,
            4_221 + 78 + 40_560,
            607 + 544 + 838
        )
    );
    assert_eq!(provider_usage(&ledger, "codex"), (3, 50_000, 10_000, 700));
    let rows = ledger.list_all().unwrap();
    for row in rows
        .iter()
        .filter(|row| row.kind == KpiEventKind::TokenUsage)
    {
        assert!(row.dedup_key.is_some());
        let expected = if row.provider == "codex" {
            ("tab-codex", "pl_codex")
        } else {
            ("tab-claude", "pl_claude")
        };
        assert_eq!(
            (row.tab_id.as_str(), row.plan_id.as_deref()),
            (expected.0, Some(expected.1))
        );
    }

    let again = recorder.rebuild_usage().unwrap();
    assert_eq!(again.responses, outcome.responses);
    assert_eq!(again.legacy_rows_removed, 0);
    assert_eq!(again.unattributed_responses, 0);
    assert_eq!(ledger.list_all().unwrap(), rows);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_rebuild_persists_codex_totals_for_the_next_ingest() {
    let dir = scratch("codex-resume");
    let codex = dir.join("rollout.jsonl");
    write(&codex, &first_lines(CODEX_REPEATED, 6));
    let db = dir.join("kpi_ledger.db");
    legacy_ledger(
        &db,
        &[LegacySource {
            path: codex.clone(),
            tab: "tab-codex",
            plan: "pl_codex",
            provider: "codex",
        }],
    );
    let ledger = Arc::new(KpiLedger::open_at(&db).unwrap());
    let recorder = KpiRecorder::with_ledger(ledger.clone());

    recorder.rebuild_usage().unwrap();
    assert_eq!(provider_usage(&ledger, "codex"), (2, 38_000, 8_000, 500));
    let cursor = ledger
        .load_usage_cursor(&codex.to_string_lossy())
        .unwrap()
        .unwrap();
    assert_eq!(
        cursor.cumulative,
        Some(CumulativeUsage {
            input: 38_000,
            cached_input: 30_000,
            output: 500,
            total: 38_500,
        })
    );

    // The resumed session re-emits the second turn's totals before its third turn.
    write(&codex, CODEX_REPEATED);
    let attribution = KpiAttribution::for_tab("tab-codex")
        .plan(Some("pl_codex".into()), None)
        .provider(Some("codex".into()));
    recorder.request_ingest(UsageSource::new(&codex), attribution);
    recorder.drain();

    assert_eq!(provider_usage(&ledger, "codex"), (3, 50_000, 10_000, 700));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_rebuild_counts_a_missing_transcript_and_keeps_its_rows() {
    let dir = scratch("missing");
    let claude = dir.join("claude.jsonl");
    write(&claude, CLAUDE_STREAMED);
    let db = dir.join("kpi_ledger.db");
    legacy_ledger(
        &db,
        &[LegacySource {
            path: claude.clone(),
            tab: "tab-claude",
            plan: "pl_claude",
            provider: "claude",
        }],
    );
    std::fs::remove_file(&claude).unwrap();
    let ledger = Arc::new(KpiLedger::open_at(&db).unwrap());
    let before = ledger.list_all().unwrap();

    let outcome = KpiRecorder::with_ledger(ledger.clone())
        .rebuild_usage()
        .unwrap();

    assert_eq!((outcome.sources, outcome.missing_sources), (0, 1));
    assert_eq!(ledger.list_all().unwrap(), before);

    std::fs::remove_dir_all(&dir).ok();
}
