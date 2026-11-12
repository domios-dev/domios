//! Every package role becomes a Team registration pinned to the package identity. Registering
//! spawns nothing; a later `--role` launch reads it through the one role resolver.

use std::collections::BTreeMap;

use codeterm_service_core::service::builtin_roles::builtin_prose;
use codeterm_service_core::service::team_roles::TeamRoleStore;
use codeterm_service_core::service::team_template_payload::role_file_refs;
use codeterm_service_core::service::team_template_start::AdmittedTemplate;
use codeterm_team_package::contracts::{TemplateItemState, TemplateStartItem};
use codeterm_team_package::{ManagerSlot, TeamRoleGroup};
use codeterm_types::team::role_registration::{RoleRegistration, RoleRegistrationOutcome};

use super::seats::Seat;

/// The registrations one start asks for, in package order (managers first).
pub(crate) fn registrations_for(
    team_id: &str,
    admitted: &AdmittedTemplate,
    registered_at: u64,
) -> Vec<RoleRegistration> {
    let package = &admitted.resolved.package;
    let identity = admitted.resolved.identity();
    package
        .document
        .roles()
        .into_iter()
        .filter_map(|(group, name, role)| {
            let slot = match group {
                TeamRoleGroup::Manager => ManagerSlot::parse(name),
                TeamRoleGroup::Worker => None,
            };
            // A role without its own text inherits the host's builtin of the same name.
            let instructions = package
                .role_instructions(name)
                .or_else(|| builtin_prose(name))?
                .to_string();
            // The payload leaf owns where a role's files live once applied.
            let files = role_file_refs(team_id, package, role);
            Some(RoleRegistration {
                team_id: team_id.to_string(),
                group,
                name: name.to_string(),
                label: role.name.clone().unwrap_or_else(|| name.to_string()),
                description: role.description.clone(),
                start: role.effective_start(slot),
                package: identity.clone(),
                instructions,
                model: role.model.clone(),
                reasoning: role.reasoning.clone(),
                // Managers are the Team's home; only workers run on another machine.
                placement_peer: match group {
                    TeamRoleGroup::Worker => admitted.placements.get(name).cloned(),
                    TeamRoleGroup::Manager => None,
                },
                consent_decision_id: admitted.decision_id.clone(),
                files,
                skills: role.skills.clone(),
                registered_at,
            })
        })
        .collect()
}

/// A manager that joins later keeps the owner's start-form pick on its registration, which
/// its on-demand launch reads; started managers already launched on their seat.
pub(crate) fn record_later_seats(
    registrations: &mut [RoleRegistration],
    seats: &BTreeMap<ManagerSlot, Seat>,
    started: &[ManagerSlot],
) {
    for registration in registrations {
        let Some(slot) = registration.slot().filter(|slot| !started.contains(slot)) else {
            continue;
        };
        let Some(seat) = seats.get(&slot) else {
            continue;
        };
        if let Some(preference) = seat.preference() {
            registration.model = Some(preference);
            registration.reasoning = seat.effort.clone().or(registration.reasoning.take());
        }
    }
}

/// Registers each role once. A role already pinned to this package identity is present;
/// one pinned to another version is reported and left exactly as it was.
pub(crate) fn register_all(
    store: &TeamRoleStore,
    registrations: Vec<RoleRegistration>,
) -> Result<Vec<TemplateStartItem>, String> {
    let mut items = Vec::with_capacity(registrations.len());
    for registration in registrations {
        let team_id = registration.team_id.clone();
        let identity = registration.package.clone();
        let group = registration.group;
        let role = registration.name.clone();
        let state = match store.register(registration)? {
            RoleRegistrationOutcome::Created => TemplateItemState::Created,
            RoleRegistrationOutcome::AlreadyPresent => TemplateItemState::AlreadyPresent,
            RoleRegistrationOutcome::Pinned => {
                let same_package = store
                    .registrations(&team_id)
                    .iter()
                    .any(|existing| existing.name == role && existing.package == identity);
                if same_package {
                    TemplateItemState::AlreadyPresent
                } else {
                    TemplateItemState::PinnedToOtherVersion
                }
            }
        };
        items.push(TemplateStartItem { group, role, state });
    }
    Ok(items)
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}
