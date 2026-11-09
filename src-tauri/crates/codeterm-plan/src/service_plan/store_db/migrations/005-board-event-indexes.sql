CREATE INDEX plan_tasks_event_order ON plan_tasks(
    plan_id, coalesce(json_extract(row_json,'$.created_at'),0) DESC, position DESC,
    task_id, json_extract(row_json,'$.name'));
CREATE INDEX plan_strides_event_order ON plan_strides(
    plan_id, coalesce(json_extract(row_json,'$.ts'),0) DESC, seq DESC);
CREATE INDEX plan_milestones_event_order ON plan_task_milestones(
    plan_id, coalesce(json_extract(row_json,'$.ts'),0) DESC, task_id, seq DESC);
CREATE INDEX plan_attempts_event_author ON plan_task_attempts(
    plan_id, task_id, executor_tab_id, position DESC, json_extract(row_json,'$.executor_title'))
    WHERE json_extract(row_json,'$.executor_title') IS NOT NULL;
