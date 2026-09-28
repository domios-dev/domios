use codeterm_types::agent::role_kind;
use codeterm_types::agent::role_kind::builtins::{ORCHESTRATOR, PLANNER, WATCHER};

use super::{json_str, orchestrator::SpawnAgentParams};
use crate::multiplexer::types::AgentRole;
use crate::service::SpawnIdentityProvenance;

impl SpawnAgentParams {
    pub fn from_json(params: &serde_json::Value) -> Result<Self, String> {
        let mut out = Self {
            agent: json_str(params, "agent"),
            agent_name: json_str(params, "agent_name"),
            args: params.get("args").and_then(|v| v.as_array()).map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            }),
            cwd: json_str(params, "cwd"),
            provisioned_cwd: None,
            title: super::spawn_title::normalize_agent_title(json_str(params, "title")),
            title_provenance: json_str(params, "title_provenance")
                .as_deref()
                .map(parse_spawn_identity_provenance)
                .transpose()?
                .or_else(|| {
                    json_str(params, "title")
                        .filter(|value| !value.trim().is_empty())
                        .map(|_| SpawnIdentityProvenance::UserSupplied)
                }),
            description: json_str(params, "description")
                .map(|s| crate::service::normalize_tab_description(&s))
                .transpose()?
                .flatten(),
            role: json_str(params, "role"),
            role_profile: json_str(params, "role_profile"),
            parent_tab_id: json_str(params, "parent_tab_id").or_else(|| json_str(params, "parent")),
            plan_id: json_str(params, "plan_id").or_else(|| json_str(params, "plan")),
            can_spawn: params.get("can_spawn").and_then(|v| v.as_bool()),
            task: json_str(params, "task"),
            starter_prompt: Default::default(),
            task_id: json_str(params, "task_id"),
            task_launch_authority: None,
            remote_task_binding: None,
            plan_skill_manifest: None,
            plan_task: json_str(params, "plan_task").or_else(|| json_str(params, "plan-task")),
            authorized_gates: json_string_list(params, "authorized_gates"),
            execution_intent: params
                .get("execution_intent")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            provider_reserve_pct: params
                .get("provider_reserve_pct")
                .and_then(|value| value.as_f64())
                .map(|value| value as f32),
            workspace_id: json_str(params, "workspace_id"),
            force_tab: params.get("force_tab").and_then(|v| v.as_bool()),
            existing_tab_id: json_str(params, "existing_tab_id"),
            skip_orchestrator_onboarding: None,
            report_target_tab_id: json_str(params, "report_to")
                .or_else(|| json_str(params, "report_target_tab_id")),
            report_target_mesh_peer: json_str(params, "origin_peer")
                .or_else(|| json_str(params, "report_target_mesh_peer")),
            skip_permissions: None,
            permission_bypass_authorization: None,
            spawned_by_tab_id: json_str(params, "spawned_by_tab_id"),
            team_id: json_str(params, "team_id"),
            creator_tab_id: json_str(params, "creator_tab_id"),
            ownership: params
                .get("ownership")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            model: json_str(params, "model"),
            planned_model: json_str(params, "planned_model"),
            planned_reasoning_effort: None,
            reasoning_effort: json_str(params, "reasoning_effort")
                .or_else(|| json_str(params, "reasoning-effort")),
            system_prompt: json_str(params, "system_prompt").or_else(|| json_str(params, "system")),
            preset: json_str(params, "preset"),
            resume_session_id: json_str(params, "resume_session_id"),
            charter: json_str(params, "charter"),
            tick_interval_secs: json_u64(params, "tick_interval_secs"),
            nudge_cooldown_secs: json_u64(params, "nudge_cooldown_secs"),
            max_actions_per_tick: json_u64(params, "max_actions_per_tick")
                .and_then(|v| u32::try_from(v).ok()),
            activate: params.get("activate").and_then(|v| v.as_bool()),
            audience: params
                .get("audience")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            manager_bootstrap: None,
            structured_prompt_server: None,
        };
        out.normalize_role_profile()?;
        out.starter_prompt = crate::service::starter_prompt_for_role(out.role.as_deref());
        Ok(out)
    }
}

fn parse_spawn_identity_provenance(value: &str) -> Result<SpawnIdentityProvenance, String> {
    match value.trim() {
        "user_supplied" => Ok(SpawnIdentityProvenance::UserSupplied),
        "suggested" => Ok(SpawnIdentityProvenance::Suggested),
        other => Err(format!(
            "invalid title_provenance '{other}'; expected user_supplied|suggested"
        )),
    }
}

