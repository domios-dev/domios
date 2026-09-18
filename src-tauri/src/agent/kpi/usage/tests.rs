use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kpi-usage-{}-{}", name, ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn claude_fixture() -> &'static str {
    include_str!("fixtures/claude-session.jsonl")
}

fn codex_fixture() -> &'static str {
    include_str!("fixtures/codex-rollout.jsonl")
}

fn opencode_fixture_sql() -> &'static str {
    include_str!("fixtures/opencode-messages.sql")
}

fn write_source(dir: &Path, name: &str, body: &str) -> UsageSource {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    UsageSource::new(path)
}

fn build_opencode_db(dir: &Path) -> UsageSource {
    let path = dir.join("opencode.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(opencode_fixture_sql()).unwrap();
    drop(conn);
    UsageSource::new(path)
}

#[test]
fn claude_fixture_totals_match_precomputed_values() {
    let dir = scratch("claude");
    let source = write_source(&dir, "session.jsonl", claude_fixture());

    let totals = UsageIngestor::default().ingest(&source).unwrap().totals();

    // input = uncached + cache_read per record: (24384+25382)+(1206+49764)+(10+413282)
    assert_eq!(totals.input_tokens, 514_028);
    // uncached = input_tokens + cache_creation_input_tokens: 24384 + 1206 + 10
    assert_eq!(totals.uncached_input_tokens, 25_600);
    assert_eq!(totals.output_tokens, 1_800, "204 + 88 + 1508");
    assert_eq!(totals.compactions, 1, "one compact_boundary line");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn codex_fixture_totals_are_deltas_of_cumulative_usage() {
    let dir = scratch("codex");
    let source = write_source(&dir, "rollout.jsonl", codex_fixture());

    let totals = UsageIngestor::default().ingest(&source).unwrap().totals();

    assert_eq!(
        totals.input_tokens, 38_000,
        "12642 + 25358 (last_token_usage)"
    );
    assert_eq!(
        totals.uncached_input_tokens, 8_000,
        "(12642-11136) + (25358-18864)"
    );
    assert_eq!(totals.output_tokens, 500, "164 + 336");
    assert_eq!(totals.compactions, 0, "no compaction marker in Codex logs");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opencode_sqlite_fixture_totals_match_precomputed_values() {
    let dir = scratch("opencode");
    let source = build_opencode_db(&dir);

    let totals = UsageIngestor::default().ingest(&source).unwrap().totals();

    assert_eq!(totals.input_tokens, 55_296, "(279+23296) + (21+1700+30000)");
    assert_eq!(totals.uncached_input_tokens, 2_000, "(279+0) + (21+1700)");
    assert_eq!(totals.output_tokens, 820, "(73+247) + (400+100)");
    assert_eq!(totals.compactions, 0);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sqlite_source_scoped_to_one_session_never_reads_a_sibling() {
    let dir = scratch("opencode-scoped");
    let path = dir.join("opencode.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, \
         time_updated INTEGER, data TEXT);
         INSERT INTO message VALUES ('m1', 'ses_a', 1, 1,
           '{\"role\":\"assistant\",\"tokens\":{\"input\":10,\"output\":1,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}},\"time\":{\"created\":1000}}');
         INSERT INTO message VALUES ('m2', 'ses_b', 1, 1,
           '{\"role\":\"assistant\",\"tokens\":{\"input\":99,\"output\":9,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}},\"time\":{\"created\":2000}}');
         INSERT INTO message VALUES ('m3', 'ses_a', 1, 1,
           '{\"role\":\"assistant\",\"tokens\":{\"input\":20,\"output\":2,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}},\"time\":{\"created\":3000}}');",
    )
    .unwrap();
    drop(conn);

    let a = UsageSource::scoped(&path, "ses_a");
    let b = UsageSource::scoped(&path, "ses_b");
    let mut ingestor = UsageIngestor::default();

    let a_batch = ingestor.ingest(&a).unwrap();
    assert_eq!(a_batch.records.len(), 2, "only ses_a rows");
    assert_eq!(a_batch.totals().input_tokens, 30);
    assert_eq!(a_batch.totals().output_tokens, 3);

    let b_batch = ingestor.ingest(&b).unwrap();
    assert_eq!(
        b_batch.records.len(),
        1,
        "a sibling session on the shared log keeps its own cursor"
    );
    assert_eq!(b_batch.totals().input_tokens, 99);
    assert_ne!(
        a_batch.records[0].key, b_batch.records[0].key,
        "shared-log sessions must not share a dedup key"
    );

    assert!(ingestor.ingest(&a).unwrap().is_empty());
    assert!(ingestor.ingest(&b).unwrap().is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn source_key_round_trips_a_session_scope() {
    let plain = UsageSource::new("/tmp/session.jsonl");
    assert_eq!(plain.key(), "/tmp/session.jsonl");
    assert_eq!(UsageSource::from_key(&plain.key()), plain);

    let scoped = UsageSource::scoped("/tmp/opencode.db", "ses_a");
    let key = scoped.key();
    assert_ne!(key, "/tmp/opencode.db");
    assert_eq!(UsageSource::from_key(&key), scoped);
    assert_ne!(
        UsageSource::from_key(&key),
        UsageSource::scoped("/tmp/opencode.db", "ses_b"),
        "different sessions never collide on one shared path"
    );
}

#[test]
fn second_pass_over_unchanged_source_yields_nothing() {
    let dir = scratch("idempotent");
    let claude = write_source(&dir, "session.jsonl", claude_fixture());
    let opencode = build_opencode_db(&dir);
    let mut ingestor = UsageIngestor::default();

    assert_eq!(ingestor.ingest(&claude).unwrap().records.len(), 4);
    assert!(
        ingestor.ingest(&claude).unwrap().is_empty(),
        "unchanged line log re-reads zero records"
    );
    assert_eq!(ingestor.ingest(&opencode).unwrap().records.len(), 2);
    assert!(
        ingestor.ingest(&opencode).unwrap().is_empty(),
        "unchanged sqlite source re-reads zero rows"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn appended_lines_are_read_without_reparsing_the_prefix() {
    let dir = scratch("append");
    let source = write_source(&dir, "session.jsonl", claude_fixture());
    let mut ingestor = UsageIngestor::default();

    let first = ingestor.ingest(&source).unwrap();
    let cursor_after_first = ingestor.cursor_offset(&source).unwrap();
    assert_eq!(cursor_after_first, claude_fixture().len() as u64);

    let appended = "{\"type\":\"assistant\",\"timestamp\":\"2026-07-17T13:09:00.000Z\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":5,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":95,\"output_tokens\":7}}}\n";
    let mut body = claude_fixture().to_string();
    body.push_str(appended);
    std::fs::write(&source.path, &body).unwrap();

    let second = ingestor.ingest(&source).unwrap();
    assert_eq!(second.records.len(), 1, "only the appended line is parsed");
    let totals = second.totals();
    assert_eq!(totals.input_tokens, 100);
    assert_eq!(totals.uncached_input_tokens, 5);
    assert_eq!(totals.output_tokens, 7);
    assert_eq!(
        ingestor.cursor_offset(&source).unwrap(),
        cursor_after_first + appended.len() as u64,
        "cursor advanced by exactly the appended bytes"
    );
    assert_eq!(first.records.len(), 4);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn half_written_trailing_line_is_reread_whole_on_the_next_pass() {
    let dir = scratch("partial");
    let complete = "{\"type\":\"assistant\",\"timestamp\":\"2026-07-17T13:09:00.000Z\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":5,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":95,\"output_tokens\":7}}}\n";
    let split_at = complete.len() / 2;
    let source = write_source(&dir, "session.jsonl", &complete[..split_at]);
    let mut ingestor = UsageIngestor::default();

    assert!(
        ingestor.ingest(&source).unwrap().is_empty(),
        "a line without its newline is not consumed"
    );
    assert_eq!(ingestor.cursor_offset(&source).unwrap(), 0);

    std::fs::write(&source.path, complete).unwrap();
    let batch = ingestor.ingest(&source).unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.totals().output_tokens, 7);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn truncated_source_resets_the_cursor_instead_of_skipping_content() {
    let dir = scratch("truncate");
    let source = write_source(&dir, "session.jsonl", claude_fixture());
    let mut ingestor = UsageIngestor::default();
    assert_eq!(ingestor.ingest(&source).unwrap().records.len(), 4);

    let rotated = "{\"timestamp\":\"2026-05-21T06:48:35.288Z\",\"payload\":{\"info\":{\"last_token_usage\":{\"input_tokens\":700,\"cached_input_tokens\":200,\"output_tokens\":9}}}}\n";
    assert!(rotated.len() < claude_fixture().len());
    std::fs::write(&source.path, rotated).unwrap();
    let batch = ingestor.ingest(&source).unwrap();
    assert_eq!(
        batch.records.len(),
        1,
        "shorter file is re-read from byte zero"
    );
    assert_eq!(batch.totals().input_tokens, 700);
    assert_eq!(batch.totals().uncached_input_tokens, 500);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn non_usage_lines_and_garbage_are_skipped_without_failing_the_pass() {
    let dir = scratch("garbage");
    let source = write_source(
        &dir,
        "session.jsonl",
        "not json at all\n{\"type\":\"user\"}\n\n{\"tokens\":{\"input\":1,\"output\":2,\"cache\":{\"read\":3,\"write\":4}},\"time\":{\"created\":1784633081000}}\n",
    );

    let batch = UsageIngestor::default().ingest(&source).unwrap();
    assert_eq!(batch.records.len(), 1);
    let totals = batch.totals();
    assert_eq!(totals.input_tokens, 8, "1 + 4 (write) + 3 (read)");
    assert_eq!(totals.uncached_input_tokens, 5);
    assert_eq!(totals.output_tokens, 2);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn codex_compacted_marker_is_recognized_as_compaction() {
    let dir = scratch("codex-compaction");
    let source = write_source(
        &dir,
        "rollout.jsonl",
        "{\"timestamp\":\"2026-05-21T06:50:00.000Z\",\"type\":\"compacted\",\"payload\":{\"message\":\"summary\",\"window_number\":1}}\n",
    );

    let batch = UsageIngestor::default().ingest(&source).unwrap();
    assert_eq!(batch.totals().compactions, 1);
    assert!(batch.records[0].tokens.is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opencode_summary_message_is_recognized_as_compaction() {
    let dir = scratch("opencode-compaction");
    let source = write_source(
        &dir,
        "messages.jsonl",
        "{\"role\":\"assistant\",\"modelID\":\"big-pickle\",\"providerID\":\"opencode\",\"summary\":true,\"cost\":0,\"tokens\":{\"total\":0,\"input\":0,\"output\":0,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}},\"time\":{\"created\":1784633090000,\"completed\":1784633095000}}\n",
    );

    let batch = UsageIngestor::default().ingest(&source).unwrap();
    assert_eq!(batch.totals().compactions, 1);
    assert!(batch.records[0].tokens.is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opencode_user_message_summary_object_is_not_mistaken_for_compaction() {
    let dir = scratch("opencode-user-summary");
    let source = write_source(
        &dir,
        "messages.jsonl",
        "{\"role\":\"user\",\"summary\":{\"title\":\"t\",\"diffs\":[]},\"tokens\":{\"input\":1,\"output\":2,\"cache\":{\"read\":3,\"write\":4}},\"time\":{\"created\":1784633090000}}\n",
    );

    let batch = UsageIngestor::default().ingest(&source).unwrap();
    assert_eq!(
        batch.totals().compactions,
        0,
        "object-shaped summary on a user row is not a compaction marker"
    );
    assert_eq!(batch.totals().input_tokens, 8);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn seed_cursor_resumes_from_a_persisted_watermark_instead_of_byte_zero() {
    let dir = scratch("seed-cursor");
    let source = write_source(&dir, "session.jsonl", claude_fixture());
    let mut ingestor = UsageIngestor::default();

    ingestor.seed_cursor(
        &source,
        UsageCursor {
            offset: claude_fixture().len() as u64,
            ..Default::default()
        },
    );
    assert!(
        ingestor.ingest(&source).unwrap().is_empty(),
        "seeding at end-of-file skips every already-recorded line, mirroring what a \
         restarted daemon must do to avoid re-ingesting the whole session"
    );

    // A cursor already present for this run (e.g. from an earlier ingest this pass)
    // must win over a stale/short seed value — seeding never rewinds.
    let mut ingestor = UsageIngestor::default();
    ingestor.ingest(&source).unwrap();
    let advanced = ingestor.cursor_offset(&source).unwrap();
    ingestor.seed_cursor(&source, UsageCursor::default());
    assert_eq!(ingestor.cursor_offset(&source).unwrap(), advanced);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn record_timestamps_come_from_the_provider_line_not_the_ingest_clock() {
    let dir = scratch("timestamps");
    let source = write_source(&dir, "session.jsonl", claude_fixture());

    let batch = UsageIngestor::default().ingest(&source).unwrap();
    // 2026-07-17T13:01:25Z
    assert_eq!(batch.records[0].timestamp, 1_784_293_285);
    assert!(
        batch
            .records
            .windows(2)
            .all(|w| w[0].timestamp <= w[1].timestamp),
        "records stay in provider-log order"
    );

    std::fs::remove_dir_all(&dir).ok();
}
