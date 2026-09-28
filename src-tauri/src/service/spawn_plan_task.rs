//! Worker spawn binds a named plan task (`--plan-task`) to the new tab. A
//! read-only Worker binds none: it verifies, owns no task, and carries only the
//! gate tokens it was spawned with.

use crate::plan::Task;
use crate::service::plan::PlanStore;
use crate::service::{ServiceContext, SpawnAgentParams};

fn resolved_worker_launch_effort(
    identity: &crate::multiplexer::types::ReasoningEffortIdentity,
) -> Option<String> {
    identity.resolved.clone()
}

pub(crate) fn planned_task_title(task: &Task) -> Option<String> {
    crate::service::plan::worker_identity::task_executor_name(task)
        .ok()
        .flatten()
}

pub(crate) fn worker_spawn_plan_task_ref(params: &SpawnAgentParams) -> Option<&str> {
    params
        .plan_task
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn is_worker_spawn(params: &SpawnAgentParams) -> bool {
    params.role.as_deref() == Some("Worker")
}

/// A Worker holds a task when the spawn names one, by either flag that can.
fn worker_spawn_is_task_bound(params: &SpawnAgentParams) -> bool {
    worker_spawn_plan_task_ref(params).is_some()
        || params
            .task_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|task_id| !task_id.is_empty())
}

/// The safeguard that survives the task-less shape: work that changes source
/// still owns the task it changes it under. Read-only verification does not.
pub(crate) fn validate_worker_plan_task_spawn(params: &SpawnAgentParams) -> Result<(), String> {
    if worker_spawn_plan_task_ref(params).is_some() && !is_worker_spawn(params) {
        return Err("--plan-task is only valid with --role Worker".into());
    }
    if !params.authorized_gates.is_empty() {
        if !is_worker_spawn(params) {
            return Err("authorized gates are only valid with --role Worker".into());
        }
        if worker_spawn_plan_task_ref(params).is_some() {
            return Err(concat!(
                "authorized gates are not carried by a task-bound spawn; declare them ",
                "on the dispatch, where they are also enforced"
            )
            .into());
        }
    }
    if is_worker_spawn(params) && worker_spawn_is_task_bound(params) {
        if let Some(title) = params.title.as_deref() {
            crate::service::plan::worker_identity::validate_executor_name(title)?;
        }
    }
    if is_worker_spawn(params)
        && !worker_spawn_is_task_bound(params)
        && params.execution_intent == Some(crate::plan::ExecutionIntent::Mutating)
    {
        return Err(concat!(
            "a mutating Worker must own a task: pass --plan-task <task> (or --plan <id> ",
            "--task-id <task>). A Worker that only verifies declares ",
            "--execution-intent read_only and owns none."
        )
        .into());
    }
    Ok(())
}

/// The declared gates are the whole scope a task-less verification Worker has,
/// so they ride into its brief through the writer the dispatch path already
/// uses rather than a second renderer that could drift from it.
fn append_authorized_gates(params: &mut SpawnAgentParams) {
    if params.authorized_gates.is_empty() {
        return;
    }
    let gates = crate::service::plan::task_brief::render_authorized_gates(&params.authorized_gates);
    let existing = params.task.take().unwrap_or_default();
    let existing = existing.trim_end();
    params.task = Some(if existing.is_empty() {
        gates
    } else {
        format!("{existing}\n\n{gates}")
    });
}

/// Resolve task identity and surface advisory review pressure before provisioning.
pub(crate) fn preflight_worker_plan_task_wip_gate(
    store: &PlanStore,
    plan_id: Option<&str>,
    task_ref: &str,
) -> Result<(), String> {
    let resolved = store
        .resolve_task_ref(plan_id, task_ref)
        .map_err(|e| e.to_string())?;
    let plan = store
        .get(&resolved.plan_id)
        .ok_or_else(|| "plan not found".to_string())?;
    let task = plan
        .tasks
        .iter()
        .find(|t| t.id == resolved.task_id)
        .ok_or_else(|| "task not found".to_string())?;
    store
        .validate_task_startup_status(&resolved.plan_id, &resolved.task_id)
        .map_err(|error| error.to_string())?;
    if !matches!(
        &task.status,
        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::Blocked
    ) {
        return Ok(());
    }
    crate::service::plan::review_gate::validate_dispatch_review_capacity(&plan, task)
        .map_err(|error| error.to_string())
}