pub(super) fn normalize_spawn_params(params: &mut SpawnAgentParams) -> Result<(), String> {
    params.role = params
        .role
        .take()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    params.role_profile = params
        .role_profile
        .take()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let role = params
        .role
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let profile = params
        .role_profile
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match role {
        Some("Planner") | Some("planner") => {
            params.role = Some("Manager".into());
            params.role_profile = Some(PLANNER.into());
        }
        Some("Watcher") | Some("watcher") => {
            params.role = Some("Manager".into());
            params.role_profile = Some(WATCHER.into());
        }
        Some("Manager") => {
            params.role = Some("Manager".into());
            match profile {
                Some("planner" | "Planner") => params.role_profile = Some(PLANNER.into()),
                Some("watcher" | "Watcher") => params.role_profile = Some(WATCHER.into()),
                Some(other) => params.role_profile = Some(normalize_manager_profile(other)?),
                None => return Err("Manager requires a named role_profile".into()),
            }
        }
        // Sugar resolves here, once: core code branches on the kind, never on
        // the wire literal.
        Some("Orchestrator" | "Conductor") => {
            params.role = Some("Manager".into());
            if profile.is_some_and(|profile| profile != ORCHESTRATOR) {
                log::warn!("[spawn] ignoring role_profile on Orchestrator spawn");
            }
            params.role_profile = Some(ORCHESTRATOR.into());
        }
        Some("Worker") => {
            params.role = Some("Worker".into());
            params.role_profile = profile.map(normalize_worker_profile).transpose()?;
        }
        Some(other) => {
            return Err(format!(
                "Invalid role: {other}. Use: Orchestrator, Manager, Worker, Planner, Watcher"
            ));
        }
        None => {
            if profile.is_some() {
                return Err(format!(
                    "role_profile {:?} requires role Manager (or role Planner|Watcher sugar)",
                    profile
                ));
            }
            params.role_profile = None;
        }
    }
    Ok(())
}

/// The Worker subtype a task's executor role names, when it names a builtin
/// leaf kind. Only a builtin leaf is promoted: a free-text executor role stays a
/// plain Worker, and a managing kind (Planner, Watcher, Cartographer) is never
/// demoted into a Worker subtype.
pub(crate) fn builtin_worker_profile(executor_role: Option<&str>) -> Option<String> {
    let role = executor_role
        .map(str::trim)
        .filter(|role| !role.is_empty())?;
    let descriptor = role_kind::builtin_kind(&role.to_ascii_lowercase())?;
    (!descriptor.anchors_team() && !descriptor.has_team_authority() && !descriptor.manages())
        .then(|| descriptor.id().to_string())
}

/// A Worker subtype narrows within leaf capability, so a subtype id is kept
/// verbatim — overlay kinds are hyphenated and must still match their file.
/// A managing kind's id is refused here rather than silently downgraded, so a
/// caller never believes it spawned a Planner.
fn normalize_worker_profile(profile: &str) -> Result<String, String> {
    let profile = profile.trim();
    if role_kind::kind_id(Some(AgentRole::Worker), Some(profile)) != Some(profile) {
        return Err(format!(
            "Worker does not accept role_profile {profile:?}; use role Manager for planner|watcher|cartographer"
        ));
    }
    Ok(profile.to_string())
}

fn normalize_manager_profile(profile: &str) -> Result<String, String> {
    let normalized = profile.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    if normalized.is_empty()
        || !normalized
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(format!("Invalid Manager role_profile '{profile}'"));
    }
    Ok(normalized)
}

fn json_u64(params: &serde_json::Value, key: &str) -> Option<u64> {
    params.get(key).and_then(|v| v.as_u64())
}

