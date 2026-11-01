use super::*;
use std::cell::Cell;
use std::future::poll_fn;
use std::task::Poll;

#[test]
fn desktop_readiness_does_not_wait_for_background_restore() {
    let mut events = StartupEvents::default();
    assert!(events.push(b"event: startup-state\ndata: {\"state\":\"starting\",\"desktop_ready\":true,\"routes_ready\":false}\n\n").unwrap());
}

struct FixtureBody(tokio::sync::mpsc::Receiver<Vec<u8>>);

impl StartupBody for FixtureBody {
    async fn chunk(&mut self) -> Result<Option<Vec<u8>>, ReadinessFailure> {
        Ok(self.0.recv().await)
    }
}

#[tokio::test]
async fn empty_or_starting_then_eof_refuses_after_one_request() {
    for starting in [false, true] {
        let (sender, body) = tokio::sync::mpsc::channel(2);
        if starting {
            sender
                .send(b"event: startup-state\ndata: {\"state\":\"starting\"}\n\n".to_vec())
                .await
                .unwrap();
        }
        drop(sender);
        let requests = Cell::new(0);
        let result = observe(async {
            requests.set(requests.get() + 1);
            Ok(FixtureBody(body))
        })
        .await;
        assert_eq!(result, Err(ReadinessFailure::EndOfStream));
        assert_eq!(requests.get(), 1);
    }
}

#[tokio::test]
async fn retained_stream_accepts_later_ready_without_another_request() {
    let (sender, body) = tokio::sync::mpsc::channel(2);
    sender
        .send(b"event: startup-state\ndata: {\"state\":\"starting\"}\n\n".to_vec())
        .await
        .unwrap();
    let requests = Cell::new(0);
    let mut waiting = Box::pin(observe(async {
        requests.set(requests.get() + 1);
        Ok(FixtureBody(body))
    }));
    assert!(poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
        .await
        .is_pending());
    assert_eq!(requests.get(), 1);
    sender
        .send(b"event: startup-state\ndata: {\"state\":\"ready\"}\n\n".to_vec())
        .await
        .unwrap();
    assert_eq!(waiting.await, Ok(()));
    assert_eq!(requests.get(), 1);
}

#[test]
fn daemon_frontend_waits_for_a_complete_ready_snapshot() {
    let mut events = StartupEvents::default();
    for chunk in [
        b"event: startup-state\r\ndata: {\"state\":\"starting\"}\r\n\r\n".as_slice(),
        b"event: other\ndata: {\"state\":\"ready\"}\n\n",
        b"event: startup-state\ndata: {\"sta",
        b"te\":\"ready\"}\n",
    ] {
        assert!(!events.push(chunk).unwrap());
    }
    assert!(events.push(b"\n").unwrap());
}

#[test]
fn daemon_frontend_refuses_failed_or_invalid_startup() {
    for snapshot in ["{\"state\":\"failed\"}", "{}", "invalid"] {
        let mut events = StartupEvents::default();
        assert!(events
            .push(format!("event: startup-state\ndata: {snapshot}\n\n").as_bytes())
            .is_err());
    }
}

#[test]
fn daemon_frontend_bounds_startup_event_memory() {
    assert!(StartupEvents::default()
        .push(&vec![b'x'; 64 * 1024 + 1])
        .is_err());
}
