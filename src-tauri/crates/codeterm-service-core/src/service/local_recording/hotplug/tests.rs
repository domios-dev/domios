use super::*;
use codeterm_local_recording::{LocalRecordingButton, LocalRecordingTrigger};

const PORT_A: &str =
    r"\\?\HID#VID_1234&PID_5678&MI_00&Col01#7&port_a&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
const PORT_B: &str =
    r"\\?\HID#VID_1234&PID_5678&MI_00&Col01#8&port_b&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
const PORT_C: &str =
    r"\\?\HID#VID_1234&PID_5678&MI_00&Col01#9&port_c&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
const OTHER_HW: &str =
    r"\\?\HID#VID_9999&PID_5678&MI_00&Col01#7&port_a&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";

fn trigger_decl() -> LocalRecordingTrigger {
    LocalRecordingTrigger {
        vendor_id: 0x1234,
        product_id: 0x5678,
        usage_page: 0x000c,
        usage: 0x0001,
        button: Some(LocalRecordingButton {
            interface_number: 0,
            collection_id: "Col01".into(),
            report_id: 6,
            report_bytes: 3,
            usage_page: 0x000c,
            press_usage: 0x00e9,
            button_usages: vec![0x00e9],
            bit_field: 0x42,
            link_collection: 0,
            debounce_ms: 40,
        }),
    }
}

fn selected(input_id: &str) -> SelectedDevices {
    SelectedDevices {
        input_id: input_id.into(),
        collection_id: "Col01".into(),
        microphone_id: "mic-1".into(),
    }
}

fn hid_candidate(input_id: &str, collection_id: &str) -> RecordingInputDevice {
    RecordingInputDevice {
        id: input_id.into(),
        label: input_id.into(),
        kind: RecordingInputKind::Hid,
        connected: true,
        input_id: input_id.into(),
        collection_id: collection_id.into(),
    }
}

#[test]
fn candidate_resolution_prefers_exact_match_even_with_second_receiver() {
    let candidates = vec![
        hid_candidate(PORT_A, "Col01"),
        hid_candidate(PORT_B, "Col01"),
    ];
    assert_eq!(
        resolve_candidate(&trigger_decl(), &selected(PORT_A), &candidates),
        CandidateResolution::Exact(selected(PORT_A))
    );
}

#[test]
fn candidate_resolution_rebinds_single_receiver_moved_to_new_usb_port() {
    let candidates = vec![hid_candidate(PORT_B, "Col01")];
    assert_eq!(
        resolve_candidate(&trigger_decl(), &selected(PORT_A), &candidates),
        CandidateResolution::Rebound(selected(PORT_B))
    );
}

#[test]
fn candidate_resolution_refuses_ambiguous_multi_receiver_without_exact_match() {
    let candidates = vec![
        hid_candidate(PORT_A, "Col01"),
        hid_candidate(PORT_B, "Col01"),
    ];
    assert_eq!(
        resolve_candidate(&trigger_decl(), &selected(PORT_C), &candidates),
        CandidateResolution::Ambiguous
    );
}

#[test]
fn candidate_resolution_refuses_wrong_hardware_class_or_collection() {
    assert_eq!(
        resolve_candidate(
            &trigger_decl(),
            &selected(PORT_A),
            &[hid_candidate(OTHER_HW, "Col01")],
        ),
        CandidateResolution::Absent
    );
    assert_eq!(
        resolve_candidate(
            &trigger_decl(),
            &selected(PORT_A),
            &[hid_candidate(PORT_B, "Col02")],
        ),
        CandidateResolution::Absent
    );
    let mut disconnected = hid_candidate(PORT_A, "Col01");
    disconnected.connected = false;
    assert_eq!(
        resolve_candidate(&trigger_decl(), &selected(PORT_A), &[disconnected]),
        CandidateResolution::Absent
    );
}

#[cfg(any(windows, target_os = "macos"))]
mod state_machine_tests {
    use super::*;
    use crate::service::local_recording::platform::{
        InputUnavailable, LocalInputSubscription, SelectedInputEvent,
    };
    use codeterm_plugin::local_recording::LocalInputSetupError;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct FakeSub {
        stopped: Arc<AtomicBool>,
    }

    impl LocalInputSubscription for FakeSub {
        fn request_stop(&mut self) -> SubscriptionTeardown {
            self.stopped.store(true, Ordering::Release);
            let (tx, rx) = mpsc::channel();
            tx.send(Ok(())).unwrap();
            SubscriptionTeardown::new(rx)
        }
    }

