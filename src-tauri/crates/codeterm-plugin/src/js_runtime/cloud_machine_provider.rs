use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::cloud_machine_provider::*;

use super::vm::{call_json_async, PluginVm, VmError};

pub(super) struct JsCloudProvider {
    vm: Arc<PluginVm>,
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Reply<T> {
    Ok { value: T },
    Error { error: CloudProviderError },
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum AccessReply {
    Preview {
        origin: String,
        header_name: String,
        header_value: String,
        lifetime: AccessLifetime,
    },
    Direct {
        ip: String,
    },
}

impl JsCloudProvider {
    pub(super) fn new(vm: Arc<PluginVm>) -> Self {
        Self { vm }
    }

    async fn call<T>(&self, operation: &'static str, args: Value) -> CloudResult<T>
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        let reply = call_json_async::<Reply<T>>(
            self.vm.clone(),
            "cloudMachineProvider",
            vec![json!(operation), args],
        )
        .await
        .map_err(|error| match error {
            VmError::IoUnconfirmed => CloudProviderError::OutcomeUnknown,
            VmError::InvalidJson(..) => CloudProviderError::InvalidResponse,
            _ => CloudProviderError::ProviderUnavailable { status: None },
        })?;
        match reply {
            Reply::Ok { value } => Ok(value),
            Reply::Error { error } => Err(error),
        }
    }
}

#[async_trait::async_trait]
impl CloudProviderPort for JsCloudProvider {
    async fn prepare_image(
        &self,
        recipe_ref: &CloudImageRecipe,
        size: CloudSize,
    ) -> CloudResult<CloudImage> {
        self.call(
            "prepareImage",
            json!({ "recipe": recipe_ref, "size": size }),
        )
        .await
    }

    async fn image_status(&self, image: &CloudImage) -> CloudResult<CloudImage> {
        self.call("imageStatus", json!(image)).await
    }

    async fn delete_image(&self, image: &CloudImage) -> CloudResult<()> {
        self.call("deleteImage", json!(image)).await
    }

    async fn create(&self, request: &CloudCreate) -> CloudResult<CloudMachine> {
        request.validate_bootstrap()?;
        self.call("create", json!(request)).await
    }

    async fn start(&self, id: &str, auto_stop_minutes: u32) -> CloudResult<()> {
        self.call("start", json!({ "id": id, "minutes": auto_stop_minutes }))
            .await
    }

    async fn stop(&self, id: &str) -> CloudResult<StopSemantics> {
        self.call("stop", json!({ "id": id })).await
    }

    async fn delete(&self, id: &str) -> CloudResult<()> {
        self.call("delete", json!({ "id": id })).await
    }

    async fn list(&self, labels: &CloudLabels) -> CloudResult<Vec<CloudMachine>> {
        self.call("list", json!({ "labels": labels })).await
    }

    async fn access(&self, id: &str, port: u16) -> CloudResult<CloudAccess> {
        if port == 0 {
            return Err(CloudProviderError::InvalidRequest);
        }
        let reply: AccessReply = self
            .call("access", json!({ "id": id, "port": port }))
            .await?;
        match reply {
            AccessReply::Preview {
                origin,
                header_name,
                header_value,
                lifetime,
            } => CloudPreviewAccess::new(&origin, &header_name, &header_value, lifetime)
                .map(CloudAccess::Preview),
            AccessReply::Direct { ip } => CloudAccess::direct(&ip, port),
        }
    }

    async fn set_auto_stop(&self, id: &str, minutes: u32) -> CloudResult<()> {
        self.call("setAutoStop", json!({ "id": id, "minutes": minutes }))
            .await
    }

    async fn refresh_activity(&self, id: &str, auto_stop_minutes: u32) -> CloudResult<()> {
        self.call(
            "refreshActivity",
            json!({ "id": id, "minutes": auto_stop_minutes }),
        )
        .await
    }

    async fn size_catalogue(&self) -> CloudResult<Vec<CloudSizeOffer>> {
        self.call("sizeCatalogue", Value::Null).await
    }

    async fn sleep_mode(&self) -> CloudResult<SleepMode> {
        self.call("sleepMode", Value::Null).await
    }
}
