use std::path::Path;

use super::{
    io_error, is_radar_item_id, write_turn, RadarError, RadarItem, RadarSource, RadarStatus,
};
use crate::plan::{ExternalRef, ExternalRefKind, PlanStatus, Task, TaskStatus};
use crate::service_plan::{paths, PlanStore};

/// The hidden archived plan that held standalone tasks before Radar existed.
const LEGACY_INBOX_PLAN_ID: &str = "inbox";
const LEGACY_INBOX_DIR: &str = "_inbox";

impl PlanStore {
    /// Moves every task of the legacy hidden inbox plan into Radar, then deletes that plan.
    /// Item ids derive from task ids, so an interrupted run resumes without duplicates.
    pub fn absorb_legacy_inbox(&self, now_ms: u64) -> Result<usize, RadarError> {
        let Some(plan) = self.get(LEGACY_INBOX_PLAN_ID) else {
            return Ok(0);
        };
        let radar = self.radar();
        {
            let _turn = write_turn();
            for task in &plan.tasks {
                let mut item = legacy_item(&plan.project_path, task, now_ms);
                if !is_radar_item_id(&item.id) {
                    item.id = radar.mint_id();
                } else if radar.item_path(&item.id).exists() {
                    continue;
                }
                radar.write(&item)?;
            }
        }
        self.delete(LEGACY_INBOX_PLAN_ID).map_err(io_error)?;
        let artifact_dir = paths::plan_state_dir_unchecked(
            Path::new(&plan.project_path),
            &PlanStatus::Archived,
            LEGACY_INBOX_DIR,
        );
        match std::fs::remove_dir_all(&artifact_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        Ok(plan.tasks.len())
    }
}

fn legacy_item(project_path: &str, task: &Task, now_ms: u64) -> RadarItem {
    RadarItem {
        id: format!("rd_{}", task.id),
        project_path: project_path.to_string(),
        title: task.name.clone(),
        body: legacy_body(task),
        tags: task.tags.clone(),
        status: RadarStatus::Open,
        source: task.external_refs.first().map(legacy_source),
        pulled: None,
        created_at: if task.created_at == 0 {
            now_ms
        } else {
            task.created_at
        },
        updated_at: now_ms,
    }
}

fn legacy_source(external_ref: &ExternalRef) -> RadarSource {
    let provider = match external_ref.kind {
        ExternalRefKind::GithubIssue => "github",
    };
    RadarSource {
        provider: provider.into(),
        external_id: format!("{}#{}", external_ref.repository, external_ref.number),
        url: Some(external_ref.canonical_url.clone()),
    }
}

fn nonblank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// Folds every authored task field into the item body so nothing is lost.
fn legacy_body(task: &Task) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !task.body.trim().is_empty() {
        parts.push(task.body.trim_end().to_string());
    }
    if let Some(intent) = nonblank(task.intent.as_deref()) {
        parts.push(format!("## Intent\n{intent}"));
    }
    if !task.acceptance_criteria.is_empty() {
        let criteria: Vec<String> = task
            .acceptance_criteria
            .iter()
            .map(|criterion| format!("- {criterion}"))
            .collect();
        parts.push(format!("## Acceptance criteria\n{}", criteria.join("\n")));
    }
    for (heading, value) in [
        ("Test plan", task.test_plan.as_deref()),
        ("QA plan", task.qa_plan.as_deref()),
        ("Correctness", task.correctness.as_deref()),
        ("Model", task.model.as_deref()),
        ("Model rationale", task.model_rationale.as_deref()),
    ] {
        if let Some(text) = nonblank(value) {
            parts.push(format!("## {heading}\n{text}"));
        }
    }
    if task.external_refs.len() > 1 {
        let refs: Vec<String> = task
            .external_refs
            .iter()
            .map(|external_ref| format!("- {}", external_ref.canonical_url))
            .collect();
        parts.push(format!("## References\n{}", refs.join("\n")));
    }
    if task.status != TaskStatus::Pending || task.assigned_tab_id.is_some() {
        let mut state = format!("## Legacy inbox state\nstatus: {}", task.status.key());
        if let Some(tab) = task.assigned_tab_id.as_deref() {
            state.push_str(&format!("\nassigned tab: {tab}"));
        }
        parts.push(state);
    }
    parts.join("\n\n")
}
