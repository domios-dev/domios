use super::{load_inline_js, ClassifyNativeStub, DiscoverModelsNativeStub};
use crate::provider::AgentProvider;

#[test]
fn unbound_submit_recovery_opt_in_native_js_parity() {
    let native = ClassifyNativeStub;
    let js = load_inline_js(
        "unbound-submit-recovery-fixture",
        r#"
            module.exports.default = {
                supportsUnboundSubmitRecovery() { return true; }
            };
        "#,
    );
    assert!(native.supports_unbound_submit_recovery());
    assert_eq!(
        js.supports_unbound_submit_recovery(),
        native.supports_unbound_submit_recovery()
    );

    let native_default = DiscoverModelsNativeStub;
    let js_default = load_inline_js(
        "unbound-submit-recovery-default",
        "module.exports.default = {};",
    );
    assert!(!native_default.supports_unbound_submit_recovery());
    assert_eq!(
        js_default.supports_unbound_submit_recovery(),
        native_default.supports_unbound_submit_recovery()
    );
}
