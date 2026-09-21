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
        let mux = self.mux.write();
        let tab = mux
            .get_tab(tab_id)
            .ok_or(format!("Tab not found: {}", tab_id))?;
        let status = if let Some(ref pty_id) = tab.pty_id {
            let tracker = self.agent_tracker.lock();
            format!("{:?}", tracker.get_status(pty_id))
        } else {
            "Idle".to_string()
        };
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
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::{PendingPrompt, PromptOption};

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
