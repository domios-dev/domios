use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use codeterm_plugin::cloud_machine_provider::{
    AccessLifetime, CloudAccess, CloudCreate, CloudCurrency, CloudImageRecipe, CloudImageState,
    CloudMachineState, CloudProviderError, CloudProviderPort, CloudSize, SleepMode,
};

use super::ports::{CloudBackendPort, CloudCreateRequest, CloudRouteSpec};
use super::state::{ObservedProviderMachine, ObservedProviderState};
use super::types::{
    CloudError, CloudOwnershipReceipt, CloudRate, CloudRateBasis, CloudSleepMode,
    DEFAULT_BYO_MACHINE_LIMIT,
};

const DEFAULT_CLOUD_OCI_IMAGE: &str = "ghcr.io/rollacode/domios-cloud:latest";
const DEFAULT_CLOUD_START_COMMAND: &str = "/usr/local/bin/domios-cloud-start";
const DEFAULT_CLOUD_ARTIFACT_NAME: &str = "domios-cloud";

pub struct PluginCloudBackend {
    provider_id: String,
    port: Arc<dyn CloudProviderPort>,
}

impl PluginCloudBackend {
    pub fn new(provider_id: impl Into<String>, port: Arc<dyn CloudProviderPort>) -> Self {
        Self {
            provider_id: provider_id.into(),
            port,
        }
    }

    pub fn resolve_from_service(
        svc: &crate::service::ServiceContext,
        requested: Option<&str>,
    ) -> Result<(String, Arc<dyn CloudBackendPort>), CloudError> {
        if let Some(id) = requested.map(str::trim).filter(|id| !id.is_empty()) {
            let port = svc
                .cloud_machine_provider
                .get(id)
                .ok_or_else(|| CloudError::ProviderNotFound(id.to_owned()))?;
            return Ok((id.to_owned(), Arc::new(Self::new(id, port))));
        }
        let mut ids = svc.cloud_machine_provider.ids();
        ids.sort();
        match ids.as_slice() {
            [] => Err(CloudError::ProviderNotFound(
                "no cloud machine provider plugin is installed".into(),
            )),
            [only] => {
                let port = svc
                    .cloud_machine_provider
                    .get(only)
                    .ok_or_else(|| CloudError::ProviderNotFound(only.clone()))?;
                Ok((only.clone(), Arc::new(Self::new(only.clone(), port))))
            }
            many => Err(CloudError::ProviderNotFound(format!(
                "multiple cloud providers installed ({}); pass --provider",
                many.join(", ")
            ))),
        }
    }
}

fn parse_cloud_size(size: &str) -> Result<CloudSize, CloudError> {
    match size.trim().to_ascii_lowercase().as_str() {
        "" | "small" => Ok(CloudSize::Small),
        other => Err(CloudError::ProviderUnavailable(format!(
            "unsupported cloud machine size: {other}"
        ))),
    }
}

fn map_provider_state(state: CloudMachineState) -> ObservedProviderState {
    match state {
        CloudMachineState::Starting => ObservedProviderState::Starting,
        CloudMachineState::Running => ObservedProviderState::Running,
        CloudMachineState::Stopping => ObservedProviderState::Stopping,
        CloudMachineState::Stopped => ObservedProviderState::Stopped,
        CloudMachineState::Paused => ObservedProviderState::Paused,
        CloudMachineState::Deleting => ObservedProviderState::Deleting,
        CloudMachineState::Failed => ObservedProviderState::Failed,
        CloudMachineState::Unknown => ObservedProviderState::Unknown,
    }
}

fn map_sleep_mode(mode: SleepMode) -> CloudSleepMode {
    match mode {
        SleepMode::Pause => CloudSleepMode::Pause,
        SleepMode::Stop => CloudSleepMode::Stop,
        SleepMode::StopBilled => CloudSleepMode::StopBilled,
        SleepMode::DeleteRecreate => CloudSleepMode::DeleteRecreate,
    }
}

fn map_currency(currency: CloudCurrency) -> &'static str {
    match currency {
        CloudCurrency::Usd => "USD",
        CloudCurrency::Rub => "RUB",
    }
}

fn map_provider_error(provider: &str, error: CloudProviderError) -> CloudError {
    match error {
        CloudProviderError::Quota => CloudError::LimitReached {
            limit: DEFAULT_BYO_MACHINE_LIMIT,
        },
        CloudProviderError::NotFound => CloudError::MachineNotFound(provider.to_owned()),
        CloudProviderError::DeleteConfirmationRequired => CloudError::DeleteConfirmationRequired {
            machine_id: provider.to_owned(),
            warning: "provider requires explicit confirmation before deleting this machine".into(),
        },
        other => CloudError::ProviderUnavailable(format!("{provider}: {other}")),
    }
}

