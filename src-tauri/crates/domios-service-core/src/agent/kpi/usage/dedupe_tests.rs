use super::*;
use crate::agent::kpi_ledger::CumulativeUsage;

const CLAUDE_STREAMED: &str = include_str!("fixtures/claude-streamed-responses.jsonl");
const CODEX_REPEATED: &str = include_str!("fixtures/codex-repeated-token-count.jsonl");

const SECOND_TURN_TOTALS: CumulativeUsage = CumulativeUsage {
    input: 38_000,
    cached_input: 30_000,
    output: 500,
    total: 38_500,
};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kpi-dedupe-{}-{}", name, ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn first_lines(body: &str, count: usize) -> String {
    body.split_inclusive('\n').take(count).collect()
}

fn write_source(dir: &Path, name: &str, body: &str) -> UsageSource {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    UsageSource::new(path)
}

fn token_triple(batch: &UsageBatch) -> (u64, u64, u64) {
    let totals = batch.totals();
    (
        totals.input_tokens,
        totals.uncached_input_tokens,
        totals.output_tokens,
    )
}

#[test]
fn kpi_usage_dedupe_claude_streamed_lines_make_one_record_per_response() {
    let dir = scratch("claude-streamed");
    let source = write_source(&dir, "session.jsonl", CLAUDE_STREAMED);

    let batch = UsageIngestor::default().ingest(&source).unwrap();

    let keys: Vec<_> = batch.records.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "usage:response:msg_01AAAAAAAAAAAAAAAAAAAAAA:req_01AAAAAAAAAAAAAAAAAAAAAA",
            "usage:response:msg_01BBBBBBBBBBBBBBBBBBBBBB:req_01BBBBBBBBBBBBBBBBBBBBBB",
        ]
    );
    let lines_per_response: Vec<_> = batch.records.iter().map(|r| r.observed.len()).collect();
    assert_eq!(lines_per_response, vec![3, 2]);
    // Largest line per response: (8 + 1_896 + 36_329, out 160) and (8 + 2_309 + 38_225, out 447).
    assert_eq!(token_triple(&batch), (78_775, 4_221, 607));
    assert_eq!(batch.records[0].timestamp, batch.records[0].observed[0].0);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_codex_repeated_token_count_adds_nothing() {
    let dir = scratch("codex-repeated");
    let source = write_source(&dir, "rollout.jsonl", CODEX_REPEATED);

    let batch = UsageIngestor::default().ingest(&source).unwrap();

    assert_eq!(batch.records.len(), 3);
    assert_eq!(batch.superseded.len(), 3);
    // The final cumulative totals: 50_000 input of which 40_000 cached, 700 output.
    assert_eq!(token_triple(&batch), (50_000, 10_000, 700));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_codex_resume_from_persisted_totals_counts_only_new_turns() {
    let dir = scratch("codex-resume");
    let source = write_source(&dir, "rollout.jsonl", &first_lines(CODEX_REPEATED, 6));
    let mut first_run = UsageIngestor::default();
    let before = first_run.ingest(&source).unwrap();
    assert_eq!(before.records.len(), 2);
    let persisted = first_run.cursor(&source).unwrap().persisted();
    assert_eq!(persisted.cumulative, Some(SECOND_TURN_TOTALS));

    std::fs::write(&source.path, CODEX_REPEATED).unwrap();
    let mut restarted = UsageIngestor::default();
    restarted.seed_cursor(&source, UsageCursor::from_persisted(persisted));
    let after = restarted.ingest(&source).unwrap();

    assert_eq!(after.records.len(), 1);
    assert_eq!(after.superseded.len(), 1);
    assert_eq!(token_triple(&after), (12_000, 2_000, 200));
    assert_eq!(token_triple(&before).0 + token_triple(&after).0, 50_000);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_watermark_without_totals_counts_only_the_line_itself() {
    let dir = scratch("codex-unknown");
    let source = write_source(&dir, "rollout.jsonl", CODEX_REPEATED);
    let mut ingestor = UsageIngestor::default();
    let legacy = UsageCursorState {
        offset: first_lines(CODEX_REPEATED, 7).len() as u64,
        cumulative: None,
    };
    ingestor.seed_cursor(&source, UsageCursor::from_persisted(legacy));

    let batch = ingestor.ingest(&source).unwrap();

    assert_eq!(batch.records.len(), 1);
    assert_eq!(token_triple(&batch), (12_000, 2_000, 200));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kpi_usage_dedupe_sqlite_rows_are_keyed_by_their_message_id() {
    let dir = scratch("sqlite-identity");
    let path = dir.join("opencode.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("fixtures/opencode-messages.sql"))
        .unwrap();
    drop(conn);

    let batch = UsageIngestor::default()
        .ingest(&UsageSource::new(&path))
        .unwrap();

    let keys: Vec<_> = batch.records.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, vec!["usage:response:msg_2", "usage:response:msg_3"]);

    std::fs::remove_dir_all(&dir).ok();
}
