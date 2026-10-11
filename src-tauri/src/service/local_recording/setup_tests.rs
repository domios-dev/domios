use super::*;
use codeterm_plugin::local_recording::{LocalInputSetupError, LocalInputSubscription};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

fn declaration() -> codeterm_plugin::local_recording::LocalRecordingTrigger {
    codeterm_plugin::local_recording::LocalRecordingTrigger {
        vendor_id: 1,
        product_id: 2,
        usage_page: 3,
        usage: 4,
    }
}

fn devices() -> SelectedDevices {
    SelectedDevices {
        input_id: "input".into(),
        collection_id: "collection".into(),
        microphone_id: "mic".into(),
    }
}

fn configured() -> LocalRecordingRegistryHandle {
    let registry = Arc::new(codeterm_plugin::local_recording::LocalRecordingRegistry::default());
    registry.register("trigger", declaration(), true);
    registry.select_devices("trigger", devices()).unwrap();
    registry.set_enabled("trigger", true).unwrap();
    registry
}

struct FakeSubscription {
    stopped: Arc<AtomicBool>,
    teardown: Option<SubscriptionTeardown>,
}

impl LocalInputSubscription for FakeSubscription {
    fn request_stop(&mut self) -> SubscriptionTeardown {
        self.stopped.store(true, Ordering::Release);
        self.teardown.take().unwrap()
    }
}

struct FakeAdapter {
    registry: LocalRecordingRegistryHandle,
    entered: Mutex<Option<Sender<()>>>,
    release: Mutex<Receiver<()>>,
    teardown: Mutex<Option<SubscriptionTeardown>>,
    stopped: Arc<AtomicBool>,
    fail: bool,
}

impl platform::LocalInputAdapter for FakeAdapter {
    fn subscribe(
        &self,
        trigger: &codeterm_plugin::local_recording::LocalRecordingTrigger,
        selected: &SelectedDevices,
        generation: u64,
        _: Arc<dyn Fn(platform::SelectedInputEvent) + Send + Sync>,
    ) -> Result<Box<dyn LocalInputSubscription>, LocalInputSetupError> {
        assert_eq!(trigger, &declaration());
        assert_eq!(selected, &devices());
        assert_eq!(
            self.registry.status("trigger").unwrap().generation,
            generation
        );
        assert!(!self.registry.status("trigger").unwrap().available);
        assert_eq!(
            self.registry
                .begin("trigger", selected, generation)
                .unwrap_err(),
            "devices_unavailable"
        );
        if let Some(entered) = self.entered.lock().take() {
            entered.send(()).unwrap();
        }
        self.release.lock().recv().unwrap();
        let teardown = self.teardown.lock().take().unwrap();
        if self.fail {
            Err(LocalInputSetupError {
                reason: "native_setup_not_acknowledged".into(),
                teardown: Some(teardown),
            })
        } else {
            Ok(Box::new(FakeSubscription {
                stopped: self.stopped.clone(),
                teardown: Some(teardown),
            }))
        }
    }
}

fn fake_adapter(
    registry: LocalRecordingRegistryHandle,
    fail: bool,
) -> (
    Arc<FakeAdapter>,
    Receiver<()>,
    Sender<()>,
    Sender<Result<(), String>>,
) {
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (teardown_tx, teardown_rx) = std::sync::mpsc::channel();
    (
        Arc::new(FakeAdapter {
            registry,
            entered: Mutex::new(Some(entered_tx)),
            release: Mutex::new(release_rx),
            teardown: Mutex::new(Some(SubscriptionTeardown::new(teardown_rx))),
            stopped: Arc::new(AtomicBool::new(false)),
            fail,
        }),
        entered_rx,
        release_tx,
        teardown_tx,
    )
}

async fn wait_entered(receiver: Receiver<()>) {
    tokio::task::spawn_blocking(move || receiver.recv().unwrap())
        .await
        .unwrap();
}

