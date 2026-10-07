//! Real `opencode models` output must keep custom-provider models (the
//! `xiaomi-token-plan-sgp` lines) through the listing read path. The seam that
//! used to drop them is a stale registry: a changed `opencode.json` never voided
//! the snapshot and free-form listings never re-probed.

use super::*;

/// Captured `opencode models` stdout from a machine whose
/// `~/.config/opencode/opencode.json` declares custom providers. Full output,
/// one id per line.
const REAL_OPENCODE_MODELS_OUTPUT: &str = "\
opencode/big-pickle
opencode/claude-fable-5
opencode/claude-fable-5-1
opencode/claude-haiku-4-5
opencode/claude-opus-4-5
opencode/claude-opus-4-6
opencode/claude-opus-4-7
opencode/claude-opus-4-8
opencode/claude-opus-5
opencode/claude-opus-5-5
opencode/claude-sonnet-4
opencode/claude-sonnet-4-5
opencode/claude-sonnet-4-6
opencode/claude-sonnet-5
opencode/claude-sonnet-5-5
opencode/deepseek-v4-flash
opencode/deepseek-v4-flash-vision-exp
opencode/deepseek-v4-pro
opencode/deepseek-v4.1-flash
opencode/fledge-alpha-free
opencode/gemini-3-flash
opencode/gemini-3.1-pro
opencode/gemini-3.5-flash
opencode/gemini-3.5-flash-lite
opencode/gemini-3.6-flash
opencode/gemini-3.7-flash
opencode/gemini-3.8-flash
opencode/glm-5
opencode/glm-5.1
opencode/glm-5.2
opencode/glm-5.3
opencode/glm-5.3-flash
opencode/gpt-5
opencode/gpt-5-codex
opencode/gpt-5-nano
opencode/gpt-5.1
opencode/gpt-5.1-codex
opencode/gpt-5.1-codex-max
opencode/gpt-5.1-codex-mini
opencode/gpt-5.2
opencode/gpt-5.2-codex
opencode/gpt-5.3-codex
opencode/gpt-5.3-codex-spark
opencode/gpt-5.4
opencode/gpt-5.4-mini
opencode/gpt-5.4-nano
opencode/gpt-5.4-pro
opencode/gpt-5.5
opencode/gpt-5.5-pro
opencode/gpt-5.6-luna
opencode/gpt-5.6-sol
opencode/gpt-5.6-terra
opencode/gpt-6-astra
opencode/gpt-6-luna
opencode/gpt-6-sol
opencode/gpt-6.1-sol
opencode/grok-4.5
opencode/grok-4.6
opencode/grok-4.7
opencode/grok-build-0.1
opencode/kimi-k2.5
opencode/kimi-k2.6
opencode/kimi-k2.7-code
opencode/kimi-k3
opencode/ling-3.0-flash-fin-free
opencode/ling-3.1-flash-free
opencode/longcat-2.5-preview-free
opencode/mimo-v2.6-flash-free
opencode/minimax-m2.5
opencode/minimax-m2.7
opencode/minimax-m3
opencode/muse-spark-1.2
opencode/muse-spark-1.3
opencode/muse-spark-1.3-contributor-free
opencode/nemotron-3-ultra-free
opencode/nemotron-3.5-lightning-free
opencode/qwen3.5-plus
opencode/qwen3.6-plus
opencode/qwen3.8-flash
opencode/qwen3.8-max
opencode/space-bunny-free
deepseek/deepseek-flash
deepseek/deepseek-v4-pro
mimo-sgp-anthropic/mimo-v2.6-flash
mimo-sgp-anthropic/mimo-v2.6-pro
xai/grok-4.20-0309-non-reasoning
xai/grok-4.20-0309-reasoning
xai/grok-4.20-multi-agent-0309
xai/grok-4.3
xai/grok-4.5
xai/grok-4.6
xai/grok-4.7
xai/grok-build-0.1
xai/grok-imagine-image
xai/grok-imagine-image-quality
xai/grok-imagine-video
xai/grok-imagine-video-1.5
xai/grok-imagine-video-1.5-lite
xiaomi-token-plan-sgp/mimo-v2-tts
xiaomi-token-plan-sgp/mimo-v2.5
xiaomi-token-plan-sgp/mimo-v2.5-pro
xiaomi-token-plan-sgp/mimo-v2.5-tts
xiaomi-token-plan-sgp/mimo-v2.5-tts-voiceclone
xiaomi-token-plan-sgp/mimo-v2.5-tts-voicedesign
xiaomi-token-plan-sgp/mimo-v2.6-flash
xiaomi-token-plan-sgp/mimo-v2.6-pro
";

const XIAOMI_PRO: &str = "xiaomi-token-plan-sgp/mimo-v2.6-pro";
const XIAOMI_FLASH: &str = "xiaomi-token-plan-sgp/mimo-v2.6-flash";

/// The `discoverModels` result for the captured output: the display name is the
/// id and the group is the provider prefix (`opencode` -> "OpenCode Zen"),
/// mirroring plugins/provider-opencode/plugin.js. This is the discovery fixture;
/// the catalogue read path below is the unit under test.
fn discovered_models() -> Vec<ModelInfo> {
    REAL_OPENCODE_MODELS_OUTPUT
        .lines()
        .map(|id| {
            let group = match id.split('/').next() {
                Some("opencode") => "OpenCode Zen",
                Some(prefix) => prefix,
                None => "OpenCode Zen",
            };
            ModelInfo {
                group: Some(group.into()),
                ..model(id, id)
            }
        })
        .collect()
}

