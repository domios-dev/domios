//! One resolver for the text a role carries: task > Team registration. A Team has exactly the
//! roles it registers; one with no registrations is the builtin default package as a whole.
//! Role prose comes first; host runtime text follows it and no package can remove or replace it.

use std::borrow::Cow;

use codeterm_team_package::{RoleStart, StartMode};
use codeterm_types::agent::role_kind::{self, builtins, RoleKindOrigin};
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::{PinnedPackage, RoleProvenance, RoleRegistration};

use crate::builtin_roles;

/// Where one guidance fragment came from; rendered as provenance by `domios role show`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentSource {
    /// The role's own prose: the Team's registration, or the default package for a Team
    /// with no registrations.
    Role(RoleProvenance),
    /// Host runtime text appended per kind and manager name.
    Host,
    /// Caller-supplied persona/guidance owned by this delivery.
    Task,
}

impl FragmentSource {
    /// Contract fragments describe the role itself; task fragments describe one delivery.
    pub fn is_contract(&self) -> bool {
        !matches!(self, Self::Task)
    }

    pub fn package(&self) -> Option<&PinnedPackage> {
        match self {
            Self::Role(RoleProvenance::Builtin { package })
            | Self::Role(RoleProvenance::Team { package }) => Some(package),
            Self::Host | Self::Task => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Role(RoleProvenance::Builtin { .. }) => "builtin",
            Self::Role(RoleProvenance::Team { .. }) => "team_registration",
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
    /// Structural role, derived from the registration's kind.
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

    /// Everything the role carries by itself, with the delivery's own persona left out.
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

/// The registrations a Team resolves against: its own, or the default package as a whole when
/// it has none. The flag says whether they came from the default package.
pub fn effective(team: &[RoleRegistration]) -> (Cow<'_, [RoleRegistration]>, bool) {
    if team.is_empty() {
        (Cow::Owned(builtin_roles::default_registrations("")), true)
    } else {
        (Cow::Borrowed(team), false)
    }
}

/// The Team's registration for a role, matched by name and kind.
pub fn registration<'a>(
    team: &'a [RoleRegistration],
    role: AgentRole,
    kind_id: &str,
) -> Option<&'a RoleRegistration> {
    team.iter()
        .find(|registration| registration.names(role.package_kind(), kind_id))
}

/// Whether a manager name gets the generic custom-manager host text: no built-in name and
/// not a host-owned kind.
pub fn is_custom_manager(kind_id: &str) -> bool {
    !builtins::is_builtin_manager(kind_id)
        && kind_id != builtins::WATCHER
        && kind_id != builtins::GENERAL_AGENT
}

/// Host runtime text for a kind and manager name, in delivery order.
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
        (AgentRole::Manager, kind) if is_custom_manager(kind) => {
            vec![crate::prompts::CUSTOM_MANAGER]
        }
        (AgentRole::Manager, _) => Vec::new(),
    }
}

/// The Domios authority a role carries: a built-in manager descriptor's label, or none.
pub fn authority_label(role: AgentRole, kind_id: &str) -> &'static str {
    match role_kind::registered_descriptor(role, kind_id) {
        Some(descriptor)
            if role == AgentRole::Manager && descriptor.origin() == RoleKindOrigin::Builtin =>
        {
            descriptor.label()
        }
        _ => "none",
    }
}

fn start_line(registration: &RoleRegistration) -> String {
    match &registration.start {
        RoleStart::Mode(StartMode::WithTeam) => "starts with the Team".to_string(),
        RoleStart::Mode(StartMode::OnDemand) => format!(
            "on demand, start it with `domios agent spawn --role {}`",
            registration.name
        ),
        RoleStart::Schedule { schedule } => format!("runs on the schedule {schedule}"),
    }
}

