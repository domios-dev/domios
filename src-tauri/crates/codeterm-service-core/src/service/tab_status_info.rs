use crate::service::orchestration_data::TabStatusInfo;

use crate::service::ServiceContext;

/// Why a tab's agent is not running: a failed boot or a degraded runtime, with a one-line reason.
pub use codeterm_wire::api_contracts::TabFailure;

fn first_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.to_string())
}

fn tab_failure(
    boot_stage: Option<codeterm_types::pty::BootStage>,
    boot_failure_reason: Option<&str>,
    runtime_state: Option<&serde_json::Value>,
) -> Option<TabFailure> {
    let boot_stage = boot_stage
        .filter(|stage| *stage == codeterm_types::pty::BootStage::SpawnFailed)
        .map(|stage| stage.as_str().to_string());
    let field = |key: &str| runtime_state.and_then(|state| state[key].as_str());
    let code = field("failureCode").map(str::to_string);
    let runtime_failed = field("state") == Some("degraded") || code.is_some();
    let runtime_state = field("state")
        .filter(|_| runtime_failed)
        .map(str::to_string);
    let access_failure = field("accessFailure");
    if boot_stage.is_none() && !runtime_failed && access_failure.is_none() {
        return None;
    }
    let code = code.or_else(|| {
        access_failure.map(|_| {
            crate::service::agent_runtime::full_access::FULL_ACCESS_UNAVAILABLE.to_string()
        })
    });
    let reason = boot_stage
        .as_ref()
        .and(boot_failure_reason)
        .and_then(first_line)
        .or_else(|| field("reason").and_then(first_line))
        .or_else(|| access_failure.and_then(first_line))
        .unwrap_or_else(|| "no reason recorded".into());
    Some(TabFailure {
        boot_stage,
        runtime_state,
        code,
        reason,
    })
}

impl ServiceContext {
    pub fn get_tab_status(&self, tab_id: &str) -> Result<TabStatusInfo, String> {
        let ack_failure = self.worker_ack.reported_ack_failure(tab_id);
        let full_access = self.effective_full_access(tab_id);
        let mux = self.mux.read();
        let tab = mux
            .get_tab(tab_id)
            .ok_or(format!("Tab not found: {}", tab_id))?;
        let (status, browser_failure) = self.browser_tab_status_fields(tab).unwrap_or_else(|| {
            (
                crate::multiplexer::state::tab_status_label(tab, self.resolved_tab_status(tab)),
                None,
            )
        });
        let children = mux.get_children_ids(tab_id);
        Ok(TabStatusInfo {
            tab_id: tab.id.clone(),
            status: status.into(),
            agent_type: tab.agent_type.clone(),
            agent_session_id: tab.agent_session_id.clone(),
            role: tab.role,
            role_profile: tab.role_profile.clone(),
            parent_tab_id: tab.parent_tab_id.clone(),
            can_spawn: tab.can_spawn,
            depth: tab.depth,
            children,
            cwd: tab.cwd.clone(),
            title: tab.title.clone(),
            team_id: tab.team_id.clone(),
            pending_prompt: tab.pending_prompt.clone(),
            failure: browser_failure
                .map(|reason| TabFailure {
                    boot_stage: None,
                    runtime_state: None,
                    code: None,
                    reason: first_line(&reason).unwrap_or(reason),
                })
                .or_else(|| {
                    tab_failure(
                        tab.boot_stage,
                        tab.boot_failure_reason.as_deref(),
                        tab.agent_runtime_state.as_ref(),
                    )
                })
                .or_else(|| {
                    ack_failure.map(|reason| TabFailure {
                        boot_stage: None,
                        runtime_state: None,
                        code: Some(
                            crate::service::worker_ack::ack_failure::WORKER_ACK_FAILED.to_string(),
                        ),
                        reason,
                    })
                }),
            full_access,
        })
    }
}

#[cfg(test)]
mod tests;
