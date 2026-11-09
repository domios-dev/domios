DROP TABLE IF EXISTS plan_fts;
CREATE INDEX IF NOT EXISTS plan_attempts_id ON plan_task_attempts(plan_id, task_id, attempt_id);
CREATE INDEX IF NOT EXISTS plan_milestones_id ON plan_task_milestones(json_extract(row_json, '$.id'));
CREATE INDEX IF NOT EXISTS plan_operations_entity_id
        ON plan_operations(entity_id);
CREATE INDEX IF NOT EXISTS plan_operations_finished_id
        ON plan_operations(finished_at DESC, id DESC)
        WHERE state IN ('committed', 'failed') AND finished_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS plan_strides_id ON plan_strides(json_extract(row_json, '$.id'));
CREATE INDEX IF NOT EXISTS plan_tasks_bound_tabs ON plan_tasks(plan_id, assigned_tab_id) WHERE assigned_tab_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS plan_tasks_handle_order ON plan_tasks(plan_id, position, json_extract(row_json, '$.handle'), task_id);
CREATE INDEX IF NOT EXISTS plan_tasks_status_summary
        ON plan_tasks(plan_id, json_extract(row_json, '$.status'), json_extract(row_json, '$.status_label'), task_id);
PRAGMA application_id = 1146113102;
