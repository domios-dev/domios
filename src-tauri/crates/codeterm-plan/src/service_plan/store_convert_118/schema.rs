pub(super) const LEGACY_SCHEMA: &str = r#"
    CREATE TABLE plans (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        team_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        document_json TEXT NOT NULL
    );
    CREATE UNIQUE INDEX one_active_plan_per_team
        ON plans(team_id)
        WHERE team_id IS NOT NULL AND status = 'active';
    CREATE INDEX plans_status_updated ON plans(status, updated_at DESC);
    CREATE TABLE specs (
        id TEXT PRIMARY KEY,
        plan_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        FOREIGN KEY(plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE plan_operations (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        state TEXT NOT NULL CHECK(state IN ('queued','running','committed','failed')),
        submitted_at INTEGER NOT NULL,
        finished_at INTEGER,
        error TEXT
    );
    CREATE INDEX plan_operations_state_id ON plan_operations(state, id);
    CREATE VIRTUAL TABLE plan_fts USING fts5(plan_id UNINDEXED, title, body);
    CREATE TABLE plan_tasks (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        milestone_count INTEGER NOT NULL,
        milestone_digest INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE plan_task_milestones (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, seq),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
    CREATE TABLE plan_task_attempts (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        attempt_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        document_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, position),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
"#;
