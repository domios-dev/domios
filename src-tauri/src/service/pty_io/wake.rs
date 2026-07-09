//! Wake-on-delivery confirmation: an injected report/nudge/gate whose submit
//! Enter was swallowed strands in an idle pane's composer forever. This re-presses
//! Enter while the pane is parked with the body still visible, and stops on drain.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::status::PaneStatus;
use crate::service::inject_verify::screen_contains_echo;
use crate::service::ServiceContext;
use crate::{AgentTrackerState, PtyState, ScreenStateHandle};

const CONFIRM_WINDOW: Duration = Duration::from_secs(15);
const TICK: Duration = Duration::from_millis(1200);
const MAX_NUDGES: u32 = 4;
const TAIL_ROWS: usize = 8;

/// Per-pty watcher state. The needle is the LATEST delivered body: a burst of
/// deliveries to one pane replaces it (and resets the window + nudge budget) so
/// the newest message is never dropped — only the composer's last paste matters.
struct WatchState {
    needle: String,
    deadline: Instant,
    nudges_used: u32,
}

fn watchers() -> &'static Mutex<HashMap<String, WatchState>> {
    static MAP: OnceLock<Mutex<HashMap<String, WatchState>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register/refresh a pty's watcher. Returns `true` when a fresh confirm loop
/// must be spawned (no watcher was active); `false` coalesces into the running one.
fn register(
    map: &mut HashMap<String, WatchState>,
    pty_id: &str,
    body: String,
    now: Instant,
) -> bool {
    match map.get_mut(pty_id) {
        Some(st) => {
            st.needle = body;
            st.deadline = now + CONFIRM_WINDOW;
            st.nudges_used = 0;
            false
        }
        None => {
            map.insert(
                pty_id.to_string(),
                WatchState {
                    needle: body,
                    deadline: now + CONFIRM_WINDOW,
                    nudges_used: 0,
                },
            );
            true
        }
    }
}

/// Retire the watcher only if the drained needle is still the current one. A newer
/// delivery having replaced it means work remains — keep watching.
fn finalize_submitted(map: &mut HashMap<String, WatchState>, pty_id: &str, observed: &str) -> bool {
    match map.get(pty_id) {
        Some(st) if st.needle == observed => {
            map.remove(pty_id);
            true
        }
        _ => false,
    }
}

fn bump_nudge(map: &mut HashMap<String, WatchState>, pty_id: &str, observed: &str) {
    if let Some(st) = map.get_mut(pty_id) {
        if st.needle == observed {
            st.nudges_used += 1;
        }
    }
}

/// One tick's decision. Pure so the parked-gate / drain / cap are unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeStep {
    Submitted,
    Nudge,
    Wait,
}

/// The drain check is the idempotency guard: Enter fires ONLY while the body is
/// still parked, so a processed message is never re-submitted nor a generating
/// pane interrupted.
pub fn decide_wake(
    status: PaneStatus,
    body_present: bool,
    nudges_used: u32,
    max_nudges: u32,
) -> WakeStep {
    if !body_present {
        return WakeStep::Submitted;
    }
    if !matches!(status, PaneStatus::Idle | PaneStatus::Waiting) {
        return WakeStep::Wait;
    }
    if nudges_used >= max_nudges {
        return WakeStep::Wait;
    }
    WakeStep::Nudge
}

fn confirm_loop(
    screen_state: &ScreenStateHandle,
    agent_tracker: &AgentTrackerState,
    pty: &PtyState,
    pty_id: &str,
) {
    loop {
        std::thread::sleep(TICK);
        let (needle, nudges_used) = {
            let mut map = watchers().lock();
            let Some(st) = map.get(pty_id) else {
                return;
            };
            if Instant::now() >= st.deadline {
                map.remove(pty_id);
                return;
            }
            (st.needle.clone(), st.nudges_used)
        };

        let tail = match screen_state.lock().tail_text(pty_id, TAIL_ROWS) {
            Some(t) => t,
            None => {
                watchers().lock().remove(pty_id);
                return;
            }
        };
        let status = agent_tracker.lock().get_status(pty_id);
        let body_present = screen_contains_echo(&tail, &needle);

        match decide_wake(status, body_present, nudges_used, MAX_NUDGES) {
            WakeStep::Submitted => {
                let mut map = watchers().lock();
                if finalize_submitted(&mut map, pty_id, &needle) {
                    if nudges_used > 0 {
                        log::info!(
                            "[wake] pty={} submitted after {} nudge(s)",
                            &pty_id[..pty_id.len().min(8)],
                            nudges_used
                        );
                    }
                    return;
                }
            }
            WakeStep::Wait => {}
            WakeStep::Nudge => {
                let _ = pty.lock().write(pty_id, b"\r");
                log::info!(
                    "[wake] idle pty={} holds an un-submitted message — re-pressing Enter",
                    &pty_id[..pty_id.len().min(8)]
                );
                bump_nudge(&mut watchers().lock(), pty_id, &needle);
            }
        }
    }
}

