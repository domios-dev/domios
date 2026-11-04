use super::*;
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Clone, Debug)]
struct Child {
    killed: Arc<AtomicBool>,
}

impl portable_pty::ChildKiller for Child {
    fn kill(&mut self) -> std::io::Result<()> {
        self.killed.store(true, Ordering::Release);
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(self.clone())
    }
}

impl portable_pty::Child for Child {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        Ok(self
            .killed
            .load(Ordering::Acquire)
            .then(|| portable_pty::ExitStatus::with_exit_code(0)))
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        assert!(self.killed.load(Ordering::Acquire));
        Ok(portable_pty::ExitStatus::with_exit_code(0))
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

struct HeldWriter {
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}

impl Write for HeldWriter {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(3)).unwrap();
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "cancelled",
        ))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn teardown_kills_and_reaps_without_waiting_for_a_guarded_writer_or_holding_the_manager() {
    let (entered, observed) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let (cancel, cancelled) = mpsc::sync_channel(1);
    let killed = Arc::new(AtomicBool::new(false));
    let session = Arc::new(PtySession {
        generation: 1,
        pair: StdMutex::new(None),
        size: StdMutex::new((80, 24)),
        reader: StdMutex::new(None),
        write_queue: WriteQueue::new_cancellable(portable_pty::CancellableWriter {
            writer: Box::new(HeldWriter {
                entered,
                release: held,
            }),
            cancel: Arc::new(move || {
                let _ = cancel.try_send(());
            }),
        }),
        guarded_input: GuardedInputGate::default(),
        child: StdMutex::new(Box::new(Child {
            killed: killed.clone(),
        })),
        backend_reader_tx: StdMutex::new(None),
        backend_output: Default::default(),
        backend_reader_active: AtomicBool::new(false),
        teardown: Default::default(),
        child_exit: Arc::default(),
    });
    session.guarded_input.rearm();
    let permit = session.guarded_input.issue().unwrap();
    let writer_session = session.clone();
    let writer = std::thread::spawn(move || {
        writer_session.write_and_flush_guarded(vec![1], permit, Duration::from_secs(3))
    });
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    let manager = Arc::new(PlMutex::new(PtyManager::new()));
    manager
        .lock()
        .sessions
        .insert("held".into(), session.clone());
    let teardown_manager = manager.clone();
    let (finished, completion) = mpsc::sync_channel(1);
    let teardown = std::thread::spawn(move || {
        let reaper = teardown_manager.lock().begin_kill("held").unwrap();
        finished
            .send(reaper.wait_for_termination_bounded(Duration::from_secs(1)))
            .unwrap();
    });
    cancelled.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(manager.try_lock().is_some());
    assert!(manager.lock().get_session("held").is_none());
    completion
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert!(killed.load(Ordering::Acquire));
    assert!(session.write_queue.send(vec![2]).is_err());
    assert!(session.guarded_input.issue().is_none());
    release.send(()).unwrap();
    assert!(writer.join().unwrap().is_err());
    teardown.join().unwrap();
}
