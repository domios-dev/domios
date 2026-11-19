use super::SpawnTabSpec;
use crate::multiplexer::state::MultiplexerState;
use crate::service::general_agent::refuse_parenting_under_general_agent;
use crate::service::orchestrator::skip_local_worker_role;
use crate::service::team_roles::stamp_role_label;
use crate::service::{MuxEmitCause, ServiceContext};

pub(super) fn place_chatbackend_role_kind(
    mux: &mut MultiplexerState,
    tab_id: &str,
    role: &str,
    profile: Option<&str>,
    parent: Option<&str>,
    can_spawn: Option<bool>,
) -> Result<(), String> {
    let (parsed, profile) = ServiceContext::parse_role_kind(role, profile)?;
    mux.set_tab_role_kind(tab_id, parsed, profile.as_deref(), parent)?;
    if let Some(can_spawn) = can_spawn {
        mux.set_can_spawn(tab_id, can_spawn)?;
    }
    Ok(())
}

impl ServiceContext {
    pub(super) fn assign_chatbackend_spawn_role(
        &self,
        tab_id: &str,
        role: &str,
        role_profile: Option<&str>,
        spec: &SpawnTabSpec,
        can_spawn: Option<bool>,
        emit_state_change: bool,
    ) -> Result<(), String> {
        let (parsed, profile) = Self::parse_role_kind(role, role_profile)?;
        let parent = spec.parent_tab_id.as_deref();
        let params = spec
            .chatbackend_launch
            .as_ref()
            .map(|launch| &launch.params);
        if skip_local_worker_role(
            &parsed,
            parent,
            params.and_then(|params| params.report_target_mesh_peer.as_deref()),
        ) {
            self.mux
                .write()
                .set_detached_worker_role(tab_id, profile.as_deref())?;
            if emit_state_change {
                self.emit_state_changed(MuxEmitCause::TabLifecycle);
            }
            return Ok(());
        }
        refuse_parenting_under_general_agent(&self.mux.read(), parent)?;
        if profile.as_deref() == Some(codeterm_types::agent::role_kind::builtins::PLANNER) {
            if let Some(plan_id) = params
                .and_then(|params| params.plan_id.as_deref())
                .filter(|id| !id.trim().is_empty())
            {
                self.place_planner_on_plan(tab_id, plan_id, parent, false)?;
                stamp_role_label(self, tab_id);
                return Ok(());
            }
        }
        if emit_state_change {
            self.set_tab_role_with_profile(tab_id, role, profile.as_deref(), parent, can_spawn)?;
        } else {
            let mut mux = self.mux.write();
            place_chatbackend_role_kind(
                &mut mux,
                tab_id,
                role,
                profile.as_deref(),
                parent,
                can_spawn,
            )?;
        }
        stamp_role_label(self, tab_id);
        Ok(())
    }
}
