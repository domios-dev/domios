use super::JsProvider;
use crate::agent_runtime::RuntimeProfileLimitsSource;
use serde::Deserialize;
use serde_json::json;

const MAX_FAILURE_CHARS: usize = 240;

/// A plugin answers with its UsageSnapshot-shaped reading, or a failure naming the cause.
#[derive(Deserialize)]
#[serde(untagged)]
enum ProfileLimitsReply {
    Reading(String),
    Failure { error: String },
}

/// The reason is plugin-authored and reaches the store and the bubble as one bounded line.
fn bounded_failure(error: &str) -> String {
    let line: String = error
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_FAILURE_CHARS)
        .collect();
    let line = line.trim();
    if line.is_empty() {
        "Balance read failed".to_string()
    } else {
        line.to_string()
    }
}

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
        let outcome = match super::call_json::<Option<ProfileLimitsReply>>(
            &self.vm,
            "readProfileLimits",
            &[request, json!(now_ms)],
        ) {
            Ok(Some(ProfileLimitsReply::Reading(raw))) => Ok(raw),
            Ok(Some(ProfileLimitsReply::Failure { error })) => Err(bounded_failure(&error)),
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

#[cfg(test)]
mod tests;
