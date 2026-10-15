use super::*;
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::agent::registry::AgentRegistry;
use crate::multiplexer::state::MultiplexerState;

struct RelayProvider;
impl AgentProvider for RelayProvider {
    fn id(&self) -> &str {
        "fixture"
    }
    fn display_name(&self) -> &str {
        "fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn is_tool_less(&self) -> bool {
        true
    }
    fn supports_reply_relay(&self) -> bool {
        true
    }
    fn session_starts_on_first_user_turn(&self) -> bool {
        true
    }
}

#[test]
fn confirmed_visible_spawn_task_stages_exact_parent_reply_relay() {
    let (mut mux, parent) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&parent).unwrap();
    let child = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&parent).unwrap().agent_type = Some("fixture".into());
    let tab = mux.get_tab_mut(&child).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.parent_tab_id = Some(parent.clone());
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(RelayProvider));
    let relays = ReplyRelayRegistry::default();
    let selection = crate::service::launch_delivery::select_visible_spawn_task(
        None,
        Some("opaque task"),
        false,
        false,
    );
    assert!(selection.provider_launch.is_none());
    let crate::service::launch_delivery::VisibleSpawnFirstTurn::Task(task) =
        selection.confirmed_first_turn.unwrap()
    else {
        panic!("visible task selection")
    };
    let (text, pending) = relays
        .spawn_task_message(&mux, &registry, &child, task)
        .unwrap();
    assert!(pending.is_some());
    assert_ne!(text, "opaque task");
    relays.arm_spawn(&child, "nonce");
    let staged = relays.staged.lock();
    let pending = &staged[&child];
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].sender_tab, parent);
    assert_eq!(pending[0].user_text, text);
    assert_eq!(pending[0].binding_nonce, "nonce");
    assert_eq!(pending[0].after_offset, 0);
}

#[test]
fn user_created_visible_spawn_task_has_no_reply_relay() {
    let (mut mux, child) = MultiplexerState::new();
    mux.get_tab_mut(&child).unwrap().agent_type = Some("fixture".into());
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(RelayProvider));
    let relays = ReplyRelayRegistry::default();
    let selection = crate::service::launch_delivery::select_visible_spawn_task(
        None,
        Some("opaque task"),
        false,
        false,
    );
    let crate::service::launch_delivery::VisibleSpawnFirstTurn::Task(task) =
        selection.confirmed_first_turn.unwrap()
    else {
        panic!("visible task selection")
    };
    let text = relays
        .spawn_task_message(&mux, &registry, &child, task)
        .map(|(text, _)| text)
        .unwrap_or_else(|| task.into());
    assert_eq!(text, "opaque task");
    assert!(relays.staged.lock().is_empty());
}
