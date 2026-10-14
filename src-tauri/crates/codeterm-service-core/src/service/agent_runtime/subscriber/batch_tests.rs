use super::batch::{drain_ready, handle, Drained, Poll};
use super::bookkeeping::{self, Step};
use super::*;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::BufReader;

fn delta(seq: usize) -> RuntimeFrame {
    RuntimeFrame::Notification {
        method: "fixture/compacted".into(),
        params: json!({ "seq": seq }),
    }
}

fn request(id: &str) -> RuntimeFrame {
    RuntimeFrame::Request {
        request_id: RpcId::String(id.into()),
        method: "fixture/known".into(),
        params: json!({}),
    }
}

async fn answer_resync(peer: &mut BufReader<tokio::io::DuplexStream>) -> Vec<Value> {
    let mut other = Vec::new();
    for method in ["thread/resume", "thread/read"] {
        loop {
            let line = fixtures::read(peer).await;
            if line["method"] == method {
                fixtures::write(peer, json!({"id": line["id"], "result": {}})).await;
                break;
            }
            other.push(line);
        }
    }
    other
}

async fn run(
    scene: &mut fixtures::Scene,
    frames: Vec<RuntimeFrame>,
    lagged: bool,
    resync: bool,
) -> (Step, Vec<Value>) {
    let process = scene.session.process.lock().clone().unwrap();
    let epoch = scene.session.context.lock().epoch;
    let svc = scene.fixture.service.clone();
    let session = scene.session.clone();
    let work = handle(&svc, &session, &process, epoch, frames, lagged);
    if resync {
        tokio::join!(work, answer_resync(&mut scene.peer))
    } else {
        (work.await, Vec::new())
    }
}

fn polls(count: usize, tail: Poll<usize>) -> impl FnMut() -> Poll<usize> {
    let mut next = 1;
    let mut tail = Some(tail);
    move || {
        if next < count {
            next += 1;
            Poll::Frame(next - 1)
        } else {
            tail.take().unwrap_or(Poll::Empty)
        }
    }
}

#[test]
fn drain_takes_every_ready_frame_in_order_and_stops_at_empty() {
    let Drained::Ready { frames, lagged } =
        drain_ready(Poll::Frame(0), polls(5, Poll::Empty), 1024)
    else {
        panic!("expected ready")
    };
    assert_eq!(frames, vec![0, 1, 2, 3, 4]);
    assert!(!lagged);
}

#[test]
fn drain_respects_the_batch_cap_and_reports_lag_and_close() {
    let Drained::Ready { frames, .. } = drain_ready(Poll::Frame(0), polls(100, Poll::Empty), 8)
    else {
        panic!("expected ready")
    };
    assert_eq!(frames.len(), 8);
    let Drained::Ready { frames, lagged } = drain_ready(Poll::Frame(0), polls(3, Poll::Lagged), 8)
    else {
        panic!("expected ready")
    };
    assert_eq!((frames.len(), lagged), (3, true));
    let Drained::Closed { frames } = drain_ready(Poll::Frame(0), polls(3, Poll::Closed), 8) else {
        panic!("expected closed")
    };
    assert_eq!(frames.len(), 3);
}

#[tokio::test]
async fn one_drained_batch_is_one_reduce_with_frames_in_order() {
    let mut scene = fixtures::scene();
    let frames: Vec<_> = (0..40).map(delta).collect();
    let (step, _) = run(&mut scene, frames.clone(), false, false).await;
    assert!(matches!(step, Step::Continue));
    let calls = scene.probe.reduces.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], frames);
}

#[tokio::test]
async fn unmapped_server_requests_reply_in_arrival_order() {
    let mut scene = fixtures::scene();
    let frames = vec![request("a"), delta(0), request("b"), request("c")];
    let (step, _) = run(&mut scene, frames, false, false).await;
    assert!(matches!(step, Step::Continue));
    assert_eq!(scene.probe.reduces.lock().len(), 1);
    for expected in ["a", "b", "c"] {
        let reply = fixtures::read(&mut scene.peer).await;
        assert_eq!(reply["id"], expected);
        assert_eq!(reply["error"]["code"], -32601);
    }
    assert!(scene.session.prompts.lock().is_empty());
}

#[tokio::test]
async fn reducer_failure_warns_resyncs_and_keeps_hook_state() {
    let mut scene = fixtures::scene();
    *scene.session.hook_state.lock() = json!({"keep": 1});
    scene.probe.fail_next.store(1, Ordering::SeqCst);
    let (step, replies) = run(&mut scene, vec![request("r"), delta(0)], false, true).await;
    assert!(matches!(step, Step::Continue));
    let reply = &replies[0];
    assert_eq!(reply["id"], "r");
    assert_eq!(reply["error"]["code"], -32602);
    assert_eq!(*scene.session.hook_state.lock(), json!({"keep": 1}));
    assert_eq!(
        scene.session.bookkeeping_failures.load(Ordering::Relaxed),
        1
    );
    assert!(scene.session.process.lock().is_some());
}

#[tokio::test]
async fn a_successful_batch_resets_the_failure_run() {
    let mut scene = fixtures::scene();
    scene.probe.fail_next.store(1, Ordering::SeqCst);
    run(&mut scene, vec![delta(0)], false, true).await;
    assert_eq!(
        scene.session.bookkeeping_failures.load(Ordering::Relaxed),
        1
    );
    let (step, _) = run(&mut scene, vec![delta(1)], false, false).await;
    assert!(matches!(step, Step::Continue));
    assert_eq!(
        scene.session.bookkeeping_failures.load(Ordering::Relaxed),
        0
    );
}

#[tokio::test]
async fn eight_consecutive_failures_close_without_an_eighth_resync() {
    let mut scene = fixtures::scene();
    scene.probe.fail_next.store(8, Ordering::SeqCst);
    for seq in 0..bookkeeping::CONSECUTIVE_CLOSE - 1 {
        let (step, _) = run(&mut scene, vec![delta(seq as usize)], false, true).await;
        assert!(matches!(step, Step::Continue));
    }
    let (step, _) = tokio::time::timeout(
        Duration::from_secs(2),
        run(&mut scene, vec![delta(7)], false, false),
    )
    .await
    .expect("the closing failure must not wait on a resync");
    assert!(matches!(step, Step::Close(RuntimeFailure::HookFailed(_))));
    assert_eq!(scene.probe.reduce_failures.load(Ordering::SeqCst), 8);
}

#[tokio::test]
async fn lag_is_bookkeeping_not_a_close() {
    let mut scene = fixtures::scene();
    let (step, _) = run(&mut scene, Vec::new(), true, true).await;
    assert!(matches!(step, Step::Continue));
    assert!(scene.probe.reduces.lock().is_empty());
    assert_eq!(
        scene.session.bookkeeping_failures.load(Ordering::Relaxed),
        1
    );
    let process = scene.session.process.lock().clone().unwrap();
    assert!(process.client.ended().borrow().is_none());
}

#[test]
fn warning_ids_are_unique_inside_one_epoch() {
    assert_ne!(bookkeeping::warning_id(3), bookkeeping::warning_id(3));
}
