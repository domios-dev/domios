use crate::agent::registry::AgentRegistry;

pub use crate::multiplexer::types::ModelFallbackNotice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FreshAgentModelResolution {
    pub requested_model: Option<String>,
    pub model: Option<String>,
    pub fallback_notice: Option<ModelFallbackNotice>,
    pub warning: Option<String>,
}

pub(crate) fn attach_planned_model(
    resolution: &mut FreshAgentModelResolution,
    planned_model: Option<&str>,
) {
    let planned_model = planned_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    if let Some(notice) = resolution.fallback_notice.as_mut() {
        notice.planned_model = planned_model;
    }
}

pub(crate) fn persist_model_launch_identity(
    tab: &mut crate::multiplexer::types::Tab,
    planned_model: Option<&str>,
    resolution: &FreshAgentModelResolution,
) {
    tab.agent_planned_model = planned_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    tab.agent_requested_model = resolution.requested_model.clone();
    tab.agent_provider_resolved_model = resolution.model.clone();
    tab.agent_session_model = None;
    tab.agent_session_model_identity = None;
    tab.model_fallback = resolution.fallback_notice.clone();
    tab.model_identity_mismatch = None;
}

pub(crate) fn model_fallback_notice_title(notice: &ModelFallbackNotice) -> String {
    format!(
        "model {} unavailable -> using {}",
        notice.requested_model, notice.provider_resolved_model
    )
}

/// [`codeterm_mem::worker_agent::BackgroundModelPort`] impl over the catalog
/// this crate owns — the seam that lets `codeterm-mem`'s shared worker-agent
/// resolver (used by every memory background-worker call site) reach
/// provider-model compatibility checking without a reverse crate dependency.
pub(crate) struct CatalogBackgroundModelPort;

impl codeterm_mem::worker_agent::BackgroundModelPort for CatalogBackgroundModelPort {
    fn resolve(&self, provider_id: &str, configured: &str) -> (String, bool) {
        let Some(configured) = non_empty_model(configured.to_string()) else {
            return (String::new(), false);
        };
        let canonical = codeterm_types::agent::models::canonicalize_short(&configured);
        let resolved = crate::agent::model_catalog::resolve_model(provider_id, Some(canonical));
        if resolved.fell_back {
            (String::new(), true)
        } else {
            (resolved.id, false)
        }
    }
}

pub(crate) fn resolve_fresh_agent_model(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: Option<String>,
    resume_session_id: Option<&str>,
) -> Option<String> {
    resolve_fresh_agent_model_with_notice(registry, provider_id, requested, resume_session_id).model
}

pub(crate) fn resolve_fresh_agent_model_with_notice(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: Option<String>,
    resume_session_id: Option<&str>,
) -> FreshAgentModelResolution {
    if resume_session_id.is_some() {
        let requested_model = requested
            .and_then(non_empty_model)
            .map(|model| normalize_session_model(&model));
        let warning = requested_model
            .as_deref()
            .filter(|_| live_catalogue_missing(registry, provider_id))
            .map(|model| unavailable_live_catalogue_warning(provider_id, model));
        return FreshAgentModelResolution {
            model: requested_model.clone(),
            requested_model,
            fallback_notice: None,
            warning,
        };
    }
    if let Some(model) = requested.and_then(non_empty_model) {
        let catalogue_missing = live_catalogue_missing(registry, provider_id);
        let matched = if catalogue_missing {
            None
        } else if capability.live_discovery_required {
            crate::agent::model_catalog::resolve_live_catalog_model(provider_id, &model)
        } else {
            crate::agent::model_catalog::resolve_catalog_model(provider_id, &model)
        };
        let resolved_model = matched.map(|entry| entry.id).unwrap_or_else(|| model.clone());
        let warning = catalogue_missing
            .then(|| unavailable_live_catalogue_warning(provider_id, &model));
        return FreshAgentModelResolution {
            model: non_empty_model(resolved_model).map(|id| normalize_session_model(&id)),
            requested_model: Some(normalize_session_model(&model)),
            fallback_notice: None,
            warning,
        };
    }
    if let Some(model) = crate::config::provider_config(provider_id)
        .default_model
        .and_then(non_empty_model)
    {
        let resolved = crate::agent::model_catalog::resolve_model(provider_id, Some(&model));
        if !resolved.fell_back {
            return FreshAgentModelResolution {
                model: non_empty_model(resolved.id).map(|id| normalize_session_model(&id)),
                requested_model: None,
                fallback_notice: None,
                warning: None,
            };
        }
        log::warn!(
            "[spawn] ignoring stale default model '{}' for provider '{}'",
            model,
            provider_id
        );
    }
    let provider_default = registry
        .with_provider(provider_id, |p| p.default_model())
        .flatten()
        .and_then(non_empty_model);
    let resolved =
        crate::agent::model_catalog::resolve_model(provider_id, provider_default.as_deref());
    FreshAgentModelResolution {
        model: non_empty_model(resolved.id).map(|id| normalize_session_model(&id)),
        requested_model: None,
        fallback_notice: None,
        warning: None,
    }
}

