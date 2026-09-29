use std::fmt;

use crate::agent::provider::{ModelCatalogueAuthority, ModelInfo};
use crate::agent::registry::AgentRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InvalidModelError {
    pub provider_id: String,
    pub requested_model: String,
    pub closest_model_ids: Vec<String>,
    pub provider_mismatch: Option<String>,
    pub ambiguous_providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentModelSelection {
    pub provider_id: String,
    pub model: Option<String>,
}

impl InvalidModelError {
    pub(crate) fn rpc_data(&self) -> serde_json::Value {
        serde_json::json!({
            "type": if self.ambiguous_providers.is_empty() { "invalid_model" } else { "ambiguous_model" },
            "provider": self.provider_id.clone(),
            "requested_model": self.requested_model.clone(),
            "closest_model_ids": self.closest_model_ids.clone(),
            "ambiguous_providers": self.ambiguous_providers.clone(),
            "refresh_command": self.refresh_command(),
            "message": self.to_string(),
        })
    }

    pub(crate) fn refresh_command(&self) -> String {
        format!("codeterm agent models {}", self.provider_id)
    }
}

impl fmt::Display for InvalidModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.ambiguous_providers.is_empty() {
            return write!(
                formatter,
                "ambiguous model id '{}': owned by {}; pass --agent to select a provider on dispatch, or provide it positionally to agent spawn",
                self.requested_model,
                self.ambiguous_providers.join(", ")
            );
        }
        if let Some(requested_provider) = self.provider_mismatch.as_deref() {
            return write!(
                formatter,
                "model provider '{}' does not match selected provider '{}'; select that provider explicitly or choose one of its model ids",
                requested_provider, self.provider_id
            );
        }
        let suggestions = if self.closest_model_ids.is_empty() {
            "no model ids are currently advertised".to_string()
        } else {
            self.closest_model_ids.join(", ")
        };
        write!(
            formatter,
            "model id '{}' is not available from provider '{}'; valid model ids: {}",
            self.requested_model, self.provider_id, suggestions
        )
    }
}

pub(crate) fn model_catalogue_capability(
    registry: &AgentRegistry,
    provider_id: &str,
) -> crate::agent::provider::ModelCatalogueCapability {
    registry
        .with_provider(provider_id, |provider| {
            provider.model_catalogue_capability()
        })
        .unwrap_or_default()
}

pub(crate) fn normalize_requested_model(provider_id: &str, requested: &str) -> String {
    let requested = requested.trim();
    match requested.split_once(':') {
        Some((prefix, model)) if prefix.eq_ignore_ascii_case(provider_id) => model.trim().into(),
        _ => requested.into(),
    }
}

pub(crate) fn catalogue_lookup_model(requested: &str, catalogue: &[ModelInfo]) -> String {
    let requested = requested.trim();
    let without_context = requested
        .get(..requested.len().saturating_sub(4))
        .filter(|_| requested.to_ascii_lowercase().ends_with("[1m]"))
        .unwrap_or(requested);
    let lowercase = without_context.to_ascii_lowercase();
    let canonical = codeterm_types::agent::models::canonicalize_short(&lowercase);
    if canonical != lowercase
        && catalogue
            .iter()
            .any(|model| model.available != Some(false) && model.id.eq_ignore_ascii_case(canonical))
    {
        canonical.to_string()
    } else {
        lowercase
    }
}

pub(crate) fn catalogue_models_match(
    registry: &AgentRegistry,
    provider_id: &str,
    left: &str,
    right: &str,
) -> bool {
    let capability = model_catalogue_capability(registry, provider_id);
    let live = crate::agent::model_catalog::has_live_snapshot(provider_id);
    let catalogue = if live {
        crate::agent::model_catalog::resolve_live_models(provider_id)
    } else if capability.live_discovery_required {
        None
    } else {
        Some(crate::agent::model_catalog::resolve_models(provider_id))
    };
    let Some(catalogue) = catalogue else {
        return false;
    };
    let left = catalogue_model_identity(provider_id, left, &catalogue, live);
    let right = catalogue_model_identity(provider_id, right, &catalogue, live);
    matches!((left, right), (Some(left), Some(right)) if left.eq_ignore_ascii_case(&right))
}

