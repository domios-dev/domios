use super::*;
use crate::plan::{CanvasElement, ElementCanvas, Goal};

pub(super) fn apply(
    tx: &Transaction<'_>,
    plan_id: &str,
    op: BatchOp,
    result: &mut BatchResult,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    match op {
        BatchOp::SetGoal {
            client_id,
            vector,
            vision,
            constraints,
            postulates,
        } => {
            header_edits::set_field(
                tx,
                plan_id,
                "$.goal",
                &Goal {
                    vector,
                    vision,
                    constraints,
                    postulates,
                },
                receipt,
            )?;
            if let Some(client_id) = client_id {
                result.ids.insert(client_id, plan_id.into());
            }
        }
        BatchOp::Link { to } => {
            let to = reference(&to, result);
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM plans p,json_each(p.header_json,'$.links') l WHERE p.id=?1 AND l.value=?2)",params![plan_id,to],|row|row.get(0)).map_err(db_error)?;
            if !exists {
                header_edits::append_field(tx, plan_id, "$.links", &to, receipt)?;
            }
        }
        BatchOp::IngestSections { body } => {
            let ingested = ingest_rows::apply(tx, plan_id, &body, now_ms, receipt)?;
            for section in ingested.sections {
                result.ids.insert(section.handle, section.section_id);
            }
        }
        BatchOp::AddElement {
            client_id,
            kind,
            content,
            link,
            links,
            x,
            y,
        } => {
            let id = crate::plan::ids::short_id("el");
            let mut links: Vec<_> = links.iter().map(|link| reference(link, result)).collect();
            if let Some(link) = link {
                links.push(reference(&link, result));
            }
            element_rows::insert(
                tx,
                plan_id,
                &CanvasElement {
                    id: id.clone(),
                    kind,
                    content,
                    links,
                    canvas: ElementCanvas {
                        x,
                        y,
                        collapsed: false,
                    },
                },
                receipt,
            )?;
            if let Some(client_id) = client_id {
                result.ids.insert(client_id, id.clone());
            }
            result.element_ids.push(id);
        }
        _ => return Err(PlanError::Io("expected plan field operation".into())),
    }
    Ok(())
}
