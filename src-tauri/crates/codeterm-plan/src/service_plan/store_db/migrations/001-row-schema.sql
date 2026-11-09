
    CREATE TABLE IF NOT EXISTS plans (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        team_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        stride_count INTEGER NOT NULL DEFAULT 0,
        stride_digest INTEGER NOT NULL DEFAULT 0,
        header_json TEXT NOT NULL,
        owner_tab_id TEXT GENERATED ALWAYS AS (json_extract(header_json, '$.owner.tab_id')) VIRTUAL
    );
    CREATE UNIQUE INDEX IF NOT EXISTS one_active_plan_per_team
        ON plans(team_id)
        WHERE team_id IS NOT NULL AND status = 'active';
    CREATE INDEX IF NOT EXISTS plans_status_updated
        ON plans(status, updated_at DESC);
    CREATE INDEX IF NOT EXISTS plans_owner_tab_id
        ON plans(owner_tab_id) WHERE owner_tab_id IS NOT NULL;

    CREATE TABLE IF NOT EXISTS specs (
        id TEXT PRIMARY KEY,
        plan_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        FOREIGN KEY(plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS plan_operations (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        state TEXT NOT NULL CHECK(state IN ('queued','running','committed','failed')),
        submitted_at INTEGER NOT NULL,
        finished_at INTEGER,
        error TEXT
    );
    CREATE INDEX IF NOT EXISTS plan_operations_state_id
        ON plan_operations(state, id);

    CREATE VIRTUAL TABLE IF NOT EXISTS plan_fts USING fts5(
        plan_id UNINDEXED,
        title,
        body
    );

    CREATE TABLE IF NOT EXISTS plan_tasks (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        milestone_count INTEGER NOT NULL,
        milestone_digest INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        status TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.status')) VIRTUAL,
        assigned_tab_id TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.assigned_tab_id')) VIRTUAL,
        lane_worktree_path TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.lane.worktree_path')) VIRTUAL,
        lane_branch TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.lane.branch')) VIRTUAL,
        PRIMARY KEY (plan_id, task_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS plan_tasks_status ON plan_tasks(status);
    CREATE INDEX IF NOT EXISTS plan_tasks_board_cursor
        ON plan_tasks(
            plan_id,
            CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END,
            coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0),
            position,
            task_id
        );
    CREATE INDEX IF NOT EXISTS plan_tasks_assigned_tab_id
        ON plan_tasks(assigned_tab_id) WHERE assigned_tab_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_tasks_lane_worktree_path
        ON plan_tasks(lane_worktree_path) WHERE lane_worktree_path IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_tasks_lane_branch
        ON plan_tasks(lane_branch) WHERE lane_branch IS NOT NULL;
    CREATE TABLE IF NOT EXISTS plan_task_milestones (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, seq),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_task_attempts (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        attempt_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        executor_tab_id TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.executor_tab_id')) VIRTUAL,
        session_id TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.session_id')) VIRTUAL,
        session_binding_nonce TEXT GENERATED ALWAYS AS (json_extract(row_json, '$.session_binding_nonce')) VIRTUAL,
        open INTEGER GENERATED ALWAYS AS (
            json_extract(row_json, '$.closed_at') IS NULL
            AND json_extract(row_json, '$.retired_at') IS NULL
        ) VIRTUAL,
        PRIMARY KEY (plan_id, task_id, position),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS plan_task_attempts_executor_tab_id
        ON plan_task_attempts(executor_tab_id, open);
    CREATE INDEX IF NOT EXISTS plan_task_attempts_session_id
        ON plan_task_attempts(session_id) WHERE session_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_task_attempts_session_binding_nonce
        ON plan_task_attempts(session_binding_nonce) WHERE session_binding_nonce IS NOT NULL;
    CREATE INDEX IF NOT EXISTS plan_task_attempts_open
        ON plan_task_attempts(open) WHERE open;

    CREATE TABLE IF NOT EXISTS plan_sections (
        plan_id TEXT NOT NULL,
        section_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, section_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_elements (
        plan_id TEXT NOT NULL,
        element_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, element_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_strides (
        plan_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, seq),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS plan_heartbeat (
        plan_id TEXT PRIMARY KEY,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );

