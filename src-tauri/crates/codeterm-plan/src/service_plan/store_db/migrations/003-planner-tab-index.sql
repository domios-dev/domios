CREATE INDEX plans_planner_tab_id
    ON plans(json_extract(header_json, '$.planner.tab_id'), id)
    WHERE json_extract(header_json, '$.planner.tab_id') IS NOT NULL;
