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

pub(super) fn settings_schema(provider: &JsProvider) -> Vec<crate::provider::SettingField> {
    use crate::provider::AgentProvider;

    if !provider.has_custom_settings_schema {
        let mut fields = super::manifest_settings_sections(&provider.manifest);
        if provider.auth_status_probe().is_some() {
            fields.push(crate::provider::structured_signals_section());
        }
        return fields;
    }
    // Cached: describe() polls hit this; the plugin schema is a pure constant.
    provider
        .schema_cache
        .get_or_init(|| provider.compute_schemas())
        .0
        .clone()
}

pub(super) fn notifications_schema(provider: &JsProvider) -> Vec<crate::provider::SettingField> {
    provider
        .schema_cache
        .get_or_init(|| provider.compute_schemas())
        .1
        .clone()
}

pub(super) fn model_credential_files(provider: &JsProvider) -> Vec<std::path::PathBuf> {
    provider.manifest.model_catalogue.fingerprint_files(
        super::super::host_credentials::resolved_files(&provider.manifest.credentials),
    )
}
