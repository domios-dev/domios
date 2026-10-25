use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::key::{self, ApiKey, CachedKey, GateRefusal, KeySources};
use super::local::{self, LocalProvider};
use super::openai::{self, OpenAiProvider};
use super::provider::{Modality, SharedProvider};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedderSelection {
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub dimension: Option<usize>,
}

impl Default for EmbedderSelection {
    fn default() -> Self {
        Self {
            provider: local::ID.into(),
            model: None,
            dimension: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelDescriptor {
    pub model: String,
    pub dimension: usize,
    /// Smallest dimension the model can be asked for; equal to `dimension` when fixed.
    pub min_dimension: usize,
    pub max_tokens: usize,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderDescriptor {
    pub id: String,
    pub label: String,
    pub remote: bool,
    pub modalities: Vec<Modality>,
    pub models: Vec<ModelDescriptor>,
}

impl ProviderDescriptor {
    pub fn model(&self, model: Option<&str>) -> Option<&ModelDescriptor> {
        match model.filter(|model| !model.is_empty()) {
            Some(model) => self.models.iter().find(|row| row.model == model),
            None => self.models.iter().find(|row| row.default),
        }
    }
}

pub struct BuildContext {
    pub keys: KeySources,
    /// Overrides a remote provider's endpoint, e.g. with a local mock server.
    pub endpoint: Option<String>,
}

trait ProviderFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn build(
        &self,
        model: &ModelDescriptor,
        dimension: usize,
        ctx: &BuildContext,
        key: Option<ApiKey>,
    ) -> Result<SharedProvider, String>;
}

struct LocalFactory;

impl ProviderFactory for LocalFactory {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: local::ID.into(),
            label: "Local (on this machine)".into(),
            remote: false,
            modalities: vec![Modality::Text],
            models: local::MODELS
                .iter()
                .map(|row| ModelDescriptor {
                    model: row.model.into(),
                    dimension: row.dimension,
                    min_dimension: row.dimension,
                    max_tokens: row.max_tokens,
                    default: std::ptr::eq(row, local::default_model()),
                })
                .collect(),
        }
    }

    fn build(
        &self,
        model: &ModelDescriptor,
        _dimension: usize,
        _ctx: &BuildContext,
        _key: Option<ApiKey>,
    ) -> Result<SharedProvider, String> {
        let row = local::find(&model.model).ok_or("unknown local model")?;
        Ok(Arc::new(LocalProvider::new(row)))
    }
}

struct OpenAiFactory;

impl ProviderFactory for OpenAiFactory {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: openai::ID.into(),
            label: "OpenAI (remote)".into(),
            remote: true,
            modalities: vec![Modality::Text],
            models: openai::MODELS
                .iter()
                .enumerate()
                .map(|(index, row)| ModelDescriptor {
                    model: row.model.into(),
                    dimension: row.native_dimension,
                    min_dimension: 1,
                    max_tokens: row.max_tokens,
                    default: index == 0,
                })
                .collect(),
        }
    }

    fn build(
        &self,
        model: &ModelDescriptor,
        dimension: usize,
        ctx: &BuildContext,
        key: Option<ApiKey>,
    ) -> Result<SharedProvider, String> {
        let row = openai::find(&model.model).ok_or("unknown OpenAI model")?;
        let key = match key {
            Some(key) => CachedKey::seeded(ctx.keys.clone(), openai::ID, key),
            None => CachedKey::resolve(ctx.keys.clone(), openai::ID),
        };
        let endpoint = ctx.endpoint.as_deref().unwrap_or(openai::DEFAULT_ENDPOINT);
        Ok(Arc::new(OpenAiProvider::new(
            row, dimension, endpoint, key,
        )?))
    }
}

static REGISTRY: [&dyn ProviderFactory; 2] = [&LocalFactory, &OpenAiFactory];

fn registry() -> &'static [&'static dyn ProviderFactory] {
    &REGISTRY
}

