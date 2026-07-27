//! Daemon-side driver for the refresh state machine. Owns the polling thread,
//! builds `Observation`s from live pane/plan reads, and executes each phase's
//! entry side-effect through `Messenger::deliver` (wake-confirmer for free).
//! All transition logic lives in `machine`; this is the I/O shell.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::status::PaneStatus;
use crate::service::inject_verify::screen_contains_echo;
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceContext;

use super::machine::{advance, Observation, RefreshPhase, RefreshTuning, Step};
use super::signature::compaction_signature;
use super::template::{self, HANDOFF_REQUEST, REORIENT};

const TICK: Duration = Duration::from_millis(1500);
const TAIL_ROWS: usize = 40;
/// Absolute backstop so a wedged read loop can never spin forever.
const HARD_CAP: Duration = Duration::from_secs(900);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshTrigger {
    Manual,
    AfterReflection,
}

impl RefreshTrigger {
    fn tag(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AfterReflection => "after_reflection",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RefreshRequest {
    pub pane_id: String,
    pub plan_id: String,
    pub trigger: RefreshTrigger,
    pub tuning: RefreshTuning,
}

impl RefreshRequest {
    /// Build a request with tuning sourced from `[orchestration.refresh]`.
    pub fn from_config(pane_id: String, plan_id: String, trigger: RefreshTrigger) -> Self {
        let r = crate::config::load_config().orchestration.refresh;
        Self {
            pane_id,
            plan_id,
            trigger,
            tuning: RefreshTuning {
                handoff_timeout: Duration::from_secs(r.handoff_timeout_secs),
                settle_timeout: Duration::from_secs(r.settle_timeout_secs),
                reorient_timeout: Duration::from_secs(r.reorient_timeout_secs),
                verify_timeout: Duration::from_secs(r.verify_timeout_secs),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// A refresh is now running in the background for this pane.
    Started,
    /// A refresh was already in flight for this pane — coalesced, no new run.
    Coalesced,
    /// The pane is not an agent / has no PTY — nothing to refresh.
    NotEligible,
}

/// Per-pane coalescing guard. A double-trigger while a refresh is in flight is a
/// no-op — mirrors `wake::watchers` needle idempotency at the cycle level.
fn active() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Resolved pane facts captured once at start (session/cwd for the mode-B path).
struct PaneCtx {
    pane_id: String,
    pty_id: String,
    agent_type: String,
    session_id: Option<String>,
}

impl ServiceContext {
    /// Kick off the verified-ordered refresh cycle for `pane_id`. Returns
    /// immediately; the S1..S5 machine runs on a background thread. Idempotent:
    /// a second call for a pane already refreshing coalesces.
    pub fn orchestrator_refresh(&self, req: RefreshRequest) -> RefreshOutcome {
        let Some(ctx) = self.resolve_refresh_pane(&req.pane_id) else {
            return RefreshOutcome::NotEligible;
        };
        if !active().lock().insert(ctx.pane_id.clone()) {
            return RefreshOutcome::Coalesced;
        }
        let svc = self.clone();
        std::thread::spawn(move || {
            run_cycle(&svc, &req, &ctx);
            active().lock().remove(&ctx.pane_id);
        });
        RefreshOutcome::Started
    }

    /// Post-reflection trigger (config-gated OFF). Fires a refresh only after a
    /// reflection block's learning is recorded — keyed on the `reflection` tag so
    /// ordinary `orchestrator learn` calls never trip it.
    pub fn maybe_refresh_after_reflection(&self, pane_id: &str, plan_id: &str, tag: Option<&str>) {
        if tag != Some("reflection") {
            return;
        }
        if !crate::config::load_config()
            .orchestration
            .refresh
            .after_reflection
        {
            return;
        }
        let req = RefreshRequest::from_config(
            pane_id.to_string(),
            plan_id.to_string(),
            RefreshTrigger::AfterReflection,
        );
        self.orchestrator_refresh(req);
    }

    fn resolve_refresh_pane(&self, pane_id: &str) -> Option<PaneCtx> {
        let mux = self.mux.read();
        let resolved = mux
            .resolve_pane_id(pane_id)
            .unwrap_or_else(|| pane_id.to_string());
        let pane = mux.get_pane(&resolved)?;
        let pty_id = pane.pty_id.clone()?;
        let agent_type = pane.agent_type.clone()?;
        Some(PaneCtx {
            pane_id: resolved,
            pty_id,
            agent_type,
            session_id: pane.agent_session_id.clone(),
        })
    }

    fn pane_status(&self, pty_id: &str) -> PaneStatus {
        self.agent_tracker.lock().get_status(pty_id)
    }

    fn screen_tail(&self, pty_id: &str) -> String {
        self.screen_state
            .lock()
            .tail_text(pty_id, TAIL_ROWS)
            .unwrap_or_default()
    }

    /// Max handoff element seq currently on the plan — the S1 readback baseline.
    fn max_handoff_seq(&self, plan_id: &str) -> u64 {
        self.plan_store
            .list_handoffs(plan_id)
            .ok()
            .and_then(|items| items.iter().map(|h| h.seq).max())
            .unwrap_or(0)
    }

    fn inject(&self, pane_id: &str, body: &str, marked: bool) {
        let envelope = if marked {
            MessageEnvelope::reminder("CodeTerm Refresh", "")
        } else {
            MessageEnvelope::pane("", "")
        };
        if let Err(e) = self.gateway_deliver(
            pane_id,
            envelope,
            body,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        ) {
            log::warn!("[refresh] inject failed pane={} err={}", short(pane_id), e);
        }
    }
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn log_phase(ctx: &PaneCtx, trigger: RefreshTrigger, phase: RefreshPhase) {
    // Structured ordering marker the drills grep for transcript-order proof.
    log::info!(
        "[refresh] pane={} trigger={} phase={}",
        short(&ctx.pane_id),
        trigger.tag(),
        phase.tag()
    );
}

fn run_cycle(svc: &ServiceContext, req: &RefreshRequest, ctx: &PaneCtx) {
    let baseline_seq = svc.max_handoff_seq(&req.plan_id);
    let mut phase = RefreshPhase::HandoffSnapshot;
    let mut phase_started = Instant::now();
    let cycle_started = Instant::now();
    let mut renudged = false;
    let mut reorient_needle = String::new();

    // S1 entry: request the pre-compaction snapshot.
    log_phase(ctx, req.trigger, phase);
    svc.inject(
        &ctx.pane_id,
        &template::render(HANDOFF_REQUEST, &req.plan_id),
        true,
    );

    loop {
        std::thread::sleep(TICK);
        if cycle_started.elapsed() >= HARD_CAP {
            alert_stranded(svc, req, ctx, "hard cap");
            return;
        }

        let obs = build_observation(
            svc,
            req,
            ctx,
            phase,
            phase_started.elapsed(),
            baseline_seq,
            renudged,
            &reorient_needle,
        );

        match advance(phase, &obs, &req.tuning) {
            Step::Hold => {}
            Step::Enter(next) => {
                enter_phase(svc, req, ctx, next, &mut reorient_needle);
                phase = next;
                phase_started = Instant::now();
                log_phase(ctx, req.trigger, phase);
            }
            Step::Renudge => {
                log::warn!(
                    "[refresh] pane={} resumption not observed — single re-nudge",
                    short(&ctx.pane_id)
                );
                svc.inject(&ctx.pane_id, &reorient_needle, true);
                renudged = true;
                phase_started = Instant::now();
            }
            Step::Complete => {
                log::info!(
                    "[refresh] pane={} resumed — cycle complete in {:?}",
                    short(&ctx.pane_id),
                    cycle_started.elapsed()
                );
                record_cost(ctx, cycle_started.elapsed());
                return;
            }
            Step::Alert => {
                alert_stranded(svc, req, ctx, "resumption never observed");
                return;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_observation(
    svc: &ServiceContext,
    req: &RefreshRequest,
    ctx: &PaneCtx,
    phase: RefreshPhase,
    elapsed_in_phase: Duration,
    baseline_seq: u64,
    renudged: bool,
    reorient_needle: &str,
) -> Observation {
    let mut obs = Observation {
        elapsed_in_phase,
        renudged,
        ..Observation::default()
    };
    match phase {
        RefreshPhase::HandoffSnapshot => {
            obs.handoff_landed = svc.max_handoff_seq(&req.plan_id) > baseline_seq;
        }
        RefreshPhase::CompactSettle => {
            let parked = matches!(
                svc.pane_status(&ctx.pty_id),
                PaneStatus::Idle | PaneStatus::Waiting
            );
            obs.settle_detected = parked && compaction_signature(&svc.screen_tail(&ctx.pty_id));
        }
        RefreshPhase::Reorient => {
            // Delivered == the injected body drained from the composer (submitted).
            let tail = svc.screen_tail(&ctx.pty_id);
            obs.reorient_delivered =
                !reorient_needle.is_empty() && !screen_contains_echo(&tail, reorient_needle);
        }
        RefreshPhase::VerifyResumption => {
            obs.resumption_observed = matches!(svc.pane_status(&ctx.pty_id), PaneStatus::Working);
        }
        RefreshPhase::Compact => {}
    }
    obs
}

fn enter_phase(
    svc: &ServiceContext,
    req: &RefreshRequest,
    ctx: &PaneCtx,
    next: RefreshPhase,
    reorient_needle: &mut String,
) {
    match next {
        RefreshPhase::Compact => send_compact(svc, req, ctx),
        RefreshPhase::Reorient => {
            let body = svc.plan_store.prepend_heartbeat_for_plan(
                &req.plan_id,
                &template::render(REORIENT, &req.plan_id),
            );
            svc.inject(&ctx.pane_id, &body, true);
            *reorient_needle = body;
        }
        // Settle / Verify only start a timer; no entry side-effect.
        _ => {}
    }
}

/// S2 — capability-keyed compaction. Mode A: the provider's declared compact
/// command. Mode B (no compact capability): clean pane restart + session resume.
/// Neither available → skip-with-log. Never pretends.
fn send_compact(svc: &ServiceContext, _req: &RefreshRequest, ctx: &PaneCtx) {
    let provider = svc.agent_registry.provider_arc(&ctx.agent_type);
    let compact = provider.as_ref().and_then(|p| p.compact_command());
    if let Some(cmd) = compact {
        svc.inject(&ctx.pane_id, cmd, false);
        log::info!(
            "[refresh] pane={} compact mode=A cmd={}",
            short(&ctx.pane_id),
            cmd
        );
        return;
    }
    let can_resume = provider
        .as_ref()
        .and_then(|p| {
            ctx.session_id
                .as_deref()
                .and_then(|s| p.build_resume_command(s, false))
        })
        .is_some();
    if can_resume {
        log::info!(
            "[refresh] pane={} compact mode=B (restart+resume)",
            short(&ctx.pane_id)
        );
        if let Err(e) = svc.agent_restart(&ctx.pane_id, false, 3) {
            log::warn!(
                "[refresh] pane={} mode-B restart failed: {:?} — proceeding to re-orient",
                short(&ctx.pane_id),
                e
            );
        }
        return;
    }
    log::warn!(
        "[refresh] pane={} no compact capability and no resume — skip-with-log",
        short(&ctx.pane_id)
    );
}

fn alert_stranded(svc: &ServiceContext, req: &RefreshRequest, ctx: &PaneCtx, reason: &str) {
    let title = format!(
        "Refresh stranded the orchestrator on Tab {}",
        short(&ctx.pane_id)
    );
    let body = format!(
        "plan {} — {reason}. The orchestrator did not resume after refresh + one re-nudge. Decide: re-seed manually or hand off.",
        req.plan_id
    );
    log::error!("[refresh] ALERT {} — {}", title, body);
    svc.create_notification(title, Some(body), Some("error"));
    // Planner-routed decision gate: a tagged heartbeat learning the orchestrator
    // status surfaces — decide re-seed vs hand off.
    let _ = svc.plan_store.add_heartbeat_learning(
        Some(&req.plan_id),
        Some(&ctx.pane_id),
        None,
        &format!(
            "refresh stranded orchestrator on Tab {} ({reason}) — decide re-seed vs hand off",
            short(&ctx.pane_id)
        ),
        Some("refresh-stranded"),
        crate::delayed_send::now_ms(),
    );
}

/// Token-accounting hook — one refresh cycle vs ~10 post-drift turns. The real
/// numbers land in the drill's `token-accounting.md`; here we log the wall-clock
/// cost so the drill can correlate.
fn record_cost(ctx: &PaneCtx, elapsed: Duration) {
    log::info!(
        "[refresh] pane={} cycle_cost_wall_ms={}",
        short(&ctx.pane_id),
        elapsed.as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_guard_coalesces_duplicate_pane() {
        // Cycle-level idempotency: a second trigger for an in-flight pane is a
        // no-op — mirrors wake.rs needle coalescing.
        let key = "pane-refresh-coalesce-unit-test";
        assert!(active().lock().insert(key.to_string()), "first arms a run");
        assert!(
            !active().lock().insert(key.to_string()),
            "second coalesces — no second run"
        );
        active().lock().remove(key);
        assert!(
            active().lock().insert(key.to_string()),
            "re-armable once the prior run cleared"
        );
        active().lock().remove(key);
    }

    #[test]
    fn short_id_is_bounded() {
        assert_eq!(short("abcdef1234567890"), "abcdef12");
        assert_eq!(short("ab"), "ab");
    }
}
