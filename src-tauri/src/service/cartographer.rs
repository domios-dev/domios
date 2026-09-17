//! When a workspace admits a Cartographer, and which boundary it is admitted
//! for. The role is opt-in per workspace and one-shot per boundary: a plan wave
//! closing or a release offers exactly one run, and a recorded cartography
//! milestone disarms the offer so nothing respawns on the next heartbeat.

use std::path::Path;

use crate::plan::Plan;
use crate::service::plan::{heartbeat, milestone_gates};

/// Milestone kinds that disarm a boundary. The Orchestrator records
/// `map_requested` when it dispatches the run; the run records
/// `map_generated`. Either is enough, so a dispatched Cartographer is never
/// offered a second time while it works.
pub(crate) const MAP_REQUESTED_KIND: &str = "map_requested";
pub(crate) const MAP_GENERATED_KIND: &str = "map_generated";

/// Source essences the map is generated from. A workspace without them has no
/// map to author, whatever the config says.
const SOURCE_ESSENCES: [&str; 2] = ["essences/capabilities.md", "essences/core.md"];

/// Stable lead of the heartbeat line, so a caller can find the offer without
/// matching its prose.
pub(crate) const CARTOGRAPHY_DIGEST_PREFIX: &str = "Essence map boundary";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapAvailability {
    Enabled,
    DisabledByConfig,
    SourcesAbsent,
}

impl MapAvailability {
    pub(crate) fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Boundary {
    WaveClose { key: String, label: String },
    Release,
}

impl Boundary {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::WaveClose { label, .. } => format!("wave {label} closed"),
            Self::Release => "release preparation".to_string(),
        }
    }
}

/// Both halves of the gate: the owner opted in, and this root actually carries
/// the essences a map is generated from.
pub(crate) fn availability(enabled: bool, root: &Path) -> MapAvailability {
    if !enabled {
        return MapAvailability::DisabledByConfig;
    }
    if SOURCE_ESSENCES
        .iter()
        .all(|relative| root.join(relative).is_file())
    {
        MapAvailability::Enabled
    } else {
        MapAvailability::SourcesAbsent
    }
}

fn latest_cartography_ts(plan: &Plan) -> u64 {
    plan.strides
        .iter()
        .chain(plan.tasks.iter().flat_map(|task| task.milestones.iter()))
        .filter(|milestone| {
            milestone.kind == MAP_REQUESTED_KIND || milestone.kind == MAP_GENERATED_KIND
        })
        .map(|milestone| milestone.ts)
        .max()
        .unwrap_or(0)
}

fn release_boundary_ts(plan: &Plan) -> Option<u64> {
    if !heartbeat::is_plan_complete(plan) {
        return None;
    }
    plan.tasks.iter().map(|task| task.status_changed_at).max()
}

fn group_keys(plan: &Plan) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for key in plan
        .tasks
        .iter()
        .filter_map(|task| task.group.as_deref())
        .chain(plan.task_groups.iter().map(|group| group.key.as_str()))
    {
        if !keys.iter().any(|seen| seen == key) {
            keys.push(key.to_string());
        }
    }
    keys
}

fn group_label(plan: &Plan, key: &str) -> String {
    plan.task_groups
        .iter()
        .find(|group| group.key == key)
        .map(|group| group.label.clone())
        .unwrap_or_else(|| key.to_string())
}

/// The one boundary this plan currently offers, or none. At most one: a
/// boundary is a place to run the Cartographer once, not a recurring cadence.
pub(crate) fn pending_boundary(plan: &Plan, availability: MapAvailability) -> Option<Boundary> {
    if !availability.is_enabled() {
        return None;
    }
    let since = latest_cartography_ts(plan);
    if release_boundary_ts(plan).is_some_and(|ts| ts > since) {
        return Some(Boundary::Release);
    }
    group_keys(plan)
        .into_iter()
        .filter_map(|key| {
            milestone_gates::group_completion(plan, &key)
                .filter(|closed_at| *closed_at > since)
                .map(|closed_at| (closed_at, key))
        })
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, key)| Boundary::WaveClose {
            label: group_label(plan, &key),
            key,
        })
}

fn boundary_line(plan: &Plan, boundary: &Boundary) -> String {
    let plan_id = &plan.id;
    let record = format!(
        "record it on that task with `codeterm plan record {plan_id} --task <task> --step {MAP_REQUESTED_KIND}:<boundary>`"
    );
    match boundary {
        Boundary::WaveClose { .. } => format!(
            "{CARTOGRAPHY_DIGEST_PREFIX} — {}: dispatch one Cartographer to regenerate the map from the essences, and {record}.",
            boundary.label()
        ),
        Boundary::Release => format!(
            "{CARTOGRAPHY_DIGEST_PREFIX} — {}: dispatch one Cartographer, {record}, and give the owner the single `codeterm essence-map release-summary` with its revert choice.",
            boundary.label()
        ),
    }
}

/// The heartbeat contribution: one line when a boundary is open, nothing
/// otherwise.
pub(crate) fn boundary_digest_lines(plan: &Plan, availability: MapAvailability) -> Vec<String> {
    pending_boundary(plan, availability)
        .map(|boundary| vec![boundary_line(plan, &boundary)])
        .into_iter()
        .flatten()
        .collect()
}

/// The same contribution, resolved against the owner's config and the plan's
/// own project root.
pub(crate) fn digest_lines(plan: &Plan) -> Vec<String> {
    let enabled = crate::config::load_config().orchestration.map.enabled;
    let root = if plan.canonical_project_path.is_empty() {
        plan.project_path.as_str()
    } else {
        plan.canonical_project_path.as_str()
    };
    boundary_digest_lines(plan, availability(enabled, Path::new(root)))
}

#[cfg(test)]
#[path = "cartographer/tests.rs"]
mod tests;