    struct FakeWatch {
        stopped: Arc<AtomicBool>,
        stop_signal: Mutex<Option<Sender<()>>>,
    }

    impl LocalInputArrivalWatch for FakeWatch {
        fn request_stop(&mut self) -> SubscriptionTeardown {
            self.stopped.store(true, Ordering::Release);
            if let Some(signal) = self.stop_signal.lock().take() {
                let _ = signal.send(());
            }
            let (tx, rx) = mpsc::channel();
            tx.send(Ok(())).unwrap();
            SubscriptionTeardown::new(rx)
        }
    }

    struct HotplugFixtureAdapter {
        candidates: Mutex<Vec<RecordingInputDevice>>,
        sub_callback: Mutex<Option<Arc<dyn Fn(SelectedInputEvent) + Send + Sync>>>,
        sub_stopped: Mutex<Option<Arc<AtomicBool>>>,
        watch_callback: Mutex<Option<Arc<dyn Fn(DeviceArrivalEvent) + Send + Sync>>>,
        watch_stopped: Mutex<Option<Arc<AtomicBool>>>,
        watch_started_tx: Mutex<Option<Sender<()>>>,
        watch_stopped_tx: Mutex<Option<Sender<()>>>,
        subscribe_calls: AtomicUsize,
        watch_calls: AtomicUsize,
        last_subscribed: Mutex<Option<(SelectedDevices, u64)>>,
    }

    impl HotplugFixtureAdapter {
        fn new(initial_candidates: Vec<RecordingInputDevice>) -> Arc<Self> {
            Arc::new(Self {
                candidates: Mutex::new(initial_candidates),
                sub_callback: Mutex::new(None),
                sub_stopped: Mutex::new(None),
                watch_callback: Mutex::new(None),
                watch_stopped: Mutex::new(None),
                watch_started_tx: Mutex::new(None),
                watch_stopped_tx: Mutex::new(None),
                subscribe_calls: AtomicUsize::new(0),
                watch_calls: AtomicUsize::new(0),
                last_subscribed: Mutex::new(None),
            })
        }

        fn set_candidates(&self, next: Vec<RecordingInputDevice>) {
            *self.candidates.lock() = next;
        }

        fn arm_watch_started_signal(&self) -> Receiver<()> {
            let (tx, rx) = mpsc::channel();
            *self.watch_started_tx.lock() = Some(tx);
            rx
        }

        fn arm_watch_stopped_signal(&self) -> Receiver<()> {
            let (tx, rx) = mpsc::channel();
            *self.watch_stopped_tx.lock() = Some(tx);
            rx
        }

        fn emit_input_event(&self, event: SelectedInputEvent) {
            let cb = self
                .sub_callback
                .lock()
                .clone()
                .expect("active sub callback");
            cb(event);
        }

        fn emit_arrival(&self, input_id: &str) {
            let cb = self
                .watch_callback
                .lock()
                .clone()
                .expect("active watch callback");
            cb(DeviceArrivalEvent {
                input_id: input_id.into(),
            });
        }

        fn last_sub_stopped(&self) -> bool {
            self.sub_stopped
                .lock()
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
        }

        fn last_watch_stopped(&self) -> bool {
            self.watch_stopped
                .lock()
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
        }
    }

    impl LocalInputAdapter for HotplugFixtureAdapter {
        fn subscribe(
            &self,
            _declaration: &LocalRecordingTrigger,
            selected: &SelectedDevices,
            generation: u64,
            callback: Arc<dyn Fn(SelectedInputEvent) + Send + Sync>,
        ) -> Result<Box<dyn LocalInputSubscription>, LocalInputSetupError> {
            self.subscribe_calls.fetch_add(1, Ordering::AcqRel);
            *self.sub_callback.lock() = Some(callback);
            *self.last_subscribed.lock() = Some((selected.clone(), generation));
            let stopped = Arc::new(AtomicBool::new(false));
            *self.sub_stopped.lock() = Some(stopped.clone());
            Ok(Box::new(FakeSub { stopped }))
        }

        fn enumerate_inputs(
            &self,
            _declaration: &LocalRecordingTrigger,
        ) -> Result<Option<Vec<RecordingInputDevice>>, String> {
            Ok(Some(self.candidates.lock().clone()))
        }

