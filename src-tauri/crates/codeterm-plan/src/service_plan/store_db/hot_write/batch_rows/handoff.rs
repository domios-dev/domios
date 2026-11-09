use super::*;
use crate::plan::{CanvasElement, ElementCanvas, PlanStatus, TransferRecord};
use crate::service_plan::{body_files, paths};

pub(super) fn apply(
    tx: &Transaction<'_>,
    plan_id: &str,
    op: BatchOp,
    result: &mut BatchResult,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let BatchOp::AddHandoff {
        client_id,
        mode,
        mut content,
        body,
        task,
        from,
        to,
    } = op
    else {
        return Err(PlanError::Io("expected handoff operation".into()));
    };
    text_bounds::validate_plan_text("handoff body", &body)?;
    let (title,project,root,status):(String,String,String,String)=tx.query_row("SELECT title,project_path,CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''))='' THEN project_path ELSE json_extract(header_json,'$.canonical_project_path') END,status FROM plans WHERE id=?1",[plan_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).map_err(db_error)?;
    receipt.projected_text_bytes += title.len() + project.len() + root.len() + status.len();
    let status: PlanStatus =
        serde_json::from_value(serde_json::Value::String(status)).map_err(json_error)?;
    let highest:String=tx.query_row("SELECT coalesce(max(domios_handoff_seq(row_json->'$.content.seq')),'00000000000000000000') FROM plan_elements WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='handoff'",[plan_id],|row|row.get(0)).map_err(db_error)?;
    let seq = highest
        .parse::<u64>()
        .map_err(|error| PlanError::Io(error.to_string()))?
        .checked_add(1)
        .ok_or_else(|| PlanError::Io("handoff sequence exhausted".into()))?;
    let id = crate::plan::ids::short_id("el");
    let directory = paths::plan_state_dir(std::path::Path::new(&root), &status, plan_id)
        .map_err(|error| PlanError::Io(error.to_string()))?;
    let body_path = body_files::handoff_path_in(&directory, &id);
    let relative = body_path
        .strip_prefix(std::path::Path::new(&project))
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| body_path.display().to_string());
    receipt
        .body_files
        .push(batch_sidecars::NewBodyFile::write(body_path, &body)?);
    receipt.file_bytes += body.len();
    if !content.is_object() {
        content = serde_json::json!({});
    }
    if let Some(fields) = content.as_object_mut() {
        fields.insert("seq".into(), seq.into());
        fields.insert(
            "mode".into(),
            mode.unwrap_or_else(|| "record".into()).into(),
        );
        fields.insert("plan_id".into(), plan_id.into());
        fields.insert("plan_title".into(), title.into());
        fields.insert("ts".into(), now_ms.into());
        fields.insert("body_path".into(), relative.into());
        fields.insert("body".into(), body.into());
        fields
            .entry("reseed_state")
            .or_insert_with(|| "created".into());
    }
    let mut links = vec![format!("plan:{plan_id}")];
    if seq > 1 {
        links.push(format!("handoff:{}", seq - 1));
    }
    if let Some(task) = &task {
        links.push(reference(task, result));
    }
    element_rows::insert(
        tx,
        plan_id,
        &CanvasElement {
            id: id.clone(),
            kind: "handoff".into(),
            canvas: ElementCanvas::default(),
            content,
            links,
        },
        receipt,
    )?;
    if let Some(task) = task {
        let task_id = batch_task(tx, plan_id, &reference(&task, result), receipt)?;
        declare_kind(tx, plan_id, "handoff", true, receipt)?;
        let milestone_id = crate::plan::ids::short_id("ms");
        let mut task = read_task(tx, plan_id, &task_id, receipt)?;
        task.updated_at = now_ms;
        write_task(tx, plan_id, &task, receipt)?;
        append_milestone(
            tx,
            plan_id,
            &task_id,
            &Milestone {
                id: milestone_id.clone(),
                from: from
                    .clone()
                    .unwrap_or_else(|| AgentRef::from_tab("unknown")),
                kind: "handoff".into(),
                text: format!("handoff #{seq}"),
                ts: now_ms,
                links: vec![id.clone()],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            },
            receipt,
        )?;
        task_fields::queue_task_index(tx, plan_id, &task, false, false, receipt)?;
        result.milestone_ids.push(milestone_id);
    }
    let owner: AgentRef = header_edits::read_field(tx, plan_id, "$.owner", receipt)?;
    header_edits::append_field(
        tx,
        plan_id,
        "$.owner_history",
        &TransferRecord {
            from: from.unwrap_or_else(|| owner.clone()),
            to: to.unwrap_or(owner),
            ts: now_ms,
            reason: format!("handoff #{seq}"),
        },
        receipt,
    )?;
    if let Some(client_id) = client_id {
        result.ids.insert(client_id, id.clone());
    }
    result.element_ids.push(id);
    Ok(())
}
