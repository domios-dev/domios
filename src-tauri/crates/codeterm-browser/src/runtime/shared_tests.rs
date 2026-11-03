use super::*;
use crate::cdp::{CdpConnection, CdpError, EventStream};
use crate::cdp_wire::CdpEvent;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, watch};

#[derive(Default)]
struct FakeState {
    next_target: usize,
    targets: HashSet<String>,
    sessions: HashMap<String, String>,
    requests: Vec<(Option<String>, String, Value)>,
    subscribers: Vec<(EventFilter, mpsc::Sender<Arc<CdpEvent>>)>,
}

struct FakeConnection {
    state: Mutex<FakeState>,
    closed: watch::Sender<Option<String>>,
    fail_metrics: std::sync::atomic::AtomicBool,
}

impl FakeConnection {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FakeState::default()),
            closed: watch::channel(None).0,
            fail_metrics: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn emit(&self, method: &str, session: Option<&str>, params: Value) {
        let event = Arc::new(CdpEvent {
            method: method.into(),
            session_id: session.map(str::to_string),
            params,
        });
        self.state.lock().subscribers.retain(|(filter, sender)| {
            if filter.matches(&event) {
                let _ = sender.try_send(event.clone());
            }
            !sender.is_closed()
        });
    }

    fn count(&self, method: &str) -> usize {
        self.state
            .lock()
            .requests
            .iter()
            .filter(|(_, sent, _)| sent == method)
            .count()
    }
}

#[async_trait::async_trait]
impl CdpConnection for FakeConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/devtools/browser/injected"
    }
    fn closed(&self) -> watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, reason: &str) {
        self.closed.send_replace(Some(reason.into()));
        self.state.lock().subscribers.clear();
    }
    fn subscribe(&self, filter: EventFilter) -> EventStream {
        let (sender, receiver) = mpsc::channel(256);
        self.state.lock().subscribers.push((filter, sender));
        EventStream::from_receiver(receiver)
    }
    async fn send_to(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        _: Duration,
    ) -> Result<Value, CdpError> {
        if let Some(reason) = self.closed.borrow().clone() {
            return Err(CdpError::Closed(reason));
        }
        {
            let mut state = self.state.lock();
            state
                .requests
                .push((session.map(str::to_string), method.into(), params.clone()));
            if let Some(session) = session {
                let live = state
                    .sessions
                    .get(session)
                    .is_some_and(|target| state.targets.contains(target));
                if !live {
                    return Err(CdpError::Closed("target closed".into()));
                }
            }
        }
        match method {
            "Target.createTarget" => {
                let target = {
                    let mut state = self.state.lock();
                    state.next_target += 1;
                    let target = format!("T{}", state.next_target);
                    state.targets.insert(target.clone());
                    target
                };
                self.emit(
                    "Target.targetCreated",
                    None,
                    json!({ "targetInfo": { "targetId": target } }),
                );
                Ok(json!({ "targetId": target }))
            }
            "Target.attachToTarget" => {
                let target = params["targetId"].as_str().unwrap();
                let session = format!("S{target}");
                self.state
                    .lock()
                    .sessions
                    .insert(session.clone(), target.into());
                self.emit(
                    "Target.attachedToTarget",
                    None,
                    json!({ "sessionId": session, "targetInfo": { "targetId": target } }),
                );
                Ok(json!({ "sessionId": session }))
            }
            "Target.closeTarget" => {
                let removed = self
                    .state
                    .lock()
                    .targets
                    .remove(params["targetId"].as_str().unwrap());
                Ok(json!({ "success": removed }))
            }
            "Page.getLayoutMetrics" if self.fail_metrics.load(Ordering::SeqCst) => {
                Err(CdpError::Malformed("injected metrics failure".into()))
            }
            "Page.getLayoutMetrics" => Ok(
                json!({"cssLayoutViewport":{"pageX":0,"pageY":0,"clientWidth":390,"clientHeight":844},"cssContentSize":{"x":0,"y":0,"width":400,"height":1000}}),
            ),
            "Runtime.evaluate" if params["expression"] == "window.devicePixelRatio" => {
                Ok(json!({"result":{"type":"number","value":1.0}}))
            }
            "Page.captureScreenshot" => {
                Ok(json!({ "data": if session == Some("ST1") { "QQ==" } else { "Qg==" } }))
            }
            _ => Ok(json!({})),
        }
    }
}

fn hub() -> Arc<FrameHub> {
    FrameHub::new(super::super::screencast::ScreencastParams::new(
        Viewport::default(),
    ))
}

