//! When a workspace admits a Cartographer, and which boundary it is admitted
//! for. The role is opt-in per workspace and one-shot per boundary: a plan wave
//! closing or a release offers exactly one run, and a recorded cartography
//! milestone disarms the offer so nothing respawns on the next heartbeat.

use std::path::Path;

#[cfg(test)]
use crate::plan::Plan;
#[cfg(test)]
use crate::service::plan::{heartbeat, milestone_gates};

/// Milestone kinds that disarm a boundary. The Orchestrator records
/// `map_requested` when it dispatches the run; the run records
/// `map_generated`. Either is enough, so a dispatched Cartographer is never
/// offered a second time while it works.
pub(crate) const MAP_REQUESTED_KIND: &str = "map_requested";
#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn release_boundary_ts(plan: &Plan) -> Option<u64> {
    if !heartbeat::is_plan_complete(plan) {
        return None;
    }
    plan.tasks.iter().map(|task| task.status_changed_at).max()
}

#[cfg(test)]
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

#[cfg(test)]
fn group_label(plan: &Plan, key: &str) -> String {
    plan.task_groups
        .iter()
        .find(|group| group.key == key)
        .map(|group| group.label.clone())
        .unwrap_or_else(|| key.to_string())
}

/// The one boundary this plan currently offers, or none. At most one: a
/// boundary is a place to run the Cartographer once, not a recurring cadence.
#[cfg(test)]
pub(crate) fn pending_boundary(plan: &Plan, availability: MapAvailability) -> Option<Boundary> {
    if !availability.is_enabled() {
        return None;
    }
    pending_boundary_from(&cartography_facts(plan), availability)
}

#[cfg(test)]
pub(crate) fn cartography_facts(plan: &Plan) -> codeterm_plan::service_plan::NoticeCartographyRead {
    let closed_group = group_keys(plan)
        .into_iter()
        .filter_map(|key| milestone_gates::group_completion(plan, &key).map(|at| (at, key)))
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(
            |(closed_at, key)| codeterm_plan::service_plan::NoticeClosedGroup {
                label: group_label(plan, &key),
                key,
                closed_at,
            },
        );
    codeterm_plan::service_plan::NoticeCartographyRead {
        latest_map_at: latest_cartography_ts(plan),
        release_at: release_boundary_ts(plan),
        closed_group,
    }
}

fn pending_boundary_from(
    facts: &codeterm_plan::service_plan::NoticeCartographyRead,
    availability: MapAvailability,
) -> Option<Boundary> {
    if !availability.is_enabled() {
        return None;
    }
    if facts.release_at.is_some_and(|at| at > facts.latest_map_at) {
        return Some(Boundary::Release);
    }
    facts
        .closed_group
        .as_ref()
        .filter(|group| group.closed_at > facts.latest_map_at)
        .map(|group| Boundary::WaveClose {
            key: group.key.clone(),
            label: group.label.clone(),
        })
}

fn boundary_line(plan_id: &str, boundary: &Boundary) -> String {
    let record = format!(
        "record it on that task with `domios plan record {plan_id} --task <task> --step {MAP_REQUESTED_KIND}:<boundary>`"
    );
    match boundary {
        Boundary::WaveClose { .. } => format!(
            "{CARTOGRAPHY_DIGEST_PREFIX} — {}: dispatch one Cartographer to regenerate the map from the essences, and {record}.",
            boundary.label()
        ),
        Boundary::Release => format!(
            "{CARTOGRAPHY_DIGEST_PREFIX} — {}: dispatch one Cartographer, {record}, and give the owner the single `domios essence-map release-summary` with its revert choice.",
            boundary.label()
        ),
    }
}

/// The heartbeat contribution: one line when a boundary is open, nothing
/// otherwise.
#[cfg(test)]
pub(crate) fn boundary_digest_lines(plan: &Plan, availability: MapAvailability) -> Vec<String> {
    pending_boundary(plan, availability)
        .map(|boundary| vec![boundary_line(&plan.id, &boundary)])
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(test)]
#[path = "cartographer/tests.rs"]
mod tests;

pub(crate) fn digest_lines_from_input(
    input: &codeterm_plan::service_plan::NoticeDigestInput,
    enabled: bool,
) -> Vec<String> {
    let available = availability(enabled, Path::new(&input.project_root));
    boundary_digest_lines_from_input(input, available)
}

pub(crate) fn boundary_digest_lines_from_input(
    input: &codeterm_plan::service_plan::NoticeDigestInput,
    available: MapAvailability,
) -> Vec<String> {
    pending_boundary_from(&input.cartography, available)
        .map(|boundary| vec![boundary_line(&input.plan_id, &boundary)])
        .unwrap_or_default()
}
