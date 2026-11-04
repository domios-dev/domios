use super::*;

pub(super) fn edit(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    milestone_id: &str,
    kind: Option<&str>,
    text: Option<&str>,
    remove: bool,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    edit_selected(
        tx,
        plan_id,
        task_id,
        milestone_id,
        remove,
        receipt,
        |milestone, receipt| {
            if let Some(kind) = kind {
                declare_kind(tx, plan_id, kind, false, receipt)?;
                milestone.kind = kind.into();
            }
            if let Some(text) = text {
                milestone.text = text.into();
            }
            Ok(())
        },
    )
}

pub(super) fn edit_selected(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    milestone_id: &str,
    remove: bool,
    receipt: &mut CommittedRows,
    change: impl FnOnce(&mut Milestone, &mut CommittedRows) -> Result<(), PlanError>,
) -> Result<(), PlanError> {
    if !tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND task_id=?2)",
            params![plan_id, task_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(db_error)?
    {
        return Err(PlanError::TaskNotFound);
    }
    let (seq, json): (i64, String) = tx.query_row("SELECT seq, row_json FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 AND json_extract(row_json,'$.id')=?3 ORDER BY seq LIMIT 1", params![plan_id, task_id, milestone_id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
    receipt.decoded_bytes += json.len();
    let mut milestone: Milestone = serde_json::from_str(&json).map_err(json_error)?;
    if remove {
        tx.execute(
            "DELETE FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 AND seq=?3",
            params![plan_id, task_id, seq],
        )
        .map_err(db_error)?;
    } else {
        change(&mut milestone, receipt)?;
        let json = encode(&milestone, receipt)?;
        tx.execute("UPDATE plan_task_milestones SET row_json=?4 WHERE plan_id=?1 AND task_id=?2 AND seq=?3", params![plan_id, task_id, seq, json]).map_err(db_error)?;
    }
    let mut statement = tx.prepare_cached("SELECT row_json FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 ORDER BY seq").map_err(db_error)?;
    let mut digest = 0;
    let mut count = 0;
    for row in statement
        .query_map(params![plan_id, task_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?
    {
        let json = row.map_err(db_error)?;
        receipt.hashed_bytes += json.len();
        digest = store_rows::chain_hash(digest, &json);
        count += 1;
    }
    tx.execute("UPDATE plan_tasks SET milestone_count=?3, milestone_digest=?4 WHERE plan_id=?1 AND task_id=?2", params![plan_id, task_id, count, digest]).map_err(db_error)?;
    receipt.tasks.insert(task_id.into());
    receipt.task_rows += 1;
    receipt.milestones += 1;
    queue_milestone_index(tx, plan_id, task_id, &milestone, remove, receipt)
}

impl PlanDatabase {
    pub(in crate::service_plan) fn with_milestone_rows<T>(
        &self,
        plan_id: &str,
        task_id: &str,
        milestone_id: &str,
        now_ms: u64,
        change: impl FnOnce(&mut Milestone) -> Result<T, PlanError>,
    ) -> Result<(T, CommittedRows), PlanError> {
        let mut result = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let mut task = read_task(tx, plan_id, task_id, receipt)?;
            edit_selected(
                tx,
                plan_id,
                &task.id,
                milestone_id,
                false,
                receipt,
                |milestone, _| {
                    result = Some(change(milestone)?);
                    Ok(())
                },
            )?;
            task.updated_at = now_ms;
            write_task(tx, plan_id, &task, receipt)?;
            task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)
        })?;
        Ok((
            result.ok_or_else(|| PlanError::Io("milestone edit returned no result".into()))?,
            receipt,
        ))
    }
}
