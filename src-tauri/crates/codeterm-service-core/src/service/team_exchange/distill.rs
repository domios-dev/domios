//! Bake: turns a live Team's registrations, its source package and its collected payload
//! into one package. Roles keep exactly what they were registered with, a role without a
//! model stays without one, and the coordinator's runbook joins the orchestrator.

use std::collections::BTreeMap;

use codeterm_team_package::contracts::TemplateDistillRequest;
use codeterm_team_package::{
    ManagerSlot, OrderedMap, Package, PackageLineage, TeamRole, TeamRoleGroup,
};
use codeterm_types::team::role_registration::{PinnedPackage, RoleRegistration};

use crate::service::team_template_payload::{package_path, Collected};

pub const RUNBOOK_HEADING: &str = "## Runbook";

/// The package identity most of the Team's registrations are pinned to.
pub fn source_identity(registrations: &[RoleRegistration]) -> Option<PinnedPackage> {
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for registration in registrations {
        *counts
            .entry((
                registration.package.name.clone(),
                registration.package.version.clone(),
            ))
            .or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then(right.0.cmp(&left.0)))
        .map(|((name, version), _)| PinnedPackage { name, version })
}

/// A registered role as the Team holds it; tools come from the source package's role, and
/// only files inside the Team's knowledge directory travel.
fn role_from(team_id: &str, registration: &RoleRegistration, base: &Package) -> TeamRole {
    TeamRole {
        name: match registration.group {
            TeamRoleGroup::Manager => Some(registration.label.clone()),
            TeamRoleGroup::Worker => None,
        },
        description: registration.description.clone(),
        start: Some(registration.start.clone()),
        model: registration.model.clone(),
        reasoning: registration.reasoning.clone(),
        skills: registration.skills.clone(),
        tools: base
            .document
            .role(&registration.name)
            .map(|role| role.tools.clone())
            .unwrap_or_default(),
        files: registration
            .files
            .iter()
            .filter_map(|file| package_path(team_id, &file.path))
            .collect(),
        instructions: Some(registration.instructions.clone()),
        instructions_file: None,
        extensions: OrderedMap::new(),
    }
}

/// A role taken from the source package, with its instructions inline.
fn inline_role(base: &Package, name: &str, role: &TeamRole) -> TeamRole {
    let mut role = role.clone();
    role.instructions = base.role_instructions(name).map(str::to_string);
    role.instructions_file = None;
    role
}

fn with_runbook(instructions: Option<String>, runbook: Option<&str>) -> Option<String> {
    let Some(runbook) = runbook.map(str::trim).filter(|text| !text.is_empty()) else {
        return instructions;
    };
    let base = instructions.unwrap_or_default();
    let base = base.trim_end();
    Some(if base.is_empty() {
        format!("{RUNBOOK_HEADING}\n\n{runbook}\n")
    } else {
        format!("{base}\n\n{RUNBOOK_HEADING}\n\n{runbook}\n")
    })
}

/// Bakes the package. `base` is the source package (or the builtin default when the Team
/// has none); `derived_from` is set only when the Team was started from a template.
pub fn bake(
    request: &TemplateDistillRequest,
    registrations: &[RoleRegistration],
    base: &Package,
    collected: Collected,
) -> Package {
    let team_id = request.team_id.as_str();
    let mut document = base.document.clone();
    document.name = request.name.clone();
    document.version = request.version.clone();
    document.description = request.description.clone();
    document.title = request.title.clone();
    document.license = request.license.clone();
    document.authors = request.authors.clone();
    document.links = None;
    document.when = request.when.clone();
    document.icon = None;
    document.derived_from = source_identity(registrations).map(|pinned| PackageLineage {
        name: pinned.name,
        version: pinned.version,
    });

    let mut managers = document.managers.clone();
    for slot in ManagerSlot::ALL {
        let registered = registrations.iter().find(|registration| {
            registration.group == TeamRoleGroup::Manager && registration.slot() == Some(slot)
        });
        let role = match registered {
            Some(registration) => Some(role_from(team_id, registration, base)),
            None => base
                .document
                .managers
                .slot(slot)
                .map(|role| inline_role(base, slot.as_str(), role)),
        };
        match slot {
            ManagerSlot::Orchestrator => {
                if let Some(role) = role {
                    managers.orchestrator = role;
                }
            }
            ManagerSlot::Planner => managers.planner = role,
            ManagerSlot::Cartographer => managers.cartographer = role,
            ManagerSlot::Curator => managers.curator = role,
        }
    }
    managers.orchestrator.instructions = with_runbook(
        managers.orchestrator.instructions.take(),
        request.runbook.as_deref(),
    );
    document.managers = managers;

    let mut workers = OrderedMap::new();
    for registration in registrations
        .iter()
        .filter(|registration| registration.group == TeamRoleGroup::Worker)
    {
        workers.insert(
            registration.name.clone(),
            role_from(team_id, registration, base),
        );
    }
    if registrations.is_empty() {
        for (name, role) in base.document.workers.iter() {
            workers.insert(name.to_string(), inline_role(base, name, role));
        }
    }
    document.workers = workers;

    document.files = collected.files;
    document.memory = collected.memory;
    document.skills = collected.skills;
    document.automation = collected.automation;
    Package {
        header: None,
        document,
        extensions: OrderedMap::new(),
        entries: collected.entries,
    }
}
