//! Workspace & tab operations.

use crate::service::MuxEmitCause;
use std::collections::{BTreeMap, HashSet};

use super::ServiceContext;
use crate::multiplexer::state::{MultiplexerState, WorkspaceInfo};
use crate::multiplexer::types::{ClosedTab, TabCloseReason};
use crate::service::close_authority::{CloseTabTarget, MeshCloseGrant, TeamCloseSnapshot};
use crate::service::session_persist::{self, SessionPersistUrgency};
use crate::service::TeamMutationCaller;

pub(crate) fn create_workspace_with_count(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    name: String,
    cwd: Option<String>,
) -> (WorkspaceInfo, u32) {
    let ws = mux.create_workspace(name, cwd);
    (ws, super::analytics::count(mux.workspace_count()))
}

pub(crate) const MAX_WORKSPACE_NAME_CHARS: usize = 64;

pub(crate) fn validate_workspace_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("workspace name must not be empty".into());
    }
    if name.chars().count() > MAX_WORKSPACE_NAME_CHARS {
        return Err(format!(
            "workspace name must be {} characters or fewer",
            MAX_WORKSPACE_NAME_CHARS
        ));
    }
    if name
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
    {
        return Err("workspace name must be one line without control characters".into());
    }
    Ok(())
}

fn workspace_not_found_refusal_log(workspace_id: &str, known_ids: &[String]) -> String {
    format!("requested={workspace_id} known_ids={known_ids:?}")
}

#[derive(Clone, Copy)]
enum WorkspaceCloseCaller<'a> {
    LocalTab(&'a str),
    AuthenticatedMeshPeer(&'a str, &'a MeshCloseGrant),
    #[cfg(any(test, feature = "test-support"))]
    Human,
    HumanConfirmedHeaderless,
    Unattributed,
}

impl WorkspaceCloseCaller<'_> {
    fn kind(self) -> &'static str {
        match self {
            Self::LocalTab(_) => "AgentTab",
            Self::AuthenticatedMeshPeer(..) => "AuthenticatedMeshPeer",
            #[cfg(any(test, feature = "test-support"))]
            Self::Human => "Human",
            Self::HumanConfirmedHeaderless => "human-attributed (headerless, confirmed)",
            Self::Unattributed => "Unattributed",
        }
    }

    fn team_caller(self) -> TeamMutationCaller {
        match self {
            Self::LocalTab(tab_id) => TeamMutationCaller::AgentTab(tab_id.to_string()),
            Self::AuthenticatedMeshPeer(peer_id, _) => {
                TeamMutationCaller::AuthenticatedMeshPeer(peer_id.to_string())
            }
            #[cfg(any(test, feature = "test-support"))]
            Self::Human => TeamMutationCaller::Human,
            Self::HumanConfirmedHeaderless => TeamMutationCaller::HumanConfirmedHeaderless,
            Self::Unattributed => TeamMutationCaller::Unattributed,
        }
    }
}

#[derive(Eq, PartialEq)]
struct WorkspaceCloseTargets {
    tabs: BTreeMap<String, CloseTabTarget>,
    teams: BTreeMap<String, TeamCloseSnapshot>,
}

impl WorkspaceCloseTargets {
    fn capture(service: &ServiceContext, mux: &MultiplexerState, id: &str) -> Result<Self, String> {
        let workspace = mux
            .get_workspace(id)
            .ok_or_else(|| format!("Workspace not found: {id}"))?;
        let tabs = workspace
            .tabs
            .iter()
            .filter_map(|id| CloseTabTarget::capture(mux, id).map(|target| (id.clone(), target)))
            .collect();
        let team_ids = ServiceContext::workspace_team_ids(mux, id);
        let remote = if team_ids.is_empty() {
            None
        } else {
            Some(service.remote_teams.snapshot()?)
        };
        let teams = team_ids
            .into_iter()
            .map(|id| {
                let members = remote
                    .iter()
                    .flat_map(|state| state.members.iter())
                    .filter(|(_, member)| member.team_id == id)
                    .map(|(key, member)| (key.clone(), member.clone()))
                    .collect();
                let snapshot = TeamCloseSnapshot::capture(mux, &id, members);
                (id, snapshot)
            })
            .collect();
        Ok(Self { tabs, teams })
    }

