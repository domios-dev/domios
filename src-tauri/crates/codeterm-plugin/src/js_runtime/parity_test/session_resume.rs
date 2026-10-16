use super::*;

#[test]
fn exact_session_resume_native_js_parity() {
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_inline_js(
        "resume-fixture",
        "module.exports = {sessionResume: () => ({exact: true})};",
    );
    assert!(native.supports_exact_session_resume());
    assert_eq!(
        native.supports_exact_session_resume(),
        js.supports_exact_session_resume()
    );
}

#[test]
fn exact_session_resume_requires_an_explicit_true_declaration() {
    for source in [
        "module.exports = {};",
        "module.exports = {sessionResume: () => ({exact: false})};",
        "module.exports = {sessionResume: () => ({})};",
        "module.exports = {sessionResume: () => ({exact: 'true'})};",
    ] {
        let js = load_inline_js("resume-fixture", source);
        assert!(!DiscoverModelsNativeStub.supports_exact_session_resume());
        assert!(!js.supports_exact_session_resume());
    }
}

#[test]
fn bundled_resume_declarations_match_session_restore_support() {
    for (id, expected) in [
        ("claude", true),
        ("codex", true),
        ("opencode", true),
        ("cursor", false),
    ] {
        assert_eq!(
            load_js(id).supports_exact_session_resume(),
            expected,
            "{id}"
        );
    }
}
