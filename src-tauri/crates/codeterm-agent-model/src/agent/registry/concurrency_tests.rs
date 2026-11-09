use super::*;
use crate::agent::test_provider::MockProvider;
use std::sync::mpsc;

#[test]
fn held_provider_callback_does_not_block_registry_writer_or_other_provider() {
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(MockProvider::new("held")));
    registry.register_provider(Box::new(MockProvider::new("healthy")));
    let dependency = Arc::new(Mutex::new(()));
    let held = dependency.lock();
    let (entered_tx, entered_rx) = mpsc::channel();
    let callback_registry = registry.clone();
    let callback_dependency = dependency.clone();
    let callback = std::thread::spawn(move || {
        callback_registry.with_provider("held", |provider| {
            entered_tx.send(()).unwrap();
            let _dependency = callback_dependency.lock();
            provider.id().to_owned()
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    let writer_registry = registry.clone();
    let (progress_tx, progress_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let removed = writer_registry.unregister_provider("held");
        let healthy = writer_registry.with_provider("healthy", |provider| provider.id().to_owned());
        progress_tx.send((removed, healthy)).unwrap();
    });
    let progress = progress_rx.recv_timeout(Duration::from_secs(1));
    drop(held);
    let retained_provider = callback.join().unwrap();
    writer.join().unwrap();

    assert_eq!(progress.unwrap(), (true, Some("healthy".into())));
    assert_eq!(retained_provider.as_deref(), Some("held"));
}

#[test]
fn get_pins_provider_without_pinning_registry() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(MockProvider::new("pinned")));
    let provider = registry.get("pinned").unwrap();
    assert!(registry.providers.try_write().is_some());
    assert!(registry.unregister_provider("pinned"));
    assert_eq!(provider.id(), "pinned");
    assert!(registry.get("pinned").is_none());
}

struct LockProbe {
    registry: std::sync::Weak<AgentRegistry>,
}

impl LockProbe {
    fn check(&self) {
        if let Some(registry) = self.registry.upgrade() {
            assert!(
                registry.providers.try_write().is_some(),
                "provider callback retained the registry lock"
            );
            assert!(
                registry.native_fallbacks.try_write().is_some(),
                "provider callback retained the fallback lock"
            );
        }
    }
}

impl Drop for LockProbe {
    fn drop(&mut self) {
        self.check();
    }
}

impl AgentProvider for LockProbe {
    fn id(&self) -> &str {
        self.check();
        "probe"
    }
    fn display_name(&self) -> &str {
        self.check();
        "Probe"
    }
    fn is_native(&self) -> bool {
        self.check();
        true
    }
    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        self.check();
        String::new()
    }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        self.check();
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        self.check();
        crate::agent::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        self.check();
        true
    }
    fn detect_from_output(&self, _: &str) -> bool {
        self.check();
        true
    }
    fn screen_has_tui(&self, _: &str) -> bool {
        self.check();
        true
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        self.check();
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        self.check();
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        self.check();
        None
    }
    fn available_models(&self) -> Vec<crate::agent::provider::ModelInfo> {
        self.check();
        Vec::new()
    }
    fn has_event_markers(&self, _: &str) -> bool {
        self.check();
        true
    }
    fn detect_events(&self, _: &str) -> Vec<crate::agent::detector::AgentEvent> {
        self.check();
        Vec::new()
    }
    fn status_bar_rows(&self) -> u16 {
        self.check();
        1
    }
    fn is_output_noise(&self, _: &str) -> bool {
        self.check();
        false
    }
    fn build_prompt_response(&self, _: usize, _: &str) -> Vec<Vec<u8>> {
        self.check();
        Vec::new()
    }
    fn supports_agent_state_status_reconciliation(&self) -> bool {
        self.check();
        true
    }
    fn needs_structured_prompt_server(&self) -> bool {
        self.check();
        true
    }
}

#[test]
fn provider_callbacks_and_destructors_can_reenter_registry() {
    let registry = Arc::new(AgentRegistry::new());
    let make_probe = || {
        Box::new(LockProbe {
            registry: Arc::downgrade(&registry),
        })
    };
    assert!(registry.register_provider_if_absent(make_probe()));
    assert!(!registry.register_provider_if_absent(make_probe()));
    assert_eq!(
        registry.detect_from_title("title").as_deref(),
        Some("probe")
    );
    assert_eq!(
        registry.detect_from_output("output").as_deref(),
        Some("probe")
    );
    assert_eq!(
        registry
            .detect_from_output_for(Some("probe"), "output")
            .as_deref(),
        Some("probe")
    );
    assert!(registry.any_provider_screen_has_tui("screen"));
    assert!(registry.provider_for_binary("probe").is_some());
    assert!(registry.has_event_markers("probe", "chunk"));
    assert!(registry.detect_events("probe", "screen").is_empty());
    assert_eq!(registry.try_status_bar_rows("probe"), Some(1));
    assert!(!registry.is_output_noise("probe", "screen"));
    assert!(registry
        .build_prompt_response("probe", 0, "screen")
        .unwrap()
        .is_empty());
    assert!(registry.parse_prompt("probe", "screen").is_none());
    assert!(registry
        .parse_interactive_prompt("probe", "screen")
        .is_none());
    let state = registry.state_snapshot();
    assert!(state.reconciles_status(Some("probe")));
    assert!(state.owns_structured_state(Some("probe")));
    registry.register_provider(make_probe());
    assert!(registry.native_fallback("probe").is_some());
    assert!(registry.unregister_provider("probe"));
}
