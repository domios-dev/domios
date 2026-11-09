use super::*;

#[derive(Clone)]
pub(in crate::service_plan) enum TaskHistory {
    Core,
    Escalation,
    Selected(Vec<String>),
    Respawn(String),
    Attempt(String),
    Binding { attempt: String, tab: String },
    LiveAndTarget(String),
    LiveAndReported,
    Attempts,
    Review,
}

pub(in crate::service_plan) struct TaskRowEdit {
    pub task: Task,
    pub mode: crate::plan::PlanMode,
    pub canonical_checkout: String,
    pub owner: crate::plan::OrchestratorRef,
    pub strides: Vec<Milestone>,
    pub progression: Option<(crate::plan::TaskStatus, bool)>,
    pub warn_dispatch: bool,
    pub rollback_unblock: Option<String>,
    pub legacy_dispatch_rollback: bool,
    pub owner_tab: Option<String>,
    pub tab_owners: Vec<crate::service_plan::task_authority_projection::TabAttemptOwnerRead>,
    pub superseded: bool,
    pub plan_status: String,
    pub team_id: Option<String>,
    pub review_credit: i64,
    pub terminal_check: bool,
    pub close_all_live: bool,
    pub created_tasks: Vec<Task>,
    pub removed_attempts: Vec<String>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn read_task_core(
        &self,
        plan_id: &str,
        selector: &str,
    ) -> Result<Task, PlanError> {
        self.read_task_projection(plan_id, selector, TaskHistory::Core)
    }

    fn read_task_projection(
        &self,
        plan_id: &str,
        selector: &str,
        history: TaskHistory,
    ) -> Result<Task, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        if !transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                [plan_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(db_error)?
        {
            return Err(PlanError::NotFound);
        }
        let mut receipt = CommittedRows::default();
        let mut task = read_task(&transaction, plan_id, selector, &mut receipt)?;
        task.execution_attempts =
            read_attempts(&transaction, plan_id, &task.id, &history, &mut receipt)?
                .into_iter()
                .map(|(_, attempt)| attempt)
                .collect();
        transaction.commit().map_err(db_error)?;
        Ok(task)
    }

    pub(in crate::service_plan) fn with_task_rows<T>(
        &self,
        plan_id: &str,
        selector: &str,
        history: TaskHistory,
        now_ms: u64,
        change: impl FnOnce(&mut TaskRowEdit) -> Result<T, PlanError>,
    ) -> Result<(T, CommittedRows), PlanError> {
        let mut result = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            result = Some(edit_task_rows(
                tx, plan_id, selector, history, now_ms, receipt, change,
            )?);
            Ok(())
        })?;
        Ok((
            result.ok_or_else(|| PlanError::Io("task edit returned no committed result".into()))?,
            receipt,
        ))
    }
    pub(in crate::service_plan) fn with_task_rows_batch<T>(
        &self,
        plan_id: &str,
        selectors: &[String],
        history: TaskHistory,
        now_ms: u64,
        mut change: impl FnMut(usize, &mut TaskRowEdit) -> Result<Option<T>, PlanError>,
    ) -> Result<(Vec<T>, CommittedRows), PlanError> {
        let mut results = Vec::new();
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            for (index, selector) in selectors.iter().enumerate() {
                let exists: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND task_id=?2)",
                        params![plan_id, selector],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                if !exists {
                    continue;
                }
                if let Some(result) = edit_task_rows(
                    tx,
                    plan_id,
                    selector,
                    history.clone(),
                    now_ms,
                    receipt,
                    |edit| change(index, edit),
                )? {
                    results.push(result);
                }
            }
            Ok(())
        })?;
        Ok((results, receipt))
    }

    pub(in crate::service_plan) fn claim_next_qa_rows<T>(
        &self,
        plan_id: &str,
        max_running: usize,
        now_ms: u64,
        change: impl FnOnce(&mut TaskRowEdit) -> Result<T, PlanError>,
    ) -> Result<(Option<T>, CommittedRows), PlanError> {
        let mut result = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let running: usize = tx.query_row("SELECT count(*) FROM plan_tasks WHERE plan_id=?1 AND status='in_review' AND json_extract(row_json,'$.review_stage')='qa' AND json_extract(row_json,'$.qa_state')='running'", [plan_id], |row| row.get(0)).map_err(db_error)?;
            if running >= max_running { return Ok(()); }
            let selector: Option<String> = tx.query_row("SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND status='in_review' AND json_extract(row_json,'$.review_stage')='qa' AND json_extract(row_json,'$.qa_state')='queued' ORDER BY position LIMIT 1", [plan_id], |row| row.get(0)).optional().map_err(db_error)?;
            if let Some(selector) = selector {
                result = Some(edit_task_rows(tx, plan_id, &selector, TaskHistory::Attempts, now_ms, receipt, change)?);
            }
            Ok(())
        })?;
        Ok((result, receipt))
    }
}

