use std::fmt;

use crate::agent::provider::{ModelCatalogueAuthority, ModelInfo};
use crate::agent::registry::AgentRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InvalidModelError {
    pub provider_id: String,
    pub requested_model: String,
    pub closest_model_ids: Vec<String>,
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
        let suggestions = if self.closest_model_ids.is_empty() {
            "no model ids are currently advertised".to_string()
        } else {
            self.closest_model_ids.join(", ")
        };
        write!(
            formatter,
            "model id '{}' is not in provider '{}' authoritative catalogue; closest catalogue ids: {}",
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
    let Some(requested) = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|model| normalize_requested_model(provider_id, model))
    else {
        return Ok(());
    };
    let capability = model_catalogue_capability(registry, provider_id);
    if capability.authority != ModelCatalogueAuthority::Authoritative {
        return Ok(());
    }
    let models = if capability.live_discovery_required {
        let Some(models) = crate::agent::model_catalog::resolve_live_models(provider_id) else {
            return Ok(());
        };
        models
    } else {
        crate::agent::model_catalog::resolve_models(provider_id)
    };
    let lookup = catalogue_lookup_model(&requested, &models);
    let resolved = if capability.live_discovery_required {
        crate::agent::model_catalog::resolve_live_catalog_model(provider_id, &lookup)
    } else {
        crate::agent::model_catalog::resolve_catalog_model(provider_id, &lookup)
    };
    if resolved.is_some() {
        return Ok(());
    }
    let closest_model_ids = closest_catalogue_ids(&models, &lookup);
    Err(InvalidModelError {
        provider_id: provider_id.to_string(),
        requested_model: requested,
        closest_model_ids,
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
