use super::*;

struct CountingProvider {
    base: FallbackStub,
    calls: Arc<Mutex<Vec<String>>>,
}

impl AgentProvider for CountingProvider {
    fn id(&self) -> &str {
        self.base.id()
    }
    fn display_name(&self) -> &str {
        self.base.display_name()
    }
    fn build_launch_command(&self, params: &crate::agent::provider::AgentLaunchParams) -> String {
        self.base.build_launch_command(params)
    }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        self.base.readiness_strategy()
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        self.base.task_delivery()
    }
    fn detect_from_title(&self, text: &str) -> bool {
        self.base.detect_from_title(text)
    }
    fn detect_from_output(&self, text: &str) -> bool {
        self.calls.lock().push(self.id().to_string());
        text == self.id()
    }
    fn session_file_path(&self, cwd: &str, session: &str) -> Option<std::path::PathBuf> {
        self.base.session_file_path(cwd, session)
    }
    fn detect_session_id(
        &self,
        cwd: &str,
        known: &[String],
        max_age: Option<Duration>,
    ) -> Option<String> {
        self.base.detect_session_id(cwd, known, max_age)
    }
    fn build_resume_command(&self, session: &str, skip: bool) -> Option<String> {
        self.base.build_resume_command(session, skip)
    }
}

#[test]
fn own_provider_is_first_and_fanout_only_follows_a_miss() {
    let registry = AgentRegistry::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    for id in ["own", "other"] {
        registry.register_provider(Box::new(CountingProvider {
            base: FallbackStub { id, native: false },
            calls: calls.clone(),
        }));
    }
    assert_eq!(
        registry.detect_from_output_for(Some("own"), "own"),
        Some("own".into())
    );
    assert_eq!(*calls.lock(), ["own"]);
    calls.lock().clear();
    assert_eq!(
        registry.detect_from_output_for(Some("own"), "other"),
        Some("other".into())
    );
    assert_eq!(*calls.lock(), ["own", "other"]);
    calls.lock().clear();
    assert_eq!(registry.detect_from_output_for(None, "absent"), None);
    assert_eq!(calls.lock().len(), 2);
    calls.lock().clear();
    assert_eq!(
        registry.detect_from_output_for(Some("unregistered"), "absent"),
        None
    );
    assert_eq!(calls.lock().len(), 2);
}
