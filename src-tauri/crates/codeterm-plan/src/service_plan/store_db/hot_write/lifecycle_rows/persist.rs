use super::*;
use crate::service_plan::store_db::PlanIndexRow;

pub(super) fn apply(
    tx: &Transaction<'_>,
    patch: &LifecyclePatch,
    rollback: bool,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let (from, to) = if rollback {
        (&patch.after, &patch.before)
    } else {
        (&patch.before, &patch.after)
    };
    let id = &from.id;
    set_changed(tx, id, "$.owner", &from.owner, &to.owner, receipt)?;
    set_changed(tx, id, "$.planner", &from.planner, &to.planner, receipt)?;
    set_changed(tx, id, "$.team_id", &from.team_id, &to.team_id, receipt)?;
    set_changed(tx, id, "$.status", &from.status, &to.status, receipt)?;
    set_changed(tx, id, "$.orphaned", &from.orphaned, &to.orphaned, receipt)?;
    set_changed(
        tx,
        id,
        "$.planner_inbox",
        &from.planner_inbox,
        &to.planner_inbox,
        receipt,
    )?;
    set_changed(
        tx,
        id,
        "$.planner_requests",
        &from.planner_requests,
        &to.planner_requests,
        receipt,
    )?;
    tx.execute(
        "UPDATE plans SET status=?2,team_id=?3 WHERE id=?1",
        params![
            id,
            serde_json::to_value(&to.status)
                .map_err(json_error)?
                .as_str(),
            to.team_id
        ],
    )
    .map_err(db_error)?;
    receipt.header_rows += 1;
    if rollback {
        remove_appended(
            tx,
            id,
            "$.owner_history",
            patch.before.owner_history_len,
            patch.after.owner_history.len(),
            receipt,
        )?;
        remove_appended(
            tx,
            id,
            "$.lifecycle_events",
            patch.before.lifecycle_events_len,
            patch.after.lifecycle_events.len(),
            receipt,
        )?;
        if !patch.after.strides.is_empty() {
            receipt.strides += tx
                .execute(
                    "DELETE FROM plan_strides WHERE plan_id=?1 AND seq>=?2 AND seq<?3",
                    params![
                        id,
                        patch.before.stride_count,
                        patch.before.stride_count + patch.after.strides.len()
                    ],
                )
                .map_err(db_error)?;
            tx.execute("UPDATE plans SET stride_count=?2,stride_digest=?3,header_json=CASE WHEN ?4 THEN json_set(header_json,'$._stride_digest_invalid',json('true')) ELSE json_remove(header_json,'$._stride_digest_invalid') END WHERE id=?1", params![id, patch.before.stride_count, patch.before.stride_digest, patch.before.stride_digest_invalid]).map_err(db_error)?;
            receipt.header_rows += 1;
        }
    } else {
        for transfer in &to.owner_history {
            header_edits::append_field(tx, id, "$.owner_history", transfer, receipt)?;
        }
        for event in &to.lifecycle_events {
            header_edits::append_field(tx, id, "$.lifecycle_events", event, receipt)?;
        }
        for stride in &to.strides {
            append_stride(tx, id, stride, receipt)?;
        }
    }
    for original in &from.tasks {
        let desired = to
            .tasks
            .iter()
            .find(|task| task.id == original.id)
            .ok_or_else(conflict)?;
        if desired == original {
            continue;
        }
        if rollback {
            for milestone in &original.milestones {
                milestones::edit_selected(
                    tx,
                    id,
                    &original.id,
                    &milestone.id,
                    true,
                    receipt,
                    |_, _| Ok(()),
                )?;
            }
        }
        let ids = original
            .execution_attempts
            .iter()
            .map(|attempt| attempt.id.clone())
            .collect();
        review_rows::edit_task_rows(
            tx,
            id,
            &original.id,
            TaskHistory::Selected(ids),
            to.updated_at,
            receipt,
            |edit| {
                let mut expected = original.clone();
                expected.milestones.clear();
                if edit.task != expected {
                    return Err(conflict());
                }
                edit.task = desired.clone();
                Ok(())
            },
        )?;
    }
    receipt.plan_metadata = Some(PlanIndexRow {
        id: id.clone(),
        status: to.status.clone(),
        team_id: to.team_id.clone(),
        project_path: to.project_path.clone(),
        title: to.title.clone(),
        updated_at: to.updated_at,
    });
    Ok(())
}

fn set_changed<T: serde::Serialize + PartialEq>(
    tx: &Transaction<'_>,
    id: &str,
    path: &str,
    from: &T,
    to: &T,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    if from != to {
        header_edits::set_field(tx, id, path, to, receipt)?;
    }
    Ok(())
}

fn remove_appended(
    tx: &Transaction<'_>,
    id: &str,
    path: &str,
    offset: usize,
    count: usize,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    for index in (offset..offset + count).rev() {
        tx.execute(
            "UPDATE plans SET header_json=json_remove(header_json,?2) WHERE id=?1",
            params![id, format!("{path}[{index}]")],
        )
        .map_err(db_error)?;
        receipt.header_rows += 1;
    }
    Ok(())
}
