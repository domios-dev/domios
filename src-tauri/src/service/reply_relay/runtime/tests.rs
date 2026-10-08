use super::*;
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

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
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(RelayProvider));
    let selection = crate::service::launch_delivery::select_visible_spawn_task(
        None,
        Some("opaque task"),
        false,
        false,
    );
    assert!(selection.provider_launch.is_none());
    let text = fixture.service.confirmed_spawn_first_turn(
        &child,
        "fixture",
        selection.confirmed_first_turn.unwrap(),
    );
    assert_ne!(text, "opaque task");
    fixture.service.arm_spawn_reply_relay(&child, "nonce");
    let staged = fixture.service.reply_relays.staged.lock();
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
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(RelayProvider));
    let text = fixture.service.confirmed_spawn_first_turn(
        &child,
        "fixture",
        crate::service::launch_delivery::VisibleSpawnFirstTurn::Task("opaque task"),
    );
    assert_eq!(text, "opaque task");
    assert!(fixture.service.reply_relays.staged.lock().is_empty());
}
