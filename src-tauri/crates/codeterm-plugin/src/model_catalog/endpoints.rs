use super::*;
use crate::model_providers::ModelProviderDefinition;
use std::collections::BTreeSet;

#[derive(Default)]
struct EndpointCatalogue {
    namespaces: BTreeSet<String>,
    models: Vec<ModelInfo>,
}

fn catalogues() -> &'static RwLock<HashMap<String, EndpointCatalogue>> {
    static CATALOGUES: OnceLock<RwLock<HashMap<String, EndpointCatalogue>>> = OnceLock::new();
    CATALOGUES.get_or_init(RwLock::default)
}

pub(super) fn refresh(provider: &dyn AgentProvider, stamp: &str) {
    let Some(adapter) = provider.model_provider_config() else {
        if clear(provider.id()) {
            super::invalidate(provider.id());
        }
        return;
    };
    let endpoints = (|| {
        let path = adapter.path().ok()?;
        let config = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).ok()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
            Err(_) => return None,
        };
        adapter.read(&config).ok()
    })();
    let Some(endpoints) = endpoints else {
        log::debug!(
            "[model_catalog] endpoint config unavailable for {}",
            provider.id()
        );
        return;
    };
    super::inputs::publish_current(provider, stamp, || put(provider.id(), endpoints));
}

fn put(provider: &str, endpoints: Vec<ModelProviderDefinition>) {
    {
        let mut catalogues = catalogues().write();
        let catalogue = catalogues.entry(provider.into()).or_default();
        catalogue.models.clear();
        for endpoint in endpoints {
            catalogue.namespaces.insert(endpoint.id.clone());
            for model in endpoint.models {
                catalogue.models.push(ModelInfo {
                    id: format!("{}/{model}", endpoint.id),
                    display_name: model,
                    group: Some(endpoint.name.clone().unwrap_or_else(|| endpoint.id.clone())),
                    ..Default::default()
                });
            }
        }
    }
    super::invalidate(provider);
}

pub(super) fn clear(provider: &str) -> bool {
    catalogues().write().remove(provider).is_some()
}

pub(super) fn merge(provider: &str, mut models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    let catalogues = catalogues().read();
    let Some(catalogue) = catalogues.get(provider) else {
        return models;
    };
    models.retain(|model| !owns(catalogue, &model.id));
    models.extend(catalogue.models.clone());
    models
}

fn owns(catalogue: &EndpointCatalogue, requested: &str) -> bool {
    requested.split_once('/').is_some_and(|(endpoint, _)| {
        catalogue
            .namespaces
            .iter()
            .any(|id| id.eq_ignore_ascii_case(endpoint))
    })
}

pub fn resolve(provider: &str, requested: &str) -> Option<Result<ModelInfo, Vec<String>>> {
    let catalogues = catalogues().read();
    let catalogue = catalogues.get(provider)?;
    if !owns(catalogue, requested) {
        return None;
    }
    Some(
        catalogue
            .models
            .iter()
            .find(|model| model.id == requested)
            .cloned()
            .ok_or_else(|| {
                let endpoint = requested
                    .split_once('/')
                    .map(|(endpoint, _)| endpoint)
                    .unwrap_or_default();
                catalogue
                    .models
                    .iter()
                    .filter(|model| {
                        model
                            .id
                            .split_once('/')
                            .is_some_and(|(id, _)| id.eq_ignore_ascii_case(endpoint))
                    })
                    .map(|model| model.id.clone())
                    .collect()
            }),
    )
}

#[cfg(test)]
mod tests;
