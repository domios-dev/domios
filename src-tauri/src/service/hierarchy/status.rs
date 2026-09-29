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
}

impl ServiceContext {
    pub fn get_tab_status(&self, tab_id: &str) -> Result<TabStatusInfo, String> {
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
    #[cfg_attr(
        not(windows),
        ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
    )]
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
    #[cfg_attr(
        not(windows),
        ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
    )]
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
    #[cfg_attr(
        not(windows),
        ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
    )]
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
    #[cfg_attr(
        not(windows),
        ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
    )]
    fn get_tab_status_includes_pending_prompt_for_terminal_choice() {
        let (mut mux, tab_id) = MultiplexerState::new();
        mux.get_tab_mut(&tab_id).unwrap().pending_prompt = Some(PendingPrompt {
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
