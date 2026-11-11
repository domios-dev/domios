use codeterm_service_core::service::team_roles::{self, TeamRoleStore};
use codeterm_service_core::service::ServiceState;
use codeterm_team_package::ModelPreference;
use codeterm_types::agent::role_kind::builtins;
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::RoleRegistration;
use serde_json::Value;

/// What a dispatched role type supplies: the Worker profile and its registration defaults.
pub(super) struct RoleParams {
    pub(super) profile: Option<String>,
    /// The registration's first `harness:model` preference; the task's model overrides it.
    pub(super) model: Option<String>,
}

fn requested_role(params: Option<&Value>) -> Option<&str> {
    params
        .and_then(|value| value.get("role"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|role| !role.is_empty())
}

fn preferred_model(registration: &RoleRegistration) -> Option<String> {
    match registration.model.as_ref()? {
        ModelPreference::List(models) => models
            .first()
            .map(|model| {
                model
                    .split_once(':')
                    .map_or(model.as_str(), |(_, model)| model)
            })
            .map(str::to_string),
        ModelPreference::SameAs { .. } => None,
    }
}

/// Resolves `--role` (or the task's builtin executor role) against the Team's registry.
/// A registration placed on another peer refuses local dispatch and names the `--mesh` peer.
pub(super) fn resolve(
    svc: &ServiceState,
    params: Option<&Value>,
    parent_tab_id: &str,
    executor_role: Option<&str>,
) -> Result<RoleParams, String> {
    let team_id = svc
        .mux
        .read()
        .get_tab(parent_tab_id)
        .and_then(|tab| tab.team_id.clone());
    let kind_id = match requested_role(params) {
        Some(value) => {
            let role_type = TeamRoleStore::global()
                .role_type(team_id.as_deref(), value)
                .map_err(|error| error.to_string())?;
            if !role_type.is_worker() {
                return Err(format!(
                    "plan task dispatch launches a worker type; `{value}` is a manager type, use agent spawn --role {value}"
                ));
            }
            Some(role_type.kind_id)
        }
        None => codeterm_service_core::service::role_profile::builtin_worker_profile(executor_role),
    };
    let registration = team_roles::registration_for(
        team_id.as_deref(),
        AgentRole::Worker,
        kind_id.as_deref().unwrap_or(builtins::WORKER),
    );
    if let Some(peer) = registration
        .as_ref()
        .and_then(|registration| registration.placement_peer.as_deref())
    {
        return Err(format!(
            "worker type `{}` is placed on peer `{peer}`; dispatch it with --mesh {peer}",
            kind_id.as_deref().unwrap_or(builtins::WORKER)
        ));
    }
    Ok(RoleParams {
        profile: kind_id.filter(|kind| kind != builtins::WORKER),
        model: registration.as_ref().and_then(preferred_model),
    })
}