fn catalogue_model_identity(
    provider_id: &str,
    requested: &str,
    catalogue: &[ModelInfo],
    live: bool,
) -> Option<String> {
    let lookup = catalogue_lookup_model(requested, catalogue);
    let resolved = if live {
        crate::agent::model_catalog::resolve_live_catalog_model(provider_id, &lookup)
    } else {
        crate::agent::model_catalog::resolve_catalog_model(provider_id, &lookup)
    };
    if let Some(model) = resolved {
        return Some(model.id);
    }
    let matches = catalogue
        .iter()
        .filter(|model| model.available != Some(false))
        .filter(|model| {
            model
                .id
                .rsplit('/')
                .next()
                .is_some_and(|leaf| leaf.eq_ignore_ascii_case(requested.trim()))
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0].id.clone())
}

pub(crate) fn preserve_context_suffix(resolved: String, requested: &str) -> String {
    if requested.to_ascii_lowercase().ends_with("[1m]") {
        format!("{resolved}[1m]")
    } else {
        resolved
    }
}

pub(crate) fn validate_agent_model(
    registry: &AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
) -> Result<(), InvalidModelError> {
    let default_provider = provider_id.to_string();
    resolve_agent_model_selection(registry, Some(provider_id), model, &default_provider).map(|_| ())
}

pub(crate) fn resolve_agent_model_selection(
    registry: &AgentRegistry,
    explicit_provider: Option<&str>,
    model: Option<&str>,
    default_provider: &str,
) -> Result<AgentModelSelection, InvalidModelError> {
    let raw_requested = model.map(str::trim).filter(|value| !value.is_empty());
    let providers = registry.provider_ids();
    let explicit_provider = explicit_provider
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (provider_id, routed_model) = if let Some(explicit) = explicit_provider {
        if let Some(requested) = raw_requested {
            if let Some((qualified_provider, model_id)) =
                qualified_provider_model(requested, &providers)
            {
                if !qualified_provider.eq_ignore_ascii_case(explicit) {
                    return Err(provider_mismatch_error(
                        explicit,
                        requested,
                        &qualified_provider,
                    ));
                }
                (explicit.to_string(), Some(model_id))
            } else if providers
                .iter()
                .any(|provider| provider.eq_ignore_ascii_case(requested))
                && !requested.eq_ignore_ascii_case(explicit)
            {
                return Err(provider_mismatch_error(explicit, requested, requested));
            } else {
                (explicit.to_string(), Some(requested.to_string()))
            }
        } else {
            (explicit.to_string(), None)
        }
    } else if let Some(requested) = raw_requested {
        if let Some((provider, model_id)) = qualified_provider_model(requested, &providers) {
            (provider, Some(model_id))
        } else if let Some(provider) = providers
            .iter()
            .find(|provider| provider.eq_ignore_ascii_case(requested))
        {
            (provider.clone(), None)
        } else {
            let owners = model_owners(registry, requested);
            match owners.as_slice() {
                [provider] => (provider.clone(), Some(requested.to_string())),
                [] => (default_provider.to_string(), Some(requested.to_string())),
                _ => {
                    return Err(InvalidModelError {
                        provider_id: default_provider.to_string(),
                        requested_model: requested.to_string(),
                        closest_model_ids: Vec::new(),
                        provider_mismatch: None,
                        ambiguous_providers: owners,
                    })
                }
            }
        }
    } else {
        (default_provider.to_string(), None)
    };
    if let Some(requested) = routed_model.as_deref() {
        let normalized = normalize_requested_model(&provider_id, requested);
        let selected_lists_model = provider_lists_model(registry, &provider_id, &normalized);
        validate_provider_model(registry, &provider_id, Some(&normalized))?;
        let foreign_owners = model_owners(registry, &normalized)
            .into_iter()
            .filter(|owner| !owner.eq_ignore_ascii_case(&provider_id))
            .collect::<Vec<_>>();
        if !selected_lists_model && !foreign_owners.is_empty() {
            return Err(provider_mismatch_error(
                &provider_id,
                &normalized,
                &foreign_owners.join(", "),
            ));
        }
    }
    Ok(AgentModelSelection {
        provider_id,
        model: routed_model,
    })
}

