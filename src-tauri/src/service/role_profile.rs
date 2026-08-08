use codeterm_types::agent::role_kind::builtins::{ORCHESTRATOR, PLANNER, WATCHER};

use super::{json_str, orchestrator::SpawnAgentParams};
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
            lane_setup_command: None,
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
            plan_skill_manifest: None,
            plan_task: json_str(params, "plan_task").or_else(|| json_str(params, "plan-task")),
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
            if profile.is_some() {
                return Err(format!(
                    "Worker does not accept role_profile {:?}; use role Manager for planner|watcher",
                    profile
                ));
            }
            params.role_profile = None;
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

#[cfg(test)]
mod normalization_tests {
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

    #[test]
    fn no_normalized_role_is_ever_an_orchestrator_literal() {
        for literal in ["Orchestrator", "Conductor", "Planner", "Watcher", "Worker"] {
            let (role, _) = normalized(literal, None);
            assert_ne!(role.as_deref(), Some("Orchestrator"), "{literal}");
            assert_ne!(role.as_deref(), Some("Conductor"), "{literal}");
        }
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
