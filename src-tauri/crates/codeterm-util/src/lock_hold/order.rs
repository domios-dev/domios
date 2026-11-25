//! Debug-build lock-order witness for named hot locks.
//! Each thread records the timed guards it holds; a blocking acquisition that
//! re-enters a held lock or inverts the canonical
//! `mux -> output_buf -> screen_state -> agent_tracker -> pty` order is reported
//! as `[LOCK-ORDER]` and counted, so load tests can prove ordering under stress.

use std::cell::RefCell;
use std::panic::Location;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use parking_lot::Mutex;

const RECORDED_LIMIT: usize = 4096;

/// Canonical rank of a named lock; unnamed or subsystem-local locks are unranked.
pub fn rank(name: &str) -> Option<u8> {
    match name {
        "mux" => Some(10),
        "output_buf" => Some(20),
        "screen_state_map" => Some(30),
        "screen_state_parser" => Some(31),
        "agent_tracker" => Some(40),
        "pty" => Some(50),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Exclusive,
    Shared,
}

struct Held {
    token: u64,
    name: &'static str,
    addr: usize,
    access: Access,
    site: &'static Location<'static>,
}

thread_local! {
    static HELD: RefCell<Vec<Held>> = const { RefCell::new(Vec::new()) };
}

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static VIOLATIONS: AtomicUsize = AtomicUsize::new(0);
static RECORDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Checks a blocking acquisition against the locks this thread already holds.
pub(super) fn check_blocking(
    name: &'static str,
    addr: usize,
    access: Access,
    site: &'static Location<'static>,
) {
    let found = HELD.with(|held| {
        held.borrow()
            .iter()
            .find_map(|held| conflict(held, name, addr, access, site))
    });
    if let Some(report) = found {
        record(report);
    }
}

fn conflict(
    held: &Held,
    name: &'static str,
    addr: usize,
    access: Access,
    site: &'static Location<'static>,
) -> Option<String> {
    let at = |site: &Location<'_>| format!("{}:{}", site.file(), site.line());
    if held.addr == addr {
        let kind = if held.access == Access::Shared && access == Access::Shared {
            "recursive shared"
        } else {
            "reentrant"
        };
        return Some(format!(
            "[LOCK-ORDER] {kind} acquisition of {name} at {} while held since {}",
            at(site),
            at(held.site)
        ));
    }
    let (held_rank, wanted_rank) = (rank(held.name)?, rank(name)?);
    (held_rank >= wanted_rank).then(|| {
        format!(
            "[LOCK-ORDER] {name} acquired at {} while holding {} since {}",
            at(site),
            held.name,
            at(held.site)
        )
    })
}

pub(super) fn enter(
    name: &'static str,
    addr: usize,
    access: Access,
    site: &'static Location<'static>,
) -> u64 {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    HELD.with(|held| {
        held.borrow_mut().push(Held {
            token,
            name,
            addr,
            access,
            site,
        })
    });
    token
}

pub(super) fn exit(token: u64) {
    let _ = HELD.try_with(|held| {
        let mut held = held.borrow_mut();
        if let Some(index) = held.iter().rposition(|entry| entry.token == token) {
            held.remove(index);
        }
    });
}

/// Reports a blocking runtime wait entered while this thread holds a timed guard.
#[track_caller]
pub fn check_blocking_wait(what: &str) {
    let site = Location::caller();
    let held = held_names();
    if !held.is_empty() {
        record(format!(
            "[LOCK-ORDER] {what} at {}:{} while holding {}",
            site.file(),
            site.line(),
            held.join(", ")
        ));
    }
}

/// Names of the timed guards held by the current thread, oldest first.
pub fn held_names() -> Vec<&'static str> {
    HELD.with(|held| held.borrow().iter().map(|held| held.name).collect())
}

fn record(report: String) {
    let thread = std::thread::current();
    let report = format!("{report} thread={}", thread.name().unwrap_or("unnamed"));
    log::debug!("{report}");
    VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    let mut recorded = RECORDED.lock();
    if recorded.len() < RECORDED_LIMIT {
        recorded.push(report);
    }
}

/// Total violations observed by this process.
pub fn violation_count() -> usize {
    VIOLATIONS.load(Ordering::Relaxed)
}

/// Recorded reports mentioning `needle`; tests scope them by their own thread names.
pub fn violations_mentioning(needle: &str) -> Vec<String> {
    RECORDED
        .lock()
        .iter()
        .filter(|report| report.contains(needle))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests;
