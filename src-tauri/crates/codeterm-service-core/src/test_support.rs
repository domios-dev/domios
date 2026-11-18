use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn fixture_manifest_dir() -> &'static Path {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY.get_or_init(|| {
        std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    })
}

pub struct ProductAnalyticsRecording {
    worker: codeterm_analytics::Worker,
    recording: std::sync::Arc<std::sync::Mutex<codeterm_analytics::sinks::fake::Recording>>,
}

impl ProductAnalyticsRecording {
    pub async fn set_enabled(&self, enabled: bool) {
        self.worker
            .set_consent(if enabled {
                codeterm_analytics::Consent::On
            } else {
                codeterm_analytics::Consent::Off
            })
            .await
            .unwrap();
    }

    pub async fn events(&self) -> Vec<serde_json::Value> {
        self.worker.flush().await.unwrap();
        self.recording
            .lock()
            .unwrap()
            .batches
            .iter()
            .flatten()
            .map(|item| serde_json::to_value(&item.event).unwrap())
            .collect()
    }
}

pub async fn record_product_analytics(
    svc: &crate::service::ServiceContext,
    settings: std::path::PathBuf,
) -> ProductAnalyticsRecording {
    let recording = std::sync::Arc::new(std::sync::Mutex::new(
        codeterm_analytics::sinks::fake::Recording::default(),
    ));
    let worker = codeterm_analytics::Worker::start_with_sink(
        settings,
        codeterm_analytics::Config::default(),
        Box::new(codeterm_analytics::sinks::fake::Fake {
            recording: recording.clone(),
        }),
    )
    .unwrap();
    worker
        .set_consent(codeterm_analytics::Consent::On)
        .await
        .unwrap();
    worker.flush().await.unwrap();
    recording.lock().unwrap().batches.clear();
    assert!(svc
        .ext::<crate::service::analytics::AnalyticsCell>()
        .0
        .set(worker.clone())
        .is_ok());
    ProductAnalyticsRecording { worker, recording }
}