/// The seed plugins/provider-opencode/plugin.json registers at startup.
fn seed_models() -> Vec<ModelInfo> {
    [
        ("anthropic/claude-opus-4-8", "Claude Opus 4.8", "Anthropic"),
        (
            "anthropic/claude-sonnet-4-6",
            "Claude Sonnet 4.6",
            "Anthropic",
        ),
        (
            "opencode/gpt-5.4-mini",
            "OpenCode Zen GPT-5.4 Mini",
            "OpenCode Zen",
        ),
        ("opencode/gpt-5.5", "OpenCode Zen GPT-5.5", "OpenCode Zen"),
        (
            "opencode/deepseek-v4-flash-free",
            "OpenCode Zen DeepSeek V4 Flash Free",
            "OpenCode Zen",
        ),
        (
            "opencode/big-pickle",
            "OpenCode Zen Big Pickle",
            "OpenCode Zen",
        ),
    ]
    .into_iter()
    .map(|(id, display, group)| ModelInfo {
        group: Some(group.into()),
        ..model(id, display)
    })
    .collect()
}

/// A provider whose model list depends on its own config file (opencode.json),
/// so the configFiles fingerprint path drives invalidation like credentials do.
struct ConfigFileStub {
    id: String,
    catalogue: crate::provider::ModelCatalogueCapability,
}

#[async_trait::async_trait]
impl AgentProvider for ConfigFileStub {
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
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn model_catalogue_capability(&self) -> crate::provider::ModelCatalogueCapability {
        self.catalogue.clone()
    }
    fn model_credential_files(&self) -> Vec<PathBuf> {
        self.catalogue.fingerprint_files(Vec::new())
    }
    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        Some(discovered_models())
    }
}

fn config_stub(id: &str, config: &std::path::Path) -> ConfigFileStub {
    ConfigFileStub {
        id: id.to_string(),
        catalogue: crate::provider::ModelCatalogueCapability {
            config_files: vec![config.to_string_lossy().into_owned()],
            ..Default::default()
        },
    }
}

/// The live listing keeps every custom-provider model the probe reported: the
/// overlay may decorate them but neither drops them nor owns their availability.
#[test]
fn real_opencode_listing_keeps_custom_provider_models() {
    let id = "test-opencode-listing-xiaomi";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: seed_models(),
            default: Some("anthropic/claude-opus-4-8".into()),
        },
    );
    let discovered = discovered_models();
    assert!(
        discovered.iter().any(|m| m.id == XIAOMI_PRO),
        "fixture sanity: discovery reports the custom provider"
    );
    put_live_with_credentials(
        id,
        discovered,
        Some("anthropic/claude-opus-4-8".into()),
        None,
        Vec::new(),
    );

    // A user override that never names the custom models' provider, restates one
    // custom id without `group`, and tries to veto its availability.
    std::fs::write(
        override_path(id),
        format!(
            r#"[[models]]
id = "{XIAOMI_PRO}"
display_name = "MiMo v2.6 Pro (custom)"
available = false

[[models]]
id = "opencode/big-pickle"
display_name = "Big Pickle (custom)"
"#
        ),
    )
    .unwrap();

    let listing = resolve_models(id);
    let pro = listing
        .iter()
        .find(|m| m.id == XIAOMI_PRO)
        .expect("listing keeps the custom-provider model");
    assert_eq!(
        pro.group.as_deref(),
        Some("xiaomi-token-plan-sgp"),
        "the provider's group survives a restating overlay"
    );
    assert_eq!(pro.display_name, "MiMo v2.6 Pro (custom)");
    assert_eq!(
        pro.availability(),
        ModelAvailability::Available,
        "availability comes from the base probe, never the override"
    );
    let flash = listing
        .iter()
        .find(|m| m.id == XIAOMI_FLASH)
        .expect("an overlayed id the override never names keeps its base entry");
    assert_eq!(flash.group.as_deref(), Some("xiaomi-token-plan-sgp"));
    cleanup(id);
}

/// A changed declared config file voids the cached answer inside the TTL, and
/// the refreshed listing recovers the custom models - the seam the stale
/// registry used to lose them at.
#[tokio::test]
async fn changed_opencode_config_reprobes_and_the_listing_recovers_custom_models() {
    let id = "test-opencode-config-reprobe";
    cleanup(id);
    register(
        id,
        RegisteredDefaults {
            models: vec![model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("opencode.json");
    std::fs::write(&config, b"{\"provider\":{}}").unwrap();
    let provider: Arc<dyn AgentProvider> = Arc::new(config_stub(id, &config));

    // Snapshot captured against the old config: no custom providers yet.
    put_live_with_credentials(
        id,
        vec![model("seed", "Seed")],
        Some("seed".into()),
        None,
        credential_fingerprints(provider.as_ref()),
    );
    assert!(
        !is_stale(id, DEFAULT_TTL, SystemTime::now()),
        "fresh by TTL, so only the config change can force a retry"
    );

    // The user adds custom providers to opencode.json.
    std::fs::write(&config, b"{\"provider\":{\"xiaomi-token-plan-sgp\":{}}}").unwrap();

    let refreshed = refresh_for_reader(provider).await;
    assert!(
        refreshed.is_some(),
        "a changed declared config must force a probe inside the TTL"
    );
    assert!(
        resolve_models(id).iter().any(|m| m.id == XIAOMI_PRO),
        "the refreshed listing recovers the custom-provider model"
    );
    cleanup(id);
}