fn manager_lines(team: &[RoleRegistration]) -> Vec<String> {
    let mut lines = vec!["Managers of this Team:".to_string()];
    lines.extend(
        team.iter()
            .filter(|registration| registration.is_manager())
            .map(|registration| {
                let label = if registration.label == registration.name {
                    String::new()
                } else {
                    format!(" ({})", registration.label)
                };
                format!(
                    "- `{}`{label}: {}; Domios authority: {}",
                    registration.name,
                    start_line(registration),
                    authority_label(AgentRole::Manager, &registration.name)
                )
            }),
    );
    lines
}

fn worker_lines(team: &[RoleRegistration]) -> Vec<String> {
    let mut lines =
        vec!["Workers of this Team (`domios plan task dispatch --role <name>`):".to_string()];
    lines.extend(
        team.iter()
            .filter(|registration| registration.is_worker())
            .map(|registration| match &registration.description {
                Some(description) => format!("- `{}`: {description}", registration.name),
                None => format!("- `{}`", registration.name),
            }),
    );
    lines
}

/// A manager's view of the Team's managers and workers, plus any role's own files and skills.
/// Workers dispatch nobody, so their launch carries only their own files and skills.
pub fn team_context(
    team: &[RoleRegistration],
    role: AgentRole,
    own: Option<&RoleRegistration>,
) -> String {
    let mut lines = match role {
        AgentRole::Manager => {
            let mut lines = manager_lines(team);
            lines.extend(worker_lines(team));
            lines
        }
        AgentRole::Worker => Vec::new(),
    };
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

/// Resolves against the builtin default package as a whole.
pub fn resolve(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
) -> Option<RolePreset> {
    resolve_in(&[], role, role_profile, task_guidance)
}

/// A default-package role's contract (prose plus host text) under its kind.
pub fn builtin_guidance(kind_id: &str) -> Option<String> {
    let role = if builtins::is_builtin_manager(kind_id) || kind_id == builtins::WATCHER {
        AgentRole::Manager
    } else {
        AgentRole::Worker
    };
    resolve(Some(role), Some(kind_id), None)
        .filter(|preset| preset.kind_id == kind_id && preset.provenance().is_some())
        .map(|preset| preset.contract_guidance())
}

/// The Watcher is host-owned: it reads the default package's Planner prose, never a Team's.
fn watcher_prose() -> Option<PresetFragment> {
    builtin_roles::builtin_prose(builtins::WATCHER).map(|prose| PresetFragment {
        source: FragmentSource::Role(RoleProvenance::Builtin {
            package: builtin_roles::default_identity(),
        }),
        text: prose.trim().to_string(),
    })
}

pub fn resolve_in(
    team: &[RoleRegistration],
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
) -> Option<RolePreset> {
    let resolved_role = role?;
    let kind_id = role_kind::kind_id(role, role_profile)?.to_string();
    let (team, builtin) = effective(team);
    let host_owned = kind_id == builtins::GENERAL_AGENT || kind_id == builtins::WATCHER;
    let registered = registration(&team, resolved_role, &kind_id).filter(|_| !host_owned);
    let label = registered
        .map(|registration| registration.label.clone())
        .or_else(|| role_kind::kind_label(role, role_profile).map(str::to_string))
        .unwrap_or_else(|| kind_id.clone());
    let mut fragments = Vec::new();
    if let Some(registration) = registered {
        let package = registration.package.clone();
        fragments.push(PresetFragment {
            source: FragmentSource::Role(if builtin {
                RoleProvenance::Builtin { package }
            } else {
                RoleProvenance::Team { package }
            }),
            text: registration.instructions.trim().to_string(),
        });
    } else if kind_id == builtins::WATCHER {
        fragments.extend(watcher_prose());
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
        let context = team_context(&team, resolved_role, registered);
        if !context.is_empty() {
            fragments.push(PresetFragment {
                source: FragmentSource::Host,
                text: context,
            });
        }
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

/// Every default-package role and the host-only Watcher, composed, keyed by name.
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
