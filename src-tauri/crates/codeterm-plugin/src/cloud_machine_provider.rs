use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};

use reqwest::{
    header::{HeaderName, HeaderValue},
    Url,
};
use serde::{Deserialize, Serialize};

pub type CloudLabels = BTreeMap<String, String>;
pub type CloudResult<T> = Result<T, CloudProviderError>;
pub type CloudProviderRegistry = crate::capability::CapabilityRegistry<dyn CloudProviderPort>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSize {
    Small,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudSizeOffer {
    pub size: CloudSize,
    pub vcpu: u16,
    pub memory_mib: u32,
    pub price_per_hour: CloudHourlyPrice,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct CloudHourlyPrice {
    pub amount: f64,
    pub currency: CloudCurrency,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum CloudCurrency {
    #[serde(rename = "USD")]
    Usd,
    #[serde(rename = "RUB")]
    Rub,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CloudImageRecipe {
    Oci {
        artifact_name: String,
        oci_image: String,
        start_command: String,
    },
    CloudInit {
        oci_image: String,
        template: String,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudImage {
    pub id: String,
    pub build_id: Option<String>,
    pub state: CloudImageState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress_port: Option<u16>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CloudImageState {
    Building,
    Ready,
    Failed,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCreate {
    pub size: CloudSize,
    pub image_id: String,
    pub request_id: uuid::Uuid,
    pub labels: CloudLabels,
    pub env: BTreeMap<String, String>,
    pub auto_stop_minutes: u32,
}

pub const BOOTSTRAP_CONTROLLER_KEY_ENV: &str = "DOMIOS_BOOTSTRAP_CONTROLLER_KEY";
pub const BOOTSTRAP_NONCE_ENV: &str = "DOMIOS_BOOTSTRAP_NONCE";

impl CloudCreate {
    pub(crate) fn validate_bootstrap(&self) -> CloudResult<()> {
        let controller = self.env.get(BOOTSTRAP_CONTROLLER_KEY_ENV);
        let nonce = self.env.get(BOOTSTRAP_NONCE_ENV);
        if self.labels.contains_key(BOOTSTRAP_CONTROLLER_KEY_ENV)
            || self.labels.contains_key(BOOTSTRAP_NONCE_ENV)
        {
            return Err(CloudProviderError::InvalidRequest);
        }
        match (controller, nonce) {
            (None, None) => Ok(()),
            (Some(controller), Some(nonce))
                if is_bootstrap_value(controller)
                    && is_bootstrap_value(nonce)
                    && !self
                        .labels
                        .iter()
                        .any(|(key, value)| key.contains(nonce) || value.contains(nonce)) =>
            {
                Ok(())
            }
            _ => Err(CloudProviderError::InvalidRequest),
        }
    }
}

fn is_bootstrap_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 43
        && bytes[..42]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_'))
        && b"AEIMQUYcgkosw048".contains(&bytes[42])
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudMachine {
    pub id: String,
    pub labels: CloudLabels,
    pub state: CloudMachineState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CloudMachineState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Paused,
    Deleting,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StopSemantics {
    Pause,
    Stop,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SleepMode {
    Pause,
    Stop,
    StopBilled,
    DeleteRecreate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccessLifetime {
    RotatesOnRestart,
    Unspecified,
    Stable,
}

#[derive(Debug)]
pub enum CloudAccess {
    Direct { address: SocketAddrV4 },
    Preview(CloudPreviewAccess),
}

impl CloudAccess {
    pub(crate) fn direct(ip: &str, port: u16) -> CloudResult<Self> {
        let ip = ip
            .parse::<Ipv4Addr>()
            .map_err(|_| CloudProviderError::InvalidResponse)?;
        let [a, b, c, _] = ip.octets();
        if port == 0
            || matches!(
                (a, b, c),
                (0 | 10 | 127 | 224..=255, _, _)
                    | (100, 64..=127, _)
                    | (169, 254, _)
                    | (172, 16..=31, _)
                    | (192, 0 | 168, _)
                    | (198, 18 | 19, _)
                    | (198, 51, 100)
                    | (203, 0, 113)
            )
        {
            return Err(CloudProviderError::InvalidResponse);
        }
        Ok(Self::Direct {
            address: SocketAddrV4::new(ip, port),
        })
    }

    pub fn lifetime(&self) -> AccessLifetime {
        match self {
            Self::Direct { .. } => AccessLifetime::Stable,
            Self::Preview(access) => access.lifetime,
        }
    }
}

#[derive(Debug)]
pub struct CloudPreviewAccess {
    origin: Url,
    header_name: HeaderName,
    header_value: HeaderValue,
    lifetime: AccessLifetime,
}

impl CloudPreviewAccess {
    pub fn new(
        origin: &str,
        header_name: &str,
        header_value: &str,
        lifetime: AccessLifetime,
    ) -> CloudResult<Self> {
        let invalid = || CloudProviderError::InvalidResponse;
        let origin = Url::parse(origin).map_err(|_| invalid())?;
        if origin.scheme() != "https"
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || header_value.is_empty()
        {
            return Err(invalid());
        }
        let header_name = HeaderName::from_bytes(header_name.as_bytes()).map_err(|_| invalid())?;
        let mut header_value = HeaderValue::from_str(header_value).map_err(|_| invalid())?;
        header_value.set_sensitive(true);
        Ok(Self {
            origin,
            header_name,
            header_value,
            lifetime,
        })
    }

    pub fn origin(&self) -> &Url {
        &self.origin
    }

    pub fn header(&self) -> (&HeaderName, &HeaderValue) {
        (&self.header_name, &self.header_value)
    }

    pub fn lifetime(&self) -> AccessLifetime {
        self.lifetime
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloudProviderError {
    Auth,
    Quota,
    NotFound,
    ProviderUnavailable {
        status: Option<u16>,
    },
    ScopeMissing {
        scope: String,
    },
    Refused {
        status: u16,
        reason: Option<CloudRefusalReason>,
    },
    InvalidRequest,
    InvalidResponse,
    OutcomeUnknown,
    AccessUnavailable,
    ImageSetup {
        image: CloudImage,
        cause: Box<CloudProviderError>,
    },
    MachineSetup {
        id: String,
        cause: Box<CloudProviderError>,
    },
    ResourceScopeMismatch,
    DeleteConfirmationRequired,
    Unsupported,
    PersistenceUnavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CloudRefusalReason {
    UnresponsiveGuest,
}

impl std::fmt::Display for CloudProviderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CloudProviderError {}

#[async_trait::async_trait]
pub trait CloudProviderPort: Send + Sync {
    async fn prepare_image(
        &self,
        recipe_ref: &CloudImageRecipe,
        size: CloudSize,
    ) -> CloudResult<CloudImage>;
    async fn image_status(&self, image: &CloudImage) -> CloudResult<CloudImage>;
    async fn delete_image(&self, image: &CloudImage) -> CloudResult<()>;
    async fn create(&self, request: &CloudCreate) -> CloudResult<CloudMachine>;
    async fn start(&self, id: &str, auto_stop_minutes: u32) -> CloudResult<()>;
    async fn stop(&self, id: &str) -> CloudResult<StopSemantics>;
    async fn delete(&self, id: &str) -> CloudResult<()>;
    async fn list(&self, labels: &CloudLabels) -> CloudResult<Vec<CloudMachine>>;
    async fn access(&self, id: &str, port: u16) -> CloudResult<CloudAccess>;
    async fn set_auto_stop(&self, id: &str, minutes: u32) -> CloudResult<()>;
    async fn refresh_activity(&self, id: &str, auto_stop_minutes: u32) -> CloudResult<()>;
    async fn size_catalogue(&self) -> CloudResult<Vec<CloudSizeOffer>>;
    async fn sleep_mode(&self) -> CloudResult<SleepMode>;
}

#[cfg(test)]
mod tests;
