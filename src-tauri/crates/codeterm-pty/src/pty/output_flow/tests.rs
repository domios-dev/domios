use super::*;
use std::future::Future;
use std::task::{Context, Poll, Waker};

fn attached() -> (Arc<OutputFlow>, Uuid, Uuid, Instant) {
    let flow = Arc::new(OutputFlow::default());
    let view = Uuid::new_v4();
    let attachment = Uuid::new_v4();
    let now = Instant::now();
    flow.bind(7);
    assert!(flow.attach(7, view, attachment, 0, now));
    (flow, view, attachment, now)
}

#[test]
fn high_and_low_watermarks_have_hysteresis() {
    let (flow, view, attachment, now) = attached();
    flow.produced(7, HIGH_WATERMARK - 1);
    assert!(flow.pause_deadline(now).is_none());
    flow.produced(7, HIGH_WATERMARK);
    assert!(flow.pause_deadline(now).is_some());
    assert!(flow.acknowledge(view, attachment, HIGH_WATERMARK - LOW_WATERMARK - 1, now));
    assert!(flow.pause_deadline(now).is_some());
    assert!(flow.acknowledge(view, attachment, HIGH_WATERMARK - LOW_WATERMARK, now));
    assert!(flow.pause_deadline(now).is_none());
}

#[test]
fn observers_cannot_acknowledge_or_replace_the_primary() {
    let (flow, view, attachment, now) = attached();
    flow.produced(7, HIGH_WATERMARK);
    let remote = Uuid::new_v4();
    assert!(!flow.attach(7, remote, Uuid::new_v4(), 0, now));
    assert!(!flow.acknowledge(remote, attachment, HIGH_WATERMARK, now));
    assert!(!flow.acknowledge(view, Uuid::new_v4(), HIGH_WATERMARK, now));
    flow.detach(remote, None);
    assert!(flow.pause_deadline(now).is_some());
    flow.detach(view, Some(attachment));
    flow.produced(7, u64::MAX);
    assert!(flow.pause_deadline(now).is_none());
}

#[test]
fn stale_inflated_and_duplicate_acknowledgements_do_not_extend_authority() {
    let (flow, view, attachment, now) = attached();
    flow.produced(7, HIGH_WATERMARK);
    assert!(!flow.acknowledge(view, attachment, HIGH_WATERMARK + 1, now));
    assert!(!flow.acknowledge(view, attachment, 0, now));
    assert!(flow.pause_deadline(now).is_some());
    assert!(flow.pause_deadline(now + ACK_FAILURE_BOUND).is_none());
    assert!(!flow.acknowledge(view, attachment, HIGH_WATERMARK, now + ACK_FAILURE_BOUND));
}

#[test]
fn replay_and_replacement_start_new_generation_authority() {
    let (flow, view, attachment, now) = attached();
    flow.produced(7, 2 * HIGH_WATERMARK);
    flow.detach(view, None);
    assert!(!flow.attach(6, view, attachment, 0, now));
    assert!(!flow.attach(7, view, attachment, 2 * HIGH_WATERMARK + 1, now));
    assert!(flow.attach(7, view, attachment, 2 * HIGH_WATERMARK, now));
    assert!(flow.pause_deadline(now).is_none());
    flow.bind(8);
    assert!(flow.pause_deadline(now).is_none());
    assert!(!flow.acknowledge(view, attachment, 1, now));
}

#[test]
fn detach_and_readiness_permits_wake_a_registered_reader() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let (flow, view, attachment, now) = attached();
    flow.produced(7, HIGH_WATERMARK);
    let mut reader = Box::pin(flow.wait_to_read());
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(reader.as_mut().poll(&mut context), Poll::Pending);
    let permit = flow.suspend();
    assert_eq!(reader.as_mut().poll(&mut context), Poll::Ready(()));
    let nested = flow.suspend();
    drop(permit);
    assert!(flow.pause_deadline(now).is_none());
    drop(nested);
    let mut reader = Box::pin(flow.wait_to_read());
    assert_eq!(reader.as_mut().poll(&mut context), Poll::Pending);
    flow.detach(view, Some(attachment));
    assert_eq!(reader.as_mut().poll(&mut context), Poll::Ready(()));
}

#[test]
fn readiness_permit_survives_rebinding_without_decrementing_another_waiter() {
    let (flow, view, attachment, now) = attached();
    let first = flow.suspend();
    flow.bind(8);
    let second = flow.suspend();
    assert!(flow.attach(8, view, attachment, 0, now));
    flow.produced(8, HIGH_WATERMARK);
    drop(first);
    assert!(flow.pause_deadline(now).is_none());
    drop(second);
    assert!(flow.pause_deadline(now).is_some());
}

#[test]
fn idle_time_never_revokes_a_live_renderer_before_backlog_requires_a_pause() {
    let (flow, _, _, now) = attached();
    let later = now + ACK_FAILURE_BOUND * 100;
    assert!(flow.pause_deadline(later).is_none());
    flow.produced(7, HIGH_WATERMARK);
    assert_eq!(flow.pause_deadline(later), Some(later + ACK_FAILURE_BOUND));
}

#[test]
fn launch_readiness_cannot_own_a_renderer_pause() {
    use codeterm_types::pty::BootStage;
    assert!(renderer_may_pause(false, false, None));
    assert!(renderer_may_pause(
        false,
        false,
        Some(BootStage::ShellReady)
    ));
    assert!(renderer_may_pause(false, true, Some(BootStage::AgentReady)));
    assert!(!renderer_may_pause(true, false, None));
    for stage in BootStage::HAPPY_PATH {
        assert_eq!(
            renderer_may_pause(false, true, Some(stage)),
            stage == BootStage::AgentReady
        );
    }
    assert!(!renderer_may_pause(
        false,
        false,
        Some(BootStage::ReaderReady)
    ));
}

#[test]
fn an_old_reader_cannot_inflate_a_replacement_generations_pending_bytes() {
    let (flow, view, attachment, now) = attached();
    flow.bind(8);
    flow.produced(7, u64::MAX);
    assert!(flow.attach(8, view, attachment, 0, now));
    assert!(flow.pause_deadline(now).is_none());
    flow.produced(8, HIGH_WATERMARK);
    assert!(flow.pause_deadline(now).is_some());
    assert!(flow.acknowledge(view, attachment, HIGH_WATERMARK, now));
    assert!(flow.pause_deadline(now).is_none());
}