impl ServiceContext {
    /// Bounded, idempotent backstop that the message injected into `pane_id`
    /// actually submitted. No-op for chatBackend/non-agent/unlinked panes and
    /// empty bodies; only ever presses Enter while the pane is parked with `body`
    /// still visible. Bursty deliveries coalesce onto the latest body.
    pub fn confirm_wake_async(&self, pane_id: &str, body: &str) {
        if body.trim().is_empty() || self.pane_chat_backing(pane_id).is_some() {
            return;
        }
        let (pty_id, is_agent) = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_pane_id(pane_id)
                .unwrap_or_else(|| pane_id.to_string());
            match mux.get_pane(&resolved) {
                Some(p) => (p.pty_id.clone(), p.agent_type.is_some()),
                None => (None, false),
            }
        };
        let (Some(pty_id), true) = (pty_id, is_agent) else {
            return;
        };
        let spawn = register(
            &mut watchers().lock(),
            &pty_id,
            body.to_string(),
            Instant::now(),
        );
        if !spawn {
            return;
        }
        let screen_state = self.screen_state.clone();
        let agent_tracker = self.agent_tracker.clone();
        let pty = self.pty.clone();
        std::thread::spawn(move || {
            confirm_loop(&screen_state, &agent_tracker, &pty, &pty_id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT_A: &str = "task: pl_x/task_a\nstatus: In Review\nsha: aaaa\nfirst body";
    const REPORT_B: &str = "task: pl_x/task_b\nstatus: In Review\nsha: bbbb\nsecond body";

    #[test]
    fn drained_body_is_submitted_regardless_of_status() {
        for status in [
            PaneStatus::Idle,
            PaneStatus::Waiting,
            PaneStatus::Working,
            PaneStatus::Dead,
        ] {
            assert_eq!(
                decide_wake(status, false, 0, MAX_NUDGES),
                WakeStep::Submitted
            );
        }
    }

    #[test]
    fn parked_with_pending_body_nudges() {
        assert_eq!(
            decide_wake(PaneStatus::Idle, true, 0, MAX_NUDGES),
            WakeStep::Nudge
        );
        assert_eq!(
            decide_wake(PaneStatus::Waiting, true, 2, MAX_NUDGES),
            WakeStep::Nudge
        );
    }

    #[test]
    fn working_or_dead_pane_is_never_nudged() {
        assert_eq!(
            decide_wake(PaneStatus::Working, true, 0, MAX_NUDGES),
            WakeStep::Wait
        );
        assert_eq!(
            decide_wake(PaneStatus::Dead, true, 0, MAX_NUDGES),
            WakeStep::Wait
        );
    }

    #[test]
    fn nudges_are_bounded() {
        assert_eq!(
            decide_wake(PaneStatus::Idle, true, MAX_NUDGES, MAX_NUDGES),
            WakeStep::Wait
        );
    }

    #[test]
    fn second_delivery_replaces_needle_without_new_spawn() {
        let mut map = HashMap::new();
        let now = Instant::now();
        assert!(
            register(&mut map, "X", REPORT_A.to_string(), now),
            "first arms a loop"
        );
        assert!(
            !register(&mut map, "X", REPORT_B.to_string(), now),
            "second coalesces — no new loop, must not be dropped"
        );
        assert_eq!(map.get("X").unwrap().needle, REPORT_B);
        assert_eq!(
            map.get("X").unwrap().nudges_used,
            0,
            "window/budget reset for the new body"
        );
    }

    #[test]
    fn bursty_second_body_is_nudged_after_first_drains() {
        // The exact bounce: A submits fast, B is the one stranded in the composer.
        let mut map = HashMap::new();
        let now = Instant::now();
        register(&mut map, "X", REPORT_A.to_string(), now);
        register(&mut map, "X", REPORT_B.to_string(), now);
        let needle = map.get("X").unwrap().needle.clone();

        // A drained; its needle is stale so we must NOT retire the watcher.
        assert!(!finalize_submitted(&mut map, "X", REPORT_A));
        assert!(
            map.contains_key("X"),
            "stale-needle drain must keep watching B"
        );

        // B still parked in composer (codex placeholder), pane Idle => nudge B.
        let tail = "> [Pasted text #2 +3 lines]\n  Press Enter to send";
        let present = screen_contains_echo(tail, &needle);
        assert!(present);
        assert_eq!(
            decide_wake(PaneStatus::Idle, present, 0, MAX_NUDGES),
            WakeStep::Nudge
        );
    }

    #[test]
    fn no_double_submit_after_both_drain() {
        let mut map = HashMap::new();
        let now = Instant::now();
        register(&mut map, "X", REPORT_A.to_string(), now);
        register(&mut map, "X", REPORT_B.to_string(), now);
        let needle = map.get("X").unwrap().needle.clone();

        // Composer cleared: the current needle is gone => submitted, watcher retires.
        let tail = "assistant reply here\n> ";
        let present = screen_contains_echo(tail, &needle);
        assert!(!present);
        assert_eq!(
            decide_wake(PaneStatus::Working, present, 1, MAX_NUDGES),
            WakeStep::Submitted
        );
        assert!(
            finalize_submitted(&mut map, "X", &needle),
            "current-needle drain retires"
        );
        assert!(
            !map.contains_key("X"),
            "no watcher left to press Enter again"
        );
    }
}
