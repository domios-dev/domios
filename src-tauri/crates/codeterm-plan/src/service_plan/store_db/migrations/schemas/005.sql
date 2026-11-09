CREATE TABLE plan_elements (
        plan_id TEXT NOT NULL,
        element_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, element_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
CREATE TABLE plan_heartbeat (
        plan_id TEXT PRIMARY KEY,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
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
CREATE TABLE plan_sections (
        plan_id TEXT NOT NULL,
        section_id TEXT NOT NULL,
        position INTEGER NOT NULL,
        content_hash INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, section_id),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
CREATE TABLE plan_strides (
        plan_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, seq),
        FOREIGN KEY (plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
CREATE TABLE plan_task_attempts (
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
CREATE TABLE plan_task_milestones (
        plan_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        PRIMARY KEY (plan_id, task_id, seq),
        FOREIGN KEY (plan_id, task_id) REFERENCES plan_tasks(plan_id, task_id) ON DELETE CASCADE
    );
CREATE TABLE plan_tasks (
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
CREATE TABLE plans (
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
CREATE TABLE specs (
        id TEXT PRIMARY KEY,
        plan_id TEXT,
        project_path TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        row_json TEXT NOT NULL,
        FOREIGN KEY(plan_id) REFERENCES plans(id) ON DELETE CASCADE
    );
CREATE UNIQUE INDEX one_active_plan_per_team
        ON plans(team_id)
        WHERE team_id IS NOT NULL AND status = 'active';
CREATE INDEX plan_attempts_event_author ON plan_task_attempts(
    plan_id, task_id, executor_tab_id, position DESC, json_extract(row_json,'$.executor_title'))
    WHERE json_extract(row_json,'$.executor_title') IS NOT NULL;
CREATE INDEX plan_attempts_id ON plan_task_attempts(plan_id, task_id, attempt_id);
CREATE INDEX plan_milestones_event_order ON plan_task_milestones(
    plan_id, coalesce(json_extract(row_json,'$.ts'),0) DESC, task_id, seq DESC);
CREATE INDEX plan_milestones_id ON plan_task_milestones(json_extract(row_json, '$.id'));
CREATE INDEX plan_operations_entity_id
        ON plan_operations(entity_id);
CREATE INDEX plan_operations_finished_id
        ON plan_operations(finished_at DESC, id DESC)
        WHERE state IN ('committed', 'failed') AND finished_at IS NOT NULL;
CREATE INDEX plan_operations_state_id
        ON plan_operations(state, id);
CREATE INDEX plan_strides_event_order ON plan_strides(
    plan_id, coalesce(json_extract(row_json,'$.ts'),0) DESC, seq DESC);
CREATE INDEX plan_strides_id ON plan_strides(json_extract(row_json, '$.id'));
CREATE INDEX plan_task_attempts_executor_tab_id
        ON plan_task_attempts(executor_tab_id, open);
CREATE INDEX plan_task_attempts_open
        ON plan_task_attempts(open) WHERE open;
CREATE INDEX plan_task_attempts_session_binding_nonce
        ON plan_task_attempts(session_binding_nonce) WHERE session_binding_nonce IS NOT NULL;
CREATE INDEX plan_task_attempts_session_id
        ON plan_task_attempts(session_id) WHERE session_id IS NOT NULL;
CREATE INDEX plan_tasks_assigned_tab_id
        ON plan_tasks(assigned_tab_id) WHERE assigned_tab_id IS NOT NULL;
CREATE INDEX plan_tasks_board_cursor
        ON plan_tasks(
            plan_id,
            CASE WHEN status IN ('pending', 'paused') THEN 'pending' ELSE status END,
            coalesce(CAST(json_extract(row_json, '$.created_at') AS INTEGER), 0),
            position,
            task_id
        );
CREATE INDEX plan_tasks_bound_tabs ON plan_tasks(plan_id, assigned_tab_id) WHERE assigned_tab_id IS NOT NULL;
CREATE INDEX plan_tasks_event_order ON plan_tasks(
    plan_id, coalesce(json_extract(row_json,'$.created_at'),0) DESC, position DESC,
    task_id, json_extract(row_json,'$.name'));
CREATE INDEX plan_tasks_handle_order ON plan_tasks(plan_id, position, json_extract(row_json, '$.handle'), task_id);
CREATE INDEX plan_tasks_lane_branch
        ON plan_tasks(lane_branch) WHERE lane_branch IS NOT NULL;
CREATE INDEX plan_tasks_lane_tab_id
    ON plan_tasks(json_extract(row_json, '$.lane.tab_id'), plan_id)
    WHERE json_extract(row_json, '$.lane.tab_id') IS NOT NULL;
CREATE INDEX plan_tasks_lane_worktree_path
        ON plan_tasks(lane_worktree_path) WHERE lane_worktree_path IS NOT NULL;
CREATE INDEX plan_tasks_status ON plan_tasks(status);
CREATE INDEX plan_tasks_status_summary
        ON plan_tasks(plan_id, json_extract(row_json, '$.status'), json_extract(row_json, '$.status_label'), task_id);
CREATE INDEX plans_owner_tab_id
        ON plans(owner_tab_id) WHERE owner_tab_id IS NOT NULL;
CREATE INDEX plans_planner_tab_id
    ON plans(json_extract(header_json, '$.planner.tab_id'), id)
    WHERE json_extract(header_json, '$.planner.tab_id') IS NOT NULL;
CREATE INDEX plans_status_updated
        ON plans(status, updated_at DESC);
PRAGMA application_id = 1146113102;
