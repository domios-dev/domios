use super::ProcessSlot;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone)]
struct FakeProcess(Arc<AtomicBool>);

impl FakeProcess {
    fn spawn() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }

    fn stop(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    fn alive(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[test]
fn close_between_spawn_and_install_stops_the_late_process() {
    let slot = ProcessSlot::new(None);
    let spawned = FakeProcess::spawn();
    assert!(slot.close().is_none());
    assert!(!slot.install(spawned.clone(), FakeProcess::stop));
    assert!(!spawned.alive());
    assert!(slot.lock().is_none());
}

#[test]
fn normal_startup_keeps_the_process_until_close() {
    let slot = ProcessSlot::new(None);
    let spawned = FakeProcess::spawn();
    assert!(slot.install(spawned.clone(), FakeProcess::stop));
    assert!(spawned.alive());
    slot.close().unwrap().stop();
    assert!(!spawned.alive());
    assert!(slot.lock().is_none());
}

#[test]
fn closing_an_old_generation_does_not_cancel_its_replacement() {
    let old = ProcessSlot::new(None);
    let current = ProcessSlot::new(None);
    old.close();
    let late = FakeProcess::spawn();
    let replacement = FakeProcess::spawn();
    assert!(!old.install(late.clone(), FakeProcess::stop));
    assert!(current.install(replacement.clone(), FakeProcess::stop));
    assert!(!late.alive());
    assert!(replacement.alive());
    current.close().unwrap().stop();
}

#[test]
fn rejected_process_stops_outside_the_slot_lock() {
    let slot = ProcessSlot::new(None);
    slot.close();
    let spawned = FakeProcess::spawn();
    assert!(!slot.install(spawned.clone(), |process| {
        assert!(slot.lock().is_none());
        process.stop();
    }));
    assert!(!spawned.alive());
}
