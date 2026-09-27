//! Paired creation (task_7e5f9354): spawn an Orchestrator + Planner together,
//! linked from birth by tab hierarchy; the plan is created lazily on the
//! Orchestrator's first goal/plan create. Builds entirely on the
//! planner-attachment primitives already wired by the peer-foundation
//! (`crate::plan::peer`) and Lane 1 (`crate::service::plan::peer_mandate`,
//! Manager + planner binding in `Self::spawn_agent`) - this module
//! only sequences the two spawns and records the pending pair.

use codeterm_types::agent::role_kind::builtins::{ORCHESTRATOR, PLANNER};
use codeterm_types::team::general_agent_membership::{CorrectiveShape, MembershipAttempt};
use serde::{Deserialize, Serialize};

use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::orchestrator::SpawnAgentParams;
use crate::service::startup_lease::StartupTeamLease;
use crate::service::ServiceContext;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OrchestrationPairParams {
    pub orchestrator_agent: String,
    pub orchestrator_model: Option<String>,
    /// Requested reasoning effort for the Orchestrator. Capability-validated
    /// against the provider at spawn; unsupported values are rejected.
    #[serde(default)]
    pub orchestrator_reasoning_effort: Option<String>,
    pub planner_agent: String,
    pub planner_model: Option<String>,
    /// Requested reasoning effort for the Planner (same validation as above).
    #[serde(default)]
    pub planner_reasoning_effort: Option<String>,
    pub workspace_id: Option<String>,
    pub spawned_by_tab_id: Option<String>,
    /// Blank picker tab to claim for the Orchestrator in place, instead of
    /// spawning it as a brand-new tab (new-session picker confirm path).
    pub existing_tab_id: Option<String>,
    /// Explicit "I know I am a leaf and I am doing this deliberately"
    /// declaration — codeterm#400. A caller tab holding a leaf role (Manager
    /// or Worker) is refused unless this is set; it never widens who may
    /// create a pair from a non-leaf tab, and it never substitutes an
    /// intermediary — the newly minted Team is still attributed to the
    /// declaring tab itself.
    #[serde(default)]
    pub leaf_creation_acknowledged: bool,
}

/// Success shape only — a failed Planner spawn returns `Err(String)` with the
/// exact recovery command embedded (see `create_orchestration_pair`), never a
/// half-populated `Ok`. There is no silent half-pair to represent here.
#[derive(Debug, Clone, Serialize)]
pub struct OrchestrationPairResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    pub orchestrator_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestrator_model: Option<String>,
    pub planner_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Advisory tab-budget nudge (task_4d649dfd) for the workspace AFTER both
    /// tabs land — a pair adds two, so its count is the authoritative one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_notice: Option<crate::service::tab_budget::TabBudgetNotice>,
}

fn pair_retry_command(
    workspace_id: &str,
    orchestrator_agent: &str,
    orchestrator_model: Option<&str>,
    planner_agent: &str,
    planner_model: Option<&str>,
) -> String {
    let provider_spec = |provider: &str, model: Option<&str>| match model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        Some(model) => format!("{provider}:{model}"),
        None => provider.to_string(),
    };
    format!(
        "codeterm team pair --orchestrator {} --planner {} --workspace {}",
        provider_spec(orchestrator_agent, orchestrator_model),
        provider_spec(planner_agent, planner_model),
        workspace_id
    )
}

fn explicit_pair_model(model: Option<String>) -> Option<String> {
    model.filter(|model| !model.trim().is_empty())
}

fn derive_planner_name<'a>(
    existing_titles: impl IntoIterator<Item = &'a str>,
    orch_title: &str,
) -> String {
    let mut occupied: Vec<String> = existing_titles.into_iter().map(str::to_string).collect();
    occupied.push(orch_title.to_string());
    occupied.push("Planner".to_string());
    for _ in 0..8 {
        let candidate = crate::multiplexer::state::names::generate_unique_tab_name(
            occupied.iter().map(String::as_str),
        );
        if !candidate.to_ascii_lowercase().starts_with("planner") {
            return candidate;
        }
        occupied.push(candidate);
    }
    "Specialist".to_string()
}

