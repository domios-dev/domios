//! Timer-driven live 3-state recompute. `process_pty_output` recomputes a tab's
//! agent state on every output chunk, but the working→idle edge has no output to
//! ride on: when a turn ends the agent goes silent, so a periodic sweep is what
//! lets the quiescence clocks cross their thresholds and flip the tab to Idle.
//! Pairs with the output-driven path through the shared
//! `ServiceContext::recompute_agent_state`.

use std::time::Duration;

use crate::service::ServiceState;

/// How often the sweep re-runs the classifier per tab. Fast enough that the
/// "your turn" indicator appears promptly after a turn settles (DONE_MS = 800),
/// cheap enough to run over every tracked pty (one grid read + a pure classify).
const POLL_INTERVAL: Duration = Duration::from_millis(350);

/// Spawn the live-state sweep on its own thread. Each tick re-classifies every
/// tracked tab; `recompute_agent_state` gates to agent tabs and emits only on
/// a real change.
pub fn start(svc: ServiceState) {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL_INTERVAL);
        let pty_ids = svc.agent_tracker.lock().all_ids();
        for pty_id in pty_ids {
            svc.recompute_agent_state(&pty_id);
            svc.run_watcher.complete_via_semantic_idle(&svc, &pty_id);
        }
    });
}
