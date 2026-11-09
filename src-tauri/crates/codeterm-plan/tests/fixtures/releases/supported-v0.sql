
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

PRAGMA user_version = 0;
INSERT INTO plans(id,title,status,team_id,project_path,updated_at,document_json) VALUES ('pl_release', 'Published plan', 'active', 'release-team', '/synthetic/repo', 10, '{"id":"pl_release","title":"Published plan","status":"active","owner":{"tab_id":"release-orch"},"team_id":"release-team","project_path":"/synthetic/repo","created_at":2,"updated_at":10,"sections":[{"id":"sec_release","title":"Retained section","body":"published body"}],"elements":[],"strides":[{"id":"stride_release","from":{"tab_id":"release-orch"},"kind":"note","text":"retained evidence","ts":9,"links":[]}],"heartbeat":{"current_focus":"retained focus"}}');
INSERT INTO plan_tasks(plan_id,task_id,position,updated_at,content_hash,milestone_count,milestone_digest,document_json) VALUES ('pl_release', 'task_release', 0, 7, 0, 1, 0, '{"id":"task_release","name":"Retained task","type":"implement","status":"in_progress","created_at":3,"updated_at":7}');
INSERT INTO plan_task_milestones(plan_id,task_id,seq,document_json) VALUES ('pl_release', 'task_release', 0, '{"id":"m_release","from":{"tab_id":"release-orch"},"kind":"note","text":"retained evidence","ts":9,"links":[]}');
INSERT INTO plan_task_attempts(plan_id,task_id,attempt_id,position,content_hash,document_json) VALUES ('pl_release', 'task_release', 'attempt_release', 0, 0, '{"id":"attempt_release","executor_tab_id":"release-worker","session_id":"release-session","closed_at":8}');
INSERT INTO specs(id,plan_id,project_path,updated_at,document_json) VALUES ('spec_release', 'pl_release', '/synthetic/repo', 10, '{"id":"spec_release","plan_id":"pl_release","title":"Published spec","status":"active","project_path":"/synthetic/repo","created_at":2,"updated_at":10,"sections":[{"id":"spec_section","title":"Requirement","body":"keep records"}]}');
INSERT INTO plan_operations(id,entity_id,kind,state,submitted_at,finished_at,error) VALUES (11, 'pl_release', 'fixture', 'committed', 1, 20, NULL);
INSERT INTO plan_operations(id,entity_id,kind,state,submitted_at,finished_at,error) VALUES (12, 'pl_release', 'fixture', 'failed', 2, 21, 'synthetic failure');
INSERT INTO plan_operations(id,entity_id,kind,state,submitted_at,finished_at,error) VALUES (13, 'pl_release', 'fixture', 'queued', 3, NULL, NULL);
INSERT INTO plan_operations(id,entity_id,kind,state,submitted_at,finished_at,error) VALUES (14, 'pl_release', 'fixture', 'running', 4, NULL, NULL);
