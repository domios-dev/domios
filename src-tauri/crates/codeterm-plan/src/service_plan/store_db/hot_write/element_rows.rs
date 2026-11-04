use super::*;
use crate::plan::CanvasElement;

pub(in crate::service_plan) enum ElementChange {
    Add(CanvasElement),
    Update {
        id: String,
        kind: Option<String>,
        content: Option<serde_json::Value>,
        links: Option<Vec<String>>,
    },
    Remove {
        id: String,
    },
    Layout {
        id: String,
        x: Option<f64>,
        y: Option<f64>,
    },
}

impl PlanDatabase {
    pub(in crate::service_plan) fn change_element(
        &self,
        plan_id: &str,
        change: ElementChange,
        now_ms: u64,
    ) -> Result<CommittedRows, PlanError> {
        self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            apply(tx, plan_id, change, now_ms, receipt)
        })
    }
}

pub(super) fn apply(
    tx: &Transaction<'_>,
    plan_id: &str,
    change: ElementChange,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let element = match change {
        ElementChange::Add(element) => {
            insert(tx, plan_id, &element, receipt)?;
            return Ok(());
        }
        ElementChange::Update {
            id,
            kind,
            content,
            links,
        } => {
            let mut element = read(tx, plan_id, &id, receipt)?;
            if let Some(kind) = kind {
                element.kind = kind;
            }
            if let Some(content) = content {
                element.content = content;
            }
            if let Some(links) = links {
                element.links = links;
            }
            element
        }
        ElementChange::Remove { id } => {
            let removed = tx
                .execute(
                    "DELETE FROM plan_elements WHERE plan_id=?1 AND element_id=?2",
                    params![plan_id, id],
                )
                .map_err(db_error)?;
            if removed == 0 {
                return Err(PlanError::ElementNotFound);
            }
            receipt.element_rows += removed;
            return Ok(());
        }
        ElementChange::Layout { id, x, y } => {
            let section: bool = tx
                .query_row(
                    &format!(
                        "SELECT EXISTS(SELECT 1 FROM ({}) WHERE id=?2)",
                        super::super::read_projections::SECTION_ROWS
                    ),
                    params![plan_id, id],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            if section {
                section_rows::apply(
                    tx,
                    plan_id,
                    SectionChange::Canvas { selector: id, x, y },
                    now_ms,
                    receipt,
                )?;
                return Ok(());
            }
            let task: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND task_id=?2)",
                    params![plan_id, id],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            if task {
                let mut task = read_task(tx, plan_id, &id, receipt)?;
                task.canvas.x = x;
                task.canvas.y = y;
                return write_task(tx, plan_id, &task, receipt);
            }
            let milestone: Option<String> = tx.query_row("SELECT m.task_id FROM plan_task_milestones m JOIN plan_tasks t ON t.plan_id=m.plan_id AND t.task_id=m.task_id WHERE m.plan_id=?1 AND json_extract(m.row_json,'$.id')=?2 ORDER BY t.position,m.seq LIMIT 1", params![plan_id,id], |row| row.get(0)).optional().map_err(db_error)?;
            if let Some(task_id) = milestone {
                receipt.projected_text_bytes += task_id.len();
                return milestones::edit_selected(
                    tx,
                    plan_id,
                    &task_id,
                    &id,
                    false,
                    receipt,
                    |milestone, _| {
                        milestone.canvas.x = x;
                        milestone.canvas.y = y;
                        Ok(())
                    },
                );
            }
            let mut element = match read(tx, plan_id, &id, receipt) {
                Err(PlanError::ElementNotFound) => return Err(PlanError::NotFound),
                result => result?,
            };
            element.canvas.x = x;
            element.canvas.y = y;
            element
        }
    };
    let document = encode(&element, receipt)?;
    receipt.hashed_bytes += document.len();
    receipt.element_rows += tx.execute("UPDATE plan_elements SET row_json=?3,content_hash=?4 WHERE plan_id=?1 AND element_id=?2", params![plan_id,element.id,document,store_rows::row_hash(&document)]).map_err(db_error)?;
    Ok(())
}

pub(super) fn insert(
    tx: &Transaction<'_>,
    plan_id: &str,
    element: &CanvasElement,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let document = encode(element, receipt)?;
    receipt.hashed_bytes += document.len();
    receipt.element_rows += tx.execute("INSERT INTO plan_elements(plan_id,element_id,position,content_hash,row_json) VALUES (?1,?2,(SELECT coalesce(max(position)+1,0) FROM plan_elements WHERE plan_id=?1),?3,?4)", params![plan_id,element.id,store_rows::row_hash(&document),document]).map_err(db_error)?;
    Ok(())
}

fn read(
    tx: &Transaction<'_>,
    plan_id: &str,
    id: &str,
    receipt: &mut CommittedRows,
) -> Result<CanvasElement, PlanError> {
    let document: String = tx
        .query_row(
            "SELECT row_json FROM plan_elements WHERE plan_id=?1 AND element_id=?2",
            params![plan_id, id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?
        .ok_or(PlanError::ElementNotFound)?;
    receipt.decoded_bytes += document.len();
    serde_json::from_str(&document).map_err(json_error)
}

#[cfg(test)]
mod tests;
