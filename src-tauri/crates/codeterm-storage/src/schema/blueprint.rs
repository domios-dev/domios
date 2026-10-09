pub(super) const SCHEMA: &str = r#"
ALTER TABLE sentinel_buckets RENAME TO blueprint_buckets;
ALTER TABLE blueprint_buckets RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_command_cleanup RENAME TO blueprint_command_cleanup;
ALTER TABLE blueprint_command_cleanup RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_command_results RENAME TO blueprint_command_results;
ALTER TABLE blueprint_command_results RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_definitions RENAME TO blueprint_definitions;
ALTER TABLE blueprint_definitions RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_handler_results RENAME TO blueprint_handler_results;
ALTER TABLE blueprint_handler_results RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_handler_targets RENAME TO blueprint_handler_targets;
ALTER TABLE blueprint_handler_targets RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_handlers RENAME TO blueprint_handlers;
ALTER TABLE blueprint_handlers RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_heads RENAME TO blueprint_heads;
ALTER TABLE blueprint_heads RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_runtime RENAME TO blueprint_runtime;
ALTER TABLE blueprint_runtime RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinel_transitions RENAME TO blueprint_transitions;
ALTER TABLE blueprint_transitions RENAME COLUMN sentinel_id TO blueprint_id;
ALTER TABLE sentinels RENAME TO blueprints;
ALTER TABLE blueprint_step_journal RENAME COLUMN sentinel_id TO blueprint_id;
CREATE TEMP TABLE blueprint_handlers_hold AS SELECT * FROM blueprint_handlers;
DROP TABLE blueprint_handlers;
CREATE TABLE blueprint_handlers (
    blueprint_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    bucket TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    handler_id TEXT NOT NULL CHECK (length(handler_id) > 0),
    kind TEXT NOT NULL CHECK (kind IN ('notify', 'send', 'cron', 'blueprint', 'stop', 'command',
        'respond_prompt', 'escalate')),
    target_ref TEXT,
    command TEXT,
    label TEXT,
    timeout_ms INTEGER,
    safe_kind TEXT CHECK (safe_kind IS NULL OR length(safe_kind) > 0),
    escalate_to TEXT CHECK (escalate_to IN ('creator', 'agent')),
    PRIMARY KEY (blueprint_id, version, bucket, ordinal),
    UNIQUE (blueprint_id, version, handler_id),
    FOREIGN KEY (blueprint_id, version, bucket)
        REFERENCES blueprint_buckets(blueprint_id, version, bucket),
    CHECK ((kind IN ('send', 'cron', 'blueprint') OR escalate_to IS 'agent')
        = (target_ref IS NOT NULL)),
    CHECK ((kind = 'command') = (command IS NOT NULL)),
    CHECK ((command IS NULL) = (label IS NULL) AND (command IS NULL) = (timeout_ms IS NULL)),
    CHECK ((kind = 'respond_prompt') = (safe_kind IS NOT NULL)),
    CHECK ((kind = 'escalate') = (escalate_to IS NOT NULL))
) STRICT;
INSERT INTO blueprint_handlers SELECT blueprint_id, version, bucket, ordinal, handler_id, CASE kind WHEN 'sentinel' THEN 'blueprint' ELSE kind END, target_ref, command, label, timeout_ms, safe_kind, escalate_to FROM blueprint_handlers_hold;
DROP TABLE blueprint_handlers_hold;
UPDATE blueprint_handler_targets SET kind = 'blueprint' WHERE kind = 'sentinel';
DROP INDEX sentinels_by_scope;
CREATE INDEX blueprints_by_scope ON blueprints(workspace_id, team_id);
DROP INDEX sentinels_by_team;
CREATE INDEX blueprints_by_team ON blueprints(team_id);
DROP INDEX sentinels_by_scenario;
CREATE INDEX blueprints_by_scenario ON blueprints(scenario_id);
CREATE TEMP TABLE blueprint_definitions_hold AS SELECT * FROM blueprint_definitions;
DROP TABLE blueprint_definitions;
CREATE TABLE blueprint_definitions (
    blueprint_id TEXT NOT NULL REFERENCES blueprints(id),
    version INTEGER NOT NULL CHECK (version > 0),
    provenance TEXT NOT NULL CHECK (provenance IN ('authored', 'imported')),
    name TEXT,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('probe', 'observe', 'input', 'event', 'terminal', 'pipeline')),
    source_spec TEXT,
    probe TEXT NOT NULL,
    observe_kind TEXT CHECK (observe_kind IN ('tab', 'cron_run')),
    observe_ref TEXT CHECK (observe_ref IS NULL OR length(observe_ref) > 0),
    question TEXT NOT NULL,
    interval_ms INTEGER NOT NULL CHECK (interval_ms > 0),
    until_bucket TEXT,
    escalation_timeout_ms INTEGER
        CHECK (escalation_timeout_ms IS NULL OR escalation_timeout_ms > 0),
    max_transitions INTEGER NOT NULL CHECK (max_transitions BETWEEN 1 AND 65535),
    max_actions INTEGER NOT NULL CHECK (max_actions BETWEEN 1 AND 65535),
    replaced_version INTEGER,
    author_tab_id TEXT CHECK (author_tab_id IS NULL OR length(author_tab_id) > 0),
    cause_transition_id TEXT
        CHECK (cause_transition_id IS NULL OR length(cause_transition_id) > 0),
    authored_at INTEGER CHECK (authored_at IS NULL OR authored_at >= 0),
    created_revision INTEGER NOT NULL CHECK (created_revision > 0),
    pipeline_spec TEXT,
    terminal_spec TEXT CHECK (terminal_spec IS NULL OR length(terminal_spec) <= 8192),
    session INTEGER NOT NULL DEFAULT 0 CHECK (session IN (0, 1)),
    PRIMARY KEY (blueprint_id, version),
    CHECK ((source_kind = 'pipeline') = (pipeline_spec IS NOT NULL)),
    CHECK ((source_kind = 'terminal') = (terminal_spec IS NOT NULL)),
    CHECK ((source_kind = 'probe') = (length(probe) > 0)),
    CHECK ((source_kind IN ('observe', 'terminal')) = (observe_kind IS NOT NULL)),
    CHECK ((source_kind IN ('input', 'event')) = (source_spec IS NOT NULL)),
    CHECK ((observe_kind IS NULL) = (observe_ref IS NULL)),
    CHECK (replaced_version IS NULL OR (replaced_version > 0 AND replaced_version < version)),
    CHECK ((replaced_version IS NULL) = (author_tab_id IS NULL)
        AND (replaced_version IS NULL) = (authored_at IS NULL)),
    CHECK (cause_transition_id IS NULL OR replaced_version IS NOT NULL)
) STRICT;
INSERT INTO blueprint_definitions SELECT blueprint_id, version, provenance, name,
    CASE WHEN pipeline_spec IS NOT NULL THEN 'pipeline' WHEN terminal_spec IS NOT NULL THEN 'terminal' ELSE source_kind END,
    source_spec, CASE WHEN pipeline_spec IS NOT NULL THEN '' ELSE probe END,
    observe_kind, observe_ref, question, interval_ms, until_bucket, escalation_timeout_ms,
    max_transitions, max_actions, replaced_version, author_tab_id, cause_transition_id,
    authored_at, created_revision, pipeline_spec, terminal_spec, session FROM blueprint_definitions_hold;
