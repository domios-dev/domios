use super::*;
use crate::plan::{PlanMode, TaskStatus};

impl TaskRowEdit {
    pub(in crate::service_plan) fn unblock(&mut self, action: &str, attempt: &str, now: u64) {
        if self.task.status != TaskStatus::Blocked {
            return;
        }
        let reason = self
            .task
            .status_label
            .as_deref()
            .unwrap_or("blocked without a recorded reason");
        self.strides.push(Milestone {
            id: crate::plan::ids::short_id("ms"),
            from: self.owner.clone(),
            kind: "decision".into(),
            text: format!("unblocked by {action}: {reason}\nAttempt id: {attempt}"),
            ts: now,
            links: vec![self.task.id.clone()],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: Vec::new(),
        });
        self.task.status = TaskStatus::Pending;
        self.task.status_changed_at = now;
    }

    pub(in crate::service_plan) fn progress(&mut self, competitive: bool, attempt_id: &str) {
        let has_executor = if competitive {
            crate::service_plan::mutations::live_competitive_executor(&self.task, attempt_id)
        } else {
            self.task
                .assigned_tab_id
                .as_deref()
                .or(self.task.lane.tab_id.as_deref())
                .is_some_and(|tab| !tab.trim().is_empty())
        };
        self.progression = Some((self.task.status.clone(), has_executor));
    }
}

pub(super) fn finish(
    tx: &Transaction<'_>,
    plan_id: &str,
    original: &Task,
    original_mode: &PlanMode,
    edit: &mut TaskRowEdit,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    if let Some((from, has_executor)) = &edit.progression {
        crate::service_plan::mutations::status_transition::validate(
            Some(original),
            from,
            &TaskStatus::InProgress,
            None,
            *has_executor,
            |task| task_fields::warn_review_backlog(tx, plan_id, task),
            || task_fields::reject_open_subtasks(tx, plan_id, &edit.task.id),
        )?;
    }
    if edit.warn_dispatch {
        task_fields::warn_review_backlog(tx, plan_id, original)?;
    }
    if edit.mode != *original_mode {
        let mode = encode(&edit.mode, receipt)?;
        tx.execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.mode',json(?2)) WHERE id=?1",
            params![plan_id, mode],
        )
        .map_err(db_error)?;
        receipt.header_rows += 1;
    }
    for stride in &edit.strides {
        append_stride(tx, plan_id, stride, receipt)?;
    }
    if let Some(attempt_id) = &edit.rollback_unblock {
        if let Some(reason) = annotate_rollback(
            tx,
            plan_id,
            &edit.task.id,
            attempt_id,
            edit.legacy_dispatch_rollback,
            receipt,
        )? {
            if edit.legacy_dispatch_rollback {
                edit.task.status = TaskStatus::Blocked;
                edit.task.status_label = Some(reason);
            }
        }
    }
    Ok(())
}

fn annotate_rollback(
    tx: &Transaction<'_>,
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    legacy: bool,
    receipt: &mut CommittedRows,
) -> Result<Option<String>, PlanError> {
    let marker = format!("\nAttempt id: {attempt_id}");
    let row: Option<(i64, String)> = tx.query_row("SELECT seq,row_json FROM plan_strides s WHERE plan_id=?1 AND EXISTS(SELECT 1 FROM json_each(s.row_json,'$.links') WHERE value=?2) AND (substr(json_extract(row_json,'$.text'),1,23)='unblocked by dispatch: ' OR (NOT ?4 AND substr(json_extract(row_json,'$.text'),1,22)='unblocked by respawn: ')) AND substr(json_extract(row_json,'$.text'),-length(?3))=?3 ORDER BY seq DESC LIMIT 1", params![plan_id, task_id, marker, legacy], |row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(db_error)?;
    let Some((position, json)) = row else {
        return Ok(None);
    };
    receipt.decoded_bytes += json.len();
    let mut stride: Milestone = serde_json::from_str(&json).map_err(json_error)?;
    let reason = stride
        .text
        .strip_prefix("unblocked by dispatch: ")
        .or_else(|| stride.text.strip_prefix("unblocked by respawn: "))
        .and_then(|text| text.strip_suffix(&marker))
        .unwrap_or_default()
        .to_string();
    let prefix = if legacy { "dispatch" } else { "startup" };
    stride.text = format!("{prefix} rolled back; task remains blocked: {reason}");
    let json = encode(&stride, receipt)?;
    tx.execute(
        "UPDATE plan_strides SET row_json=?3 WHERE plan_id=?1 AND seq=?2",
        params![plan_id, position, json],
    )
    .map_err(db_error)?;
    tx.execute("UPDATE plans SET header_json=json_set(header_json,'$._stride_digest_invalid',json('true')) WHERE id=?1", [plan_id]).map_err(db_error)?;
    receipt.strides += 1;
    receipt.header_rows += 1;
    Ok(Some(reason))
}
