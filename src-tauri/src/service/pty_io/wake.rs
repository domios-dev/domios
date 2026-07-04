//! Wake-on-delivery confirmation for injected system messages.
//!
//! A report / reconciliation nudge / decision-gate notification injected into an
//! idle agent pane must actually trigger provider generation. If the submit
//! Enter is swallowed while the TUI ingests the bracket-paste, the body sits
//! un-submitted in the composer and the pane stays idle indefinitely (the
//! 35-minute stranded-orchestrator incident). This bounded, idempotent confirmer
//! re-presses Enter while the pane is PARKED with the body still visible, and
//! stops the instant the composer drains — so it never double-submits and never
//! blasts Enter into a pane that is mid-generation. Capability-agnostic: it keys
//! on the pane's own idle status + composer-drain, never a provider id.

use std::collections::HashSet;
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

/// One confirmer per pty — a second delivery mid-confirm coalesces into the
/// first instead of racing a duplicate Enter into the same pane.
fn active_confirms() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// One tick's decision. Pure so the parked-gate / drain / cap are unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeStep {
    /// Body drained from the composer — it submitted; stop confirming.
    Submitted,
    /// Parked with body still visible — re-press Enter.
    Nudge,
    /// Working (mid-generation) or nudges exhausted — wait, never press Enter.
    Wait,
}

/// The drain check is the idempotency guard: Enter fires ONLY while the injected
/// body is still parked in the composer. Once it submits (needle gone), or the
/// pane is Working, no Enter is sent — so a delivered-and-processed message is
/// never re-submitted and an actively generating pane is never interrupted.
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
    needle: &str,
) {
    let deadline = Instant::now() + CONFIRM_WINDOW;
    let mut nudges_used = 0u32;
    loop {
        std::thread::sleep(TICK);
        if Instant::now() >= deadline {
            return;
        }
        let tail = match screen_state.lock().tail_text(pty_id, TAIL_ROWS) {
            Some(t) => t,
            None => return, // pty gone — nothing to confirm
        };
        let status = agent_tracker.lock().get_status(pty_id);
        let body_present = screen_contains_echo(&tail, needle);
        match decide_wake(status, body_present, nudges_used, MAX_NUDGES) {
            WakeStep::Submitted => {
                if nudges_used > 0 {
                    log::info!(
                        "[wake] pane pty={} submitted after {} nudge(s)",
                        &pty_id[..pty_id.len().min(8)],
                        nudges_used
                    );
                }
                return;
            }
            WakeStep::Wait => {}
            WakeStep::Nudge => {
                nudges_used += 1;
                log::info!(
                    "[wake] idle pane pty={} holds an un-submitted message — re-pressing Enter ({}/{})",
                    &pty_id[..pty_id.len().min(8)],
                    nudges_used,
                    MAX_NUDGES
                );
                let _ = pty.lock().write(pty_id, b"\r");
            }
        }
    }
}

impl ServiceContext {
    /// Kick off a bounded background confirmation that a system message injected
    /// into `pane_id` actually submitted (triggered generation). No-op for
    /// chatBackend panes (they submit host-side, no Enter), non-agent panes,
    /// unlinked panes, and empty bodies. Safe to call on every delivery: it only
    /// ever presses Enter while the pane is parked with `body` still visible.
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
        if !active_confirms().lock().insert(pty_id.clone()) {
            return; // a confirmer is already watching this pane
        }
        let screen_state = self.screen_state.clone();
        let agent_tracker = self.agent_tracker.clone();
        let pty = self.pty.clone();
        let needle = body.to_string();
        std::thread::spawn(move || {
            confirm_loop(&screen_state, &agent_tracker, &pty, &pty_id, &needle);
            active_confirms().lock().remove(&pty_id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drained_body_is_submitted_regardless_of_status() {
        // The instant the composer drains, we stop — no matter the status.
        for status in [
            PaneStatus::Idle,
            PaneStatus::Waiting,
            PaneStatus::Working,
            PaneStatus::Dead,
        ] {
            assert_eq!(decide_wake(status, false, 0, MAX_NUDGES), WakeStep::Submitted);
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
    fn working_pane_is_never_nudged() {
        // Mid-generation: body may still show but we must not press Enter.
        assert_eq!(
            decide_wake(PaneStatus::Working, true, 0, MAX_NUDGES),
            WakeStep::Wait
        );
    }

    #[test]
    fn dead_pane_is_never_nudged() {
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
        assert_eq!(
            decide_wake(PaneStatus::Idle, true, MAX_NUDGES + 1, MAX_NUDGES),
            WakeStep::Wait
        );
    }

    #[test]
    fn placeholder_body_counts_as_present_for_codex_style_paste() {
        // Codex renders a multi-line paste as a placeholder; the drain check must
        // treat the placeholder as the body still parked (so we keep confirming).
        let tail = "> [Pasted text #1 +6 lines]\n  Press Enter to send";
        let body = "task: pl_x/task_y\nstatus: In Review\nsha: deadbeef\nall done";
        assert!(screen_contains_echo(tail, body));
        assert_eq!(
            decide_wake(PaneStatus::Idle, screen_contains_echo(tail, body), 0, MAX_NUDGES),
            WakeStep::Nudge
        );
    }

    #[test]
    fn cleared_composer_reads_as_submitted() {
        // After Enter lands, codex clears the composer — no placeholder, no body.
        let tail = "assistant reply here\n> ";
        let body = "task: pl_x/task_y\nstatus: In Review\nsha: deadbeef\nall done";
        assert!(!screen_contains_echo(tail, body));
        assert_eq!(
            decide_wake(PaneStatus::Working, screen_contains_echo(tail, body), 1, MAX_NUDGES),
            WakeStep::Submitted
        );
    }
}
