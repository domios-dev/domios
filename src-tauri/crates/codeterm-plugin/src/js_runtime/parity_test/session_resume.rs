use super::*;

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
