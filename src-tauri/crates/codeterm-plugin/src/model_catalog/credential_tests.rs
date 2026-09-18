//! Cache-invalidation tests driven by the provider binary and by credential
//! files. Split out of `model_catalog/tests.rs` to keep that file under the
//! source-size limit.

use super::*;

/// Provider whose live list is scoped to one credential file, so the catalog's
/// credential invalidation can be driven without a real CLI.
struct CredentialStub {
    id: String,
    credential: PathBuf,
}

#[async_trait::async_trait]
impl AgentProvider for CredentialStub {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
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
    fn model_credential_files(&self) -> Vec<PathBuf> {
        vec![self.credential.clone()]
    }
    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        Some(vec![model("live", "Live")])
    }
}

/// A cached answer describes one binary. Replace that binary and the answer is
/// no longer authoritative, whatever its TTL says.
#[test]
fn cache_is_not_served_when_the_provider_binary_changed() {
    let id = "test-cache-binary-changed";
    cleanup(id);
    let seed = RegisteredDefaults {
        models: vec![model("opus", "Seed")],
        default: Some("opus".into()),
    };
    register(id, seed.clone());
    let bin = temp_binary("changed", b"v1");
    put_live_against(
        id,
        vec![model("opus", "Live")],
        Some("opus".into()),
        fingerprint_path(&bin),
    );

    // The CLI self-updates: same path, different bytes.
    std::fs::write(&bin, b"v2-longer-payload").unwrap();
    assert!(provider_binary_changed(id));

    registry().write().remove(id);
    seed_registry().write().remove(id);
    register(id, seed);
    prime_from_cache(id);

    let _ = std::fs::remove_file(&bin);
    assert_eq!(
        resolve_models(id)[0].display_name,
        "Seed",
        "a snapshot captured against a replaced binary must not prime"
    );
    assert!(
        is_stale(id, DEFAULT_TTL, SystemTime::now()),
        "a rejected snapshot must not carry its TTL forward"
    );
    cleanup(id);
}

/// The same snapshot against an untouched binary is still good, so the
/// invalidation cannot be a blanket refusal to warm-start.
#[test]
fn cache_still_primes_when_the_provider_binary_is_untouched() {
    let id = "test-cache-binary-untouched";
    cleanup(id);
    let seed = RegisteredDefaults {
        models: vec![model("opus", "Seed")],
        default: Some("opus".into()),
    };
    register(id, seed.clone());
    let bin = temp_binary("untouched", b"v1");
    put_live_against(
        id,
        vec![model("opus", "Live")],
        Some("opus".into()),
        fingerprint_path(&bin),
    );
    assert!(!provider_binary_changed(id));

    registry().write().remove(id);
    seed_registry().write().remove(id);
    register(id, seed);
    prime_from_cache(id);

    let display = resolve_models(id)[0].display_name.clone();
    let _ = std::fs::remove_file(&bin);
    assert_eq!(display, "Live");
    cleanup(id);
}

/// A credential-scoped list must re-probe the moment the credential appears:
/// connecting a provider is the event, and the TTL knows nothing about it.
#[tokio::test]
async fn reader_refresh_reprobes_when_a_credential_file_appears() {
    let id = "test-reader-refresh-credential-appeared";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let credential = dir.path().join("auth.json");
    // Snapshot captured before the provider was connected: no credential file.
    put_live_with_credentials(
        id,
        vec![model("seed", "Seed")],
        Some("seed".into()),
        None,
        Vec::new(),
    );
    assert!(
        !is_stale(id, DEFAULT_TTL, SystemTime::now()),
        "fresh by TTL, so only the credential change can force a retry"
    );

    let provider: Arc<dyn AgentProvider> = Arc::new(CredentialStub {
        id: id.to_string(),
        credential: credential.clone(),
    });
    // Connecting the provider writes the credential file.
    std::fs::write(&credential, b"{\"x\":1}").unwrap();

    let refreshed = refresh_for_reader(provider).await;
    assert!(
        refreshed.is_some(),
        "a credential file appearing must force a probe inside the TTL"
    );
    assert_eq!(resolve_models(id)[0].id, "live");
    cleanup(id);
}

/// A cached answer captured against a credential file that later changed is no
/// longer authoritative, whatever its TTL says.
#[test]
fn cache_is_not_served_when_a_credential_file_changed() {
    let id = "test-cache-credential-changed";
    cleanup(id);
    let dir = tempfile::tempdir().unwrap();
    let credential = dir.path().join("auth.json");
    std::fs::write(&credential, b"v1").unwrap();
    let seed = RegisteredDefaults {
        models: vec![model("opus", "Seed")],
        default: Some("opus".into()),
    };
    register(id, seed.clone());
    put_live_with_credentials(
        id,
        vec![model("opus", "Live")],
        Some("opus".into()),
        None,
        vec![fingerprint_path(&credential).unwrap()],
    );

    std::fs::write(&credential, b"v2-longer-payload").unwrap();

    registry().write().remove(id);
    seed_registry().write().remove(id);
    register(id, seed);
    prime_from_cache(id);

    assert_eq!(
        resolve_models(id)[0].display_name,
        "Seed",
        "a snapshot captured against a changed credential must not prime"
    );
    cleanup(id);
}
