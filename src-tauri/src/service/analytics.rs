use super::ServiceContext;
use codeterm_analytics::{Config, Worker};
use std::sync::Arc;

impl ServiceContext {
    pub async fn analytics_worker(&self) -> Result<&Worker, String> {
        self.analytics
            .get_or_try_init(|| async {
                let secrets = self.secrets.clone();
                tokio::task::spawn_blocking(move || {
                    let reader = Arc::new(move |name: &str| {
                        tauri::async_runtime::block_on(async {
                            let store = secrets.read().await;
                            store
                                .get(name)
                                .await
                                .map(|secret| secret.value.into_inner())
                                .map_err(|_| "analytics credential unavailable".to_string())
                        })
                    });
                    Worker::start(
                        crate::paths::data_dir().join("analytics-settings.json"),
                        Config::from_env()?,
                        reader,
                    )
                })
                .await
                .map_err(|_| "analytics initialization failed".to_string())?
            })
            .await
    }
}