fn live_catalogue_missing(registry: &AgentRegistry, provider_id: &str) -> bool {
    registry
        .with_provider(provider_id, |provider| provider.model_catalogue_capability())
        .is_some_and(|capability| {
            capability.authority == crate::agent::provider::ModelCatalogueAuthority::Authoritative
                && capability.live_discovery_required
                && !crate::agent::model_catalog::has_live_snapshot(provider_id)
        })
}

fn unavailable_live_catalogue_warning(provider_id: &str, model: &str) -> String {
    format!(
        "provider '{provider_id}' requires a live model catalogue, but none is available; launching requested model '{model}' unchanged. Run `codeterm agent models {provider_id}` to refresh and inspect the catalogue."
    )
}

/// The four short labels the catalog canonicalizes to. A model id outside this
/// set is the provider's own text and is kept verbatim.
const CANONICAL_SHORT_LABELS: [&str; 4] = ["opus", "sonnet", "haiku", "fable"];

/// One spelling for one model, whichever producer supplied it. The launch side
/// hands over catalog ids (`opus`, `claude-fable-5-1`) and the runtime detector
/// hands over the provider's display name (`Fable`); without this fold the two
/// disagree on the same tab and every label reader inherits the disagreement.
pub(crate) fn normalize_session_model(model: &str) -> String {
    let trimmed = model.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let canonical = codeterm_types::agent::models::canonicalize_short(&lowered);
    if CANONICAL_SHORT_LABELS.contains(&canonical) {
        canonical.to_string()
    } else {
        trimmed.to_string()
    }
}

/// A tab launched with a DECLARED model was dispatched as that agent; running a
/// different one is a failure, not a note. Without a declaration the operator
/// took the default and a substitution is only worth a warning.
pub(crate) fn unrequested_substitution_severity(planned_model: Option<&str>) -> &'static str {
    match planned_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        Some(_) => "error",
        None => "warning",
    }
}

pub(crate) fn non_empty_model(model: String) -> Option<String> {
    let trimmed = model.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod model_casing_tests {
    use super::*;

    #[test]
    fn both_producers_spell_a_known_model_the_same_way() {
        // The launch side supplies catalog ids; the runtime detector supplies
        // the provider's display name. Same tab, same label, either way.
        for (launch_side, runtime_side) in [
            ("opus", "Opus"),
            ("claude-opus-5", "OPUS"),
            ("fable", "Fable"),
            ("claude-fable-5-1", "Fable"),
            ("sonnet", " Sonnet "),
            ("haiku", "Haiku"),
        ] {
            assert_eq!(
                normalize_session_model(launch_side),
                normalize_session_model(runtime_side),
                "{launch_side} vs {runtime_side}"
            );
        }
    }

    #[test]
    fn a_known_model_folds_to_its_canonical_short_label() {
        assert_eq!(normalize_session_model("Fable"), "fable");
        assert_eq!(normalize_session_model("claude-opus-5"), "opus");
    }

    #[test]
    fn an_unknown_model_id_keeps_the_providers_own_text() {
        assert_eq!(normalize_session_model("GPT-5-Codex"), "GPT-5-Codex");
        assert_eq!(normalize_session_model("  qwen3-coder  "), "qwen3-coder");
    }

    #[test]
    fn an_unrequested_substitution_on_a_declared_model_is_a_failure() {
        assert_eq!(unrequested_substitution_severity(Some("opus")), "error");
    }

    #[test]
    fn a_substitution_with_no_declared_model_stays_a_warning() {
        for planned in [None, Some(""), Some("   ")] {
            assert_eq!(unrequested_substitution_severity(planned), "warning");
        }
    }
}
