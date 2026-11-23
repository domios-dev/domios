//! Browser load: navigations, input batches, screencast frames and CSS cursor
//! events share one CDP connection and frame hub without stalls or loss.

use super::*;
use crate::screencast::{DriverCommand, FrameHub, FrameMetadata, ScreencastParams};
use crate::viewport::Viewport;

const FAILURE_BOUND: Duration = Duration::from_secs(60);
const NAVIGATORS: usize = 3;
const NAVIGATIONS: usize = 8;
const TYPISTS: usize = 3;
const CLICKS: usize = 8;
const FRAMES: u64 = 12;
const CURSORS: usize = 12;
const VIEWERS: usize = 4;

fn viewport() -> Viewport {
    Viewport {
        width: 1280,
        height: 720,
        dpr: 1.0,
        mobile: false,
    }
}

fn session_event(method: &str, params: Value) -> Message {
    Message::text(json!({"method": method, "params": params, "sessionId": "S1"}).to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn browser_nav_input_screencast_and_cursor_share_one_connection_under_load() {
    let chrome = fake_chrome().await;
    let events = chrome.outbound.clone();
    let page = PageChannel {
        client: CdpClient::connect(&chrome.endpoint).await.unwrap(),
        session_id: "S1".into(),
        target_id: "T1".into(),
    };
    let methods: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
    let responder = tokio::spawn({
        let methods = methods.clone();
        async move {
            let FakeChrome {
                mut inbound,
                outbound,
                ..
            } = chrome;
            while let Some(request) = inbound.recv().await {
                *methods
                    .lock()
                    .entry(request["method"].as_str().unwrap_or_default().to_owned())
                    .or_default() += 1;
                let reply = json!({"id": request["id"], "result": {}});
                if outbound.send(Message::text(reply.to_string())).is_err() {
                    break;
                }
            }
        }
    });

    let hub = FrameHub::new(ScreencastParams::new(viewport()));
    let (driver, mut commands) = mpsc::unbounded_channel();
    let generation = hub.bind(driver);
    let (cursor_tx, cursor_rx) = watch::channel("default".to_string());

    let load = async {
        let navigators = (0..NAVIGATORS).map(|navigator| {
            let page = page.clone();
            tokio::spawn(async move {
                for index in 0..NAVIGATIONS {
                    let url = format!("https://load.test/{navigator}/{index}");
                    page.send("Page.navigate", json!({ "url": url }))
                        .await
                        .expect("navigation reply");
                }
            })
        });
        let typists = (0..TYPISTS).map(|_| {
            let page = page.clone();
            tokio::spawn(async move {
                for _ in 0..CLICKS {
                    let phases = vec![
                        ("Input.dispatchMouseEvent", json!({"type": "mousePressed"})),
                        ("Input.dispatchMouseEvent", json!({"type": "mouseReleased"})),
                    ];
                    assert_eq!(page.send_batch(phases).await.expect("input").len(), 2);
                }
            })
        });
        let actors: Vec<_> = navigators.chain(typists).collect();

        let (frame_ack, mut frame_acked) = mpsc::unbounded_channel::<u64>();
        let mut frame_events = page.subscribe_session(&["Page.screencastFrame"]);
        let caster = tokio::spawn({
            let hub = hub.clone();
            async move {
                while let Some(event) = frame_events.recv().await {
                    let index = event.params["index"].as_u64().unwrap();
                    let metadata = FrameMetadata::still(viewport(), index as f64);
                    assert!(hub.publish(generation, vec![index as u8], metadata, false));
                    frame_ack.send(index).unwrap();
                    if index == FRAMES {
                        return;
                    }
                }
                panic!("screencast event stream ended early");
            }
        });
        let (cursor_ack, mut cursor_acked) = mpsc::unbounded_channel::<usize>();
        let mut cursor_events = page.subscribe_session(&["Runtime.bindingCalled"]);
        let mirror = tokio::spawn(async move {
            while let Some(event) = cursor_events.recv().await {
                let index = event.params["index"].as_u64().unwrap() as usize;
                let cursor = event.params["payload"].as_str().unwrap().to_owned();
                cursor_tx.send_if_modified(|current| {
                    let changed = *current != cursor;
                    *current = cursor;
                    changed
                });
                cursor_ack.send(index).unwrap();
                if index == CURSORS {
                    return;
                }
            }
            panic!("cursor event stream ended early");
        });
        let viewers: Vec<_> = (0..VIEWERS)
            .map(|_| {
                let mut subscription = hub.subscribe();
                tokio::spawn(async move {
                    let mut last = 0;
                    loop {
                        let delivery = subscription.next().await.expect("hub open");
                        assert!(delivery.frame.seq > last, "frames went backwards");
                        last = delivery.frame.seq;
                        if delivery.frame.seq == FRAMES {
                            return;
                        }
                    }
                })
            })
            .collect();
        let cursor_viewer = {
            let mut cursor = cursor_rx.clone();
            tokio::spawn(async move {
                cursor
                    .wait_for(|value| value == "pointer-final")
                    .await
                    .expect("cursor channel open");
            })
        };
        let churn = tokio::spawn({
            let hub = hub.clone();
            async move {
                for round in 0..FRAMES {
                    let transient = hub.subscribe();
                    let _hold = hub.hold_for_capture();
                    assert!(hub.viewer_count() > VIEWERS);
                    drop(transient);
                    let _ = (hub.params(), hub.latest(), round);
                }
            }
        });

        let mut churn = Some(churn);
        for index in 1..=FRAMES {
            if index == FRAMES {
                churn.take().unwrap().await.unwrap();
            }
            events
                .send(session_event(
                    "Page.screencastFrame",
                    json!({ "index": index }),
                ))
                .unwrap();
            assert_eq!(frame_acked.recv().await, Some(index));
        }
        for index in 1..=CURSORS {
            let payload = if index == CURSORS {
                "pointer-final".to_string()
            } else {
                ["pointer", "text", "grab"][index % 3].to_string()
            };
            events
                .send(session_event(
                    "Runtime.bindingCalled",
                    json!({ "index": index, "payload": payload }),
                ))
                .unwrap();
            assert_eq!(cursor_acked.recv().await, Some(index));
        }
        for actor in actors {
            actor.await.unwrap();
        }
        caster.await.unwrap();
        mirror.await.unwrap();
        for viewer in viewers {
            viewer.await.unwrap();
        }
        cursor_viewer.await.unwrap();
    };
    tokio::time::timeout(FAILURE_BOUND, load)
        .await
        .expect("browser load stalled");

    assert_eq!(hub.viewer_count(), 0);
    assert_eq!(hub.latest().map(|frame| frame.seq), Some(FRAMES));
    assert_eq!(*cursor_rx.borrow(), "pointer-final");
    let mut transitions = Vec::new();
    while let Ok(command) = commands.try_recv() {
        transitions.push(command);
    }
    assert!(matches!(transitions.first(), Some(DriverCommand::Start(_))));
    assert_eq!(transitions.last(), Some(&DriverCommand::Stop));
    responder.abort();
    let methods = methods.lock();
    assert_eq!(methods["Page.navigate"], NAVIGATORS * NAVIGATIONS);
    assert_eq!(methods["Input.dispatchMouseEvent"], TYPISTS * CLICKS * 2);
}