    fn validate_remaining(
        &self,
        service: &ServiceContext,
        mux: &MultiplexerState,
        id: &str,
    ) -> Result<(), String> {
        let current = Self::capture(service, mux, id)?;
        if current
            .tabs
            .iter()
            .any(|(id, target)| self.tabs.get(id) != Some(target))
            || current
                .teams
                .iter()
                .any(|(id, snapshot)| self.teams.get(id) != Some(snapshot))
        {
            return Err(format!("Workspace changed while closing: {id}"));
        }
        Ok(())
    }
}

impl ServiceContext {
    pub fn ensure_storage_available(&self) -> Result<(), String> {
        let Some(authority) = self.try_ext::<crate::startup::StorageAuthority>() else {
            return Ok(());
        };
        authority
            .require_ready()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn log_workspace_not_found(&self, workspace_id: &str) {
        let known_ids = self
            .mux
            .read()
            .list_workspaces()
            .into_iter()
            .map(|workspace| workspace.id)
            .collect::<Vec<_>>();
        log::warn!(
            "[workspace] refusing missing workspace {}",
            workspace_not_found_refusal_log(workspace_id, &known_ids)
        );
    }

    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>, String> {
        self.ensure_storage_available()?;
        Ok(self.mux.read().list_workspaces())
    }

    pub(super) fn ensure_workspace_plans_loaded(&self, cwd: Option<&str>) {
        let Some(cwd) = cwd.filter(|c| !c.is_empty()) else {
            return;
        };
        let path = std::path::Path::new(cwd);
        if self
            .ext::<crate::service::LoadedPlanProjects>()
            .0
            .lock()
            .iter()
            .any(|root| path.starts_with(root))
        {
            return;
        }
        if let Err(e) = self.ensure_plan_project_loaded(path) {
            log::debug!("[workspace] no project plans loaded for {cwd}: {e}");
        }
    }

    pub fn create_workspace(
        &self,
        name: String,
        cwd: Option<String>,
        activate: bool,
    ) -> Result<WorkspaceInfo, String> {
        validate_workspace_name(&name)?;
        if let Some(cwd_str) = cwd.as_deref() {
            let existing = self.mux.write().find_workspace_info_by_cwd(cwd_str);
            if let Some(ws) = existing {
                if activate {
                    let _ = self.mux.write().set_active_workspace(&ws.id);
                }
                self.ensure_workspace_plans_loaded(Some(cwd_str));
                log::info!(
                    "[workspace] reused existing workspace '{}' ({}) for cwd={}",
                    ws.name,
                    ws.id,
                    cwd_str
                );
                return Ok(WorkspaceInfo {
                    existing: Some(true),
                    ..ws
                });
            }
        }

        let gate = self.limit_gate();
        let (mut ws, workspaces) = {
            let mut mux = self.mux.write();
            gate.admit(
                super::billing::LimitKey::Workspaces,
                mux.limited_workspace_count(),
            )
            .map_err(|refusal| refusal.to_string())?;
            create_workspace_with_count(&mut mux, name, cwd.clone())
        };
        if activate {
            let _ = self.mux.write().set_active_workspace(&ws.id);
        }
        ws.cwd_missing = self.refresh_workspace_cwd_state(&ws.id)?;
        self.ensure_workspace_plans_loaded(cwd.as_deref());
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt);
        self.record_product_event(codeterm_analytics::Event::WorkspaceCreated { workspaces });
        Ok(ws)
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<(), String> {
        validate_workspace_name(name)?;
        self.mux.write().rename_workspace(id, name)?;
        self.emit_state_changed(MuxEmitCause::Workspace);
        session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt)?;
        Ok(())
    }

