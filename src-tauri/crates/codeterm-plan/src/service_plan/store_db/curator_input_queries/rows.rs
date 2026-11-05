use crate::service_plan::store_db::document_queries::regression::unsigned_timestamp;

pub(super) fn prefix() -> String {
    let facts = crate::service_plan::store_db::curator_notice_queries::reminder_task_facts()
        .replace("?2", "?7");
    let changed = unsigned_timestamp("t.row_json", "status_changed_at", true);
    let created = unsigned_timestamp("t.row_json", "created_at", true);
    let asked = unsigned_timestamp("g.value", "asked_at", false);
    let answer = unsigned_timestamp("g.value", "answer.at", false);
    let shown_at = unsigned_timestamp("s.row_json", "ts", false);
    let agent_since = unsigned_timestamp("a.value", "since", false);
    format!("{facts}, runtime_scope AS MATERIALIZED (
        SELECT owner_tab_id,json_type(header_json,'$.control_loop.last_round')='object' AS has_last_round FROM plans WHERE id=?1
     ), task_meta AS MATERIALIZED (
        SELECT t.task_id,t.position,t.status,{created} AS created,{changed} AS changed,
            CASE WHEN domios_curator_after({changed},?2) THEN json_extract(t.row_json,'$.name') ELSE '' END AS name,
            CASE WHEN json_type(t.row_json,'$.depends_on') IS NULL THEN '[]'
                WHEN json_type(t.row_json,'$.depends_on')='array' THEN t.row_json->'$.depends_on'
                ELSE json('invalid curator dependencies') END AS dependencies
        FROM plan_tasks t WHERE t.plan_id=?1
     ), prior_states AS MATERIALIZED (
        SELECT s.key AS task_id,s.value AS state FROM plans p,json_each(p.header_json,'$.control_loop.task_states') s WHERE p.id=?1
     ), gate_facts AS MATERIALIZED (
        SELECT json_extract(g.value,'$.id') AS id,json_extract(g.value,'$.status') AS status,{asked} AS asked,
            CASE WHEN json_type(g.value,'$.answer') IS NULL OR json_type(g.value,'$.answer')='null' THEN NULL
                WHEN json_type(g.value,'$.answer')='object' THEN {answer} ELSE json('invalid curator gate answer') END AS answer,
            CASE WHEN json_type(g.value,'$.blocking_task_ids') IS NULL THEN '[]'
                WHEN json_type(g.value,'$.blocking_task_ids')='array' THEN g.value->'$.blocking_task_ids'
                ELSE json('invalid blocking task ids') END AS blocking
        FROM plans p,json_each(p.header_json,'$.gates') g WHERE p.id=?1
     ), task_waits AS MATERIALIZED (
        SELECT t.task_id,d.value AS wait FROM task_meta t,json_each(t.dependencies) d
            JOIN dependency_targets target ON target.task_id=d.value WHERE target.status NOT IN ('done','cancelled')
        UNION ALL SELECT b.value,g.id FROM gate_facts g,json_each(g.blocking) b WHERE domios_curator_gate_open(g.status)
     ), task_documents AS MATERIALIZED (
        SELECT domios_curator_task_rows(t.task_id,t.name,t.status,t.created,t.changed,
            (SELECT json_group_array(wait) FROM task_waits WHERE task_id=t.task_id),
            (SELECT owner_tab_id FROM runtime_scope),prior.state,json_extract(f.decision,'$.orchestrator_actionable'),?2,?6) AS document
        FROM task_meta t JOIN facts f USING(task_id) LEFT JOIN prior_states prior USING(task_id)
     ), gate_documents AS MATERIALIZED (
        SELECT domios_curator_gate_rows(id,status,asked,answer,?2,?6) AS document FROM gate_facts
     ), observations AS MATERIALIZED (
        SELECT json_extract(n.value,'$.id') AS id,n.value->'$.timestamp' AS at,
            json_extract(n.value,'$.action_needed') AS action_needed,json_extract(n.value,'$.answered') AS answered,
            json_extract(n.value,'$.resolved') AS resolved,json_extract(n.value,'$.title') AS title,
            json_extract(n.value,'$.body') AS body FROM json_each(?5) n
     ), observed_answers AS MATERIALIZED (
        SELECT a.value AS id FROM plans p,json_each(p.header_json,'$.control_loop.observed_owner_answers') a WHERE p.id=?1
     ), shown_facts AS MATERIALIZED (
        SELECT printf('%20s',{shown_at}) AS at,s.row_json->'$.links' AS links FROM plan_strides s
        WHERE s.plan_id=?1 AND json_extract(s.row_json,'$.kind') IN ('owner_notice','shown') AND ?5 IS NOT NULL
     ), shown_tasks AS MATERIALIZED (
        SELECT l.value AS task_id,max(s.at) AS at FROM shown_facts s,json_each(s.links) l GROUP BY l.value
     ), input_rows(section,id,kind,state,summary) AS (
        SELECT section.key,json_extract(r.value,'$.id'),json_extract(r.value,'$.kind'),json_extract(r.value,'$.state'),json_extract(r.value,'$.summary')
        FROM task_documents d,json_each(d.document) section,json_each(section.value) r
        UNION ALL SELECT section.key,json_extract(r.value,'$.id'),json_extract(r.value,'$.kind'),json_extract(r.value,'$.state'),json_extract(r.value,'$.summary')
        FROM gate_documents d,json_each(d.document) section,json_each(section.value) r
        UNION ALL SELECT 'review backlog',json_extract(r,'$.id'),json_extract(r,'$.kind'),json_extract(r,'$.state'),json_extract(r,'$.summary') FROM (
            SELECT domios_curator_review_row(?1,count(*),coalesce(max(domios_curator_age(created,changed,?6)),'0')) AS r FROM task_meta WHERE status='in_review')
        UNION ALL SELECT 'agent capacity',json_extract(r,'$.id'),json_extract(r,'$.kind'),json_extract(r,'$.state'),json_extract(r,'$.summary') FROM (
            SELECT domios_curator_agent_row(a.key,json_extract(a.value,'$.state'),{agent_since},?6) AS r FROM plans p,json_each(p.header_json,'$.control_loop.agent_states') a WHERE p.id=?1 AND ?5 IS NOT NULL)
        UNION ALL SELECT 'owner questions',id,'question','open',domios_curator_elapsed(at,?6) FROM observations WHERE action_needed AND NOT answered AND NOT resolved
        UNION ALL SELECT 'decisions',id,'owner_answer','newly_observed',?6 FROM observations
            WHERE action_needed AND answered AND id NOT IN (SELECT id FROM observed_answers)
                AND ((SELECT has_last_round FROM runtime_scope) OR domios_curator_after(at,?2))
        UNION ALL SELECT 'done but not shown',t.task_id,'accepted','notice_unconfirmed',t.changed FROM task_meta t
            WHERE ?5 IS NOT NULL AND t.status='done' AND domios_curator_after(t.changed,?2)
                AND NOT EXISTS(SELECT 1 FROM shown_tasks s WHERE s.task_id=t.task_id AND s.at>=printf('%20s',t.changed))
                AND NOT EXISTS(SELECT 1 FROM observations n WHERE printf('%20s',n.at)>=printf('%20s',t.changed)
                    AND (instr(n.title,t.task_id)>0 OR instr(coalesce(n.body,''),t.task_id)>0))
     ) ")
}