fn is_pair_creation_leaf(tab: &crate::multiplexer::types::Tab) -> bool {
    matches!(
        tab.role,
        Some(crate::multiplexer::types::AgentRole::Manager)
            | Some(crate::multiplexer::types::AgentRole::Worker)
    ) && !crate::service::general_agent::is_general_agent(tab)
}

/// The exact command a refused leaf caller runs to create the pair
/// deliberately, attributed to its own tab — never to a throwaway
/// intermediary shell.
const LEAF_PAIR_CREATION_FIX_COMMAND: &str = "codeterm team pair --acknowledge-leaf ...";

fn leaf_pair_creation_refusal() -> String {
    format!(
        "Manager and Worker Tabs need an explicit declaration to create orchestration pairs: \
         pass --acknowledge-leaf to create it attributed to this tab, e.g. `{LEAF_PAIR_CREATION_FIX_COMMAND}`."
    )
}

/// codeterm#400: gated on STATED INTENT, not a held capability. No grant for
/// this exists today, and gating on one would leave the workaround this
/// replaces open for every caller without the grant — a leaf still spawns an
/// anonymous throwaway shell and creates the pair from there, attributed to
/// that intermediary instead of the real caller. An explicit flag both proves
/// the caller's awareness and is what lets attribution land on the caller's
/// own tab; a held capability could not do the latter without also being
/// granted to every leaf that would otherwise take the workaround.
fn pair_creation_refusal_for_leaf(
    is_leaf: bool,
    leaf_creation_acknowledged: bool,
) -> Option<String> {
    (is_leaf && !leaf_creation_acknowledged).then(leaf_pair_creation_refusal)
}

fn validate_pair_creator(
    mux: &crate::multiplexer::state::MultiplexerState,
    creator_tab_id: Option<&str>,
    leaf_creation_acknowledged: bool,
) -> Result<(), String> {
    let Some(creator_tab_id) = creator_tab_id else {
        return Ok(());
    };
    let creator_tab_id = creator_tab_id.trim();
    if creator_tab_id.is_empty() {
        return Err("spawned_by_tab_id must identify the calling Tab".into());
    }
    let tab = mux
        .get_tab(creator_tab_id)
        .ok_or_else(|| format!("calling Tab '{creator_tab_id}' does not exist"))?;
    if let Some(refusal) =
        pair_creation_refusal_for_leaf(is_pair_creation_leaf(tab), leaf_creation_acknowledged)
    {
        return Err(refusal);
    }
    Ok(())
}

