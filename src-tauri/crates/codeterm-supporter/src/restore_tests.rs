use super::*;
use codeterm_decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPrimitives,
    DecisionRequest, TokenMode,
};
use std::sync::mpsc;
use std::time::Duration;

/// Model discovery that blocks until the test releases it, like a slow CLI probe.
struct BlockingDiscovery {
    entered: parking_lot::Mutex<Option<mpsc::Sender<()>>>,
    release: parking_lot::Mutex<mpsc::Receiver<()>>,
    selected: parking_lot::Mutex<Option<String>>,
    restored: parking_lot::Mutex<Option<mpsc::Sender<()>>>,
}

impl DecisionModel for BlockingDiscovery {
    fn id(&self) -> &str {
        "blocking"
    }
    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }
    fn model_id(&self) -> Option<String> {
        self.selected.lock().clone()
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives::default()
    }
    fn models(&self) -> Vec<DecisionModelInfo> {
        if let Some(entered) = self.entered.lock().take() {
            let _ = entered.send(());
        }
        let _ = self.release.lock().recv();
        vec![DecisionModelInfo {
            id: "discovered".into(),
            display_name: "Discovered".into(),
        }]
    }
    fn select_model(&self, id: &str) -> bool {
        *self.selected.lock() = Some(id.into());
        if let Some(restored) = self.restored.lock().take() {
            let _ = restored.send(());
        }
        true
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        Err(DecisionError::Unsupported)
    }
}

#[test]
fn registration_returns_while_model_discovery_is_blocked_and_restores_afterwards() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (restored_tx, restored_rx) = mpsc::channel();
    let adapter: Arc<dyn DecisionModel> = Arc::new(BlockingDiscovery {
        entered: parking_lot::Mutex::new(Some(entered_tx)),
        release: parking_lot::Mutex::new(release_rx),
        selected: parking_lot::Mutex::new(None),
        restored: parking_lot::Mutex::new(Some(restored_tx)),
    });
    let registry = Arc::new(SupporterRegistry::with_registration_hook(|id, adapter| {
        restore_model_in_background(id, adapter, SupporterConfig::default);
    }));
    let (registered_tx, registered_rx) = mpsc::channel();
    let registering = registry.clone();
    let registered_adapter = adapter.clone();
    let registration = std::thread::spawn(move || {
        registering.register("blocking", registered_adapter);
        let _ = registered_tx.send(());
    });
    // Timeouts bound only the failure path; a synchronous restore never reports registration.
    let registered = registered_rx.recv_timeout(Duration::from_secs(5));
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("model discovery must still run");
    release_tx.send(()).unwrap();
    registration.join().unwrap();
    assert!(
        registered.is_ok(),
        "registration must not wait for model discovery"
    );
    restored_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the configured model is restored after discovery completes");
    assert_eq!(adapter.model_id().as_deref(), Some("discovered"));
    assert!(registry.contains("blocking"));
}
