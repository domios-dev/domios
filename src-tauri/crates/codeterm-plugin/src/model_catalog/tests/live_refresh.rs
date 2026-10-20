//! A live probe either publishes or names why it did not.

use super::*;

/// Resolves its own CLI during discovery, as a JS provider's binary fingerprint does.
struct ResolvingStub {
    id: String,
    binary: String,
    during_discovery: Option<Box<dyn Fn() + Send + Sync>>,
}

#[async_trait::async_trait]
impl AgentProvider for ResolvingStub {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn cli_binary(&self) -> Option<&str> {
        Some(&self.binary)
    }
    fn build_launch_command(&self, _: &crate::provider::AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        let _ = crate::js_runtime::probe_bin(&self.binary);
        if let Some(change) = &self.during_discovery {
            change();
        }
        Some(vec![model("fixture/live", "Live")])
    }
}

fn executable(tag: &str) -> PathBuf {
    let path = temp_binary(tag, b"v1");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[tokio::test]
async fn a_probe_that_resolves_its_cli_still_publishes() {
    let id = "test-live-refresh-resolving-cli";
    cleanup(id);
    let bin = executable("resolving");
    let provider = Arc::new(ResolvingStub {
        id: id.into(),
        binary: bin.to_string_lossy().into_owned(),
        during_discovery: None,
    });
    let published = refresh_live(provider, Duration::from_secs(5)).await;
    let _ = std::fs::remove_file(&bin);
    assert_eq!(published.map(|models| models.len()), Some(1));
    assert_eq!(last_refresh_failure(id), None);
    assert!(!provider_binary_changed(id));
    cleanup(id);
}

#[tokio::test]
async fn a_cli_rewritten_mid_probe_names_the_changed_input() {
    let id = "test-live-refresh-cli-rewritten";
    cleanup(id);
    let bin = executable("rewritten");
    let rewrite = bin.clone();
    let provider = Arc::new(ResolvingStub {
        id: id.into(),
        binary: bin.to_string_lossy().into_owned(),
        during_discovery: Some(Box::new(move || {
            std::fs::write(&rewrite, b"v2-longer-payload").unwrap();
        })),
    });
    assert!(refresh_live(provider, Duration::from_secs(5))
        .await
        .is_none());
    let _ = std::fs::remove_file(&bin);
    assert_eq!(
        last_refresh_failure(id),
        Some(RefreshFailure::InputsChangedDuringDiscovery)
    );
    cleanup(id);
}

#[tokio::test]
async fn a_failed_probe_records_its_reason_until_one_publishes() {
    let id = "test-live-refresh-failure-reason";
    cleanup(id);
    let silent = Arc::new(DiscoverStub {
        id: id.into(),
        default: None,
        discovery: None,
    });
    assert!(refresh_live(silent, Duration::from_secs(1)).await.is_none());
    assert_eq!(last_refresh_failure(id), Some(RefreshFailure::NoLiveList));

    let non_chat = Arc::new(DiscoverStub {
        id: id.into(),
        default: None,
        discovery: Some(vec![model("text-embedding-3", "Embedding")]),
    });
    assert!(refresh_live(non_chat, Duration::from_secs(1))
        .await
        .is_none());
    assert_eq!(
        last_refresh_failure(id),
        Some(RefreshFailure::OnlyNonChatModels)
    );
    assert!(!RefreshFailure::OnlyNonChatModels.to_string().is_empty());

    let live = Arc::new(DiscoverStub {
        id: id.into(),
        default: None,
        discovery: Some(vec![model("fixture/live", "Live")]),
    });
    assert!(refresh_live(live, Duration::from_secs(1)).await.is_some());
    assert_eq!(last_refresh_failure(id), None);
    cleanup(id);
}
