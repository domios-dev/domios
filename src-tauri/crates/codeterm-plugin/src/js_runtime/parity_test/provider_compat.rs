use super::*;
use crate::provider_compat::{CompatibilityManifest, CompatibilityVerdict};

#[test]
fn parity_provider_compat_claude_manifest_and_default() {
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    assert_eq!(native.cli_compatibility(), js.cli_compatibility());
    for version in [None, Some("2.1.292"), Some("3.0.0"), Some("bad")] {
        assert_eq!(
            native.cli_compatibility().unwrap().evaluate(version),
            js.cli_compatibility().unwrap().evaluate(version),
        );
    }
    let undeclared = load_inline_js("no-policy", "module.exports.default = {};");
    assert_eq!(
        DiscoverModelsNativeStub.cli_compatibility(),
        undeclared.cli_compatibility()
    );
}

#[test]
fn parity_provider_compat_bundled_declarations_are_strict_and_honest() {
    for id in ["claude", "codex", "opencode", "cursor"] {
        let js = load_js(id);
        let manifest: CompatibilityManifest =
            js.cli_compatibility().expect("declared compatibility");
        manifest.validate().unwrap();
        assert_eq!(
            manifest.evaluate(None).verdict,
            CompatibilityVerdict::Unknown
        );
        if manifest.direct_sqlite {
            assert_eq!(
                manifest.evaluate(Some("2.0.0")).verdict,
                CompatibilityVerdict::Breaking
            );
            assert_eq!(
                manifest.evaluate(Some("1.18.33")).verdict,
                CompatibilityVerdict::Unknown
            );
            assert_eq!(
                manifest.evaluate(Some("1.18.35")).verdict,
                CompatibilityVerdict::Supported
            );
            assert_eq!(
                manifest.evaluate(Some("1.18.36")).verdict,
                CompatibilityVerdict::Unknown
            );
            for version in ["1.19.0", "1.19.99"] {
                assert_eq!(
                    manifest.evaluate(Some(version)).verdict,
                    CompatibilityVerdict::Supported
                );
            }
            assert_eq!(
                manifest.evaluate(Some("1.20.0")).verdict,
                CompatibilityVerdict::Unknown
            );
            assert_eq!(
                manifest.evaluate(Some("3.0.0")).verdict,
                CompatibilityVerdict::Unknown
            );
        }
    }
}

#[test]
fn parity_provider_compat_rejects_invalid_ranges_before_plugin_load() {
    let json = r#"{"id":"invalid-policy","version":"1.0.0","displayName":"Fixture",
        "cliCompatibility":{"supported":["bad range"]}}"#;
    assert!(super::super::PluginManifest::from_json_str(json).is_err());
}
