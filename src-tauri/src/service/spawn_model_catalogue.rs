use std::fmt;

use crate::agent::provider::{ModelCatalogueAuthority, ModelInfo};
use crate::agent::registry::AgentRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InvalidModelError {
    pub provider_id: String,
    pub requested_model: String,
    pub closest_model_ids: Vec<String>,
    pub provider_mismatch: Option<String>,
}

impl InvalidModelError {
    pub(crate) fn rpc_data(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "invalid_model",
            "provider": self.provider_id.clone(),
            "requested_model": self.requested_model.clone(),
            "closest_model_ids": self.closest_model_ids.clone(),
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
    let Some(raw_requested) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if registry
        .provider_ids()
        .iter()
        .any(|registered| registered.eq_ignore_ascii_case(raw_requested))
        && !raw_requested.eq_ignore_ascii_case(provider_id)
    {
        return Err(InvalidModelError {
            provider_id: provider_id.to_string(),
            requested_model: raw_requested.to_string(),
            closest_model_ids: Vec::new(),
            provider_mismatch: Some(raw_requested.to_string()),
        });
    }
    if let Some((requested_provider, _)) = raw_requested.split_once(':') {
        if registry
            .provider_ids()
            .iter()
            .any(|registered| registered.eq_ignore_ascii_case(requested_provider))
            && !requested_provider.eq_ignore_ascii_case(provider_id)
        {
            return Err(InvalidModelError {
                provider_id: provider_id.to_string(),
                requested_model: raw_requested.to_string(),
                closest_model_ids: Vec::new(),
                provider_mismatch: Some(requested_provider.to_string()),
            });
        }
    } else {
        let owners = registry
            .provider_ids()
            .into_iter()
            .filter(|candidate| !candidate.eq_ignore_ascii_case(provider_id))
            .filter(|candidate| {
                registry
                    .with_provider(candidate, |provider| {
                        provider
                            .available_models()
                            .iter()
                            .any(|entry| entry.id.eq_ignore_ascii_case(raw_requested))
                    })
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if !owners.is_empty() {
            return Err(InvalidModelError {
                provider_id: provider_id.to_string(),
                requested_model: raw_requested.to_string(),
                closest_model_ids: Vec::new(),
                provider_mismatch: Some(owners.join(", ")),
            });
        }
    }
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