pub(crate) fn prepare_worker_plan_task_prompt(
    store: &PlanStore,
    params: &mut SpawnAgentParams,
) -> Result<(), String> {
    let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
        append_authorized_gates(params);
        return Ok(());
    };
    let resolved = store
        .resolve_task_ref(params.plan_id.as_deref(), task_ref)
        .map_err(|error| error.to_string())?;
    let detail = store
        .show_task(&resolved.task_id)
        .ok_or_else(|| "task not found".to_string())?;
    store
        .validate_task_startup_status(&resolved.plan_id, &resolved.task_id)
        .map_err(|error| error.to_string())?;
    if let Some(title) = planned_task_title(&detail.task) {
        params.title = Some(title);
        params.title_provenance = Some(crate::service::SpawnIdentityProvenance::Suggested);
    }
    if params.planned_reasoning_effort.is_none() {
        params.planned_reasoning_effort = params.reasoning_effort.clone();
    }
    params.task_launch_authority = Some(crate::service::TaskLaunchAuthority {
        plan_id: resolved.plan_id.clone(),
        task_id: resolved.task_id.clone(),
        attempt_id: crate::plan::ids::short_id("attempt"),
        lane_base: detail.task.lane.base.clone(),
    });
    let instruction = params.task.take();
    let manifest = crate::service::plan::plan_skills::plan_skill_manifest(store, &resolved.plan_id)
        .map_err(|error| error.to_string())?;
    let brief = crate::service::plan::task_brief::render_task_brief(
        &detail.task,
        &detail.sections,
        detail.task.personality.as_deref(),
        instruction.as_deref(),
        Some(&resolved.plan_id),
        &manifest,
        &[],
    );
    params.plan_skill_manifest = Some(manifest);
    params.task = Some(brief);
    params.plan_id = Some(resolved.plan_id);
    params.task_id = Some(resolved.task_id);
    Ok(())
}

#[derive(Debug)]
struct WorkerTaskBinding {
    plan_id: String,
    task_id: String,
    attempt_id: String,
}

