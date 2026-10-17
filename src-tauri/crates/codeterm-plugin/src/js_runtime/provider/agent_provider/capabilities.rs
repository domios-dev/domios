use super::JsProvider;

pub(super) fn supports_subagent_visibility(provider: &JsProvider) -> bool {
    provider.manifest.capabilities.subagent_visibility
        || provider
            .native()
            .is_some_and(|native| native.supports_subagent_visibility())
}

pub(super) fn record_source<'a>(
    provider: &'a JsProvider,
    source: &str,
) -> Option<&'a crate::jsonl_window::records::RecordSource> {
    let hook = match source {
        "sessionTranscript" => "parseSessionRecords",
        "storedHistory" => "runtimeStoredRecords",
        _ => return None,
    };
    if !provider.has_js(hook) {
        return None;
    }
    provider.manifest.capabilities.record_sources.get(source)
}
