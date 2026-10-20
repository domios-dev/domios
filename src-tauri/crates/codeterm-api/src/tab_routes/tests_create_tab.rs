//! POST /api/tabs chatBackend-spawn validation (MED robustness fix). The route
//! used to accept ANY `chat_backend` id and 200, then the async openSession
//! closed the tab. [`validate_chat_backend_spawn`] now rejects unknown /
//! non-spawnable ids up-front with 400.
//!
//! [`validate_chat_backend_spawn`]: super::validate_chat_backend_spawn

use std::sync::Arc;

use codeterm_service_core::agent::chat_types::StructuredChat;
use codeterm_service_core::agent::provider::ModelInfo;
use codeterm_service_core::agent::registry::AgentRegistry;
use codeterm_service_core::chatbackend::{
    ChatBackendError, ChatBackendProvider, ChatBackendRegistry,
};
use codeterm_service_core::provider::ProviderRegistry;

struct Backend {
    id: &'static str,
    spawnable: bool,
}

impl ChatBackendProvider for Backend {
    fn id(&self) -> &str {
        self.id
    }
    fn spawnable(&self) -> bool {
        self.spawnable
    }
    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("s".into())
    }
    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        Ok(StructuredChat::default())
    }
    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

fn registry() -> ProviderRegistry {
    let agents = Arc::new(AgentRegistry::new());
    let backends = Arc::new(ChatBackendRegistry::new());
    backends.register(
        "lmstudio",
        Arc::new(Backend {
            id: "lmstudio",
            spawnable: true,
        }),
    );
    backends.register(
        "npc",
        Arc::new(Backend {
            id: "npc",
            spawnable: false,
        }),
    );
    ProviderRegistry::new(agents, backends)
}

#[test]
fn unknown_chat_backend_is_rejected() {
    let reg = registry();
    let err = super::validate_chat_backend_spawn(&reg, "nope").unwrap_err();
    assert!(err.contains("unknown"), "unexpected message: {err}");
}

#[test]
fn non_spawnable_chat_backend_is_rejected() {
    let reg = registry();
    // bare `chatBackend: true` (backend-only) must not be openable as a tab.
    let err = super::validate_chat_backend_spawn(&reg, "npc").unwrap_err();
    assert!(
        err.contains("not user-spawnable"),
        "unexpected message: {err}"
    );
}

#[test]
fn spawnable_chat_backend_is_accepted() {
    let reg = registry();
    assert!(super::validate_chat_backend_spawn(&reg, "lmstudio").is_ok());
}

#[test]
fn post_tabs_rejects_watcher_mode_chatbackend_config() {
    let err = codeterm_service_core::service::spawn::reject_direct_watcher_chatbackend_config(
        &serde_json::json!({ "mode": "watcher" }),
    )
    .unwrap_err();
    assert_eq!(
        err,
        "watcher Tabs spawn via codeterm agent spawn --role Watcher"
    );
}

#[test]
fn shell_profile_create_body_preserves_wire_selection() {
    let body: super::CreateShellTabBody = serde_json::from_value(serde_json::json!({
        "shell": "wsl:Ubuntu", "purpose": "qa", "cmd": "uname -a"
    }))
    .unwrap();
    assert_eq!(body.shell.as_deref(), Some("wsl:Ubuntu"));
    assert_eq!(body.cmd.as_deref(), Some("uname -a"));
    let body: super::CreateShellTabBody = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(body.shell.is_none());
}
