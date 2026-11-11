//! One resolver for the text a role type carries: task > Team registration > builtin default package.
//! Role prose comes first; host runtime text follows it and no package can remove or replace it.

use codeterm_team_package::TeamRoleGroup;
use codeterm_types::agent::role_kind::{self, builtins};
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::{PinnedPackage, RoleProvenance, RoleRegistration};

use crate::builtin_roles;

/// Where one guidance fragment came from; rendered as provenance by `domios role show`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentSource {
    /// The role type's own prose, from the builtin package or the Team's registration.
    Role(RoleProvenance),
    /// A Worker type with no prose of its own reads the builtin Worker's.
    InheritedBase(PinnedPackage),
    /// Host runtime text appended per group and manager slot.
    Host,
    /// Caller-supplied persona/guidance owned by this delivery.
    Task,
}

impl FragmentSource {
    /// Contract fragments describe the role type itself; task fragments describe one delivery.
    pub fn is_contract(&self) -> bool {
        !matches!(self, Self::Task)
    }

    pub fn package(&self) -> Option<&PinnedPackage> {
        match self {
            Self::Role(RoleProvenance::Builtin { package })
            | Self::Role(RoleProvenance::Team { package })
            | Self::InheritedBase(package) => Some(package),
            Self::Host | Self::Task => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Role(RoleProvenance::Builtin { .. }) => "builtin",
            Self::Role(RoleProvenance::Team { .. }) => "team_registration",
            Self::InheritedBase(_) => "inherited_base",
            Self::Host => "host",
            Self::Task => "task_overlay",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetFragment {
    pub source: FragmentSource,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePreset {
    /// Structural role, derived from the role type's group and slot.
    pub role: AgentRole,
    pub kind_id: String,
    pub label: String,
    pub fragments: Vec<PresetFragment>,
}

impl RolePreset {
    /// The role's full guidance: prose, host text, then the task persona.
    pub fn guidance(&self) -> String {
        self.join(|_| true)
    }

    /// Everything the role type carries by itself, with the delivery's own persona left out.
    pub fn contract_guidance(&self) -> String {
        self.join(FragmentSource::is_contract)
    }

    /// Only the persona this delivery supplied.
    pub fn task_guidance(&self) -> String {
        self.join(|source| !source.is_contract())
    }

    pub fn provenance(&self) -> Option<&RoleProvenance> {
        self.fragments
            .iter()
            .find_map(|fragment| match &fragment.source {
                FragmentSource::Role(provenance) => Some(provenance),
                _ => None,
            })
    }

    fn join(&self, keep: impl Fn(&FragmentSource) -> bool) -> String {
        self.fragments
            .iter()
            .filter(|fragment| keep(&fragment.source) && !fragment.text.is_empty())
            .map(|fragment| fragment.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

fn group_of(role: AgentRole) -> TeamRoleGroup {
    match role {
        AgentRole::Manager => TeamRoleGroup::Manager,
        AgentRole::Worker => TeamRoleGroup::Worker,
    }
}

/// The Team's registration for a role type, matched by name and structural group.
pub fn registration<'a>(
    team: &'a [RoleRegistration],
    role: AgentRole,
    kind_id: &str,
) -> Option<&'a RoleRegistration> {
    let name = builtin_roles::prose_role(kind_id);
    team.iter()
        .find(|registration| registration.name == name && registration.group == group_of(role))
}

/// Host runtime text for a group and manager slot, in delivery order.
pub fn host_fragments(role: AgentRole, kind_id: &str) -> Vec<&'static str> {
    match (role, kind_id) {
        (AgentRole::Worker, _) => vec![
            crate::prompts::TESTING_DISCIPLINE,
            crate::prompts::QA_PIPELINE_DISCIPLINE,
        ],
        (AgentRole::Manager, builtins::ORCHESTRATOR | builtins::PLANNER | builtins::WATCHER) => {
            vec![
                crate::prompts::PLAN_TASK_FLAGS,
                crate::prompts::MEM_MANAGEMENT,
            ]
        }
        (AgentRole::Manager, _) => Vec::new(),
    }
}

/// The Team's worker names and descriptions plus this role's own files and skills.
pub fn team_context(team: &[RoleRegistration], own: Option<&RoleRegistration>) -> String {
    let mut workers = builtin_roles::builtin_workers();
    for registration in team
        .iter()
        .filter(|registration| registration.group == TeamRoleGroup::Worker)
    {
        let entry = (registration.name.clone(), registration.description.clone());
        match workers
            .iter_mut()
            .find(|(name, _)| *name == registration.name)
        {
            Some(existing) => *existing = entry,
            None => workers.push(entry),
        }
    }
    let mut lines = vec!["Worker types available to this Team:".to_string()];
    lines.extend(workers.iter().map(|(name, description)| match description {
        Some(description) => format!("- `{name}`: {description}"),
        None => format!("- `{name}`"),
    }));
    if let Some(own) = own {
        if !own.files.is_empty() {
            lines.push("Your files:".to_string());
            lines.extend(own.files.iter().map(|file| match &file.description {
                Some(description) => format!("- `{}`: {description}", file.path),
                None => format!("- `{}`", file.path),
            }));
        }
        if !own.skills.is_empty() {
            lines.push(format!("Your skills: {}", own.skills.join(", ")));
        }
    }
    lines.join("\n")
}

/// Resolves with the builtin default package only.
pub fn resolve(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
) -> Option<RolePreset> {
    resolve_in(&[], role, role_profile, task_guidance)
}

/// A builtin role type's contract (prose plus host text) under its structural role.
pub fn builtin_guidance(kind_id: &str) -> Option<String> {
    let role = if builtins::lookup(kind_id).is_some_and(|descriptor| descriptor.manages()) {
        AgentRole::Manager
    } else {
        AgentRole::Worker
    };
    resolve(Some(role), Some(kind_id), None)
        .filter(|preset| preset.kind_id == kind_id && preset.provenance().is_some())
        .map(|preset| preset.contract_guidance())
}

pub fn resolve_in(
    team: &[RoleRegistration],
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
) -> Option<RolePreset> {
    let resolved_role = role?;
    let kind_id = role_kind::kind_id(role, role_profile)?.to_string();
    let registered = registration(team, resolved_role, &kind_id)
        .filter(|_| kind_id != builtins::GENERAL_AGENT && kind_id != builtins::WATCHER);
    let label = registered
        .map(|registration| registration.label.clone())
        .or_else(|| role_kind::kind_label(role, role_profile).map(str::to_string))
        .unwrap_or_else(|| kind_id.clone());
    let kind_manages = builtins::lookup(&kind_id).is_some_and(|descriptor| descriptor.manages());
    let mut fragments = Vec::new();
    match (registered, builtin_roles::builtin_prose(&kind_id)) {
        (Some(registration), _) => fragments.push(PresetFragment {
            source: FragmentSource::Role(RoleProvenance::Team {
                package: registration.package.clone(),
            }),
            text: registration.instructions.trim().to_string(),
        }),
        (None, Some(prose)) => fragments.push(PresetFragment {
            source: FragmentSource::Role(RoleProvenance::Builtin {
                package: builtin_roles::default_identity(),
            }),
            text: prose.trim().to_string(),
        }),
        (None, None) if resolved_role == AgentRole::Worker && !kind_manages => {
            fragments.push(PresetFragment {
                source: FragmentSource::InheritedBase(builtin_roles::default_identity()),
                text: builtin_roles::builtin_prose(builtins::WORKER)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            })
        }
        (None, None) => {}
    }
    if !fragments.is_empty() {
        fragments.extend(
            host_fragments(resolved_role, &kind_id)
                .into_iter()
                .map(|text| PresetFragment {
                    source: FragmentSource::Host,
                    text: text.trim().to_string(),
                }),
        );
        fragments.push(PresetFragment {
            source: FragmentSource::Host,
            text: team_context(team, registered),
        });
    }
    if let Some(overlay) = task_guidance
        .map(str::trim)
        .filter(|guidance| !guidance.is_empty())
    {
        fragments.push(PresetFragment {
            source: FragmentSource::Task,
            text: overlay.to_string(),
        });
    }
    Some(RolePreset {
        role: resolved_role,
        kind_id,
        label,
        fragments,
    })
}

/// Every builtin role type composed from the default package, keyed by type.
pub fn builtin_presets() -> Vec<(&'static str, RolePreset)> {
    let managers = [
        builtins::ORCHESTRATOR,
        builtins::PLANNER,
        builtins::WATCHER,
        builtins::CARTOGRAPHER,
        builtins::CURATOR,
    ]
    .map(|kind| (kind, AgentRole::Manager));
    let workers = [
        builtins::WORKER,
        builtins::QA,
        builtins::REVIEWER,
        builtins::RELEASE_MANAGER,
    ]
    .map(|kind| (kind, AgentRole::Worker));
    managers
        .into_iter()
        .chain(workers)
        .filter_map(|(kind, role)| {
            resolve(Some(role), Some(kind), None).map(|preset| (kind, preset))
        })
        .collect()
}

#[cfg(test)]
#[path = "role_preset/tests.rs"]
mod tests;
