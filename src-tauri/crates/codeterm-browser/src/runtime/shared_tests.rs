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
}

impl FakeConnection {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FakeState::default()),
            closed: watch::channel(None).0,
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
