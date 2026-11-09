pub(super) const TASK_EVIDENCE: &str = "WITH needed_dependencies AS MATERIALIZED (
            SELECT DISTINCT dep.value AS task_id FROM plan_tasks selected,json_each(coalesce(selected.row_json->'$.depends_on','[]')) dep
            WHERE selected.plan_id=?1 AND (?3 IS NULL OR selected.task_id=?3)
        ), hash_targets AS MATERIALIZED (
            SELECT task_id,status,CASE WHEN status NOT IN ('done','cancelled') THEN json_extract(row_json,'$.name') END AS name FROM plan_tasks
            WHERE plan_id=?1 AND task_id IN(SELECT task_id FROM needed_dependencies)
        ) SELECT t.task_id,
            domios_reminder_hash('task',json_object('status',t.status,'status_label',json_extract(t.row_json,'$.status_label'),
                'pause_reason',json_extract(t.row_json,'$.pause_reason'),'interruption',json(coalesce(t.row_json->'$.interruption',domios_reminder_default('task'))),
                'review_stage',coalesce(json_extract(t.row_json,'$.review_stage'),'review'),'qa_state',json_extract(t.row_json,'$.qa_state'),
                'assigned_tab_id',t.assigned_tab_id,
                'dependency_waits',json((SELECT json_group_array(json_object('task_id',dep.value,'name',coalesce(target.name,'missing dependency '||dep.value),
                    'status',target.status,'missing',json(CASE WHEN target.task_id IS NULL THEN 'true' ELSE 'false' END)))
                    FROM json_each(coalesce(t.row_json->'$.depends_on','[]')) dep LEFT JOIN hash_targets target ON target.task_id=dep.value
                    WHERE target.task_id IS NULL OR target.status NOT IN ('done','cancelled'))))),
            domios_reminder_hash('review',coalesce(t.row_json->'$.review_evidence','[]')),
            json_object('status_changed_at',json(coalesce(t.row_json->'$.status_changed_at','0')),
                'handoff_required',json(coalesce(t.row_json->'$.interruption.handoff_required','false')),
                'blocked_fingerprint',CASE WHEN t.status='blocked' THEN domios_reminder_block('task_blocked',coalesce(
                    nullif(trim(json_extract(t.row_json,'$.pause_reason'),?2),''),nullif(trim(json_extract(t.row_json,'$.status_label'),?2),''),t.task_id)) END,
                'last_review',CASE WHEN json_array_length(t.row_json,'$.review_evidence')>0 THEN json_object('id',json_extract(t.row_json,'$.review_evidence[#-1].id'),'verdict',json_extract(t.row_json,'$.review_evidence[#-1].verdict')) END,
                'lane_path',json_extract(t.row_json,'$.lane.worktree_path'),'lane_branch',json_extract(t.row_json,'$.lane.branch'))
         FROM plan_tasks t WHERE t.plan_id=?1 AND (?3 IS NULL OR t.task_id=?3) ORDER BY t.position";

pub(super) const ATTEMPT_EVIDENCE: &str = "SELECT a.task_id,json_object('id',a.attempt_id,'executor_tab_id',a.executor_tab_id,'session_id',a.session_id,
            'session_binding_nonce',a.session_binding_nonce,'worktree_path',json_extract(a.row_json,'$.worktree_path'),'branch',json_extract(a.row_json,'$.branch'),
            'closed',json(CASE WHEN json_extract(a.row_json,'$.closed_at') IS NULL THEN 'false' ELSE 'true' END),
            'acknowledged',json(CASE WHEN json_extract(a.row_json,'$.acknowledged_at') IS NULL THEN 'false' ELSE 'true' END),
            'report_id',json_extract(a.row_json,'$.report.id'),
            'interruption_fingerprint',CASE WHEN json_type(a.row_json,'$.interruption.reason') IS NOT NULL AND json_type(a.row_json,'$.interruption.reason')<>'null'
                THEN domios_reminder_block('machine_hold',json_extract(a.row_json,'$.interruption.reason')) END),
            domios_reminder_hash('attempt',coalesce(a.row_json->'$.interruption',domios_reminder_default('attempt'))),
            CASE WHEN json_type(a.row_json,'$.report') IS NOT NULL AND json_type(a.row_json,'$.report')<>'null' THEN domios_reminder_hash('report',a.row_json->'$.report') END
         FROM plan_task_attempts a JOIN plan_tasks t ON t.plan_id=a.plan_id AND t.task_id=a.task_id WHERE a.plan_id=?1 AND (?2 IS NULL OR a.task_id=?2) ORDER BY t.position,a.position";