DROP TABLE blueprint_definitions_hold;
CREATE TEMP TABLE blueprint_journal_hold AS SELECT * FROM blueprint_step_journal;
DROP TABLE blueprint_step_journal;
CREATE TABLE blueprint_step_journal (
    entry_id INTEGER PRIMARY KEY AUTOINCREMENT,
    blueprint_id TEXT NOT NULL REFERENCES blueprints(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL CHECK (length(run_id) BETWEEN 1 AND 128),
    step_index INTEGER NOT NULL CHECK (step_index BETWEEN -1 AND 63),
    step_name TEXT NOT NULL CHECK (length(step_name) BETWEEN 1 AND 128),
    state TEXT NOT NULL CHECK (state IN ('running', 'done', 'failed', 'skipped', 'held')),
    outcome TEXT CHECK (outcome IS NULL OR length(outcome) BETWEEN 1 AND 128),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    ended_at_ms INTEGER CHECK (ended_at_ms IS NULL OR ended_at_ms >= started_at_ms),
    result_summary TEXT CHECK (result_summary IS NULL OR length(result_summary) <= 200),
    reason TEXT CHECK (reason IS NULL OR length(reason) <= 200),
    body_ref TEXT CHECK (body_ref IS NULL OR length(body_ref) <= 32768),
    CHECK ((state = 'running') = (ended_at_ms IS NULL)),
    CHECK (step_index != -1 OR step_name IN ('$run', '$result'))
) STRICT;
INSERT INTO blueprint_step_journal SELECT entry_id, blueprint_id, run_id, step_index,
    step_name, state, outcome, started_at_ms, ended_at_ms,
    CASE WHEN step_name = '$result' AND result_summary LIKE 'body_ref:%' THEN NULL ELSE result_summary END,
    reason, CASE WHEN step_name = '$result' AND result_summary LIKE 'body_ref:%'
        AND result_summary != 'body_ref:unavailable' THEN substr(result_summary, 10) ELSE NULL END
    FROM blueprint_journal_hold;
DROP TABLE blueprint_journal_hold;
CREATE INDEX blueprint_step_journal_newest ON blueprint_step_journal(blueprint_id, entry_id DESC);

"#;

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    #[test]
    fn blueprint_rename_preserves_pre_release_rows_and_foreign_keys() {
        let mut conn = Connection::open_in_memory().unwrap();
        for step in super::super::MIGRATIONS.iter().take(12) {
            conn.execute_batch(step).unwrap();
        }
        conn.execute_batch(r#"
            INSERT INTO sentinels VALUES ('sentinel_old', 0, 'tab', 'workspace', NULL, '/work', 1, 'sentinel_old', 1);
            INSERT INTO sentinel_definitions (sentinel_id, version, provenance, source_kind, probe, question, interval_ms, max_transitions, max_actions, created_revision)
                VALUES ('sentinel_old', 1, 'authored', 'probe', 'status', 'state?', 20000, 64, 256, 1);
            INSERT INTO sentinel_definitions (sentinel_id, version, provenance, source_kind, probe, question, interval_ms, max_transitions, max_actions, created_revision, pipeline_spec)
                VALUES ('sentinel_old', 2, 'authored', 'probe', 'blueprint-pipeline', '', 20000, 64, 256, 1, '{}');
            INSERT INTO sentinel_definitions (sentinel_id, version, provenance, source_kind, probe, observe_kind, observe_ref, question, interval_ms, max_transitions, max_actions, created_revision, terminal_spec)
                VALUES ('sentinel_old', 3, 'authored', 'observe', '', 'tab', 'watched', 'state?', 20000, 64, 256, 1, '{"pattern":"ready","idle_ms":null}');
            INSERT INTO sentinel_heads VALUES ('sentinel_old', 1);
            INSERT INTO sentinel_buckets VALUES ('sentinel_old', 1, 0, 'done', '^done$', 0);
            INSERT INTO sentinel_handlers (sentinel_id, version, bucket, ordinal, handler_id, kind, target_ref)
                VALUES ('sentinel_old', 1, 'done', 0, 'next', 'sentinel', 'sentinel_old');
            INSERT INTO sentinel_handler_targets VALUES ('sentinel_old', 1, 'next', 'sentinel', 'sentinel_old', 'workspace', NULL, 'tab');
            INSERT INTO blueprint_step_journal (sentinel_id, run_id, step_index, step_name, state, started_at_ms, ended_at_ms)
                VALUES ('sentinel_old', 'run', -1, '$run', 'done', 1000, 1001);
            INSERT INTO blueprint_step_journal (sentinel_id, run_id, step_index, step_name, state, started_at_ms, ended_at_ms, result_summary)
                VALUES ('sentinel_old', 'legacy-result', -1, '$result', 'done', 1000, 1001, 'body_ref:inbox/old.md');
        "#).unwrap();
        super::super::migrate(&mut conn, 12).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT probe FROM blueprint_definitions WHERE blueprint_id = 'sentinel_old' AND version = 1",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "status"
        );
        assert_eq!(
            conn.query_row(
                "SELECT source_kind, probe FROM blueprint_definitions WHERE version = 2",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            )
            .unwrap(),
            ("pipeline".into(), String::new())
        );
        assert_eq!(
            conn.query_row(
                "SELECT source_kind FROM blueprint_definitions WHERE version = 3",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "terminal"
        );
        assert_eq!(
            conn.query_row("SELECT kind FROM blueprint_handlers", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "blueprint"
        );
        assert_eq!(
            conn.query_row("SELECT kind FROM blueprint_handler_targets", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
            "blueprint"
        );
        assert_eq!(
            conn.query_row(
                "SELECT run_id FROM blueprint_step_journal WHERE blueprint_id = 'sentinel_old' AND step_name = '$run'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "run"
        );
        assert_eq!(
            conn.query_row(
                "SELECT body_ref FROM blueprint_step_journal WHERE run_id = 'legacy-result'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "inbox/old.md"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name LIKE 'sentinel%'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
