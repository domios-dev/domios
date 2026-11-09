use serde::Serialize;

use crate::multiplexer::types::AgentRole;
use crate::multiplexer::types::PendingPrompt;
use crate::service::ServiceContext;

#[derive(Debug, Clone, Serialize)]
pub struct TabStatusInfo {
    pub tab_id: String,
    pub status: String,
    pub agent_type: Option<String>,
    pub agent_session_id: Option<String>,
    pub role: Option<AgentRole>,
    pub role_profile: Option<String>,
    pub parent_tab_id: Option<String>,
    pub can_spawn: bool,
    pub depth: u8,
    pub children: Vec<String>,
    pub cwd: Option<String>,
    pub title: String,
    pub team_id: Option<String>,
    pub pending_prompt: Option<PendingPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<TabFailure>,
    /// The effective permission state, not the launch request: no questions when true.
    pub full_access: bool,
}

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
        let status =
            crate::multiplexer::state::tab_status_label(tab, self.resolved_tab_status(tab))
                .to_string();
        let children = mux.get_children_ids(tab_id);
        Ok(TabStatusInfo {
            tab_id: tab.id.clone(),
            status,
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
            failure: tab_failure(
                tab.boot_stage,
                tab.boot_failure_reason.as_deref(),
                tab.agent_runtime_state.as_ref(),
            )
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
mod tests {
    use crate::dispatch::CommandPort;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::{PendingPrompt, PromptOption};
    use codeterm_types::pty::{BootStage, TabStatus, TabVerdict, TimelineState};

    fn summary_status(service: &crate::service::ServiceContext, tab_id: &str) -> String {
        let snapshot = service.mux_snapshot();
        crate::dispatch::build_tab_summary_json(&snapshot, None, None, false, false)
            .into_iter()
            .find(|row| row["id"] == tab_id)
            .and_then(|row| row["status"].as_str().map(str::to_string))
            .expect("summary row")
    }

    #[test]
    fn tab_status_single_source() {
        let (mut mux, tab_id) = MultiplexerState::new();
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.pty_id = Some("pty-single-source".into());
            tab.agent_type = Some("opaque-provider".into());
            tab.boot_stage = Some(BootStage::AgentReady);
            tab.agent_state = Some(
                TabVerdict {
                    state: TimelineState::Idle,
                    subtype: None,
                }
                .into(),
            );
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;
        let agree = |expected: TabStatus| {
            let status = service.get_tab_status(&tab_id).unwrap().status;
            assert_eq!(status, expected.label());
            assert_eq!(summary_status(service, &tab_id), status);
        };

        service
            .agent_tracker
            .lock()
            .update_output("pty-single-source", [3u8; 32]);
        agree(TabStatus::Working);

        service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .pending_prompt = Some(PendingPrompt {
            request_id: None,
            question: "opaque-question-token".into(),
            options: vec![PromptOption {
                label: "opaque-option".into(),
                value: "1".into(),
            }],
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        });
        agree(TabStatus::Waiting);

        service.clear_pending_prompt(&tab_id);
        service.agent_tracker.lock().set_idle("pty-single-source");
        agree(TabStatus::Idle);
        service.agent_tracker.lock().set_error("pty-single-source");
        agree(TabStatus::Error);

        service.agent_tracker.lock().set_dead("pty-single-source");
        agree(TabStatus::Dead);
    }

    #[test]
    fn provider_turn_error_surfaces_status() {
        let (mut mux, tab_id) = MultiplexerState::new();
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.pty_id = Some("pty-provider-error".into());
            tab.agent_type = Some("opaque-provider".into());
            tab.boot_stage = Some(BootStage::AgentReady);
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;
        service
            .agent_tracker
            .lock()
            .update_output("pty-provider-error", [7u8; 32]);

        assert!(service.mark_agent_turn_error(&tab_id));
        assert_eq!(service.get_tab_status(&tab_id).unwrap().status, "error");
        assert_eq!(summary_status(service, &tab_id), "error");
    }

    #[test]
    fn provider_turn_error_after_user_interrupt_surfaces_idle_status() {
        let (mut mux, tab_id) = MultiplexerState::new();
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.pty_id = Some("pty-interrupted-provider-error".into());
            tab.agent_type = Some("opaque-provider".into());
            tab.boot_stage = Some(BootStage::AgentReady);
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;
        service
            .agent_tracker
            .lock()
            .update_output("pty-interrupted-provider-error", [8u8; 32]);
        service.arm_agent_stop_suppression("pty-interrupted-provider-error");

        assert!(service.mark_agent_turn_error(&tab_id));
        assert_eq!(service.get_tab_status(&tab_id).unwrap().status, "idle");
        assert_eq!(summary_status(service, &tab_id), "idle");
    }

    #[test]
    fn get_tab_status_projects_restore_failure_state_and_one_line_reason() {
        let (mut mux, tab_id) = MultiplexerState::new();
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some("opaque-provider".into());
            tab.boot_stage = Some(BootStage::SpawnFailed);
            tab.boot_failure_reason = Some("\n opaque-boot-reason \nopaque-detail".into());
            tab.agent_runtime_state = Some(serde_json::json!({
                "state": "degraded", "epoch": 1, "failureCode": "session_load_refused",
                "reason": "opaque-runtime-reason"
            }));
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;
        let json = serde_json::to_value(service.get_tab_status(&tab_id).unwrap()).unwrap();
        assert_eq!(
            json["failure"],
            serde_json::json!({"boot_stage": "spawn_failed", "runtime_state": "degraded",
                "code": "session_load_refused", "reason": "opaque-boot-reason"})
        );

        let runtime_only = super::tab_failure(
            Some(BootStage::AgentReady),
            Some("stale-boot-reason"),
            Some(
                &serde_json::json!({"state": "unloaded", "failureCode": "thread_not_found",
                "reason": "opaque-runtime-reason"}),
            ),
        )
        .unwrap();
        assert_eq!(runtime_only.boot_stage, None);
        assert_eq!(runtime_only.runtime_state.as_deref(), Some("unloaded"));
        assert_eq!(runtime_only.reason, "opaque-runtime-reason");

        {
            let mut mux = service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.boot_stage = Some(BootStage::AgentReady);
            tab.boot_failure_reason = None;
            tab.agent_runtime_state = Some(serde_json::json!({"state": "loaded", "epoch": 2}));
        }
        let healthy = serde_json::to_value(service.get_tab_status(&tab_id).unwrap()).unwrap();
        assert!(healthy.get("failure").is_none());
    }

    #[test]
    fn get_tab_status_includes_pending_prompt_for_terminal_choice() {
        let (mut mux, tab_id) = MultiplexerState::new();
        mux.get_tab_mut(&tab_id).unwrap().pending_prompt = Some(PendingPrompt {
            request_id: None,
            question: "opaque-question-token".into(),
            options: vec![
                PromptOption {
                    label: "opaque-option-one".into(),
                    value: "1".into(),
                },
                PromptOption {
                    label: "opaque-option-two".into(),
                    value: "2".into(),
                },
            ],
            sections: None,
            multi_select: Some(false),
            submit_available: Some(false),
            selected_values: None,
        });
        mux.get_tab_mut(&tab_id).unwrap().pending_prompt_kind =
            Some(codeterm_types::pty::PromptKind::Provider);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

        let status = fixture.service.get_tab_status(&tab_id).unwrap();

        assert_eq!(
            status.pending_prompt.as_ref().map(|prompt| {
                prompt
                    .options
                    .iter()
                    .map(|option| option.value.as_str())
                    .collect::<Vec<_>>()
            }),
            Some(vec!["1", "2"])
        );
    }
}
