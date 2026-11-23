//! Live transcript commits, routing dispatch, status reads and trigger changes
//! under concurrency, plus the settle-under-dispatch-lock re-entry regression.

use std::sync::{mpsc, Barrier};

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::agent_runtime::engine::test_support::live_runtime;
use crate::service::test_fixture::ServiceFixture;

const FAILURE_BOUND: Duration = Duration::from_secs(60);
const SPEAKERS: usize = 3;

fn fixture() -> (ServiceFixture, Arc<ServiceContext>, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("Transcript load".into(), None);
    let owner = mux.create_tab(&workspace.id).unwrap();
    let router = mux.create_tab(&workspace.id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = Arc::new(fixture.service.clone());
    (fixture, svc, owner, router)
}

fn on_thread<T: Send + 'static>(
    name: String,
    work: impl FnOnce() -> T + Send + 'static,
) -> std::thread::JoinHandle<T> {
    std::thread::Builder::new()
        .name(name)
        .spawn(move || {
            let _runtime = crate::async_runtime::handle().enter();
            work()
        })
        .unwrap()
}

fn dispatch_running(svc: &ServiceContext, owner: &str) -> bool {
    svc.ext_or_init(Dispatch::default)
        .owners
        .lock()
        .get(owner)
        .is_some_and(|entry| entry.running)
}

/// Resolves once the owner's dispatch worker has drained every enqueued range.
fn wait_for_dispatch_idle(svc: &ServiceContext, owner: &str) {
    let drained = async {
        loop {
            let finished = svc
                .ext_or_init(Dispatch::default)
                .owners
                .lock()
                .get(owner)
                .map(|entry| entry.dispatch_finished.clone());
            let Some(finished) = finished else { return };
            let notified = finished.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !dispatch_running(svc, owner) {
                return;
            }
            notified.await;
        }
    };
    crate::async_runtime::block_on(async { tokio::time::timeout(FAILURE_BOUND, drained).await })
        .expect("transcript dispatch never drained");
}

#[test]
fn concurrent_commits_routing_status_and_trigger_changes_never_stall_or_lose_rows() {
    run_transcript_load(4);
}

#[test]
#[ignore = "soak: run explicitly for extended transcript contention"]
fn transcript_commit_and_routing_soak() {
    run_transcript_load(200);
}

fn run_transcript_load(utterances: usize) {
    let (_fixture, svc, owner, _) = fixture();
    svc.set_live_transcript_trigger(&owner, TranscriptTrigger::DirectBatch)
        .unwrap();
    let start = Arc::new(Barrier::new(SPEAKERS + 2));
    let (done_tx, done) = mpsc::channel();
    let speakers: Vec<_> = (0..SPEAKERS)
        .map(|speaker| {
            let (svc, owner, start) = (svc.clone(), owner.clone(), start.clone());
            on_thread(format!("transcript-load-speaker-{speaker}"), move || {
                start.wait();
                (0..utterances)
                    .map(|utterance| {
                        let store = svc.open_live_transcript(&owner).unwrap();
                        let receipt = store
                            .append(
                                &owner,
                                TranscriptInput {
                                    at_ms: wall_now_ms(),
                                    content: TranscriptContent::Speech {
                                        text: format!("speaker {speaker} says {utterance}"),
                                    },
                                },
                            )
                            .unwrap();
                        let seq = receipt.row.seq;
                        on_commit(&svc, &store, &owner, receipt);
                        seq
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let status = {
        let (svc, owner, start) = (svc.clone(), owner.clone(), start.clone());
        on_thread("transcript-load-status".into(), move || {
            start.wait();
            for round in 0..utterances {
                let status = svc.live_transcript_status(&owner).unwrap();
                assert!(status.committed_seq <= (SPEAKERS * utterances) as u64);
                let trigger = if round % 2 == 0 {
                    TranscriptTrigger::WakeWord
                } else {
                    TranscriptTrigger::DirectBatch
                };
                let _ = svc.set_live_transcript_trigger(&owner, trigger);
            }
        })
    };
    let coordinator = {
        let start = start.clone();
        on_thread("transcript-load-coordinator".into(), move || {
            start.wait();
            let mut seqs: Vec<u64> = speakers
                .into_iter()
                .flat_map(|speaker| speaker.join().unwrap())
                .collect();
            status.join().unwrap();
            seqs.sort_unstable();
            done_tx.send(seqs).unwrap();
        })
    };
    let seqs = done
        .recv_timeout(FAILURE_BOUND)
        .expect("transcript load stalled on a lock");
    coordinator.join().unwrap();
    wait_for_dispatch_idle(&svc, &owner);

    let total = (SPEAKERS * utterances) as u64;
    assert_eq!(seqs, (1..=total).collect::<Vec<_>>());
    let store = svc.open_live_transcript(&owner).unwrap();
    assert_eq!(store.committed_seq(), total);
    assert!(!dispatch_running(&svc, &owner));
    assert!(svc.live_transcript_status(&owner).is_ok());
}

#[test]
fn idle_wake_requested_while_the_dispatch_lock_is_held_does_not_reenter_it() {
    let (_fixture, svc, owner, router) = fixture();
    let mut tab = svc.mux.read().get_tab(&router).unwrap().clone();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("router-session".into());
    tab.session_binding_nonce = Some("router-nonce".into());
    let _live = live_runtime(&tab);
    let session =
        crate::service::agent_runtime::engine::for_tab(&router).expect("router runtime session");
    let store = svc.open_live_transcript(&owner).unwrap();
    let (returned_tx, returned) = mpsc::channel();
    let caller = {
        let (svc, owner) = (svc.clone(), owner.clone());
        on_thread("transcript-wake-reentry".into(), move || {
            let entries = svc.ext_or_init(Dispatch::default).owners.lock();
            wake_when_idle(&svc, store, &owner, router, session);
            drop(entries);
            returned_tx.send(()).unwrap();
        })
    };
    returned
        .recv_timeout(FAILURE_BOUND)
        .expect("idle wake re-entered the held transcript dispatch lock");
    caller.join().unwrap();
}