async fn acquire(
    dir: PathBuf,
    connection: Arc<FakeConnection>,
    launches: Arc<AtomicUsize>,
) -> ProfileLease {
    let launch_dir = dir.clone();
    ProfileLease::acquire_with(dir, move || async move {
        launches.fetch_add(1, Ordering::SeqCst);
        Ok(SharedBrowser {
            client: CdpClient::from_connection(connection),
            process: None,
            binary: None,
            profile: ProfileKind::Named {
                name: "shared-test".into(),
            },
            dir: launch_dir,
        })
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn two_surfaces_share_connection_with_distinct_targets_frames_and_input() {
    for reverse in [false, true] {
        let dir = PathBuf::from(format!("/fake-profile/{}", uuid::Uuid::new_v4()));
        let connection = FakeConnection::new();
        let launches = Arc::new(AtomicUsize::new(0));
        let first = acquire(dir.clone(), connection.clone(), launches.clone()).await;
        let second = acquire(dir.clone(), connection.clone(), launches.clone()).await;
        assert!(Arc::ptr_eq(&first.browser, &second.browser));
        assert_eq!(launches.load(Ordering::SeqCst), 1);
        let first_hub = hub();
        let second_hub = hub();
        let mut first_frames = first_hub.subscribe();
        let mut second_frames = second_hub.subscribe();
        let first = Arc::new(
            from_lease(first, Viewport::default(), first_hub)
                .await
                .unwrap(),
        );
        let second = Arc::new(
            from_lease(second, Viewport::default(), second_hub)
                .await
                .unwrap(),
        );
        assert_eq!(
            (first.page.target_id(), second.page.target_id()),
            ("T1", "T2")
        );
        assert_eq!(
            (first.page.session_id(), second.page.session_id()),
            ("ST1", "ST2")
        );
        let still_a = tokio::time::timeout(Duration::from_secs(1), first_frames.next())
            .await
            .unwrap()
            .unwrap();
        let still_b = tokio::time::timeout(Duration::from_secs(1), second_frames.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (&still_a.frame.jpeg, &still_b.frame.jpeg),
            (&vec![b'A'], &vec![b'B'])
        );
        connection.emit("Page.screencastFrame", Some(first.page.session_id()),
            json!({ "sessionId": 7, "data": "YQ==", "metadata": { "deviceWidth": 1600, "deviceHeight": 900 } }));
        let cast_a = tokio::time::timeout(Duration::from_secs(1), first_frames.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cast_a.frame.jpeg, vec![b'a']);
        assert_eq!(second_frames.latest().unwrap().jpeg, vec![b'B']);
        connection.emit("Page.screencastFrame", Some(second.page.session_id()),
            json!({ "sessionId": 8, "data": "Yg==", "metadata": { "deviceWidth": 1600, "deviceHeight": 900 } }));
        let cast_b = tokio::time::timeout(Duration::from_secs(1), second_frames.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cast_b.frame.jpeg, vec![b'b']);
        assert_eq!(first_frames.latest().unwrap().jpeg, vec![b'a']);
        for (page, value) in [(&first.page, 11), (&second.page, 22)] {
            page.send("Input.dispatchMouseEvent", json!({ "x": value }))
                .await
                .unwrap();
        }
        let input: Vec<_> = connection
            .state
            .lock()
            .requests
            .iter()
            .filter(|(_, method, _)| method == "Input.dispatchMouseEvent")
            .cloned()
            .collect();
        assert_eq!(
            input,
            vec![
                (
                    Some("ST1".into()),
                    "Input.dispatchMouseEvent".into(),
                    json!({ "x": 11 })
                ),
                (
                    Some("ST2".into()),
                    "Input.dispatchMouseEvent".into(),
                    json!({ "x": 22 })
                )
            ]
        );
        let (early, survivor) = if reverse {
            (second, first)
        } else {
            (first, second)
        };
        let early_target = early.page.target_id().to_string();
        stop(early).await;
        assert!(!connection.state.lock().targets.contains(&early_target));
        assert_eq!(connection.count("Browser.close"), 0);
        assert!(survivor.is_alive());
        survivor
            .page
            .send("Page.navigate", json!({ "url": "https://survivor.test/" }))
            .await
            .unwrap();
        connection.emit("Page.screencastFrame", Some(survivor.page.session_id()),
            json!({ "sessionId": 9, "data": "Yw==", "metadata": { "deviceWidth": 1600, "deviceHeight": 900 } }));
        let survivor_frames = if reverse {
            &mut first_frames
        } else {
            &mut second_frames
        };
        let surviving_cast = tokio::time::timeout(Duration::from_secs(1), survivor_frames.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(surviving_cast.frame.jpeg, vec![b'c']);
        stop(survivor).await;
        assert!(connection.state.lock().targets.is_empty());
        assert_eq!(connection.count("Browser.close"), 1);
        assert!(connection.closed.borrow().is_some());
        assert!(!ProfileLease::leased_dirs().contains(&dir));
    }
}

#[tokio::test]
async fn crash_retires_all_old_leases_and_relaunch_survives_stale_close() {
    let dir = PathBuf::from(format!("/fake-profile/{}", uuid::Uuid::new_v4()));
    let dead = FakeConnection::new();
    let launches = Arc::new(AtomicUsize::new(0));
    let first = acquire(dir.clone(), dead.clone(), launches.clone()).await;
    let second = acquire(dir.clone(), dead.clone(), launches.clone()).await;
    let first = Arc::new(from_lease(first, Viewport::default(), hub()).await.unwrap());
    let second = Arc::new(
        from_lease(second, Viewport::default(), hub())
            .await
            .unwrap(),
    );
    let mut first_events = first.page.subscribe_session(&[]);
    let mut second_events = second.page.subscribe_session(&[]);
    dead.close("injected process crash");
    assert!(!first.is_alive());
    assert!(!second.is_alive());
    assert!(first_events.recv().await.is_none());
    assert!(second_events.recv().await.is_none());
    let replacement = FakeConnection::new();
    let next = acquire(dir, replacement.clone(), launches.clone()).await;
    assert_eq!(launches.load(Ordering::SeqCst), 2);
    stop(first).await;
    stop(second).await;
    assert!(next.browser.is_alive());
    assert_eq!(replacement.count("Browser.close"), 0);
    next.release(CLOSE_DEADLINE).await;
    assert_eq!(replacement.count("Browser.close"), 1);
}

#[tokio::test]
async fn ephemeral_instance_profiles_refuse_removal_until_last_lease_closes() {
    let root = tempfile::tempdir().unwrap();
    let layout = ProfileLayout::new(root.path().to_path_buf());
    layout
        .prepare_instance("qa", "http://localhost:7686", true)
        .unwrap();
    let dir = layout.profiles_dir().join("qa");
    let connection = FakeConnection::new();
    let launches = Arc::new(AtomicUsize::new(0));
    let first = acquire(dir.clone(), connection.clone(), launches.clone()).await;
    let second = acquire(dir.clone(), connection.clone(), launches).await;
    assert!(crate::profile::delete_named_profile(&layout, "qa")
        .await
        .unwrap_err()
        .contains("in use"));
    first.release(CLOSE_DEADLINE).await;
    assert!(dir.exists());
    second.release(CLOSE_DEADLINE).await;
    assert!(!dir.exists());
}

#[tokio::test]
async fn temporary_mobile_capture_restores_previous_metrics_after_success_and_failure() {
    for fail in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let connection = FakeConnection::new();
        let lease = acquire(
            root.path().join("profile"),
            connection.clone(),
            Arc::new(AtomicUsize::new(0)),
        )
        .await;
        let host = crate::BrowserHost::new(crate::BrowserHostConfig::under_data_dir(root.path()));
        let runtime = from_lease(lease, Viewport::default(), hub()).await.unwrap();
        host.install(runtime);
        connection.fail_metrics.store(fail, Ordering::SeqCst);
        let result = host
            .capture_with_viewport(
                Viewport {
                    width: 390,
                    height: 844,
                    dpr: 2.0,
                    mobile: true,
                },
                false,
            )
            .await;
        assert_eq!(result.is_err(), fail);
        if let Ok(shot) = result {
            assert_eq!(shot["metrics"]["cssContentSize"]["width"], 400);
            assert_eq!(shot["metrics"]["devicePixelRatio"], 2.0);
            let requests = connection.state.lock().requests.clone();
            let capture = requests
                .iter()
                .rev()
                .find(|(_, method, _)| method == "Page.captureScreenshot")
                .unwrap();
            assert_eq!(
                capture.2["clip"],
                json!({"x":0.0,"y":0.0,"width":390.0,"height":844.0,"scale":1})
            );
        }
        let metrics: Vec<_> = connection
            .state
            .lock()
            .requests
            .iter()
            .filter(|(_, method, _)| method == "Emulation.setDeviceMetricsOverride")
            .map(|(_, _, params)| params.clone())
            .collect();
        assert_eq!(metrics[metrics.len() - 2]["mobile"], true);
        assert_eq!(
            metrics.last().unwrap(),
            &json!({"width":1600,"height":900,"deviceScaleFactor":1.0,"mobile":false,
                "viewport":{"x":0,"y":0,"width":1600,"height":900,"scale":1}})
        );
        assert_eq!(host.viewport(), Viewport::default());
        host.close().await;
    }
}

#[tokio::test]
async fn ordinary_owned_captures_resolve_css_bounds_but_attached_captures_pass_through() {
    let root = tempfile::tempdir().unwrap();
    let connection = FakeConnection::new();
    let lease = acquire(
        root.path().join("profile"),
        connection.clone(),
        Arc::new(AtomicUsize::new(0)),
    )
    .await;
    let host = crate::BrowserHost::new(crate::BrowserHostConfig::under_data_dir(root.path()));
    let runtime = from_lease(lease, Viewport::default(), hub()).await.unwrap();
    let page = runtime.page.clone();
    host.install(runtime);
    for owned in [true, false] {
        if !owned {
            *host.viewport.lock() = crate::viewport::ViewportModel::new(
                crate::viewport::ViewportMode::Attached,
                Viewport::default(),
            );
        }
        let start = connection.state.lock().requests.len();
        host.capture_screenshot(&page, json!({"format":"png","captureBeyondViewport":false}))
            .await
            .unwrap();
        let requests = connection.state.lock().requests[start..].to_vec();
        assert_eq!(
            requests
                .iter()
                .any(|(_, method, _)| method == "Page.getLayoutMetrics"),
            owned
        );
        let capture = requests
            .iter()
            .find(|(_, method, _)| method == "Page.captureScreenshot")
            .unwrap();
        assert_eq!(capture.2.get("clip").is_some(), owned);
        if owned {
            assert_eq!(
                capture.2["clip"],
                json!({"x":0.0,"y":0.0,"width":390.0,"height":844.0,"scale":1})
            );
        } else {
            assert_eq!(
                capture.2,
                json!({"format":"png","captureBeyondViewport":false})
            );
        }
    }
    host.close().await;
}

#[tokio::test]
async fn owned_scaled_compositor_keeps_still_and_live_frames_in_css_coordinates() {
    let root = tempfile::tempdir().unwrap();
    let connection = FakeConnection::new();
    let lease = acquire(
        root.path().join("profile"),
        connection.clone(),
        Arc::new(AtomicUsize::new(0)),
    )
    .await;
    let host = crate::BrowserHost::new(crate::BrowserHostConfig::under_data_dir(root.path()));
    let mut viewport = Viewport {
        width: 1320,
        height: 996,
        dpr: 1.25,
        mobile: false,
    };
    host.set_viewport(crate::ViewportRequest {
        width: 1320.0,
        height: 996.0,
        dpr: viewport.dpr,
    })
    .await
    .unwrap();
    let mut viewer = host.subscribe_screencast();
    let runtime = from_lease(lease, viewport, host.frames.clone())
        .await
        .unwrap();
    let page = runtime.page.clone();
    host.install(runtime);
    for dpr in [1.25, 1.5] {
        if viewport.dpr != dpr {
            viewport.dpr = dpr;
            host.set_viewport(crate::ViewportRequest {
                width: 1320.0,
                height: 996.0,
                dpr,
            })
            .await
            .unwrap();
        }
        let still = tokio::time::timeout(Duration::from_secs(1), viewer.next())
            .await
            .unwrap()
            .unwrap();
        assert!(still.frame.still);
        assert_eq!(
            (
                still.frame.metadata.device_width,
                still.frame.metadata.device_height
            ),
            (1320.0, 996.0)
        );
        for ack in 1..=3 {
            connection.emit("Page.screencastFrame", Some(page.session_id()), json!({
                "sessionId": ack, "data": "TA==",
                "metadata": { "deviceWidth": 1320.0 * dpr, "deviceHeight": 996.0 * dpr, "scrollOffsetY": 222.0 }
            }));
            let live = tokio::time::timeout(Duration::from_secs(1), viewer.next())
                .await
                .unwrap()
                .unwrap();
            assert!(!live.frame.still);
            assert_eq!(live.frame.jpeg, vec![b'L']);
            assert_eq!(
                (
                    live.frame.metadata.device_width,
                    live.frame.metadata.device_height
                ),
                (1320.0, 996.0)
            );
            assert_eq!(live.frame.metadata.scroll_offset_y, 222.0);
        }
        let requests = connection.state.lock().requests.clone();
        let metrics = requests
            .iter()
            .rev()
            .find(|(_, method, _)| method == "Emulation.setDeviceMetricsOverride")
            .unwrap();
        assert_eq!(metrics.2["deviceScaleFactor"], dpr);
        assert_eq!(
            metrics.2["viewport"],
            json!({"x":0,"y":0,"width":1320,"height":996,"scale":1})
        );
        let start = requests
            .iter()
            .rev()
            .find(|(_, method, _)| method == "Page.startScreencast")
            .unwrap();
        let (width, height) = viewport.device_size();
        assert_eq!(start.2["maxWidth"], width);
        assert_eq!(start.2["maxHeight"], height);
    }
    host.close().await;
}