        fn watch_arrivals(
            &self,
            _declaration: &LocalRecordingTrigger,
            callback: Arc<dyn Fn(DeviceArrivalEvent) + Send + Sync>,
        ) -> Result<Box<dyn LocalInputArrivalWatch>, LocalInputSetupError> {
            self.watch_calls.fetch_add(1, Ordering::AcqRel);
            *self.watch_callback.lock() = Some(callback);
            let stopped = Arc::new(AtomicBool::new(false));
            *self.watch_stopped.lock() = Some(stopped.clone());
            let stop_signal = self.watch_stopped_tx.lock().take();
            if let Some(started) = self.watch_started_tx.lock().take() {
                let _ = started.send(());
            }
            Ok(Box::new(FakeWatch {
                stopped,
                stop_signal: Mutex::new(stop_signal),
            }))
        }
    }

    async fn wait_rx<T: Send + 'static>(rx: Receiver<T>) -> T {
        tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .await
            .unwrap()
    }

    async fn wait_reconcile_idle(service: &ServiceContext, plugin_id: &str) {
        if let Some(rx) = service
            .ext::<Arc<LocalRecordingHotplug>>()
            .wait_in_flight(plugin_id)
        {
            wait_rx(rx).await;
        }
    }

    #[tokio::test]
    async fn hotplug_removal_activates_arrival_watch_and_reconnect_rearms_without_synthetic_press()
    {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;
        let registry = service.ext::<LocalRecordingRegistryHandle>().clone();
        registry.register("trigger", trigger_decl(), true);
        service
            .local_recording_select_devices("trigger", selected(PORT_A))
            .unwrap();

        let (status_tx, status_rx) = mpsc::channel();
        let status_listener = fixture.ui.listen("local-recording-status", move |event| {
            let _ = status_tx.send(event.clone());
        });
        let capture_events = Arc::new(AtomicUsize::new(0));
        let capture_counter = capture_events.clone();
        let capture_listener = fixture.ui.listen("local-recording-capture", move |_event| {
            capture_counter.fetch_add(1, Ordering::AcqRel);
        });

        let adapter = HotplugFixtureAdapter::new(vec![hid_candidate(PORT_A, "Col01")]);
        let (_, done) = service
            .local_recording_set_enabled_with_adapter("trigger", true, adapter.clone())
            .unwrap();
        wait_rx(done.unwrap()).await;

        let initial = registry.status("trigger").unwrap();
        assert!(initial.available);
        assert_eq!(capture_events.load(Ordering::Acquire), 0);

        let watch_started = adapter.arm_watch_started_signal();
        let watch_stopped = adapter.arm_watch_stopped_signal();
        adapter.set_candidates(Vec::new());
        adapter.emit_input_event(SelectedInputEvent::Unavailable {
            generation: initial.generation,
            reason: InputUnavailable::Removed,
        });
        wait_rx(watch_started).await;
        wait_reconcile_idle(service, "trigger").await;
        assert!(adapter.last_sub_stopped());
        assert!(!registry.status("trigger").unwrap().available);
        assert!(service
            .ext::<Arc<LocalRecordingHotplug>>()
            .has_active_watcher("trigger"));

        adapter.set_candidates(vec![hid_candidate(PORT_A, "Col01")]);
        adapter.emit_arrival(PORT_A);
        wait_rx(watch_stopped).await;
        assert!(adapter.last_watch_stopped());

        let rearmed_status = tokio::task::spawn_blocking(move || loop {
            let event = status_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if event["available"] == true && event["generation"].as_u64() > Some(initial.generation)
            {
                return event;
            }
        })
        .await
        .unwrap();
        let rearmed_gen = rearmed_status["generation"].as_u64().unwrap();
        assert!(rearmed_gen > initial.generation);
        assert!(registry.status("trigger").unwrap().available);
        assert_eq!(capture_events.load(Ordering::Acquire), 0);
        assert_eq!(
            service
                .local_recording_signal("trigger", initial.generation)
                .unwrap_err(),
            "subscription_stale"
        );
        let active = registry
            .begin("trigger", &selected(PORT_A), rearmed_gen)
            .unwrap();
        assert!(active.load(Ordering::Acquire));
        registry.cancel("trigger");

        fixture.ui.unlisten(status_listener);
        fixture.ui.unlisten(capture_listener);
    }

    #[tokio::test]
    async fn hotplug_port_change_arrival_rebinds_persisted_selection_and_rearms() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;
        let registry = service.ext::<LocalRecordingRegistryHandle>().clone();
        let store = service.ext::<Arc<RecordingSelectionStore>>().clone();
        registry.register("trigger", trigger_decl(), true);
        service
            .local_recording_select_devices("trigger", selected(PORT_A))
            .unwrap();

        let adapter = HotplugFixtureAdapter::new(vec![hid_candidate(PORT_A, "Col01")]);
        let (_, done) = service
            .local_recording_set_enabled_with_adapter("trigger", true, adapter.clone())
            .unwrap();
        wait_rx(done.unwrap()).await;
        let gen_port_a = registry.status("trigger").unwrap().generation;

        let watch_started = adapter.arm_watch_started_signal();
        let watch_stopped = adapter.arm_watch_stopped_signal();
        adapter.set_candidates(Vec::new());
        adapter.emit_input_event(SelectedInputEvent::Unavailable {
            generation: gen_port_a,
            reason: InputUnavailable::Removed,
        });
        wait_rx(watch_started).await;
        wait_reconcile_idle(service, "trigger").await;

        let (status_tx, status_rx) = mpsc::channel();
        let listener = fixture.ui.listen("local-recording-status", move |event| {
            let _ = status_tx.send(event.clone());
        });
        adapter.set_candidates(vec![hid_candidate(PORT_B, "Col01")]);
        adapter.emit_arrival(PORT_B);
        wait_rx(watch_stopped).await;

        let rearmed_event = tokio::task::spawn_blocking(move || loop {
            let event = status_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if event["available"] == true && event["generation"].as_u64() > Some(gen_port_a) {
                return event;
            }
        })
        .await
        .unwrap();
        let gen_port_b = rearmed_event["generation"].as_u64().unwrap();
        assert!(gen_port_b > gen_port_a);
        assert_eq!(store.get("trigger").unwrap(), Some(selected(PORT_B)));
        assert_eq!(
            adapter.last_subscribed.lock().clone(),
            Some((selected(PORT_B), gen_port_b))
        );
        fixture.ui.unlisten(listener);
    }

    #[tokio::test]
    async fn hotplug_ambiguous_arrival_fails_closed_without_binding_or_persisting() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;
        let registry = service.ext::<LocalRecordingRegistryHandle>().clone();
        let store = service.ext::<Arc<RecordingSelectionStore>>().clone();
        store.set("trigger", &selected(PORT_C)).unwrap();
        store.set_enabled("trigger", true).unwrap();
        registry.register("trigger", trigger_decl(), true);

        let adapter = HotplugFixtureAdapter::new(vec![
            hid_candidate(PORT_A, "Col01"),
            hid_candidate(PORT_B, "Col01"),
        ]);
        let done = service
            .restore_local_recording_plugin_with_adapter("trigger", adapter.clone())
            .unwrap();
        wait_rx(done).await;

        assert_eq!(adapter.subscribe_calls.load(Ordering::Acquire), 0);
        assert!(!registry.status("trigger").unwrap().available);
        assert_eq!(store.get("trigger").unwrap(), Some(selected(PORT_C)));
    }

    #[tokio::test]
    async fn hotplug_startup_and_reload_auto_attach_connected_and_watch_when_disconnected() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;
        let registry = service.ext::<LocalRecordingRegistryHandle>().clone();
        let store = service.ext::<Arc<RecordingSelectionStore>>().clone();
        store.set("trigger", &selected(PORT_A)).unwrap();
        store.set_enabled("trigger", true).unwrap();

        registry.register("trigger", trigger_decl(), false);
        let ungranted_adapter = HotplugFixtureAdapter::new(vec![hid_candidate(PORT_A, "Col01")]);
        assert!(service
            .restore_local_recording_plugin_with_adapter("trigger", ungranted_adapter.clone())
            .is_none());
        assert_eq!(ungranted_adapter.subscribe_calls.load(Ordering::Acquire), 0);
        assert_eq!(ungranted_adapter.watch_calls.load(Ordering::Acquire), 0);

        registry.register("trigger", trigger_decl(), true);
        let connected_adapter = HotplugFixtureAdapter::new(vec![hid_candidate(PORT_A, "Col01")]);
        let done = service
            .restore_local_recording_plugin_with_adapter("trigger", connected_adapter.clone())
            .unwrap();
        wait_rx(done).await;
        assert!(registry.status("trigger").unwrap().available);
        assert_eq!(connected_adapter.subscribe_calls.load(Ordering::Acquire), 1);

        registry.register("trigger", trigger_decl(), true);
        let delayed_adapter = HotplugFixtureAdapter::new(Vec::new());
        let watch_started = delayed_adapter.arm_watch_started_signal();
        let watch_stopped = delayed_adapter.arm_watch_stopped_signal();
        let done = service
            .restore_local_recording_plugin_with_adapter("trigger", delayed_adapter.clone())
            .unwrap();
        wait_rx(done).await;
        wait_rx(watch_started).await;
        assert!(!registry.status("trigger").unwrap().available);
        assert_eq!(delayed_adapter.subscribe_calls.load(Ordering::Acquire), 0);

        let (status_tx, status_rx) = mpsc::channel();
        let listener = fixture.ui.listen("local-recording-status", move |event| {
            let _ = status_tx.send(event.clone());
        });
        delayed_adapter.set_candidates(vec![hid_candidate(PORT_A, "Col01")]);
        delayed_adapter.emit_arrival(PORT_A);
        wait_rx(watch_stopped).await;
        tokio::task::spawn_blocking(move || loop {
            let event = status_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if event["available"] == true {
                break;
            }
        })
        .await
        .unwrap();
        assert!(registry.status("trigger").unwrap().available);
        assert_eq!(delayed_adapter.subscribe_calls.load(Ordering::Acquire), 1);
        fixture.ui.unlisten(listener);

        store.set_enabled("trigger", false).unwrap();
        registry.register("trigger", trigger_decl(), true);
        let disabled_adapter = HotplugFixtureAdapter::new(vec![hid_candidate(PORT_A, "Col01")]);
        assert!(service
            .restore_local_recording_plugin_with_adapter("trigger", disabled_adapter.clone())
            .is_none());
        let status = registry.status("trigger").unwrap();
        assert!(status.devices_selected);
        assert!(!status.enabled);
        assert!(!status.available);
        assert_eq!(disabled_adapter.subscribe_calls.load(Ordering::Acquire), 0);
        assert_eq!(disabled_adapter.watch_calls.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn hotplug_disable_revoke_unload_and_cancel_stop_watcher_and_refuse_late_arrival() {
        #[derive(Clone, Copy)]
        enum StopTrigger {
            Disable,
            Revoke,
            Unload,
            Cancel,
        }

        for trigger_kind in [
            StopTrigger::Disable,
            StopTrigger::Revoke,
            StopTrigger::Unload,
            StopTrigger::Cancel,
        ] {
            let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
            let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
            let service = &fixture.service;
            let registry = service.ext::<LocalRecordingRegistryHandle>().clone();
            let store = service.ext::<Arc<RecordingSelectionStore>>().clone();
            store.set("trigger", &selected(PORT_A)).unwrap();
            store.set_enabled("trigger", true).unwrap();
            registry.register("trigger", trigger_decl(), true);

            let adapter = HotplugFixtureAdapter::new(Vec::new());
            let watch_started = adapter.arm_watch_started_signal();
            let watch_stopped = adapter.arm_watch_stopped_signal();
            let done = service
                .restore_local_recording_plugin_with_adapter("trigger", adapter.clone())
                .unwrap();
            wait_rx(done).await;
            wait_rx(watch_started).await;
            assert!(service
                .ext::<Arc<LocalRecordingHotplug>>()
                .has_active_watcher("trigger"));

            match trigger_kind {
                StopTrigger::Disable => {
                    service
                        .local_recording_set_enabled_with_adapter("trigger", false, adapter.clone())
                        .unwrap();
                }
                StopTrigger::Revoke => registry.revoke("trigger"),
                StopTrigger::Unload => registry.unregister("trigger"),
                StopTrigger::Cancel => registry.cancel("trigger"),
            }
            wait_rx(watch_stopped).await;
            assert!(adapter.last_watch_stopped());
            assert!(!service
                .ext::<Arc<LocalRecordingHotplug>>()
                .has_active_watcher("trigger"));

            if !matches!(trigger_kind, StopTrigger::Cancel) {
                adapter.set_candidates(vec![hid_candidate(PORT_A, "Col01")]);
                let late_done = service.schedule_local_recording_reconcile(
                    "trigger".into(),
                    adapter.clone(),
                    ReconcileMode::AttachOrWatch,
                );
                wait_rx(late_done).await;
                assert_eq!(adapter.subscribe_calls.load(Ordering::Acquire), 0);
                assert!(!service
                    .ext::<Arc<LocalRecordingHotplug>>()
                    .has_active_watcher("trigger"));
            }
        }
    }
}
