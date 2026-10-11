use super::*;
use codeterm_plugin::local_recording::{
    LocalInputSetupError, LocalInputSubscription, SubscriptionTeardown,
};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use tauri::Listener;

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

#[test]
fn lark_recording_contract_production_adapter_fails_closed_before_native_acquisition() {
    let adapter = platform::production_adapter();
    let selected = SelectedDevices {
        input_id: "unreviewed-device".into(),
        collection_id: "unreviewed-collection".into(),
        microphone_id: "mic".into(),
    };
    let result = adapter.subscribe(&declaration(), &selected, 7, noop_callback());
    let Err(error) = result else {
        panic!("unsupported selection unexpectedly armed a production adapter");
    };
    assert!(!error.reason.is_empty());
    assert!(error.teardown.is_none());
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
async fn lark_recording_contract_production_enable_uses_injected_readiness_boundary() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let state: crate::service::ServiceState = Arc::new(fixture.service.clone());
    assert!(fixture._app.manage(state));
    let service = &fixture.service;
    let registry = service.local_recording_registry.clone();
    registry.register("trigger", declaration(), true);
    service
        .local_recording_registry
        .select_devices("trigger", devices())
        .unwrap();
    let (adapter, entered, release, teardown) = fake_adapter(registry.clone(), false);
    let adapter: Arc<dyn platform::LocalInputAdapter> = adapter;
    let (status, finished) = service
        .local_recording_set_enabled_with_adapter("trigger", true, adapter)
        .unwrap();
    assert!(!status.available);
    wait_entered(entered).await;
    assert!(!registry.status("trigger").unwrap().available);
    release.send(()).unwrap();
    let finished = finished.expect("enabled configured trigger schedules subscription");
    tokio::task::spawn_blocking(move || finished.recv().unwrap())
        .await
        .unwrap();
    assert!(registry.status("trigger").unwrap().available);
    registry.cancel("trigger");
    teardown.send(Ok(())).unwrap();
    registry
        .wait_for_subscription_slot("trigger", Duration::ZERO)
        .unwrap();
}