pub(super) fn edit_task_rows<T>(
    tx: &Transaction<'_>,
    plan_id: &str,
    selector: &str,
    history: TaskHistory,
    now_ms: u64,
    receipt: &mut CommittedRows,
    change: impl FnOnce(&mut TaskRowEdit) -> Result<T, PlanError>,
) -> Result<T, PlanError> {
    let mut task = read_task(tx, plan_id, selector, receipt)?;
    let previous_name = task.name.clone();
    let previous_status = task.status.clone();
    let original_core = task.clone();
    let attempts = read_attempts(tx, plan_id, &task.id, &history, receipt)?;
    task.execution_attempts = attempts
        .iter()
        .map(|(_, attempt)| attempt.clone())
        .collect();
    if matches!(history, TaskHistory::Review | TaskHistory::Escalation) {
        let sql = if matches!(history, TaskHistory::Escalation) {
            "SELECT row_json FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 AND json_extract(row_json,'$.kind')='review_escalation' AND json_extract(row_json,'$.ts')>=(SELECT json_extract(row_json,'$.status_changed_at') FROM plan_tasks WHERE plan_id=?1 AND task_id=?2) ORDER BY seq DESC LIMIT 1"
        } else {
            "SELECT row_json FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 ORDER BY seq"
        };
        let mut statement = tx.prepare_cached(sql).map_err(db_error)?;
        task.milestones = statement
            .query_map(params![plan_id, task.id], |row| row.get::<_, String>(0))
            .map_err(db_error)?
            .map(|row| {
                let json = row.map_err(db_error)?;
                receipt.decoded_bytes += json.len();
                serde_json::from_str(&json).map_err(json_error)
            })
            .collect::<Result<_, _>>()?;
    }
    let milestone_count = task.milestones.len();
    let (owner_tab, plan_status, team_id): (Option<String>, String, Option<String>) = tx
        .query_row(
            "SELECT owner_tab_id, status, team_id FROM plans WHERE id=?1",
            [plan_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(db_error)?;
    receipt.projected_text_bytes += owner_tab.as_ref().map_or(0, String::len)
        + plan_status.len()
        + team_id.as_ref().map_or(0, String::len);
    let (tab_owners, superseded) = if let TaskHistory::Binding { attempt, tab } = &history {
        let owners = super::super::task_authority_queries::tab_attempt_owners(tx, tab)?;
        for owner in &owners {
            receipt.projected_text_bytes += owner.plan_id.len()
                + owner.task_id.len()
                + owner.attempt_id.as_ref().map_or(0, String::len)
                + owner.assigned_tab.as_ref().map_or(0, String::len)
                + owner.competition_group.as_ref().map_or(0, String::len)
                + owner.session_id.as_ref().map_or(0, String::len)
                + owner.nonce.as_ref().map_or(0, String::len);
        }
        let superseded = tx.query_row("WITH current_attempts AS MATERIALIZED (SELECT plan_id,task_id,position,coalesce(json_extract(row_json,'$.interruption.owner_epoch'),0) AS owner_epoch FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND attempt_id=?3) SELECT EXISTS(SELECT 1 FROM plan_task_attempts prior JOIN current_attempts current ON current.plan_id=prior.plan_id AND current.task_id=prior.task_id WHERE prior.position<current.position AND (prior.executor_tab_id=?4 OR coalesce(json_extract(prior.row_json,'$.interruption.owner_epoch'),0)<current.owner_epoch))", params![plan_id, task.id, attempt, tab], |row| row.get(0)).map_err(db_error)?;
        (owners, superseded)
    } else {
        (Vec::new(), false)
    };
    let (mode_json, owner_json, canonical_checkout): (String, String, String) = tx.query_row("SELECT json_quote(coalesce(json_extract(header_json,'$.mode'),'planning')), coalesce(json_extract(header_json,'$.owner'),'{}'), CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''))='' THEN project_path ELSE json_extract(header_json,'$.canonical_project_path') END FROM plans WHERE id=?1", [plan_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(db_error)?;
    receipt.decoded_bytes += mode_json.len() + owner_json.len();
    receipt.projected_text_bytes += canonical_checkout.len();
    let mode: crate::plan::PlanMode = serde_json::from_str(&mode_json).map_err(json_error)?;
    let original_mode = mode.clone();
    let owner = serde_json::from_str(&owner_json).map_err(json_error)?;
    let mut edit = TaskRowEdit {
        task,
        mode,
        canonical_checkout,
        owner,
        strides: Vec::new(),
        progression: None,
        warn_dispatch: false,
        rollback_unblock: None,
        legacy_dispatch_rollback: false,
        owner_tab,
        tab_owners,
        superseded,
        plan_status,
        team_id,
        review_credit: 0,
        terminal_check: false,
        close_all_live: false,
        created_tasks: Vec::new(),
        removed_attempts: Vec::new(),
    };
    let result = change(&mut edit)?;
    super::dispatch_rows::finish(
        tx,
        plan_id,
        &original_core,
        &original_mode,
        &mut edit,
        receipt,
    )?;
    if edit.terminal_check {
        task_fields::reject_open_subtasks(tx, plan_id, &edit.task.id)?;
    }
    if edit.task.execution_attempts.len() < attempts.len()
        || edit.task.milestones.len() < milestone_count
    {
        return Err(PlanError::Io(
            "task row edit changed an immutable history prefix".into(),
        ));
    }
    for id in &edit.removed_attempts {
        if attempts
            .iter()
            .filter(|(_, attempt)| &attempt.id == id)
            .count()
            != 1
        {
            return Err(PlanError::Io(
                "attempt removal conflict: exact row identity is ambiguous".into(),
            ));
        }
    }
    for ((position, previous), next) in attempts.iter().zip(&edit.task.execution_attempts) {
        if previous.id != next.id {
            return Err(PlanError::Io(
                "task row edit changed an immutable attempt identity".into(),
            ));
        }
        if edit.removed_attempts.contains(&previous.id) {
            tx.execute(
                "DELETE FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND position=?3",
                params![plan_id, edit.task.id, position],
            )
            .map_err(db_error)?;
            receipt.attempts += 1;
            receipt.tasks.insert(edit.task.id.clone());
        } else if previous != next {
            let json = encode(next, receipt)?;
            tx.execute("UPDATE plan_task_attempts SET row_json=?4, content_hash=?5 WHERE plan_id=?1 AND task_id=?2 AND position=?3", params![plan_id, edit.task.id, position, json, store_rows::row_hash(&json)]).map_err(db_error)?;
            receipt.attempts += 1;
            receipt.tasks.insert(edit.task.id.clone());
        }
    }
    for attempt in &edit.task.execution_attempts[attempts.len()..] {
        let json = encode(attempt, receipt)?;
        tx.execute("INSERT INTO plan_task_attempts(plan_id, task_id, attempt_id, position, content_hash, row_json) VALUES (?1, ?2, ?3, (SELECT coalesce(max(position)+1,0) FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2), ?4, ?5)", params![plan_id, edit.task.id, attempt.id, store_rows::row_hash(&json), json]).map_err(db_error)?;
        receipt.attempts += 1;
        receipt.tasks.insert(edit.task.id.clone());
    }
    let appended = edit.task.milestones.split_off(milestone_count);
    edit.task.milestones.clear();
    edit.task.execution_attempts.clear();
    if edit.close_all_live {
        close_all_live(tx, plan_id, &edit.task.id, now_ms, receipt)?;
    }
    if edit.review_credit != 0 {
        task_fields::change_review_credit(tx, plan_id, edit.review_credit, receipt)?;
    }
    let core_changed = edit.task != original_core;
    if core_changed {
        write_task(tx, plan_id, &edit.task, receipt)?;
    }
    for milestone in appended {
        append_milestone(tx, plan_id, &edit.task.id, &milestone, receipt)?;
    }
    if core_changed {
        task_fields::queue_task_index(
            tx,
            plan_id,
            &edit.task,
            previous_name != edit.task.name,
            previous_status != edit.task.status,
            receipt,
        )?;
    }
    let created = !edit.created_tasks.is_empty();
    for task in &edit.created_tasks {
        task_fields::insert_task(tx, plan_id, task, receipt)?;
    }
    if created {
        task_fields::normalize_handles(tx, plan_id, receipt)?;
    }
    for task in &edit.created_tasks {
        task_fields::queue_task_index(tx, plan_id, task, false, false, receipt)?;
    }
    Ok(result)
}

pub(super) fn read_attempts(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    history: &TaskHistory,
    receipt: &mut CommittedRows,
) -> Result<Vec<(i64, TaskExecutionAttempt)>, PlanError> {
    if matches!(history, TaskHistory::Core | TaskHistory::Escalation) {
        return Ok(Vec::new());
    }
    let selected_json = match history {
        TaskHistory::Selected(ids) => Some(serde_json::to_string(ids).map_err(json_error)?),
        _ => None,
    };
    let (sql, target) = match history {
        TaskHistory::Selected(_) => ("SELECT position, row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND (attempt_id IN (SELECT value FROM json_each(?3)) OR json_extract(row_json,'$.closed_at') IS NULL) ORDER BY position", selected_json.as_deref()),
        TaskHistory::Respawn(id) => ("SELECT position, row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND (attempt_id=?3 OR json_extract(row_json,'$.closed_at') IS NULL OR position=(SELECT max(position) FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2)) ORDER BY position", Some(id.as_str())),
        TaskHistory::Attempt(id) => ("SELECT position, row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND attempt_id=?3 ORDER BY position", Some(id.as_str())),
        TaskHistory::LiveAndTarget(id) | TaskHistory::Binding { attempt: id, .. } => ("SELECT position, row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND (attempt_id=?3 OR json_extract(row_json,'$.closed_at') IS NULL) ORDER BY position", Some(id.as_str())),
        TaskHistory::Review | TaskHistory::Attempts => ("SELECT position, row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND (?3 IS NULL) ORDER BY position", None),
        _ => ("SELECT position, row_json FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND (?3 IS NULL) AND (json_extract(row_json,'$.closed_at') IS NULL OR position=(SELECT max(position) FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2 AND json_extract(row_json,'$.mesh_peer') IS NOT NULL AND json_extract(row_json,'$.remote_dispatch_state')='reported')) ORDER BY position", None),
    };
    let mut statement = tx.prepare_cached(sql).map_err(db_error)?;
    let attempts = statement
        .query_map(params![plan_id, task_id, target], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?
        .map(|row| {
            let (position, json) = row.map_err(db_error)?;
            receipt.decoded_bytes += json.len();
            Ok((position, serde_json::from_str(&json).map_err(json_error)?))
        })
        .collect();
    attempts
}

pub(super) fn close_all_live(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let attempts = read_attempts(tx, plan_id, task_id, &TaskHistory::LiveAndReported, receipt)?;
    for (position, mut attempt) in attempts {
        if attempt.closed_at.is_some() {
            continue;
        }
        attempt.closed_at = Some(now_ms);
        let json = encode(&attempt, receipt)?;
        tx.execute("UPDATE plan_task_attempts SET row_json=?4, content_hash=?5 WHERE plan_id=?1 AND task_id=?2 AND position=?3", params![plan_id, task_id, position, json, store_rows::row_hash(&json)]).map_err(db_error)?;
        receipt.attempts += 1;
    }
    Ok(())
}