fn noop_callback() -> Arc<dyn Fn(platform::SelectedInputEvent) + Send + Sync> {
    Arc::new(|_| {})
}

#[tokio::test]
async fn lark_recording_contract_setup_arms_only_after_adapter_acknowledges_ready() {
    let registry = configured();
    let (adapter, entered, release, teardown) = fake_adapter(registry.clone(), false);
    let adapter: Arc<dyn platform::LocalInputAdapter> = adapter;
    let task = tokio::spawn(subscribe_local_input(
        registry.clone(),
        "trigger".into(),
        adapter,
        noop_callback(),
    ));
    wait_entered(entered).await;
    assert!(!registry.status("trigger").unwrap().available);
    assert_eq!(
        registry.begin_subscription_setup("trigger").unwrap_err(),
        "subscription_cleanup_pending"
    );
    release.send(()).unwrap();
    task.await.unwrap().unwrap();
    let status = registry.status("trigger").unwrap();
    assert!(status.available);
    assert!(!status.capture_active);
    let capture = registry
        .begin("trigger", &devices(), status.generation)
        .unwrap();
    assert!(capture.load(Ordering::Acquire));
    registry.cancel("trigger");
    teardown.send(Ok(())).unwrap();
    registry
        .wait_for_subscription_slot("trigger", Duration::ZERO)
        .unwrap();
}

#[tokio::test]
async fn lark_recording_contract_setup_failure_stays_unavailable_until_cleanup_ack() {
    let registry = configured();
    let (adapter, entered, release, teardown) = fake_adapter(registry.clone(), true);
    let adapter: Arc<dyn platform::LocalInputAdapter> = adapter;
    let task = tokio::spawn(subscribe_local_input(
        registry.clone(),
        "trigger".into(),
        adapter,
        noop_callback(),
    ));
    wait_entered(entered).await;
    assert!(!registry.status("trigger").unwrap().available);
    release.send(()).unwrap();
    assert_eq!(
        task.await.unwrap().unwrap_err(),
        "native_setup_not_acknowledged"
    );
    assert!(!registry.status("trigger").unwrap().available);
    registry.unregister("trigger");
    registry.register("trigger", declaration(), true);
    assert_eq!(
        registry.wait_for_subscription_slot("trigger", Duration::ZERO),
        Err("subscription_cleanup_pending")
    );
    teardown.send(Ok(())).unwrap();
    registry
        .wait_for_subscription_slot("trigger", Duration::ZERO)
        .unwrap();
}

#[tokio::test]
async fn lark_recording_contract_unload_during_setup_stops_late_success_before_rearm() {
    let registry = configured();
    let (adapter, entered, release, teardown) = fake_adapter(registry.clone(), false);
    let stopped = adapter.stopped.clone();
    let adapter: Arc<dyn platform::LocalInputAdapter> = adapter;
    let task = tokio::spawn(subscribe_local_input(
        registry.clone(),
        "trigger".into(),
        adapter,
        noop_callback(),
    ));
    wait_entered(entered).await;
    registry.unregister("trigger");
    registry.register("trigger", declaration(), true);
    registry.select_devices("trigger", devices()).unwrap();
    registry.set_enabled("trigger", true).unwrap();
    release.send(()).unwrap();
    assert_eq!(task.await.unwrap().unwrap_err(), "subscription_stale");
    assert!(stopped.load(Ordering::Acquire));
    assert!(!registry.status("trigger").unwrap().available);
    assert_eq!(
        registry.wait_for_subscription_slot("trigger", Duration::ZERO),
        Err("subscription_cleanup_pending")
    );
    teardown.send(Ok(())).unwrap();
    registry
        .wait_for_subscription_slot("trigger", Duration::ZERO)
        .unwrap();
    let ready = registry.begin_subscription_setup("trigger").unwrap();
    assert!(!registry.status("trigger").unwrap().capture_active);
    registry.cancel_subscription_setup(&ready);
}