#[tokio::test]
async fn lark_recording_contract_host_invalidation_retires_only_exact_pending_capture() {
    use crate::service::local_recording::chat::ChatIdentity;
    use crate::service::local_recording::completion::complete_capture;
    use crate::service::local_recording::leases::FrozenTarget;
    use codeterm_plugin::local_recording::LocalRecordingInvalidationReason;
    use std::sync::atomic::AtomicUsize;

    #[derive(Clone, Copy)]
    enum Invalidation {
        Disable,
        Revoke,
        Unload,
        Replace,
        Removed,
        AccessLost,
        Unavailable,
    }

    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let state: crate::service::ServiceState = Arc::new(fixture.service.clone());
    assert!(fixture._app.manage(state));
    let service = &fixture.service;
    service
        .install_local_recording_invalidation_observer()
        .unwrap();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let listener =
        fixture
            ._app
            .handle()
            .listen_any("local-recording-capture-invalidated", move |event| {
                let value = serde_json::from_str::<serde_json::Value>(event.payload()).unwrap();
                let _ = event_tx.send(value);
            });

    for invalidation in [
        Invalidation::Disable,
        Invalidation::Revoke,
        Invalidation::Unload,
        Invalidation::Replace,
        Invalidation::Removed,
        Invalidation::AccessLost,
        Invalidation::Unavailable,
    ] {
        let registry = &service.local_recording_registry;
        registry.register("trigger", declaration(), true);
        registry.select_devices("trigger", devices()).unwrap();
        registry.set_enabled("trigger", true).unwrap();
        let generation = crate::service::local_recording::arm_registry_fixture(registry, "trigger");
        let lease = service
            .local_recordings
            .start(
                "trigger",
                &devices(),
                generation,
                FrozenTarget::Chat(ChatIdentity {
                    tab_id: "A".into(),
                    plugin_id: "backend".into(),
                    session_id: "S".into(),
                    binding_nonce: "N".into(),
                }),
                registry,
            )
            .unwrap();
        service.local_recordings.stopped(&lease.lease).unwrap();
        let pending = service.local_recordings.transcribing(&lease.lease).unwrap();
        service
            .local_recordings
            .register_write_ticket("pending-send", &lease.lease, pending.clone())
            .unwrap();

        let (transcript_tx, transcript_rx) = tokio::sync::oneshot::channel();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let sends = Arc::new(AtomicUsize::new(0));
        let observed_sends = sends.clone();
        let task = tokio::spawn(async move {
            complete_capture(
                &pending,
                async move {
                    let _ = entered_tx.send(());
                    transcript_rx
                        .await
                        .map_err(|_| "transcription_cancelled".to_string())?
                },
                || Ok(()),
                |_| {
                    observed_sends.fetch_add(1, Ordering::AcqRel);
                    Ok(())
                },
            )
            .await
        });
        entered_rx.await.unwrap();

        match invalidation {
            Invalidation::Disable => {
                registry.set_enabled("trigger", false).unwrap();
            }
            Invalidation::Revoke => registry.revoke("trigger"),
            Invalidation::Unload => registry.unregister("trigger"),
            Invalidation::Replace => {
                registry
                    .select_devices(
                        "trigger",
                        SelectedDevices {
                            input_id: "replacement".into(),
                            ..devices()
                        },
                    )
                    .unwrap();
            }
            Invalidation::Removed => service
                .local_recording_input_event(
                    "trigger",
                    platform::SelectedInputEvent::Unavailable {
                        generation,
                        reason: platform::InputUnavailable::Removed,
                    },
                )
                .unwrap(),
            Invalidation::AccessLost => service
                .local_recording_input_event(
                    "trigger",
                    platform::SelectedInputEvent::Unavailable {
                        generation,
                        reason: platform::InputUnavailable::AccessLost,
                    },
                )
                .unwrap(),
            Invalidation::Unavailable => service
                .local_recording_input_event(
                    "trigger",
                    platform::SelectedInputEvent::Unavailable {
                        generation,
                        reason: platform::InputUnavailable::Error,
                    },
                )
                .unwrap(),
        }
        let expected_reason = match invalidation {
            Invalidation::Disable => "disabled",
            Invalidation::Revoke => "grantRevoked",
            Invalidation::Unload => "unregistered",
            Invalidation::Replace => "deviceSelectionChanged",
            Invalidation::Removed => "deviceRemoved",
            Invalidation::AccessLost => "accessLost",
            Invalidation::Unavailable => "deviceUnavailable",
        };
        let event = loop {
            let event = event_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            if event["pluginId"] == "trigger" && event["generation"] == generation {
                break event;
            }
        };
        assert_eq!(event["reason"], expected_reason);
        transcript_tx.send(Ok("stale transcript".into())).unwrap();
        assert!(task.await.unwrap().is_err());
        assert_eq!(sends.load(Ordering::Acquire), 0);
        assert!(pending.validate().is_err());
        assert!(!service.local_recordings.has_write_ticket("pending-send"));
    }

    let registry = &service.local_recording_registry;
    registry.register("trigger", declaration(), true);
    registry.select_devices("trigger", devices()).unwrap();
    registry.set_enabled("trigger", true).unwrap();
    let generation = crate::service::local_recording::arm_registry_fixture(registry, "trigger");
    service.local_recordings.set_manual_capture(true).unwrap();
    assert!(service
        .local_recording_input_event(
            "trigger",
            platform::SelectedInputEvent::Unavailable {
                generation: generation.saturating_sub(1),
                reason: platform::InputUnavailable::Removed,
            },
        )
        .is_err());
    let start = service.local_recordings.start(
        "trigger",
        &devices(),
        generation,
        FrozenTarget::Chat(ChatIdentity {
            tab_id: "A".into(),
            plugin_id: "backend".into(),
            session_id: "S".into(),
            binding_nonce: "N".into(),
        }),
        registry,
    );
    assert!(matches!(start, Err(error) if error == "manual_capture_owned"));
    assert_eq!(
        serde_json::to_value(LocalRecordingInvalidationReason::AccessLost).unwrap(),
        serde_json::json!("accessLost")
    );
    fixture._app.unlisten(listener);
}

#[cfg(all(test, target_os = "macos"))]
#[test]
fn lark_recording_contract_production_macos_adapter_requires_reviewed_element_mapping() {
    let selected = SelectedDevices {
        input_id: "macos-hid-v1:00000001:0000000000000002".into(),
        collection_id: "macos-hid-collection-v1:00000003:000c:0001".into(),
        microphone_id: "mic".into(),
    };
    let result = platform::production_adapter().subscribe(
        &codeterm_plugin::local_recording::LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 0x000c,
            usage: 1,
        },
        &selected,
        7,
        noop_callback(),
    );
    let Err(error) = result else {
        panic!("Mac adapter armed without reviewed element mapping");
    };
    assert_eq!(error.reason, "macos_hid_element_mapping_unavailable");
    assert!(error.teardown.is_none());
}
