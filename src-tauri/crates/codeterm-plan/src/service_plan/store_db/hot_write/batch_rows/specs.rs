use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanStatus, Section, Spec, SpecLink};

pub(super) fn apply(
    tx: &Transaction<'_>,
    plan_id: &str,
    op: BatchOp,
    result: &mut BatchResult,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    match op {
        BatchOp::UpsertSpec {
            client_id,
            spec_id,
            title,
        } => {
            let mut spec = if let Some(id) = spec_id {
                spec_rows::read(tx, &reference(&id, result), receipt)?
            } else {
                let project: String = tx
                    .query_row(
                        "SELECT project_path FROM plans WHERE id=?1",
                        [plan_id],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                receipt.projected_text_bytes += project.len();
                let mut spec = Spec::new(title.clone(), project);
                spec.created_at = now_ms;
                spec
            };
            spec.title = title;
            spec.updated_at = now_ms;
            spec_rows::write(tx, &mut spec, receipt)?;
            if let Some(client_id) = client_id {
                result.ids.insert(client_id, spec.id.clone());
            }
            push_once(&mut result.spec_ids, spec.id);
        }
        BatchOp::UpsertSpecSection {
            client_id,
            spec,
            section_id,
            title,
            body,
        } => {
            let mut spec = spec_rows::read(tx, &reference(&spec, result), receipt)?;
            let id = if let Some(id) = section_id {
                let id = reference(&id, result);
                let section = section_rows::selected_mut(&mut spec.sections, &id)
                    .ok_or(PlanError::NotFound)?;
                section.title = title;
                if let Some(body) = body {
                    section.body = body;
                }
                section.id.clone()
            } else {
                let id = crate::plan::ids::short_id("sec");
                spec.sections.push(Section {
                    id: id.clone(),
                    title,
                    handle: None,
                    tags: vec![],
                    body: body.unwrap_or_default(),
                    subsections: vec![],
                    canvas: Default::default(),
                    attachments: vec![],
                });
                id
            };
            spec.updated_at = now_ms;
            spec_rows::write(tx, &mut spec, receipt)?;
            if let Some(client_id) = client_id {
                result.ids.insert(client_id, id.clone());
            }
            push_once(&mut result.spec_section_ids, id);
        }
        BatchOp::SetSpecSectionBody {
            spec,
            section,
            body,
        } => {
            let mut spec = spec_rows::read(tx, &reference(&spec, result), receipt)?;
            let section =
                section_rows::selected_mut(&mut spec.sections, &reference(&section, result))
                    .ok_or(PlanError::NotFound)?;
            section.body = body;
            let id = section.id.clone();
            spec.updated_at = now_ms;
            spec_rows::write(tx, &mut spec, receipt)?;
            push_once(&mut result.spec_section_ids, id);
        }
        BatchOp::LinkSpecSection {
            spec,
            spec_section,
            plan_section,
        } => spec_rows::apply_link(
            tx,
            plan_id,
            spec_rows::SpecLinkChange::Link(SpecLink {
                spec_id: reference(&spec, result),
                spec_section_id: reference(&spec_section, result),
                plan_section_id: reference(&plan_section, result),
            }),
            now_ms,
            receipt,
        )?,
        BatchOp::UnlinkSpecSection {
            spec,
            spec_section,
            plan_section,
        } => spec_rows::apply_link(
            tx,
            plan_id,
            spec_rows::SpecLinkChange::Unlink {
                spec_id: reference(&spec, result),
                spec_section_id: spec_section.map(|id| reference(&id, result)),
                plan_section_id: plan_section.map(|id| reference(&id, result)),
            },
            now_ms,
            receipt,
        )?,
        BatchOp::CreatePlanFromSpec {
            client_id,
            spec,
            title,
        } => {
            let spec_id = reference(&spec, result);
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM specs WHERE id=?1)",
                    [&spec_id],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            if !exists {
                return Err(PlanError::NotFound);
            }
            let owner: OrchestratorRef = header_edits::read_field(tx, plan_id, "$.owner", receipt)?;
            let kinds: Vec<MilestoneKind> =
                header_edits::read_field(tx, plan_id, "$.milestone_kinds", receipt)?;
            let project: String = tx
                .query_row(
                    "SELECT project_path FROM plans WHERE id=?1",
                    [plan_id],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            receipt.projected_text_bytes += project.len();
            let mut plan = Plan::new(title, project, owner);
            plan.id = crate::plan::ids::try_mint_id("pl", |candidate| {
                tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                    [candidate],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(db_error)
            })?;
            plan.status = PlanStatus::Archived;
            plan.canonical_project_path = plan.project_path.clone();
            plan.created_at = now_ms;
            plan.updated_at = now_ms;
            plan.milestone_kinds = kinds;
            plan.spec_refs.push(spec_id.clone());
            plan.primary_spec_id = Some(spec_id);
            crate::service_plan::store_index::record_lifecycle_events(&mut plan, None, now_ms);
            let mut child = CommittedRows::default();
            spec_rows::insert_plan(tx, &plan, receipt.revision, &mut child)?;
            if let Some(client_id) = client_id {
                result.ids.insert(client_id, plan.id.clone());
            }
            receipt.created_plans.push(child);
        }
        _ => return Err(PlanError::Io("expected spec operation".into())),
    }
    Ok(())
}
