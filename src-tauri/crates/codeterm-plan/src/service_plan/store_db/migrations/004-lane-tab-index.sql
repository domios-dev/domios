CREATE INDEX plan_tasks_lane_tab_id
    ON plan_tasks(json_extract(row_json, '$.lane.tab_id'), plan_id)
    WHERE json_extract(row_json, '$.lane.tab_id') IS NOT NULL;
