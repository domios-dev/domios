use super::JsProvider;
use crate::agent_runtime::RuntimeProfileLimitsSource;
use serde_json::json;

impl JsProvider {
    /// The plugin reads its declared balance; the credential reaches only the outbound request header.
    pub(super) fn js_read_profile_limits(
        &self,
        profile: &str,
        source: &RuntimeProfileLimitsSource,
    ) -> Option<Result<String, String>> {
        if !matches!(source, RuntimeProfileLimitsSource::Balance { .. })
            || !self.has_js("readProfileLimits")
        {
            return None;
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        let denials_before = self.vm.capability_denial_count();
        let request = json!({"profile": profile, "source": source});
        let outcome = match super::call_json::<Option<String>>(
            &self.vm,
            "readProfileLimits",
            &[request, json!(now_ms)],
        ) {
            Ok(Some(raw)) => Ok(raw),
            Ok(None) => Err("readProfileLimits returned null".to_string()),
            Err(_) => Err(format!(
                "[plugin {}] readProfileLimits failed",
                self.manifest.id
            )),
        };
        Some(crate::provider::attribute_capability_denial(
            outcome,
            denials_before,
            self.vm.capability_denial_count(),
        ))
    }
}
