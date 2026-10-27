use super::ServiceFixture;
use crate::secrets::*;
use crate::service::model_providers::{
    ModelProviderClient, ModelProviderClientState, ModelProviderError,
};
use codeterm_plugin::model_providers::{
    ConfigUpdate, ModelProviderConfig, ModelProviderDefinition,
};
use codeterm_plugin::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Default)]
pub struct Calls {
    pub secret_gets: AtomicUsize,
    pub secret_writes: AtomicUsize,
    pub config_writes: AtomicUsize,
    pub http_requests: AtomicUsize,
}

impl Calls {
    pub fn snapshot(&self) -> [usize; 4] {
        [
            self.secret_gets.load(Ordering::SeqCst),
            self.secret_writes.load(Ordering::SeqCst),
            self.config_writes.load(Ordering::SeqCst),
            self.http_requests.load(Ordering::SeqCst),
        ]
    }
}

pub fn install(fixture: &ServiceFixture) -> Arc<Calls> {
    let calls = Arc::new(Calls::default());
    crate::async_runtime::block_on(async {
        *fixture.service.secrets.write().await = Arc::new(Vault(calls.clone()));
    });
    std::fs::write(
        fixture.temp.path().join("model-provider.json"),
        json!({"providers": [{
            "id": "existing", "kind": "openai-compatible",
            "base_url": "https://caller-chosen.invalid/v1", "models": ["fixture-model"]
        }]})
        .to_string(),
    )
    .unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(Provider(Arc::new(Adapter {
            path: fixture.temp.path().join("model-provider.json"),
            calls: calls.clone(),
        }))));
    assert!(fixture
        .service
        .set_ext(ModelProviderClientState(Arc::new(Client(calls.clone()))))
        .is_ok());
    calls
}

struct Vault(Arc<Calls>);

#[async_trait::async_trait]
impl SecretStore for Vault {
    fn id(&self) -> &'static str {
        "authority-fixture"
    }
    async fn status(&self) -> StoreStatus {
        StoreStatus::Unlocked {
            user: None,
            transient: true,
            endpoint: None,
        }
    }
    async fn unlock(&self, _: UnlockCredentials) -> Result<(), SecretError> {
        Ok(())
    }
    async fn lock(&self) -> Result<(), SecretError> {
        Ok(())
    }
    async fn logout(&self) -> Result<(), SecretError> {
        Ok(())
    }
    async fn get(&self, name: &str) -> Result<SecretValue, SecretError> {
        self.0.secret_gets.fetch_add(1, Ordering::SeqCst);
        assert_eq!(name, "named-vault-secret");
        Ok(SecretValue {
            item_id: "secret-item".into(),
            name: name.into(),
            value: SecretString::from("opaque-vault-value"),
            notes: None,
        })
    }
    async fn save(&self, _: SaveRequest) -> Result<SecretRef, SecretError> {
        self.0.secret_writes.fetch_add(1, Ordering::SeqCst);
        Err(SecretError::NotFound)
    }
    async fn delete(&self, _: &str) -> Result<(), SecretError> {
        self.0.secret_writes.fetch_add(1, Ordering::SeqCst);
        Err(SecretError::NotFound)
    }
    async fn list(&self, _: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        Ok(vec![])
    }
    async fn search(&self, _: &str) -> Result<Vec<SecretRef>, SecretError> {
        Ok(vec![])
    }
}

struct Client(Arc<Calls>);

impl ModelProviderClient for Client {
    fn models(
        &self,
        provider: &ModelProviderDefinition,
        key: &SecretString,
    ) -> Result<Vec<String>, ModelProviderError> {
        self.0.http_requests.fetch_add(1, Ordering::SeqCst);
        assert_eq!(provider.base_url, "https://caller-chosen.invalid/v1");
        assert_eq!(key.expose_secret(), "opaque-vault-value");
        Ok(vec!["fixture-model".into()])
    }
}

struct Adapter {
    path: PathBuf,
    calls: Arc<Calls>,
}

impl ModelProviderConfig for Adapter {
    fn path(&self) -> Result<PathBuf, String> {
        Ok(self.path.clone())
    }
    fn read(&self, config: &Value) -> Result<Vec<ModelProviderDefinition>, String> {
        serde_json::from_value(config["providers"].clone()).map_err(|error| error.to_string())
    }
    fn apply(
        &self,
        _: &Value,
        providers: &[ModelProviderDefinition],
    ) -> Result<ConfigUpdate, String> {
        self.calls.config_writes.fetch_add(1, Ordering::SeqCst);
        Ok(ConfigUpdate {
            config: json!({"providers": providers}),
            changed: true,
            conflicts: vec![],
        })
    }
    fn key_env_name(&self, _: &str) -> Result<String, String> {
        Ok("DOMIOS_PROVIDER_FIXTURE_KEY".into())
    }
}

struct Provider(Arc<Adapter>);

impl AgentProvider for Provider {
    fn id(&self) -> &str {
        "authority-fixture"
    }
    fn display_name(&self) -> &str {
        "Authority fixture"
    }
    fn model_provider_config(&self) -> Option<Arc<dyn ModelProviderConfig>> {
        Some(self.0.clone())
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
}