    /// True when any top-level tab of `workspace_id` is a coordinator. Reads
    /// the tab tab ids under one lock, then probes each after releasing it so
    /// no nested tab lock is held across the coordinator resolution.
    fn workspace_has_protected_tab(&self, workspace_id: &str) -> Result<bool, String> {
        let tab_ids: Vec<String> = {
            let mux = self.mux.read();
            match mux.get_workspace(workspace_id) {
                Some(ws) => ws.tabs.clone(),
                None => return Ok(false),
            }
        };
        for id in tab_ids {
            if self.is_self_close_protected(&id)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn workspace_agent_tab_titles(mux: &MultiplexerState, workspace_id: &str) -> Vec<String> {
        let Some(workspace) = mux.get_workspace(workspace_id) else {
            return Vec::new();
        };
        workspace
            .tabs
            .iter()
            .filter_map(|tab_id| {
                mux.get_tab(tab_id)
                    .filter(|tab| tab.agent_type.is_some())
                    .map(|tab| tab.title.clone())
            })
            .collect()
    }

    /// Close a workspace on behalf of `caller_tab_id`. Refuses when a
    /// coordinator (Orchestrator/plan owner/Planner) targets the workspace it
    /// runs in; closing any OTHER workspace is unaffected. An absent or unknown
    /// caller against a workspace that hosts a coordinator is refused.
    pub fn close_workspace_guarded(
        &self,
        caller_tab_id: Option<&str>,
        workspace_id: &str,
        force: bool,
    ) -> Result<(), String> {
        crate::async_runtime::block_on(self.close_workspace_guarded_async(
            caller_tab_id,
            workspace_id,
            force,
        ))
    }

    pub async fn close_workspace_guarded_async(
        &self,
        caller_tab_id: Option<&str>,
        workspace_id: &str,
        force: bool,
    ) -> Result<(), String> {
        let caller = caller_tab_id
            .map(WorkspaceCloseCaller::LocalTab)
            .unwrap_or(WorkspaceCloseCaller::Unattributed);
        self.close_workspace_guarded_for(caller, workspace_id, force)
            .await
    }

    #[cfg(any(test, feature = "test-support"))]
    pub async fn close_workspace_as_human(
        &self,
        workspace_id: &str,
        force: bool,
    ) -> Result<(), String> {
        self.close_workspace_guarded_for(WorkspaceCloseCaller::Human, workspace_id, force)
            .await
    }

    pub async fn close_workspace_as_confirmed_headerless_human(
        &self,
        workspace_id: &str,
        force: bool,
    ) -> Result<(), String> {
        self.close_workspace_guarded_for(
            WorkspaceCloseCaller::HumanConfirmedHeaderless,
            workspace_id,
            force,
        )
        .await
    }

    /// The relay owns peer authentication and workspace access checks before
    /// this external-caller authority can be used.
    pub fn close_workspace_from_authenticated_mesh_peer(
        &self,
        peer_id: &str,
        workspace_id: &str,
        force: bool,
        grant: &MeshCloseGrant,
    ) -> Result<(), String> {
        crate::async_runtime::block_on(self.close_workspace_from_authenticated_mesh_peer_async(
            peer_id,
            workspace_id,
            force,
            grant,
        ))
    }

    async fn close_workspace_from_authenticated_mesh_peer_async(
        &self,
        peer_id: &str,
        workspace_id: &str,
        force: bool,
        grant: &MeshCloseGrant,
    ) -> Result<(), String> {
        self.close_workspace_guarded_for(
            WorkspaceCloseCaller::AuthenticatedMeshPeer(peer_id, grant),
            workspace_id,
            force,
        )
        .await
    }

    async fn close_workspace_guarded_for(
        &self,
        caller: WorkspaceCloseCaller<'_>,
        workspace_id: &str,
        force: bool,
    ) -> Result<(), String> {
        let caller_kind = caller.kind();
        let result = self
            .close_workspace_guarded_for_inner(caller, workspace_id, force)
            .await;
        if let Err(error) = &result {
            log::warn!(
                "[workspace-close] caller={caller_kind} workspace={workspace_id} failed: {error}"
            );
            if error.starts_with("Workspace not found:") {
                self.log_workspace_not_found(workspace_id);
            }
        }
        result
    }

    async fn close_workspace_guarded_for_inner(
        &self,
        caller: WorkspaceCloseCaller<'_>,
        workspace_id: &str,
        force: bool,
    ) -> Result<(), String> {
        let refuse = match caller {
            WorkspaceCloseCaller::LocalTab(caller) => {
                let caller_workspace = { self.mux.read().find_workspace_for_tab(caller) };
                match caller_workspace {
                    Some(caller_workspace) => {
                        caller_workspace == workspace_id && self.is_self_close_protected(caller)?
                    }
                    None => self.workspace_has_protected_tab(workspace_id)?,
                }
            }
            WorkspaceCloseCaller::AuthenticatedMeshPeer(peer_id, _) => {
                if peer_id.trim().is_empty() {
                    return Err("authenticated mesh peer id is required".to_string());
                }
                log::info!("workspace close authorized for mesh peer {peer_id}");
                false
            }
            #[cfg(any(test, feature = "test-support"))]
            WorkspaceCloseCaller::Human => false,
            WorkspaceCloseCaller::HumanConfirmedHeaderless => false,
            WorkspaceCloseCaller::Unattributed => false,
        };
        if refuse {
            let error = format!(
                "{}: a coordinator cannot close the workspace it runs in ({workspace_id}) — \
                     have your parent or another Tab close it, or move first.",
                crate::service::tab::SELF_CLOSE_REFUSAL_PREFIX
            );
            return Err(error);
        }
        let (targets, agent_titles) = {
            let mux = self.mux.read();
            (
                WorkspaceCloseTargets::capture(self, &mux, workspace_id)?,
                Self::workspace_agent_tab_titles(&mux, workspace_id),
            )
        };
        if matches!(caller, WorkspaceCloseCaller::Unattributed)
            && (!targets.tabs.is_empty() || !targets.teams.is_empty())
        {
            return Err(format!(
                "{}: an unattributed caller cannot close workspace {workspace_id}; explicit human confirmation is required",
                crate::service::AGENT_TAB_CLOSE_REFUSAL_PREFIX
            ));
        }
        if !force && !agent_titles.is_empty() {
            return Err(format!(
                "{}: explicit confirmation is required because closing would destroy {} agent tab(s): {}",
                crate::service::AGENT_TAB_CLOSE_REFUSAL_PREFIX,
                agent_titles.len(),
                agent_titles.join(", ")
            ));
        }

        let inventory = self.capture_close_inventory(workspace_id, None).await?;
        let admission = (|| {
            let mux = self.mux.read();
            if targets != WorkspaceCloseTargets::capture(self, &mux, workspace_id)? {
                return Err(format!("Workspace changed while closing: {workspace_id}"));
            }
            let begin = || {
                mux.ensure_workspace_membership_mutable(workspace_id)?;
                let workspace = mux
                    .get_workspace(workspace_id)
                    .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;
                if workspace.protected {
                    return Err("Cannot close protected workspace".into());
                }
                super::close_history::WorkspaceCloseGuard::begin(workspace_id)
            };
            match caller {
                WorkspaceCloseCaller::AuthenticatedMeshPeer(_, grant) => grant.admit(begin),
                _ => begin(),
            }
        })();
        let result = match &admission {
            Ok(_) => {
                self.close_workspace_members(caller, workspace_id, force, targets)
                    .await
            }
            Err(error) => Err(error.clone()),
        };
        let service = self.clone();
        match tokio::task::spawn_blocking(move || {
            service.settle_close_inventory(inventory.as_deref())
        })
        .await
        {
            Ok(Ok(())) => {}
            other => log::warn!("[workspace-close] close history settlement failed: {other:?}"),
        }
        result
    }

    async fn close_workspace_members(
        &self,
        caller: WorkspaceCloseCaller<'_>,
        workspace_id: &str,
        force: bool,
        targets: WorkspaceCloseTargets,
    ) -> Result<(), String> {
        let mut team_close_errors = Vec::new();
        let mut team_close_refusals_only = true;
        let mut partial_team_teardown = false;
        for (team_id, snapshot) in &targets.teams {
            let team_existed = { self.mux.read().get_team(team_id).is_some() };
            if let Err(error) = crate::ports::orchestration::installed(&self.extensions)
                .close_team(
                    team_id.clone(),
                    caller.team_caller(),
                    force,
                    snapshot.clone(),
                )
                .await
            {
                team_close_refusals_only &=
                    error.starts_with(crate::service::TEAM_CLOSE_REFUSAL_PREFIX);
                let team_remains = { self.mux.read().get_team(team_id).is_some() };
                partial_team_teardown |= team_existed && !team_remains;
                team_close_errors.push(format!("{team_id}: {error}"));
            }
        }
        if !team_close_errors.is_empty() {
            let details = team_close_errors.join("; ");
            if partial_team_teardown {
                return Err(format!(
                    "{}: {details}",
                    crate::service::WORKSPACE_CLOSE_CLEANUP_FAILURE_PREFIX
                ));
            }
            if team_close_refusals_only {
                return Err(format!(
                    "{}: {details}",
                    crate::service::TEAM_CLOSE_REFUSAL_PREFIX
                ));
            }
            return Err(format!("workspace Team cleanup failed: {details}"));
        }

        let service = (*self).clone();
        let workspace_id = workspace_id.to_string();
        tokio::task::spawn_blocking(move || {
            service.close_workspace_scoped(&workspace_id, Some(&targets))
        })
        .await
        .map_err(|error| format!("workspace close teardown task failed: {error}"))?
    }

    fn workspace_team_ids(mux: &MultiplexerState, workspace_id: &str) -> Vec<String> {
        let Some(workspace) = mux.get_workspace(workspace_id) else {
            return Vec::new();
        };
        let mut pending = workspace.tabs.clone();
        let mut visited = HashSet::new();
        let mut team_ids = HashSet::new();
        while let Some(tab_id) = pending.pop() {
            if !visited.insert(tab_id.clone()) {
                continue;
            }
            if let Some(tab) = mux.get_tab(&tab_id) {
                if let Some(team_id) = tab.team_id.as_ref() {
                    team_ids.insert(team_id.clone());
                }
            }
            pending.extend(mux.get_children_ids(&tab_id));
        }
        team_ids.extend(
            mux.list_teams()
                .into_iter()
                .filter(|team| team.workspace_id == workspace_id)
                .map(|team| team.id),
        );
        let mut team_ids: Vec<_> = team_ids.into_iter().collect();
        team_ids.retain(|team_id| !super::team_core::is_remote_team(mux, team_id));
        team_ids.sort();
        team_ids
    }

    pub fn close_workspace(&self, id: &str) -> Result<(), String> {
        self.close_workspace_scoped(id, None)
    }

    fn close_workspace_scoped(
        &self,
        id: &str,
        targets: Option<&WorkspaceCloseTargets>,
    ) -> Result<(), String> {
        self.close_workspace_with_teardown(
            id,
            targets,
            |service, tab_id| service.prepare_tab_close(tab_id, TabCloseReason::WorkspaceClosed),
            |service, tab_id| service.finish_tab_close(tab_id),
        )
    }

    fn close_workspace_with_teardown<Prepare, Finish>(
        &self,
        id: &str,
        targets: Option<&WorkspaceCloseTargets>,
        mut prepare: Prepare,
        mut finish: Finish,
    ) -> Result<(), String>
    where
        Prepare: FnMut(&Self, &str) -> Result<(), String>,
        Finish: FnMut(&Self, &str) -> Result<(), String>,
    {
        let (pre_focus, prepared, provider_ids) = {
            let mut mux = self.mux.write();
            if let Some(targets) = targets {
                targets.validate_remaining(self, &mux, id)?;
            }
            let pre_focus = mux.get_focused_tab_id().map(str::to_string);
            let prepared = mux.reserve_workspace_close(id)?;
            let provider_ids = prepared
                .tab_ids()
                .iter()
                .filter_map(|tab_id| {
                    mux.get_tab(tab_id)
                        .and_then(|tab| tab.agent_type.clone())
                        .map(|provider_id| (tab_id.clone(), provider_id))
                })
                .collect::<Vec<_>>();
            (pre_focus, prepared, provider_ids)
        };
        let tab_ids = prepared.tab_ids().to_vec();
        let mut cleanup_errors = Vec::new();
        for tab_id in &tab_ids {
            if let Err(error) = prepare(self, tab_id) {
                cleanup_errors.push(format!("{tab_id}: {error}"));
            }
        }
        let (close_result, history_batch) = {
            let mut mux = self.mux.write();
            let result = mux.close_prepared_workspace(prepared);
            (result, mux.take_closed_history_batch())
        };
        if let Some(batch) = history_batch {
            let evicted = batch.persist_on_blocking_thread_collect_evicted();
            self.retire_evicted_transcript_owners(&evicted);
            self.forget_evicted_tab_todos(evicted);
            crate::service::tab_chat::closed_transcript::forget_evicted(self);
        }
        let pty_ids = close_result?;
        let post_focus = self.mux.read().get_focused_tab_id().map(str::to_string);
        for (tab_id, provider_id) in provider_ids {
            self.release_tab_launch_artifacts(&tab_id, &provider_id);
        }
        self.kill_ptys(&pty_ids);
        for tab_id in &tab_ids {
            if let Err(error) = finish(self, tab_id) {
                cleanup_errors.push(format!("{tab_id}: {error}"));
            }
        }
        if let Some(cron) = self.try_ext::<crate::CronState>() {
            let wake = self.try_ext::<crate::CronWake>();
            if let Err(error) = self.cron_delete_workspace(cron, wake, id) {
                cleanup_errors.push(format!("crons: {error}"));
            }
        }
        // After the members are gone, never before: a Team that still holds a
        // live tab is left untouched.
        if let Some(ports) = self.try_ext::<crate::ports::sentinel::SentinelPorts>() {
            if let Err(error) = ports.cleanup.remove_workspace(id) {
                cleanup_errors.push(format!("Blueprints: {error}"));
            }
        }
        self.reap_orphaned_startup_teams();
        session_persist::persist_session_after_close(self);
        self.emit_state_changed(MuxEmitCause::Workspace);
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_tab_focus_changed(&new);
            }
        }
        if cleanup_errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "workspace closed with cleanup errors: {}",
                cleanup_errors.join("; ")
            ))
        }
    }

    pub fn move_tab(
        &self,
        tab_id: &str,
        from_workspace_id: &str,
        to_workspace_id: &str,
    ) -> Result<(), String> {
        if self.mux.read().get_workspace(to_workspace_id).is_none() {
            return Err(format!("Workspace not found: {to_workspace_id}"));
        }
        {
            let mut mux = self.mux.write();
            mux.require_focusable_tab(tab_id)?;
            mux.move_tab(tab_id, from_workspace_id, to_workspace_id)?;
            let fallback = mux.get_workspace(from_workspace_id).and_then(|workspace| {
                let active = workspace.active_tab_id.as_deref()?;
                mux.require_focusable_tab(active).err()?;
                Some(
                    workspace
                        .tabs
                        .iter()
                        .rev()
                        .find(|id| mux.require_focusable_tab(id).is_ok())
                        .cloned(),
                )
            });
            if let Some(fallback) = fallback {
                if let Some(workspace) = mux.get_workspace_mut(from_workspace_id) {
                    workspace.active_tab_id = fallback;
                }
            }
        }
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Reorder the top-level tabs of a workspace to match `ordered_tab_ids`.
    pub fn reorder_tab(
        &self,
        workspace_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        self.mux
            .write()
            .reorder_tabs(workspace_id, ordered_tab_ids)?;
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Reorder the children of `parent_tab_id` to match `ordered_tab_ids`.
    pub fn reorder_child(
        &self,
        parent_tab_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        self.mux
            .write()
            .reorder_children(parent_tab_id, ordered_tab_ids)?;
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Rewrite the workspace-wide sidebar display order across every tab level.
    pub fn reorder_display(
        &self,
        workspace_id: &str,
        ordered_tab_ids: &[String],
    ) -> Result<(), String> {
        self.mux
            .write()
            .reorder_display(workspace_id, ordered_tab_ids)?;
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Reorder workspaces to match `ordered_workspace_ids`.
    pub fn reorder_workspace(&self, ordered_workspace_ids: &[String]) -> Result<(), String> {
        self.mux.write().reorder_workspaces(ordered_workspace_ids)?;
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    pub fn create_tab(&self, workspace_id: &str) -> Result<String, String> {
        self.create_tab_with_activate(workspace_id, true)
    }

    pub fn create_named_tab(&self, workspace_id: &str, title: &str) -> Result<String, String> {
        super::tab::validate_tab_title(title)?;
        let gate = self.limit_gate();
        let tab_id = {
            let mut mux = self.mux.write();
            super::billing::admit_new_tab(&gate, &mux)?;
            let tab_id = mux.create_tab(workspace_id)?;
            mux.rename_tab(&tab_id, title)?;
            let _ = mux.set_active_tab(workspace_id, &tab_id);
            tab_id
        };
        self.record_kpi_tab_created(&tab_id, crate::agent::kpi_ledger::TabCreatedKind::Terminal);
        self.emit_tab_title_pinned(&tab_id, title);
        self.emit_state_changed(MuxEmitCause::Workspace);
        session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt)?;
        Ok(tab_id)
    }

    pub fn create_tab_with_activate(
        &self,
        workspace_id: &str,
        activate: bool,
    ) -> Result<String, String> {
        let gate = self.limit_gate();
        let result = {
            let mut mux = self.mux.write();
            super::billing::admit_new_tab(&gate, &mux)?;
            let tab_id = mux.create_tab(workspace_id)?;
            if activate {
                let _ = mux.set_active_tab(workspace_id, &tab_id);
            }
            tab_id
        };
        self.record_kpi_tab_created(&result, crate::agent::kpi_ledger::TabCreatedKind::Terminal);
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt);
        Ok(result)
    }

    pub fn set_active_tab(&self, workspace_id: &str, tab_id: &str) -> Result<(), String> {
        self.mux.write().set_active_tab(workspace_id, tab_id)?;
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Atomically set active workspace and active tab in one lock acquisition,
    /// emitting a single mux-state-changed instead of two sequential ones.
    pub fn set_active_tab_and_workspace(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<(), String> {
        {
            let mut mux = self.mux.write();
            mux.set_active_workspace(workspace_id)?;
            mux.set_active_tab(workspace_id, tab_id)?;
        }
        self.emit_state_changed(MuxEmitCause::Workspace);
        let _ =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Debounced);
        Ok(())
    }

    /// Snapshot of a workspace's recently-closed ring (newest first).
    pub fn list_recently_closed(&self, workspace_id: &str) -> Vec<ClosedTab> {
        self.mux.read().list_recently_closed(workspace_id)
    }

    /// Reopen a recently-closed tab (browser-style). `index` picks the ring
    /// entry (newest = 0, default). Re-materializes the tab with its original
    /// backing in a fresh, activated tab; the restore path then resumes a PTY
    /// agent or lazily reloads a chatBackend transcript.
    pub fn reopen_closed_tab(
        &self,
        workspace_id: &str,
        index: Option<usize>,
    ) -> Result<String, String> {
        let tab_id = {
            let mut mux = self.mux.write();
            let owner = mux.reopen_closed_tab_with_lineage(workspace_id, index)?;
            let _ = mux.set_active_tab(workspace_id, &owner.live_tab_id);
            owner.live_tab_id
        };
        self.settle_reopened_transcript(&tab_id);
        self.emit_state_changed(MuxEmitCause::Workspace);
        Ok(tab_id)
    }

    /// Reopen a whole closed group (single / cascade subtree / workspace) by id.
    /// Recreates the workspace if needed, restores every tab in the group,
    /// relinks the hierarchy, and activates the resulting tab/workspace.
    /// Returns (workspace_id, tab_id).
    pub fn reopen_closed_group(&self, group_id: &str) -> Result<(String, String), String> {
        if let Some(result) = self.restore_close_inventory(group_id)? {
            return Ok(result);
        }
        let (result, lineage) = {
            let mut mux = self.mux.write();
            let (r, lineage) = mux.reopen_closed_group_with_lineage(group_id)?;
            let _ = mux.set_active_workspace(&r.0);
            let _ = mux.set_active_tab(&r.0, &r.1);
            (r, lineage)
        };
        for owner in lineage {
            self.settle_reopened_transcript(&owner.live_tab_id);
        }
        self.emit_state_changed(MuxEmitCause::Workspace);
        Ok(result)
    }

    /// Reopen a tab from the orphaned ring. Recreates the workspace and
    /// activates it. Returns (workspace_id, tab_id).
    pub fn reopen_orphaned_tab(&self, tab_id: &str) -> Result<(String, String), String> {
        let result = {
            let mut mux = self.mux.write();
            let (workspace, owner) = mux.reopen_orphaned_tab_with_lineage(tab_id)?;
            let r = (workspace, owner.live_tab_id);
            let _ = mux.set_active_workspace(&r.0);
            let _ = mux.set_active_tab(&r.0, &r.1);
            r
        };
        self.settle_reopened_transcript(&result.1);
        self.emit_state_changed(MuxEmitCause::Workspace);
        Ok(result)
    }

    fn settle_reopened_transcript(&self, live_tab_id: &str) {
        let lineage = self.mux.read().reopened_transcript_owner(live_tab_id);
        match lineage.and_then(|closed| {
            if let Some(closed) = closed {
                self.restore_transcript_owner(&closed, live_tab_id)?;
            }
            Ok(())
        }) {
            Ok(()) => {}
            Err(error) => {
                log::warn!("[live-transcript] reopened owner {live_tab_id} held: {error}")
            }
        }
    }
}

#[cfg(test)]
mod workspace_role_tests;

#[cfg(test)]
mod workspace_close_tests;

#[cfg(any(test, feature = "test-support"))]
pub(crate) mod test_fixture;