pub fn catalog() -> Vec<ProviderDescriptor> {
    registry()
        .iter()
        .map(|factory| factory.descriptor())
        .collect()
}

pub fn descriptor(provider: &str) -> Option<ProviderDescriptor> {
    catalog().into_iter().find(|row| row.id == provider)
}

pub fn default_provider() -> SharedProvider {
    Arc::new(LocalProvider::new(local::default_model()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    UnknownProvider(String),
    UnknownModel { provider: String, model: String },
    InvalidDimension(String),
    Refused(GateRefusal),
    KeyUnreadable(String),
    Failed(String),
}

impl BuildError {
    pub fn message(&self) -> String {
        match self {
            Self::UnknownProvider(provider) => format!("unknown embedder provider `{provider}`"),
            Self::UnknownModel { provider, model } => {
                format!("the {provider} embedder has no model `{model}`")
            }
            Self::InvalidDimension(message) | Self::Failed(message) => message.clone(),
            Self::KeyUnreadable(reason) => {
                format!("the stored embedder key cannot be used: {reason}")
            }
            Self::Refused(refusal) => refusal.message(),
        }
    }

    pub fn hints(&self) -> Vec<String> {
        match self {
            Self::Refused(refusal) => refusal.hints(),
            _ => vec!["domios mem embedder list".into()],
        }
    }
}

/// Resolve a selection against the registry: provider, model and dimension.
pub fn resolve(
    selection: &EmbedderSelection,
) -> Result<(ProviderDescriptor, ModelDescriptor, usize), BuildError> {
    let descriptor = descriptor(&selection.provider)
        .ok_or_else(|| BuildError::UnknownProvider(selection.provider.clone()))?;
    let model = descriptor
        .model(selection.model.as_deref())
        .cloned()
        .ok_or_else(|| BuildError::UnknownModel {
            provider: selection.provider.clone(),
            model: selection.model.clone().unwrap_or_default(),
        })?;
    let dimension = selection
        .dimension
        .filter(|d| *d > 0)
        .unwrap_or(model.dimension);
    if dimension < model.min_dimension || dimension > model.dimension {
        return Err(BuildError::InvalidDimension(format!(
            "{} supports {}..={} dimensions, got {dimension}",
            model.model, model.min_dimension, model.dimension
        )));
    }
    Ok((descriptor, model, dimension))
}

/// Build without the activation gate; used for the identity already stamped
/// on the index, whose consent was given when it was activated.
pub fn build(
    selection: &EmbedderSelection,
    ctx: &BuildContext,
) -> Result<SharedProvider, BuildError> {
    build_with_key(selection, ctx, None)
}

fn build_with_key(
    selection: &EmbedderSelection,
    ctx: &BuildContext,
    key: Option<ApiKey>,
) -> Result<SharedProvider, BuildError> {
    let (descriptor, model, dimension) = resolve(selection)?;
    let factory = registry()
        .iter()
        .find(|factory| factory.descriptor().id == descriptor.id)
        .ok_or_else(|| BuildError::UnknownProvider(descriptor.id.clone()))?;
    factory
        .build(&model, dimension, ctx, key)
        .map_err(BuildError::Failed)
}

/// Build for a switch: a remote provider needs a stored key AND the opt-in.
pub fn activate(
    selection: &EmbedderSelection,
    ctx: &BuildContext,
    remote_opt_in: bool,
) -> Result<SharedProvider, BuildError> {
    let (descriptor, _, _) = resolve(selection)?;
    let presence = ctx.keys.presence(&descriptor.id);
    key::check_remote(&descriptor.id, descriptor.remote, remote_opt_in, &presence)
        .map_err(BuildError::Refused)?;
    let key = if descriptor.remote {
        Some(
            ctx.keys
                .resolve(&descriptor.id)
                .map_err(BuildError::KeyUnreadable)?,
        )
    } else {
        None
    };
    build_with_key(selection, ctx, key)
}