fn bind_worker_task_to_tab(
    store: &PlanStore,
    plan_id: Option<&str>,
    task_ref: &str,
    tab_id: &str,
    title: &str,
    session_id: Option<&str>,
    session_binding_nonce: Option<&str>,
    intent: crate::plan::ExecutionIntent,
    allowance: Option<crate::plan::ProviderAllowance>,
    provider: Option<&str>,
    requested_model: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    attempt_id: &str,
    authorized_gates: Option<Vec<String>>,
    manifest: Vec<crate::plan::PlanSkillManifestEntry>,
    now_ms: u64,
) -> Result<WorkerTaskBinding, String> {
    let resolved = store
        .resolve_task_ref(plan_id, task_ref)
        .map_err(|e| e.to_string())?;
    let plan_id = resolved.plan_id;
    let task_id = resolved.task_id;
    let lane = store
        .show_task(&task_id)
        .ok_or_else(|| "task not found".to_string())?
        .task
        .lane;
    let attempt_id = store
        .begin_task_dispatch_with_reserved_attempt_and_gates_and_observation(
            &plan_id,
            &task_id,
            tab_id,
            title,
            lane.clone(),
            intent,
            allowance,
            provider,
            attempt_id,
            authorized_gates,
            crate::plan::WorkerPlacement::IsolatedLane,
            model.clone(),
            requested_model,
            model,
            reasoning_effort,
            None,
            false,
            false,
            now_ms,
        )
        .map_err(|error| error.to_string())?;
    if let Err(error) =
        store.set_attempt_plan_skill_manifest(&plan_id, &task_id, &attempt_id, manifest, now_ms)
    {
        let _ = store.revert_task_dispatch(&plan_id, &task_id, now_ms);
        return Err(error.to_string());
    }
    if let Some(session_id) = session_id {
        let Some(session_binding_nonce) =
            session_binding_nonce.filter(|nonce| !nonce.trim().is_empty())
        else {
            let _ = store.revert_task_dispatch(&plan_id, &task_id, now_ms);
            return Err("plan-task spawn is missing its session binding nonce".into());
        };
        if let Err(error) = store.finish_task_dispatch(
            &plan_id,
            &task_id,
            tab_id,
            &attempt_id,
            session_id,
            session_binding_nonce,
            lane,
            now_ms,
        ) {
            let _ = store.revert_task_dispatch(&plan_id, &task_id, now_ms);
            return Err(error.to_string());
        }
    }
    Ok(WorkerTaskBinding {
        plan_id,
        task_id,
        attempt_id,
    })
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ServiceContext {
    pub(crate) fn persist_dispatched_attempt_reasoning_effort(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<(), crate::service::plan::PlanError> {
        let reasoning_effort = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| tab.reasoning_effort_identity.resolved.clone())
        };
        self.plan_store.update_dispatched_attempt_reasoning_effort(
            plan_id,
            task_id,
            tab_id,
            attempt_id,
            reasoning_effort,
            now_ms,
        )
    }

    pub(crate) fn resolved_worker_launch_effort_for_tab(&self, tab_id: &str) -> Option<String> {
        let mux = self.mux.read();
        mux.get_tab(tab_id)
            .and_then(|tab| resolved_worker_launch_effort(&tab.reasoning_effort_identity))
    }

    /// Preflight the review WIP gate before any tab gets provisioned.
    pub(crate) fn preflight_worker_plan_task_spawn(
        &self,
        params: &SpawnAgentParams,
    ) -> Result<(), String> {
        // Every task-bound mutating route reaches this before a tab exists, by
        // either flag that binds a task, so none can write canonical silently.
        self.refuse_unsafe_task_bound_mutating_spawn(params)?;
        let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
            return Ok(());
        };
        preflight_worker_plan_task_wip_gate(&self.plan_store, params.plan_id.as_deref(), task_ref)?;
        let intent = params.execution_intent.as_ref().ok_or_else(|| {
            "plan-task spawn requires explicit execution_intent read_only or mutating".to_string()
        })?;
        let now_ms = epoch_ms();
        let allowance = self.provider_allowance_for_spawn(params, now_ms)?;
        crate::service::plan::interruption::provider_allowance_preflight(
            intent,
            allowance.as_ref(),
            now_ms,
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn prepare_worker_plan_task_spawn(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<(), String> {
        prepare_worker_plan_task_prompt(&self.plan_store, params)
    }

    pub(crate) fn bind_worker_plan_task_or_rollback_tab(
        &self,
        params: &SpawnAgentParams,
        tab_id: &str,
    ) -> Result<(), String> {
        let Some(task_ref) = worker_spawn_plan_task_ref(params) else {
            if let Some(binding) = params.remote_task_binding.as_ref() {
                let mut mux = self.mux.write();
                let tab = mux
                    .get_tab_mut(tab_id)
                    .ok_or_else(|| format!("remote task-bound Worker tab {tab_id} disappeared"))?;
                tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
                    plan_id: Some(binding.plan_id.clone()),
                    task_id: Some(binding.task_id.clone()),
                    attempt_id: Some(binding.attempt_id.clone()),
                    spawn_handle: None,
                    bound_at: epoch_ms(),
                });
                tab.report_target_mesh_peer = Some(binding.origin_peer_id.clone());
            }
            return Ok(());
        };
        let now_ms = epoch_ms();
        let (
            title,
            session_id,
            session_binding_nonce,
            provider,
            requested_model,
            model,
            reasoning_effort,
        ) = {
            let mux = self.mux.read();
            let title = mux.display_label_for_tab(tab_id);
            let binding = mux.get_tab(tab_id).map(|tab| {
                (
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                    tab.agent_type.clone(),
                    tab.agent_requested_model.clone(),
                    tab.agent_provider_resolved_model.clone(),
                    resolved_worker_launch_effort(&tab.reasoning_effort_identity),
                )
            });
            let (
                session_id,
                session_binding_nonce,
                provider,
                requested_model,
                model,
                reasoning_effort,
            ) = binding.unwrap_or_default();
            (
                title,
                session_id,
                session_binding_nonce,
                provider,
                requested_model,
                model,
                reasoning_effort,
            )
        };
        let binding = self.create_worker_task_binding(
            params,
            task_ref,
            tab_id,
            &title,
            session_id.as_deref(),
            session_binding_nonce.as_deref(),
            provider.as_deref(),
            requested_model,
            model,
            reasoning_effort,
            now_ms,
        );
        let binding = match binding {
            Ok(binding) => binding,
            Err(error) => {
                return Err(self.rollback_failed_agent_spawn_if_created(
                    tab_id,
                    &error,
                    params.existing_tab_id.is_none(),
                ))
            }
        };
        self.persist_current_attempt_session_model(tab_id);
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({
                "plan_id": binding.plan_id,
                "task_id": binding.task_id,
                "attempt_id": binding.attempt_id,
            }),
        );
        Ok(())
    }

    fn create_worker_task_binding(
        &self,
        params: &SpawnAgentParams,
        task_ref: &str,
        tab_id: &str,
        title: &str,
        session_id: Option<&str>,
        session_binding_nonce: Option<&str>,
        provider: Option<&str>,
        requested_model: Option<String>,
        model: Option<String>,
        reasoning_effort: Option<String>,
        now_ms: u64,
    ) -> Result<WorkerTaskBinding, String> {
        let intent = params
            .execution_intent
            .clone()
            .ok_or_else(|| "plan-task spawn requires explicit execution_intent".to_string())?;
        let allowance = self.provider_allowance_for_spawn(params, now_ms)?;
        let authority = params
            .task_launch_authority
            .as_ref()
            .ok_or_else(|| "plan-task spawn is missing launch authority".to_string())?;
        let manifest = params.plan_skill_manifest.clone().ok_or_else(|| {
            "plan-task spawn is missing its prepared plan skill manifest".to_string()
        })?;
        let authorized_gates =
            (!params.authorized_gates.is_empty()).then(|| params.authorized_gates.clone());
        bind_worker_task_to_tab(
            &self.plan_store,
            params.plan_id.as_deref(),
            task_ref,
            tab_id,
            title,
            session_id,
            session_binding_nonce,
            intent,
            allowance,
            provider,
            requested_model,
            model,
            reasoning_effort,
            &authority.attempt_id,
            authorized_gates,
            manifest,
            now_ms,
        )
    }

    fn provider_allowance_for_spawn(
        &self,
        _params: &SpawnAgentParams,
        _now_ms: u64,
    ) -> Result<Option<crate::plan::ProviderAllowance>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, PlanMode, TaskStatus};
    use crate::service::plan::{NewTask, PlanStore};

    fn store_with_task(dir: &std::path::Path) -> (PlanStore, String, String) {
        let store = PlanStore::new_for_test(dir);
        let plan = store
            .create_plan_for_team(
                "Spawn bind".into(),
                dir.display().to_string(),
                OrchestratorRef::from_tab("orch"),
                "team-orch".into(),
                vec![],
                1,
            )
            .unwrap();
        let mut p = store.get(&plan.id).unwrap();
        p.mode = PlanMode::Approved;
        store.save(&mut p, 2).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "bind-at-spawn".into(),
                    handle: Some("spawn-bind".into()),
                    personality: Some("Ada".into()),
                    role_guidance: Some("You are Ada. Keep the change bounded.".into()),
                    body: "Implement the task-owned behavior.".into(),
                    acceptance_criteria: vec!["The focused oracle passes.".into()],
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        (store, plan.id, task_id)
    }

    #[test]
    fn plan_task_binding_records_dispatched_model_and_effort() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        let effort_identity = crate::multiplexer::types::ReasoningEffortIdentity {
            requested: Some("xhigh".into()),
            resolved: Some("high".into()),
            ..Default::default()
        };
        let binding = bind_worker_task_to_tab(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "tab-worker",
            "Ada",
            Some("session-worker"),
            Some("nonce-worker"),
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            Some("codex"),
            Some("gpt-6-luna".into()),
            Some("gpt-6-luna".into()),
            resolved_worker_launch_effort(&effort_identity),
            "attempt-prepared",
            None,
            vec![crate::plan::PlanSkillManifestEntry {
                slug: "prepared".into(),
                title: "Prepared".into(),
                purpose: "Prove exact snapshot retention.".into(),
                content_digest: "sha256:prepared".into(),
            }],
            10,
        )
        .unwrap();
        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.status_label.as_deref(), Some("awaiting_worker_ack"));
        assert_eq!(task.assigned_tab_id.as_deref(), Some("tab-worker"));
        assert_eq!(task.lane.tab_id.as_deref(), Some("tab-worker"));
        assert_eq!(task.taken_by_role.as_deref(), Some("worker"));
        assert_eq!(binding.plan_id, plan_id);
        assert_eq!(binding.task_id, task_id);
        let attempt = task.execution_attempts.last().unwrap();
        assert_eq!(attempt.provider.as_deref(), Some("codex"));
        assert_eq!(attempt.model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(attempt.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(
            task.execution_attempts.last().unwrap().plan_skill_manifest[0].slug,
            "prepared"
        );
        assert_eq!(
            task.execution_attempts
                .last()
                .unwrap()
                .session_id
                .as_deref(),
            Some("session-worker")
        );
    }

    #[test]
    fn task_bound_launch_uses_the_task_ack_path_only() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _) = store_with_task(dir.path());
        let mut params = SpawnAgentParams {
            role: Some("Worker".into()),
            plan_id: Some(plan_id.clone()),
            plan_task: Some("spawn-bind".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            provider_reserve_pct: None,
            task: Some("Check the adjacent caller concern.".into()),
            ..Default::default()
        };

        prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

        assert!(params.task_launch_authority.is_some());
        let task_selection = crate::service::launch_delivery::select_visible_spawn_task(
            None,
            params.task.as_deref(),
            true,
            true,
        );
        let startup_handshake = crate::service::launch_delivery::startup_starter_prompt(
            true,
            params.task_launch_authority.is_some(),
        );
        assert_eq!(startup_handshake, None);
        assert_eq!(
            task_selection.provider_starter,
            Some(crate::agent::provider::StarterPrompt::AssignedTask)
        );
        assert_eq!(
            crate::service::spawn::visible_spawn_starter_prompt(
                startup_handshake,
                task_selection.provider_starter,
            ),
            Some(crate::agent::provider::StarterPrompt::AssignedTask)
        );

        let authority = params.task_launch_env();
        assert_eq!(
            authority.get("CODETERM_PLAN_ID").map(String::as_str),
            Some(plan_id.as_str())
        );
        assert_eq!(
            authority.get("CODETERM_TASK_ID").map(String::as_str),
            params.task_id.as_deref()
        );
        let prompt = params.task.unwrap();
        assert!(prompt.starts_with("You are Ada. Keep the change bounded."));
        let ack_instruction = codeterm_plan::service_plan::worker_identity::WORKER_ACK_INSTRUCTION;
        let ack = prompt.find(ack_instruction).unwrap();
        let body = prompt.find("Implement the task-owned behavior.").unwrap();
        let criteria = prompt.find("The focused oracle passes.").unwrap();
        let supplemental = prompt.find("## Orchestrator instruction").unwrap();
        assert!(ack < body);
        assert!(body < criteria && criteria < supplemental);
        assert_eq!(prompt.matches(ack_instruction).count(), 1);
        assert!(!prompt.contains("codeterm agent report"));
    }

    #[test]
    fn direct_plan_task_spawn_records_host_owned_effort_intent() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _) = store_with_task(dir.path());
        let mut params = SpawnAgentParams {
            role: Some("Worker".into()),
            plan_id: Some(plan_id),
            plan_task: Some("spawn-bind".into()),
            reasoning_effort: Some("high".into()),
            ..Default::default()
        };

        prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

        assert_eq!(params.planned_reasoning_effort.as_deref(), Some("high"));
        assert_eq!(params.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn validate_rejects_non_worker() {
        let err = validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Orchestrator".into()),
            plan_task: Some("spawn-bind".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            provider_reserve_pct: None,
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("--plan-task"));
    }

    #[test]
    fn tab_name_contract_spawn_name_paths_reject_multi_word_task_bound_title() {
        let error = validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Worker".into()),
            plan_task: Some("spawn-bind".into()),
            title: Some("Ada Lovelace".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            ..Default::default()
        })
        .unwrap_err();
        assert!(error.contains("one word"), "{error}");
    }

    fn verifier(gates: &[&str]) -> SpawnAgentParams {
        SpawnAgentParams {
            role: Some("Worker".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            authorized_gates: gates.iter().map(|gate| gate.to_string()).collect(),
            ..Default::default()
        }
    }

    /// The documented verification Worker: read-only, no task of its own.
    #[test]
    fn validate_accepts_a_read_only_worker_that_owns_no_task() {
        validate_worker_plan_task_spawn(&verifier(&[])).expect("no gates declared is still valid");
        validate_worker_plan_task_spawn(&SpawnAgentParams {
            plan_id: Some("pl_owner".into()),
            ..verifier(&["./build.sh verify"])
        })
        .expect("a read-only Worker binds no task");
    }

    /// The safeguard that stays: source-changing work owns the task it changes
    /// source under, so the lane and the review gate have something to hang on.
    #[test]
    fn validate_rejects_a_mutating_worker_that_owns_no_task() {
        let error = validate_worker_plan_task_spawn(&SpawnAgentParams {
            execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
            ..verifier(&[])
        })
        .expect_err("a mutating Worker must own a task");
        assert!(error.contains("--plan-task"), "{error}");
        assert!(error.contains("read_only"), "{error}");

        validate_worker_plan_task_spawn(&SpawnAgentParams {
            execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
            task_id: Some("task_owned".into()),
            ..verifier(&[])
        })
        .expect("a task-bound mutating Worker is unchanged");
    }

    #[test]
    fn mutating_worker_without_task_still_refused_without_unplanned() {
        let error = validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Worker".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
            ..Default::default()
        })
        .expect_err("plain mutating Worker spawn must not create a loose agent");
        assert!(error.contains("--plan-task"), "{error}");
    }

    /// The mesh Worker: it changes source on a peer, but the origin plan store
    /// is on another machine, so it declares no intent and owns no task. The
    /// daemon a mesh spawn lands on is where that shape is finally admitted.
    #[test]
    fn validate_accepts_a_worker_that_declares_no_intent_and_owns_no_task() {
        validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Worker".into()),
            ..Default::default()
        })
        .expect("a Worker with no binding and no declared intent is a supported shape");
    }

    #[test]
    fn validate_rejects_gates_a_spawn_cannot_enforce() {
        let roleless = validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: None,
            ..verifier(&["./build.sh verify"])
        })
        .expect_err("gates are a Worker concept");
        assert!(roleless.contains("--role Worker"), "{roleless}");

        let task_bound = validate_worker_plan_task_spawn(&SpawnAgentParams {
            plan_task: Some("spawn-bind".into()),
            ..verifier(&["./build.sh verify"])
        })
        .expect_err("a task-bound spawn declares gates on the dispatch");
        assert!(task_bound.contains("dispatch"), "{task_bound}");
    }

    /// The gates a verifier was scoped to must reach it, through the same
    /// writer the plan-task dispatch path uses.
    #[test]
    fn declared_gates_reach_a_task_less_worker_brief() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _plan_id, _) = store_with_task(dir.path());
        let mut params = SpawnAgentParams {
            task: Some("Verify the merged lane.".into()),
            ..verifier(&["./build.sh test-one app verifier", "./build.sh verify"])
        };

        prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

        let brief = params.task.expect("the brief survives");
        assert!(brief.starts_with("Verify the merged lane."), "{brief}");
        let rendered = crate::service::plan::task_brief::render_authorized_gates(&[
            "./build.sh test-one app verifier".to_string(),
            "./build.sh verify".to_string(),
        ]);
        assert!(brief.ends_with(&rendered), "{brief}");
    }

    #[test]
    fn a_task_less_worker_without_gates_keeps_its_brief_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _plan_id, _) = store_with_task(dir.path());
        let mut params = SpawnAgentParams {
            task: Some("Verify the merged lane.".into()),
            ..verifier(&[])
        };

        prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

        assert_eq!(params.task.as_deref(), Some("Verify the merged lane."));
    }

    fn block_review_wip(store: &PlanStore, plan_id: &str) {
        let mut p = store.get(plan_id).unwrap();
        p.review_policy.wip_limit = 0;
        store.save(&mut p, 4).unwrap();
    }

    #[test]
    fn preflight_allows_dispatch_at_review_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
    }

    #[test]
    fn preflight_allows_dispatch_when_gate_is_clear() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, _task_id) = store_with_task(dir.path());
        preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
    }

    #[test]
    fn dispatch_refuses_statuses_that_cannot_reach_startup() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());

        for status in [
            TaskStatus::Paused,
            TaskStatus::Done,
            TaskStatus::Cancelled,
            TaskStatus::InReview,
        ] {
            store
                .with_plan(&plan_id, 4, |plan| {
                    let task = plan
                        .tasks
                        .iter_mut()
                        .find(|task| task.id == task_id)
                        .unwrap();
                    task.status = status.clone();
                    Ok(())
                })
                .unwrap();
            assert!(
                preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").is_err()
            );
            let task = store.show_task(&task_id).unwrap().task;
            assert_eq!(task.status, status);
            assert!(task.execution_attempts.is_empty());
        }
    }

    #[test]
    fn spawn_preflight_allows_review_debt_without_capacity_grants() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        let mut plan = store.get(&plan_id).unwrap();
        plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.kind = "review".into();
        task.tags.push("review-capacity".into());
        store.save(&mut plan, 4).unwrap();
        preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();

        store
            .grant_review_capacity(
                &plan_id,
                &task_id,
                crate::plan::ReviewCapacityClass::Review,
                crate::plan::AgentRef::from_tab("orch"),
                5,
            )
            .unwrap();
        preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
    }

    #[test]
    fn review_threshold_does_not_abort_task_binding() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        let binding = bind_worker_task_to_tab(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "tab-worker",
            "Ada",
            None,
            None,
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            None,
            None,
            None,
            None,
            "attempt-review",
            None,
            vec![],
            10,
        )
        .unwrap();
        assert_eq!(binding.task_id, task_id);

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.assigned_tab_id.as_deref(), Some("tab-worker"));
        assert_eq!(task.lane.tab_id.as_deref(), Some("tab-worker"));
    }

    #[test]
    fn ack_after_direct_spawn_enters_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        let binding = bind_worker_task_to_tab(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "tab-worker",
            "Ada",
            Some("session-worker"),
            Some("nonce-worker"),
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            None,
            None,
            None,
            None,
            "attempt-ack",
            None,
            vec![],
            10,
        )
        .unwrap();
        store
            .acknowledge_task_attempt(&plan_id, &task_id, "tab-worker", &binding.attempt_id, 11)
            .unwrap();

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        let attempt = task
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == binding.attempt_id)
            .unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.status_label.as_deref(), Some("acknowledged"));
        assert_eq!(attempt.acknowledged_at, Some(11));
        assert_eq!(attempt.executor_tab_id, "tab-worker");
        assert_eq!(attempt.session_id.as_deref(), Some("session-worker"));
        assert_eq!(
            attempt.session_binding_nonce.as_deref(),
            Some("nonce-worker")
        );
    }

    #[test]
    fn respawn_of_blocked_task_yields_an_acknowledgeable_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        store
            .assign_task(&plan_id, &task_id, "worker-old", None, 4)
            .unwrap();
        let block_reason = "waiting for a dependency";
        store
            .with_plan(&plan_id, 5, |plan| {
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .unwrap();
                task.status = TaskStatus::Blocked;
                task.status_label = Some(block_reason.into());
                Ok(())
            })
            .unwrap();

        let binding = bind_worker_task_to_tab(
            &store,
            Some(&plan_id),
            "spawn-bind",
            "worker-respawn",
            "Ada",
            Some("session-respawn"),
            Some("nonce-respawn"),
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            None,
            None,
            None,
            None,
            "attempt-respawn",
            None,
            vec![],
            6,
        )
        .unwrap();
        let pending = store.show_task(&task_id).unwrap().task;
        assert_eq!(pending.status, TaskStatus::Pending);
        assert!(store.get(&plan_id).unwrap().strides.iter().any(|stride| {
            stride.text.contains("unblocked by dispatch") && stride.text.contains(block_reason)
        }));
        store
            .acknowledge_task_attempt(&plan_id, &task_id, "worker-respawn", &binding.attempt_id, 7)
            .unwrap();
        let task = store.show_task(&task_id).unwrap().task;
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(
            task.execution_attempts.last().unwrap().acknowledged_at,
            Some(7)
        );
    }

    #[test]
    fn task_binding_failure_leaves_task_unassigned() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan_id, task_id) = store_with_task(dir.path());
        block_review_wip(&store, &plan_id);

        let err = bind_worker_task_to_tab(
            &store,
            Some(&plan_id),
            "missing-task-fixture",
            "tab-worker",
            "Ada",
            None,
            None,
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            None,
            None,
            None,
            None,
            "attempt-rollback",
            None,
            vec![],
            10,
        )
        .unwrap_err();
        assert!(!err.is_empty());

        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.assigned_tab_id, None);
        assert_eq!(task.lane.tab_id, None);
    }
}