fn default_oci_image() -> String {
    std::env::var("DOMIOS_CLOUD_OCI_IMAGE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CLOUD_OCI_IMAGE.to_owned())
}

fn default_image_recipe() -> CloudImageRecipe {
    CloudImageRecipe::Oci {
        artifact_name: DEFAULT_CLOUD_ARTIFACT_NAME.to_owned(),
        oci_image: default_oci_image(),
        start_command: DEFAULT_CLOUD_START_COMMAND.to_owned(),
    }
}

fn cloud_init_image_recipe() -> CloudImageRecipe {
    let template = std::env::var("DOMIOS_CLOUD_INIT_TEMPLATE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_default();
    CloudImageRecipe::CloudInit {
        oci_image: default_oci_image(),
        template,
    }
}

#[async_trait]
impl CloudBackendPort for PluginCloudBackend {
    async fn prepare_image(&self, size: &str) -> Result<String, CloudError> {
        let size = parse_cloud_size(size)?;
        let recipe = default_image_recipe();
        let image = match self.port.prepare_image(&recipe, size).await {
            Ok(image) => image,
            Err(CloudProviderError::Unsupported) => {
                let fallback = cloud_init_image_recipe();
                self.port
                    .prepare_image(&fallback, size)
                    .await
                    .map_err(|error| map_provider_error(&self.provider_id, error))?
            }
            Err(error) => return Err(map_provider_error(&self.provider_id, error)),
        };
        match image.state {
            CloudImageState::Ready => Ok(image.id),
            CloudImageState::Building => Err(CloudError::ProviderUnavailable(format!(
                "{}: image {} is still building",
                self.provider_id, image.id
            ))),
            CloudImageState::Failed => Err(CloudError::ProviderUnavailable(format!(
                "{}: image {} preparation failed",
                self.provider_id, image.id
            ))),
        }
    }

    async fn create(
        &self,
        request: &CloudCreateRequest,
    ) -> Result<ObservedProviderMachine, CloudError> {
        let size = parse_cloud_size(&request.size)?;
        let create = CloudCreate {
            size,
            image_id: request.image_id.clone(),
            request_id: request.request_id,
            labels: request.labels.clone(),
            env: request.env.clone(),
            auto_stop_minutes: request.auto_stop_minutes,
        };
        let machine = self
            .port
            .create(&create)
            .await
            .map_err(|error| map_provider_error(&self.provider_id, error))?;
        Ok(ObservedProviderMachine {
            provider_machine_id: machine.id,
            labels: machine.labels,
            state: map_provider_state(machine.state),
        })
    }

    async fn start(
        &self,
        provider_machine_id: &str,
        auto_stop_minutes: u32,
    ) -> Result<(), CloudError> {
        self.port
            .start(provider_machine_id, auto_stop_minutes)
            .await
            .map_err(|error| map_provider_error(&self.provider_id, error))
    }

    async fn stop(&self, provider_machine_id: &str) -> Result<(), CloudError> {
        self.port
            .stop(provider_machine_id)
            .await
            .map(|_| ())
            .map_err(|error| map_provider_error(&self.provider_id, error))
    }

    async fn delete(&self, ownership: &CloudOwnershipReceipt) -> Result<(), CloudError> {
        self.port
            .delete(&ownership.provider_machine_id)
            .await
            .map_err(|error| map_provider_error(&self.provider_id, error))
    }

    async fn list(
        &self,
        labels: &BTreeMap<String, String>,
    ) -> Result<Vec<ObservedProviderMachine>, CloudError> {
        let machines = self
            .port
            .list(labels)
            .await
            .map_err(|error| map_provider_error(&self.provider_id, error))?;
        Ok(machines
            .into_iter()
            .map(|machine| ObservedProviderMachine {
                provider_machine_id: machine.id,
                labels: machine.labels,
                state: map_provider_state(machine.state),
            })
            .collect())
    }

    async fn access(
        &self,
        provider_machine_id: &str,
        port: u16,
    ) -> Result<CloudRouteSpec, CloudError> {
        let access = self
            .port
            .access(provider_machine_id, port)
            .await
            .map_err(|error| map_provider_error(&self.provider_id, error))?;
        Ok(match access {
            CloudAccess::Direct { address } => CloudRouteSpec::Direct { address },
            CloudAccess::Preview(preview) => {
                let (name, value) = preview.header();
                CloudRouteSpec::Preview {
                    origin: preview.origin().clone(),
                    header_name: name.clone(),
                    header_value: value.clone(),
                    rotates_on_restart: preview.lifetime() == AccessLifetime::RotatesOnRestart,
                }
            }
        })
    }

    async fn refresh_activity(
        &self,
        provider_machine_id: &str,
        auto_stop_minutes: u32,
    ) -> Result<(), CloudError> {
        self.port
            .refresh_activity(provider_machine_id, auto_stop_minutes)
            .await
            .map_err(|error| map_provider_error(&self.provider_id, error))
    }

    async fn hourly_rate(&self, size: &str) -> Result<Option<CloudRate>, CloudError> {
        let target = parse_cloud_size(size)?;
        let catalogue = self
            .port
            .size_catalogue()
            .await
            .map_err(|error| map_provider_error(&self.provider_id, error))?;
        Ok(catalogue
            .into_iter()
            .find(|offer| offer.size == target)
            .map(|offer| CloudRate {
                amount: offer.price_per_hour.amount,
                currency: map_currency(offer.price_per_hour.currency).to_owned(),
                basis: CloudRateBasis::Estimate,
            }))
    }

    async fn sleep_mode(&self) -> Result<CloudSleepMode, CloudError> {
        self.port
            .sleep_mode()
            .await
            .map(map_sleep_mode)
            .map_err(|error| map_provider_error(&self.provider_id, error))
    }
}