fn provider_mismatch_error(
    provider_id: &str,
    requested_model: &str,
    owner: &str,
) -> InvalidModelError {
    InvalidModelError {
        provider_id: provider_id.to_string(),
        requested_model: requested_model.to_string(),
        closest_model_ids: Vec::new(),
        provider_mismatch: Some(owner.to_string()),
        ambiguous_providers: Vec::new(),
    }
}

fn model_owners(registry: &AgentRegistry, model_id: &str) -> Vec<String> {
    let mut owners = registry
        .provider_ids()
        .into_iter()
        .filter(|provider_id| provider_lists_model(registry, provider_id, model_id))
        .collect::<Vec<_>>();
    owners.sort_unstable();
    owners
}

fn provider_lists_model(registry: &AgentRegistry, provider_id: &str, model_id: &str) -> bool {
    if registry
        .with_provider(provider_id, |provider| {
            provider
                .available_models()
                .iter()
                .any(|model| model.id.eq_ignore_ascii_case(model_id))
        })
        .unwrap_or(false)
    {
        return true;
    }
    crate::agent::model_catalog::resolve_live_models(provider_id).is_some_and(|models| {
        models
            .iter()
            .any(|model| model.id.eq_ignore_ascii_case(model_id))
    })
}

fn qualified_provider_model(model: &str, provider_ids: &[String]) -> Option<(String, String)> {
    let (provider, model) = model.split_once(':')?;
    if provider_ids
        .iter()
        .any(|registered| registered.eq_ignore_ascii_case(provider))
        && !model.trim().is_empty()
    {
        Some((provider.to_string(), model.trim().to_string()))
    } else {
        None
    }
}

fn validate_provider_model(
    registry: &AgentRegistry,
    provider_id: &str,
    model: Option<&str>,
) -> Result<(), InvalidModelError> {
    let Some(raw_requested) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let requested = normalize_requested_model(provider_id, raw_requested);
    let capability = model_catalogue_capability(registry, provider_id);
    let live = crate::agent::model_catalog::has_live_snapshot(provider_id);
    if capability.authority != ModelCatalogueAuthority::Authoritative && !live {
        return Ok(());
    }
    let models = if live {
        let Some(models) = crate::agent::model_catalog::resolve_live_models(provider_id) else {
            return Ok(());
        };
        models
    } else if capability.live_discovery_required {
        return Ok(());
    } else {
        crate::agent::model_catalog::resolve_models(provider_id)
    };
    let lookup = catalogue_lookup_model(&requested, &models);
    let resolved = if live {
        crate::agent::model_catalog::resolve_live_catalog_model(provider_id, &lookup)
    } else {
        crate::agent::model_catalog::resolve_catalog_model(provider_id, &lookup)
    };
    if resolved.is_some() {
        return Ok(());
    }
    let closest_model_ids = if capability.authority == ModelCatalogueAuthority::FreeForm {
        models
            .iter()
            .filter(|model| model.available != Some(false))
            .map(|model| model.id.clone())
            .collect()
    } else {
        closest_catalogue_ids(&models, &lookup)
    };
    Err(InvalidModelError {
        provider_id: provider_id.to_string(),
        requested_model: requested,
        closest_model_ids,
        provider_mismatch: None,
        ambiguous_providers: Vec::new(),
    })
}

fn closest_catalogue_ids(catalogue: &[ModelInfo], requested: &str) -> Vec<String> {
    let mut models: Vec<_> = catalogue
        .iter()
        .filter(|model| model.available != Some(false))
        .cloned()
        .collect();
    models.sort_by_key(|model| edit_distance(requested, &model.id));
    models.into_iter().take(3).map(|model| model.id).collect()
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<u8> = right.to_ascii_lowercase().into_bytes();
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (left_index, left_byte) in left.to_ascii_lowercase().bytes().enumerate() {
        let mut diagonal = row[0];
        row[0] = left_index + 1;
        for (right_index, right_byte) in right.iter().enumerate() {
            let previous = row[right_index + 1];
            row[right_index + 1] = (row[right_index] + 1)
                .min(previous + 1)
                .min(diagonal + usize::from(left_byte != *right_byte));
            diagonal = previous;
        }
    }
    row[right.len()]
}
