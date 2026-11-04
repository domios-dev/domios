use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanStatus, Section};
use crate::service_plan::{paths, search_index::SectionIndexDelta};

pub(in crate::service_plan) struct SpecPlanCreate {
    pub spec_id: String,
    pub title: String,
    pub owner: OrchestratorRef,
    pub team_id: String,
    pub kinds: Vec<MilestoneKind>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn create_spec_plan_rows(
        &self,
        create: SpecPlanCreate,
        now_ms: u64,
    ) -> Result<(Plan, CommittedRows), PlanError> {
        let result = self.commit_entity_rows(
            &create.spec_id,
            "create_spec_plan",
            now_ms,
            |tx, operation_id| {
                let mut receipt = CommittedRows::default();
                let mut spec = read(tx, &create.spec_id, &mut receipt)?;
                if let Some(owner) = spec.plan_id.as_deref() {
                    return Err(PlanError::Io(format!(
                        "spec {} already owned by plan {owner}",
                        spec.id
                    )));
                }
                let project_path = std::path::Path::new(&spec.project_path);
                if !project_path.is_absolute() {
                    return Err(PlanError::Io(format!(
                        "project path must be an absolute checkout path, got '{}'",
                        spec.project_path
                    )));
                }
                let root = paths::project_root(project_path);
                let metadata = std::fs::metadata(&root).map_err(|error| {
                    PlanError::Io(format!("read project path {}: {error}", root.display()))
                })?;
                if !metadata.is_dir() {
                    return Err(PlanError::Io(format!(
                        "project path is not a directory: {}",
                        root.display()
                    )));
                }
                let existing: Option<(String, String)> = tx
                    .query_row(
                        "SELECT id,title FROM plans WHERE team_id=?1 AND status='active' LIMIT 1",
                        [&create.team_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(db_error)?;
                if let Some((existing_plan_id, existing_title)) = existing {
                    return Err(PlanError::TeamAlreadyHasActivePlan {
                        team_id: create.team_id,
                        existing_plan_id,
                        existing_title,
                    });
                }
                let mut plan = Plan::new(create.title, root.display().to_string(), create.owner);
                plan.id = crate::plan::ids::try_mint_id("pl", |candidate| {
                    tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
                        [candidate],
                        |row| row.get::<_, bool>(0),
                    )
                    .map_err(db_error)
                })?;
                plan.canonical_project_path = plan.project_path.clone();
                plan.team_id = Some(create.team_id);
                plan.milestone_kinds = create.kinds;
                plan.created_at = now_ms;
                plan.updated_at = now_ms;
                plan.primary_spec_id = Some(spec.id.clone());
                plan.spec_refs.push(spec.id.clone());
                plan.sections = mirror_sections(&spec.sections, &mut plan.spec_links, &spec.id);
                crate::service_plan::tag_model::normalize_plan(&mut plan);
                crate::service_plan::store_index::record_lifecycle_events(&mut plan, None, now_ms);
                insert_plan(tx, &plan, operation_id, &mut receipt)?;
                spec.plan_id = Some(plan.id.clone());
                spec.updated_at = now_ms;
                write(tx, &mut spec, &mut receipt)?;
                Ok((plan, receipt))
            },
        )?;
        self.record_row_change(&result.1);
        Ok(result)
    }
}

pub(in crate::service_plan::store_db::hot_write) fn insert_plan(
    tx: &Transaction<'_>,
    plan: &Plan,
    operation_id: i64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let previous_revision = super::super::super::change_feed::revision(tx, &plan.id)?;
    let revision =
        super::super::super::change_feed::next_revision(previous_revision, operation_id)?;
    let rows = store_rows::split_plan(serde_json::to_value(plan).map_err(json_error)?)
        .map_err(PlanError::Io)?;
    store_rows::validate_row_sizes(&rows).map_err(PlanError::InvalidPlanText)?;
    let writes = super::super::super::upsert_plan(tx, plan, &rows, Some(revision))?;
    receipt.serialized_bytes += rows.header.len()
        + rows
            .sections
            .iter()
            .map(|row| row.document.len())
            .sum::<usize>()
        + rows.heartbeat.as_ref().map_or(0, String::len);
    receipt.hashed_bytes += rows
        .sections
        .iter()
        .map(|row| row.document.len())
        .sum::<usize>();
    receipt.section_rows += writes.sections;
    receipt.heartbeat_rows += writes.heartbeat;
    receipt.header_rows += 1;
    receipt.plan_id = plan.id.clone();
    receipt.updated_at = plan.updated_at;
    receipt.previous_revision = previous_revision;
    receipt.revision = revision;
    receipt.index_root = Some(plan.canonical_project_path.clone().into());
    receipt.plan_metadata = Some(super::super::super::PlanIndexRow {
        id: plan.id.clone(),
        title: plan.title.clone(),
        status: plan.status.clone(),
        team_id: plan.team_id.clone(),
        project_path: plan.project_path.clone(),
        updated_at: plan.updated_at,
    });
    let total = count_sections(&plan.sections);
    let mut ordinal = 1;
    for section in &plan.sections {
        receipt.section_search.push(SectionIndexDelta {
            plan_id: plan.id.clone(),
            title: plan.title.clone(),
            status: if plan.status == PlanStatus::Active {
                "active"
            } else {
                "archived"
            }
            .into(),
            project_root: plan.canonical_project_path.clone().into(),
            updated_at: plan.updated_at,
            section: Some(section.clone()),
            removed: vec![],
            ordinal,
            total,
        });
        ordinal += count_sections(std::slice::from_ref(section));
    }
    Ok(())
}

fn count_sections(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|section| 1 + count_sections(&section.subsections))
        .sum()
}

fn mirror_sections(source: &[Section], links: &mut Vec<SpecLink>, spec_id: &str) -> Vec<Section> {
    source
        .iter()
        .map(|section| {
            let id = crate::plan::ids::short_id("sec");
            let subsections = mirror_sections(&section.subsections, links, spec_id);
            links.push(SpecLink {
                spec_id: spec_id.into(),
                spec_section_id: section.id.clone(),
                plan_section_id: id.clone(),
            });
            Section {
                id,
                title: section.title.clone(),
                handle: None,
                tags: vec![],
                body: String::new(),
                subsections,
                canvas: Default::default(),
                attachments: vec![],
            }
        })
        .collect()
}