fn json_string_list(params: &serde_json::Value, key: &str) -> Vec<String> {
    params
        .get(key)
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod normalization_tests {
    use codeterm_types::agent::role_kind::builtins::{CARTOGRAPHER, GENERAL_AGENT};

    use super::*;
    use crate::service::orchestrator::SpawnAgentParams;

    fn normalized(role: &str, profile: Option<&str>) -> (Option<String>, Option<String>) {
        let mut params = SpawnAgentParams {
            role: Some(role.into()),
            role_profile: profile.map(str::to_string),
            ..Default::default()
        };
        params.normalize_role_profile().expect("normalizes");
        (params.role, params.role_profile)
    }

    /// Role sugar resolves exactly once, here. Core code never sees the literal.
    #[test]
    fn orchestrator_sugar_resolves_to_the_manager_kind_pair() {
        for literal in ["Orchestrator", "Conductor"] {
            assert_eq!(
                normalized(literal, None),
                (Some("Manager".into()), Some(ORCHESTRATOR.into())),
                "{literal}"
            );
        }
    }

    #[test]
    fn planner_and_watcher_sugar_resolve_to_their_kinds() {
        assert_eq!(
            normalized("Planner", None),
            (Some("Manager".into()), Some(PLANNER.into()))
        );
        assert_eq!(
            normalized("Watcher", None),
            (Some("Manager".into()), Some(WATCHER.into()))
        );
    }

    /// The wire seam for a verification Worker's command scope: the CLI sends
    /// `authorized_gates` as an array, and blank or non-string entries are
    /// dropped rather than becoming a gate token nothing can match.
    #[test]
    fn authorized_gates_survive_the_spawn_wire_and_drop_empty_entries() {
        let params = SpawnAgentParams::from_json(&serde_json::json!({
            "agent": "codex",
            "role": "Worker",
            "execution_intent": "read_only",
            "authorized_gates": ["  ./build.sh verify  ", "", 7, "./build.sh test-one app x"],
        }))
        .expect("a read-only Worker spawn parses");
        assert_eq!(
            params.authorized_gates,
            vec![
                "./build.sh verify".to_string(),
                "./build.sh test-one app x".to_string()
            ]
        );

        let none = SpawnAgentParams::from_json(&serde_json::json!({"agent": "codex"}))
            .expect("a plain spawn parses");
        assert!(none.authorized_gates.is_empty());
    }

    #[test]
    fn no_normalized_role_is_ever_an_orchestrator_literal() {
        for literal in ["Orchestrator", "Conductor", "Planner", "Watcher", "Worker"] {
            let (role, _) = normalized(literal, None);
            assert_ne!(role.as_deref(), Some("Orchestrator"), "{literal}");
            assert_ne!(role.as_deref(), Some("Conductor"), "{literal}");
        }
    }

    /// The Cartographer is a Manager kind now, so a Worker naming it is refused
    /// rather than quietly demoted, and Manager normalization keeps it.
    #[test]
    fn cartographer_normalizes_as_a_manager_not_a_worker_subtype() {
        assert_eq!(
            normalized("Manager", Some("cartographer")),
            (Some("Manager".into()), Some(CARTOGRAPHER.into()))
        );
        assert_eq!(
            normalized("Manager", Some("Cartographer")),
            (Some("Manager".into()), Some(CARTOGRAPHER.into())),
            "the profile is normalized to its kind id"
        );
        let mut worker = SpawnAgentParams {
            role: Some("Worker".into()),
            role_profile: Some(CARTOGRAPHER.into()),
            ..Default::default()
        };
        assert!(
            worker.normalize_role_profile().is_err(),
            "a Worker cannot take a managing kind as its subtype"
        );

        assert_eq!(
            normalized("Worker", Some("rust-backend-dev")),
            (Some("Worker".into()), Some("rust-backend-dev".into())),
            "an overlay kind id is kept verbatim so it still matches its file"
        );
        assert_eq!(normalized("Worker", None), (Some("Worker".into()), None));
        for stolen in [PLANNER, WATCHER, ORCHESTRATOR, GENERAL_AGENT, CARTOGRAPHER] {
            let mut params = SpawnAgentParams {
                role: Some("Worker".into()),
                role_profile: Some(stolen.into()),
                ..Default::default()
            };
            assert!(params.normalize_role_profile().is_err(), "{stolen}");
        }
    }

    /// Dispatch promotes a task's executor role to a role kind only when that
    /// role names a builtin leaf; a managing kind stays a plain Worker and is
    /// attached through the Orchestrator instead.
    #[test]
    fn cartographer_map_executor_role_promotes_only_a_builtin_leaf() {
        for free_text in [
            "cartographer",
            "  Cartographer  ",
            "rust-platform-dev",
            "prompt-contract-editor",
            "",
            "   ",
        ] {
            assert_eq!(builtin_worker_profile(Some(free_text)), None, "{free_text}");
        }
        for managing in [PLANNER, WATCHER, ORCHESTRATOR, GENERAL_AGENT, CARTOGRAPHER] {
            assert_eq!(builtin_worker_profile(Some(managing)), None, "{managing}");
        }
        assert_eq!(builtin_worker_profile(None), None);
    }

    #[test]
    fn a_normalized_orchestrator_spawn_reports_as_an_anchor() {
        let mut params = SpawnAgentParams {
            role: Some("Orchestrator".into()),
            ..Default::default()
        };
        params.normalize_role_profile().unwrap();
        assert!(params.spawns_anchor());
        assert_eq!(params.role_kind(), Some(ORCHESTRATOR));
    }
}
