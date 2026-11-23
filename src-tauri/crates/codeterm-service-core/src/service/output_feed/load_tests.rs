//! PTY output under concurrent readers, mux writers, resizes and tracker reads:
//! every byte lands once, nothing stalls, and the lock-order witness stays silent.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

use crate::lock_hold::order;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

const FAILURE_BOUND: Duration = Duration::from_secs(60);
const PTYS: usize = 4;
const READERS: usize = 3;

fn fixture() -> (ServiceFixture, Vec<(String, String)>) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("PTY load".into(), None).id;
    let tabs: Vec<_> = (0..PTYS)
        .map(|index| {
            let tab = mux.create_tab(&workspace).unwrap();
            let pty = format!("load-pty-{index}");
            mux.link_pty(&tab, pty.clone());
            (tab, pty)
        })
        .collect();
    (ServiceFixture::new(mux), tabs)
}

fn named(name: String, work: impl FnOnce() + Send + 'static) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new().name(name).spawn(work).unwrap()
}

#[test]
fn pty_output_survives_concurrent_readers_mux_writers_and_resizes() {
    run_pty_load(60);
}

#[test]
#[ignore = "soak: run explicitly for extended PTY contention"]
fn pty_output_soak_under_concurrent_readers_mux_writers_and_resizes() {
    run_pty_load(5_000);
}

fn run_pty_load(chunks: usize) {
    let (fixture, tabs) = fixture();
    let svc = Arc::new(fixture.service.clone());
    let stop = Arc::new(AtomicBool::new(false));
    let start = Arc::new(Barrier::new(PTYS + READERS + 2));
    let (done_tx, done) = mpsc::channel();
    let mut workers = Vec::new();

    for (index, (_, pty)) in tabs.iter().cloned().enumerate() {
        let (svc, start) = (svc.clone(), start.clone());
        workers.push(named(format!("pty-load-writer-{index}"), move || {
            start.wait();
            for chunk in 0..chunks {
                let line = format!("{pty} chunk {chunk:04} \x1b[32mok\x1b[0m\r\n");
                svc.feed_pty_output(&pty, line.as_bytes());
            }
        }));
    }
    for reader in 0..READERS {
        let (svc, start, stop, tabs) = (svc.clone(), start.clone(), stop.clone(), tabs.clone());
        workers.push(named(format!("pty-load-reader-{reader}"), move || {
            start.wait();
            let mut offsets = vec![0u64; tabs.len()];
            while !stop.load(Ordering::Acquire) {
                for (slot, (tab, pty)) in tabs.iter().enumerate() {
                    let (_, next) = svc.output_buf.lock().read_since(pty, offsets[slot]);
                    assert!(next >= offsets[slot], "output offset moved backwards");
                    offsets[slot] = next;
                    let _ = svc.screen_state.get_screen_text(pty);
                    let _ = svc.agent_tracker.lock().get_status(pty);
                    assert_eq!(svc.mux.read().tab_id_for_pty(pty).as_ref(), Some(tab));
                }
            }
        }));
    }
    {
        let (svc, start, stop, tabs) = (svc.clone(), start.clone(), stop.clone(), tabs.clone());
        workers.push(named("pty-load-mux-writer".into(), move || {
            start.wait();
            let mut round = 0u16;
            while !stop.load(Ordering::Acquire) {
                round = round.wrapping_add(1);
                for (tab, pty) in &tabs {
                    if let Some(tab) = svc.mux.write().get_tab_mut(tab) {
                        tab.title = format!("load {round}");
                    }
                    svc.screen_state
                        .resize(pty, 80 + round % 40, 24 + round % 8);
                    let mux = svc.mux.read();
                    let output = svc.output_buf.lock();
                    let tracker = svc.agent_tracker.lock();
                    let _ = (mux.tab_id_for_pty(pty), output.total_written(pty));
                    let _ = tracker.get_status(pty);
                    drop(svc.pty.lock());
                }
            }
        }));
    }
    let coordinator = {
        let (start, stop) = (start.clone(), stop.clone());
        let writers: Vec<_> = workers.drain(..PTYS).collect();
        named("pty-load-coordinator".into(), move || {
            start.wait();
            for writer in writers {
                writer.join().unwrap();
            }
            stop.store(true, Ordering::Release);
            for worker in workers {
                worker.join().unwrap();
            }
            done_tx.send(()).unwrap();
        })
    };
    done.recv_timeout(FAILURE_BOUND)
        .expect("PTY load stalled: a participant is blocked on a lock");
    coordinator.join().unwrap();

    let output = svc.output_buf.lock();
    for (_, pty) in &tabs {
        let expected: usize = (0..chunks)
            .map(|chunk| format!("{pty} chunk {chunk:04} \x1b[32mok\x1b[0m\r\n").len())
            .sum();
        assert_eq!(output.total_written(pty), expected as u64);
    }
    drop(output);
    let violations = order::violations_mentioning("thread=pty-load-");
    assert!(violations.is_empty(), "{violations:#?}");
}