pub(super) const TASK_MILESTONE_HASHES: &str = "SELECT m.task_id,json_extract(m.row_json,'$.id'),domios_reminder_hash('stride',m.row_json) FROM plan_task_milestones m JOIN plan_tasks t ON t.plan_id=m.plan_id AND t.task_id=m.task_id WHERE m.plan_id=?1 AND (?2 IS NULL OR m.task_id=?2) ORDER BY t.position,m.seq";

pub(super) const PLAN_STRIDE_HASHES: &str = "SELECT json_extract(row_json,'$.id'),domios_reminder_hash('stride',row_json) FROM plan_strides WHERE plan_id=?1 ORDER BY seq";

pub(super) const GATE_HASHES: &str = "SELECT json_object('id',json_extract(g.value,'$.id'),'status',json_extract(g.value,'$.status'),'answered',json(CASE WHEN json_type(g.value,'$.answer') IS NULL OR json_type(g.value,'$.answer')='null' THEN 'false' ELSE 'true' END)),domios_reminder_hash('gate',g.value) FROM plans p,json_each(coalesce(p.header_json->'$.gates','[]')) g WHERE p.id=?1 ORDER BY g.key";

pub(super) const LEARNING_CATALOG: &str = "SELECT json_extract(l.value,'$.id'),domios_reminder_scrubbed(json_extract(l.value,'$.text')) FROM plan_heartbeat h,json_each(coalesce(h.row_json->'$.learnings','[]')) l WHERE h.plan_id=?1 AND coalesce(json_extract(l.value,'$.tag'),'')<>'owner' AND length(trim(json_extract(l.value,'$.text'),?2))>0 ORDER BY l.key DESC LIMIT 12";

pub(super) const DELTA_CONTEXT: &str = "WITH rows(key,phase,ordinal,subordinal,body) AS (
            SELECT 'mode',0,0,0,json_object('mode',coalesce(json_extract(header_json,'$.mode'),'planning'),'status',status) FROM plans WHERE id=?1
            UNION ALL SELECT 'task:'||t.task_id,1,t.position,0,json_object('task',t.task_id,'status',t.status,
                'status_label',domios_reminder_scrubbed(json_extract(t.row_json,'$.status_label')),'pause_reason',domios_reminder_scrubbed(json_extract(t.row_json,'$.pause_reason')),
                'assigned_tab_id',t.assigned_tab_id,'presentation',json_extract(j.value,'$.projection.presentation'),
                'dependency_waits',json((SELECT json_group_array(json_object('task_id',json_extract(w.value,'$.task_id'),'status',json_extract(w.value,'$.status'),'missing',json(w.value->'$.missing'))) FROM json_each(j.value->'$.projection.dependency_waits') w)),
                'held',json(j.value->'$.held'))
            FROM plan_tasks t JOIN json_each(?3) j ON json_extract(j.value,'$.id')=t.task_id WHERE t.plan_id=?1
            UNION ALL SELECT 'report:'||json_extract(a.row_json,'$.report.id'),2,t.position,a.position,
                json_object('task',a.task_id,'status',json_extract(a.row_json,'$.report.status'),'sha',json_extract(a.row_json,'$.report.sha'),'test_result',domios_reminder_scrubbed(json_extract(a.row_json,'$.report.test_result')))
            FROM plan_task_attempts a JOIN plan_tasks t ON t.plan_id=a.plan_id AND t.task_id=a.task_id WHERE a.plan_id=?1 AND json_type(a.row_json,'$.report') IS NOT NULL AND json_type(a.row_json,'$.report')<>'null'
            UNION ALL SELECT 'stride:'||json_extract(row_json,'$.id'),3,seq,0,json_object('kind',json_extract(row_json,'$.kind'),'text',domios_reminder_scrubbed(json_extract(row_json,'$.text'))) FROM plan_strides WHERE plan_id=?1
            UNION ALL SELECT 'stride:'||json_extract(m.row_json,'$.id'),4,t.position,m.seq,json_object('kind',json_extract(m.row_json,'$.kind'),'text',domios_reminder_scrubbed(json_extract(m.row_json,'$.text'))) FROM plan_task_milestones m JOIN plan_tasks t ON t.plan_id=m.plan_id AND t.task_id=m.task_id WHERE m.plan_id=?1
            UNION ALL SELECT 'gate:'||json_extract(g.value,'$.id'),5,CAST(g.key AS INTEGER),0,json_object('status',json_extract(g.value,'$.status'),'answer',domios_reminder_scrubbed(json_extract(g.value,'$.answer.text'))) FROM plans p,json_each(coalesce(p.header_json->'$.gates','[]')) g WHERE p.id=?1
        ), selected AS (
            SELECT rows.*,row_number() OVER(PARTITION BY rows.key ORDER BY phase DESC,ordinal DESC,subordinal DESC) AS rank FROM rows JOIN json_each(?2) d ON rows.key=d.key
        ) SELECT key,body FROM selected WHERE rank=1 ORDER BY key LIMIT ?4";