fn validate_pair_provider(
    service: &ServiceContext,
    provider: &str,
    role: &str,
    role_profile: Option<&str>,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Result<(), String> {
    let spawn_params = SpawnAgentParams {
        agent: Some(provider.to_string()),
        model: model.map(str::to_string),
        reasoning_effort: reasoning_effort.map(str::to_string),
        role: Some(role.to_string()),
        role_profile: role_profile.map(str::to_string),
        ..Default::default()
    };
    if service.classify_spawn_target_for_params(provider, &spawn_params)? {
        return Ok(());
    }
    crate::service::spawn::validate_agent_model(&service.agent_registry, provider, model)
        .map_err(|error| error.to_string())?;
    crate::service::spawn::validate_agent_reasoning_effort(
        &service.agent_registry,
        provider,
        model,
        reasoning_effort,
    )
}

#[derive(Debug)]
struct ReservedPairTabs {
    orchestrator_tab_id: String,
    planner_tab_id: String,
    orchestrator_tab_created: bool,
    planner_tab_created: bool,
    claimed_orchestrator_before: Option<crate::multiplexer::types::Tab>,
    claimed_orchestrator_pad_existed: bool,
    planner_name: String,
    team: crate::service::spawn_factory::TeamIdentity,
    team_minted: bool,
}

/// A newly minted pair Team dies with the pair; a Team the claimed Tab already
/// belonged to is adopted, so it is never leased and never reaped.
fn pair_startup_lease(
    creator_tab_id: &str,
    reserved: &ReservedPairTabs,
) -> Option<StartupTeamLease> {
    reserved.team_minted.then(|| StartupTeamLease {
        team_id: reserved.team.id.clone(),
        creator_tab_id: creator_tab_id.to_string(),
        member_tab_ids: reserved_member_tab_ids(reserved),
    })
}

fn release_minted_pair_team(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    reserved: &ReservedPairTabs,
) {
    if !reserved.team_minted {
        return;
    }
    crate::service::startup_lease::release_owned_reservation(
        mux,
        &reserved.team.id,
        &reserved_member_tab_ids(reserved),
    );
}

fn reserved_member_tab_ids(reserved: &ReservedPairTabs) -> Vec<String> {
    vec![
        reserved.orchestrator_tab_id.clone(),
        reserved.planner_tab_id.clone(),
    ]
}

fn created_member_tab_ids(reserved: &ReservedPairTabs) -> Vec<String> {
    [
        (
            reserved.orchestrator_tab_created,
            &reserved.orchestrator_tab_id,
        ),
        (reserved.planner_tab_created, &reserved.planner_tab_id),
    ]
    .into_iter()
    .filter(|(created, _)| *created)
    .map(|(_, tab_id)| tab_id.clone())
    .collect()
}

fn reserve_pair_tabs(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    workspace_id: &str,
    existing_orchestrator_tab_id: Option<&str>,
    claimed_orchestrator_pad_existed: bool,
    shape: &CorrectiveShape,
) -> Result<ReservedPairTabs, String> {
    // Membership door 1: claiming a Tab as the pair Orchestrator binds a Team
    // onto it. Guarded before the claim, so a refusal reserves nothing.
    if let Some(tab_id) = existing_orchestrator_tab_id {
        crate::service::general_agent::refuse_general_agent_team_membership(
            mux,
            tab_id,
            MembershipAttempt::Join,
            shape,
        )?;
    }
    let claimed_orchestrator_before =
        existing_orchestrator_tab_id.and_then(|tab_id| mux.get_tab(tab_id).cloned());
    let orchestrator_tab_created = existing_orchestrator_tab_id.is_none();
    let mut created_tab_ids = Vec::new();
    let result = (|| {
        let orchestrator_tab_id = match existing_orchestrator_tab_id {
            Some(tab_id) => mux.claim_existing_tab(workspace_id, tab_id)?,
            None => {
                let tab_id = mux.create_tab(workspace_id)?;
                created_tab_ids.push(tab_id.clone());
                tab_id
            }
        };
        let orchestrator_label = mux.display_label_for_tab(&orchestrator_tab_id);
        let existing_titles: Vec<String> = mux
            .get_workspace(workspace_id)
            .into_iter()
            .flat_map(|workspace| workspace.tabs.iter())
            .filter_map(|tab_id| mux.get_tab(tab_id).map(|tab| tab.title.clone()))
            .collect();
        let planner_name = derive_planner_name(
            existing_titles.iter().map(String::as_str),
            &orchestrator_label,
        );
        let planner_tab_id = mux.create_tab(workspace_id)?;
        created_tab_ids.push(planner_tab_id.clone());
        mux.rename_tab_at_level(
            &planner_tab_id,
            &planner_name,
            crate::multiplexer::types::TitleLevel::User,
        )?;
        for tab_id in [&orchestrator_tab_id, &planner_tab_id] {
            if let Some(tab) = mux.get_tab_mut(tab_id) {
                tab.agent_booting = true;
                crate::service::boot_stage::apply_boot_stage(
                    tab,
                    codeterm_types::pty::BootStage::PtyStarting,
                    None,
                );
            }
        }
        let reservation = mux.resolve_or_mint_team_for_parent(&orchestrator_tab_id)?;
        if let Some(tab) = mux.get_tab_mut(&planner_tab_id) {
            tab.team_id = Some(reservation.team_id.clone());
        }
        let team_name = mux
            .get_team(&reservation.team_id)
            .and_then(|team| team.name.clone());
        Ok(ReservedPairTabs {
            orchestrator_tab_id,
            planner_tab_id,
            orchestrator_tab_created,
            planner_tab_created: true,
            claimed_orchestrator_before: claimed_orchestrator_before.clone(),
            claimed_orchestrator_pad_existed,
            planner_name,
            team: crate::service::spawn_factory::TeamIdentity::new(
                reservation.team_id,
                team_name.as_deref(),
            ),
            team_minted: reservation.minted,
        })
    })();
    if result.is_err() {
        for tab_id in created_tab_ids {
            mux.rollback_spawn_tab(workspace_id, &tab_id);
        }
        if let (Some(tab_id), Some(previous)) =
            (existing_orchestrator_tab_id, claimed_orchestrator_before)
        {
            if let Some(tab) = mux.get_tab_mut(tab_id) {
                *tab = previous;
            }
        }
    }
    result
}

fn restore_claimed_pair_tab(
    service: &ServiceContext,
    tab_id: &str,
    previous: &crate::multiplexer::types::Tab,
    pad_existed: bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    let (provider_id, pty_id) = service
        .mux
        .read()
        .get_tab(tab_id)
        .map(|tab| (tab.agent_type.clone(), tab.pty_id.clone()))
        .unwrap_or_default();
    service.cancel_agent_startup_supervision(tab_id);
    crate::service::launch_session_detection::clear_launch_marker(tab_id);
    service.chat_registry.shutdown_tab(tab_id);
    service.deferred_session_bindings.clear(tab_id);
    if let Some((plugin_id, session_id)) = service.tab_chat_backing(tab_id) {
        if let Err(error) = service.chatbackend_close(&plugin_id, &session_id) {
            errors.push(format!("claimed Tab chat session cleanup failed: {error}"));
        }
    }
    if let Some(provider_id) = provider_id {
        service.release_tab_launch_artifacts(tab_id, &provider_id);
    }
    if let Some(pty_id) = pty_id.filter(|pty_id| Some(pty_id) != previous.pty_id.as_ref()) {
        service.kill_ptys(&[pty_id]);
    }
    if !pad_existed {
        if let Err(error) = service.revoke_agent_pad(tab_id) {
            errors.push(format!("claimed Tab AgentPad cleanup failed: {error}"));
        }
    }
    if let Some(tab) = service.mux.write().get_tab_mut(tab_id) {
        *tab = previous.clone();
    } else {
        errors.push(format!(
            "claimed Tab {tab_id} disappeared during pair rollback"
        ));
    }
    service.emit_state_changed();
    errors
}

fn rollback_pair_tabs(service: &ServiceContext, reserved: &ReservedPairTabs) -> Vec<String> {
    let mut cleanup_errors = Vec::new();
    for (tab_id, created) in [
        (&reserved.planner_tab_id, reserved.planner_tab_created),
        (
            &reserved.orchestrator_tab_id,
            reserved.orchestrator_tab_created,
        ),
    ] {
        if !created {
            if let Some(previous) = reserved.claimed_orchestrator_before.as_ref() {
                cleanup_errors.extend(restore_claimed_pair_tab(
                    service,
                    tab_id,
                    previous,
                    reserved.claimed_orchestrator_pad_existed,
                ));
            } else {
                cleanup_errors.push(format!("claimed Tab {tab_id} has no saved pre-pair state"));
            }
            continue;
        }
        if service.mux.read().get_tab(tab_id).is_none() {
            service.cancel_agent_startup_supervision(tab_id);
            continue;
        }
        let cleanup = service.rollback_failed_agent_spawn(tab_id, "pair creation rolled back");
        if cleanup != "pair creation rolled back" {
            cleanup_errors.push(cleanup);
        }
    }
    {
        let mut mux = service.mux.write();
        release_minted_pair_team(&mut mux, reserved);
    }
    service.forget_pair_startup_lease(&reserved.team.id);
    let mux = service.mux.read();
    let remaining_tabs: Vec<_> = created_member_tab_ids(reserved)
        .into_iter()
        .filter(|tab_id| mux.get_tab(tab_id).is_some())
        .collect();
    if !remaining_tabs.is_empty() {
        cleanup_errors.push(format!(
            "Tabs remain after pair rollback: {}",
            remaining_tabs.join(", ")
        ));
    }
    if reserved.team_minted && mux.get_team(&reserved.team.id).is_some() {
        cleanup_errors.push(format!(
            "Team {} remains after pair rollback",
            reserved.team.id
        ));
    }
    if !reserved.orchestrator_tab_created && mux.get_tab(&reserved.orchestrator_tab_id).is_none() {
        cleanup_errors.push(format!(
            "claimed Tab {} is missing after pair rollback",
            reserved.orchestrator_tab_id
        ));
    }
    drop(mux);
    crate::service::session_persist::persist_session_after_close(service);
    service.emit_state_changed();
    cleanup_errors
}

impl ServiceContext {
    /// Bind a pending pair-spawned Planner when an Orchestrator creates its
    /// first plan. Returns true only when a pending tab was consumed.
    pub(crate) fn bind_pending_paired_planner(
        &self,
        plan_id: &str,
        owner: &OrchestratorRef,
        now_ms: u64,
    ) -> Result<bool, String> {
        let Some(orch_tab_id) = owner.tab_id.as_deref().filter(|p| !p.is_empty()) else {
            return Ok(false);
        };
        let planner_tab_id = {
            let mut mux = self.mux.write();
            let Some(orch) = mux.get_tab_mut(orch_tab_id) else {
                return Ok(false);
            };
            orch.pending_paired_planner_tab_id.take()
        };
        let Some(planner_tab_id) = planner_tab_id else {
            return Ok(false);
        };
        self.plan_store
            .set_planner_ref(
                plan_id,
                Some(PlannerRef::Tab {
                    tab_id: planner_tab_id.clone(),
                    session_id: None,
                    title: None,
                }),
                now_ms,
            )
            .map_err(|e| e.to_string())?;
        log::info!(
            "[orchestration-pair] bound pending planner {planner_tab_id} to first plan {plan_id} for orchestrator {orch_tab_id}"
        );
        Ok(true)
    }

    pub(crate) fn tab_has_pending_paired_planner(&self, tab_id: &str) -> bool {
        self.mux
            .read()
            .get_tab(tab_id)
            .and_then(|p| p.pending_paired_planner_tab_id.as_deref())
            .map(|p| !p.is_empty())
            .unwrap_or(false)
    }

    /// Create an Orchestrator + Planner pair in one action. No plan is created
    /// at spawn time; the Orchestrator's first goal/plan create claims a new
    /// plan and binds the already-parented Planner. If the Planner spawn fails,
    /// no pending planner is recorded and the error names the exact recovery
    /// command — never a silent half-pair.
    pub async fn create_orchestration_pair(
        &self,
        params: OrchestrationPairParams,
    ) -> Result<OrchestrationPairResult, String> {
        if params.orchestrator_agent.trim().is_empty() {
            return Err("orchestrator_agent is required".into());
        }
        if params.planner_agent.trim().is_empty() {
            return Err("planner_agent is required".into());
        }

        let startup_supervisor = params.spawned_by_tab_id.as_ref().and_then(|tab_id| {
            self.mux
                .read()
                .get_tab(tab_id)
                .filter(|tab| crate::service::worker_ack::is_agent_spawner(tab))
                .map(|_| tab_id.clone())
        });

        // The General Agent is a machine coordinator represented by Manager storage, not a Team leaf, so it is exempt regardless of the flag.
        validate_pair_creator(
            &self.mux.read(),
            params.spawned_by_tab_id.as_deref(),
            params.leaf_creation_acknowledged,
        )?;

        let workspace_id = {
            let mux = self.mux.write();
            params
                .workspace_id
                .clone()
                .or_else(|| mux.get_active_workspace().map(|w| w.id.clone()))
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or_else(|| "No workspace found".to_string())?
        };
        let project = self
            .mux
            .read()
            .get_workspace(&workspace_id)
            .map(|workspace| workspace.name.clone())
            .unwrap_or_else(|| workspace_id.clone());

        let orchestrator_model = explicit_pair_model(params.orchestrator_model.clone());
        let planner_model = explicit_pair_model(params.planner_model.clone());
        validate_pair_provider(
            self,
            &params.orchestrator_agent,
            "Orchestrator",
            None,
            orchestrator_model.as_deref(),
            params.orchestrator_reasoning_effort.as_deref(),
        )?;
        validate_pair_provider(
            self,
            &params.planner_agent,
            "Manager",
            Some("planner"),
            planner_model.as_deref(),
            params.planner_reasoning_effort.as_deref(),
        )?;
        let claimed_orchestrator_pad_existed = if let Some(tab_id) = params.existing_tab_id.clone()
        {
            tokio::task::spawn_blocking(move || {
                crate::service::agent_pad::AgentPad::open(&crate::paths::data_dir(), &tab_id)
                    .is_ok()
            })
            .await
            .map_err(|error| format!("failed to inspect claimed Tab AgentPad: {error}"))?
        } else {
            false
        };
        let pair_fix_command = pair_retry_command(
            &workspace_id,
            &params.orchestrator_agent,
            orchestrator_model.as_deref(),
            &params.planner_agent,
            planner_model.as_deref(),
        );

        let membership_shape = CorrectiveShape::in_workspace(Some(&workspace_id)).with_agents(
            Some(&params.orchestrator_agent),
            Some(&params.planner_agent),
        );
        let reserved = {
            let mut mux = self.mux.write();
            reserve_pair_tabs(
                &mut mux,
                &workspace_id,
                params.existing_tab_id.as_deref(),
                claimed_orchestrator_pad_existed,
                &membership_shape,
            )?
        };
        let orchestrator_label = self
            .mux
            .read()
            .display_label_for_tab(&reserved.orchestrator_tab_id);
        let pair_lease = startup_supervisor
            .as_deref()
            .and_then(|creator_tab_id| pair_startup_lease(creator_tab_id, &reserved));
        if let Some(lease) = pair_lease {
            self.register_pair_startup_lease(
                lease,
                &[
                    (
                        reserved.orchestrator_tab_id.as_str(),
                        orchestrator_label.as_str(),
                    ),
                    (
                        reserved.planner_tab_id.as_str(),
                        reserved.planner_name.as_str(),
                    ),
                ],
            );
        } else if let Some(supervisor_tab_id) = startup_supervisor.as_deref() {
            self.register_agent_startup_supervision(
                supervisor_tab_id,
                &reserved.orchestrator_tab_id,
                &orchestrator_label,
            );
            self.register_agent_startup_supervision(
                supervisor_tab_id,
                &reserved.planner_tab_id,
                &reserved.planner_name,
            );
        }
        let orch_counterpart = crate::service::spawn_factory::Counterpart::new(
            "Orchestrator",
            &orchestrator_label,
            &reserved.orchestrator_tab_id,
        );
        let planner_counterpart = crate::service::spawn_factory::Counterpart::new(
            "Planner",
            &reserved.planner_name,
            &reserved.planner_tab_id,
        );
        let mut orchestrator_bootstrap = crate::service::spawn_factory::forming_identity_in_team(
            "Orchestrator",
            ORCHESTRATOR,
            project.clone(),
            &reserved.team,
        );
        orchestrator_bootstrap.coordination =
            Some(crate::service::spawn_factory::introduction_body(
                &orch_counterpart,
                &planner_counterpart,
            ));
        let orch_params = SpawnAgentParams {
            agent: Some(params.orchestrator_agent.clone()),
            model: orchestrator_model.clone(),
            reasoning_effort: params
                .orchestrator_reasoning_effort
                .clone()
                .filter(|e| !e.trim().is_empty()),
            role: Some("Orchestrator".into()),
            can_spawn: Some(true),
            workspace_id: Some(workspace_id.clone()),
            spawned_by_tab_id: params.spawned_by_tab_id.clone(),
            task: None,
            manager_bootstrap: Some(orchestrator_bootstrap),
            existing_tab_id: Some(reserved.orchestrator_tab_id.clone()),
            skip_orchestrator_onboarding: Some(true),
            ..Default::default()
        };
        let orch_result = match self.spawn_agent(orch_params, None).await {
            Ok(result) => result,
            Err(error) => {
                let cleanup_errors = rollback_pair_tabs(self, &reserved);
                let cleanup = if cleanup_errors.is_empty() {
                    "Both reserved Tabs were rolled back.".to_string()
                } else {
                    format!(
                        "Pair rollback was incomplete: {}.",
                        cleanup_errors.join("; ")
                    )
                };
                return Err(format!(
                    "Failed to spawn Orchestrator ({error}). {cleanup} Retry with: {pair_fix_command}"
                ));
            }
        };
        let orch_tab_id = orch_result.tab_id.clone();

        let mut planner_bootstrap = crate::service::spawn_factory::forming_identity_in_team(
            "Planner",
            PLANNER,
            project,
            &reserved.team,
        );
        planner_bootstrap.coordination = Some(crate::service::spawn_factory::introduction_body(
            &planner_counterpart,
            &orch_counterpart,
        ));
        let planner_params = SpawnAgentParams {
            agent: Some(params.planner_agent.clone()),
            model: planner_model.clone(),
            reasoning_effort: params
                .planner_reasoning_effort
                .clone()
                .filter(|e| !e.trim().is_empty()),
            title: Some(reserved.planner_name.clone()),
            role: Some("Manager".into()),
            role_profile: Some("planner".into()),
            parent_tab_id: Some(orch_tab_id.clone()),
            plan_id: None,
            workspace_id: Some(workspace_id.clone()),
            spawned_by_tab_id: Some(orch_tab_id.clone()),
            task: None,
            manager_bootstrap: Some(planner_bootstrap),
            existing_tab_id: Some(reserved.planner_tab_id.clone()),
            ..Default::default()
        };

        match self.spawn_agent(planner_params, None).await {
            Ok(planner_result) => {
                let planner_tab_id = planner_result.tab_id.clone();
                if let Some(tab) = self.mux.write().get_tab_mut(&orch_tab_id) {
                    tab.pending_paired_planner_tab_id = Some(planner_tab_id.clone());
                }
                // Activate server-side: the client continuation may be gone by
                // the time the pair lands (page reload); the daemon owns focus.
                {
                    let mut mux = self.mux.write();
                    let orch_tab = mux
                        .get_workspace(&workspace_id)
                        .and_then(|ws| ws.tabs.iter().find(|tab_id| *tab_id == &orch_tab_id))
                        .cloned();
                    let _ = mux.set_active_workspace(&workspace_id);
                    if let Some(tab_id) = orch_tab {
                        let _ = mux.set_active_tab(&workspace_id, &tab_id);
                    }
                }
                let _ = self.focus_tab(&orch_tab_id);
                self.emit_state_changed();
                // final count is the authoritative one for the nudge.
                let budget_notice = self.workspace_tab_budget_notice(&workspace_id);
                Ok(OrchestrationPairResult {
                    plan_id: None,
                    orchestrator_tab_id: orch_tab_id,
                    orchestrator_model,
                    planner_tab_id,
                    planner_model,
                    warning: crate::service::spawn::merge_spawn_warnings(
                        orch_result.warning,
                        planner_result.warning,
                    ),
                    budget_notice,
                })
            }
            Err(error) => {
                let cleanup_errors = rollback_pair_tabs(self, &reserved);
                let cleanup = if cleanup_errors.is_empty() {
                    "Both reserved Tabs were rolled back.".to_string()
                } else {
                    format!(
                        "Pair rollback was incomplete: {}.",
                        cleanup_errors.join("; ")
                    )
                };
                Err(format!(
                    "Failed to spawn Planner ({error}). {cleanup} Retry with: {pair_fix_command}"
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests;
