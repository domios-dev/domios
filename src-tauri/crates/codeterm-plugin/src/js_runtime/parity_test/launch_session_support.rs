use super::*;

#[test]
fn launch_session_detection_capability_native_js_parity() {
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let exact = load_inline_js(
        "exact-launch",
        r#"module.exports.default = { detectLaunchSession: function() { return null; } };"#,
    );
    let absent = load_inline_js("absent-launch", "module.exports.default = {};");
    assert_eq!(
        native.supports_launch_session_detection(),
        exact.supports_launch_session_detection()
    );
    assert_eq!(
        DiscoverModelsNativeStub.supports_launch_session_detection(),
        absent.supports_launch_session_detection()
    );
    assert!(!absent.supports_launch_session_detection());
}

#[test]
fn two_unhooked_tabs_in_one_cwd_detect_their_own_launch_markers() {
    use crate::provider::{LaunchSessionEvidence, LaunchSessionSource};
    let js = load_inline_js(
        "marker-launch",
        r#"module.exports.default = {
        files: { "marker-a": { modified: 10 }, "marker-b": { modified: 20 } },
        detectSessionId: function() {
            return Object.keys(this.files).sort((a, b) => this.files[b].modified - this.files[a].modified)[0];
        },
        detectLaunchSession: function(evidence) {
            return this.files[evidence.launchMarker]
                ? { sessionId: evidence.launchMarker, source: "launch_marker" } : null;
        }
    };"#,
    );
    assert!(!js.has_hooks());
    assert!(js.supports_launch_session_detection());
    assert_eq!(
        js.detect_session_id("/shared/project", &[], None)
            .as_deref(),
        Some("marker-b")
    );
    for marker in ["marker-a", "marker-b", "marker-missing"] {
        let evidence = LaunchSessionEvidence {
            cwd: "/shared/project".into(),
            launch_marker: Some(marker.into()),
            launched_at_ms: 0,
            processes: vec![],
        };
        let detected = js.detect_launch_session(&evidence);
        if marker == "marker-missing" {
            assert!(detected.is_none());
            continue;
        }
        let detected = detected.unwrap();
        assert_eq!(detected.session_id, marker);
        assert_eq!(detected.source, LaunchSessionSource::LaunchMarker);
    }
}
