//! Daemon-side driver for the refresh state machine. Owns the polling thread,
//! builds `Observation`s from live tab/plan reads, and executes each phase's
//! entry side-effect through `Messenger::deliver` (wake-confirmer for free).
//! All transition logic lives in `machine`; this is the I/O shell.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::status::TabStatus;
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
    pub tab_id: String,
    pub plan_id: String,
    pub trigger: RefreshTrigger,
    pub tuning: RefreshTuning,
}

impl RefreshRequest {
    /// Build a request with tuning sourced from `[orchestration.refresh]`.
    pub fn from_config(tab_id: String, plan_id: String, trigger: RefreshTrigger) -> Self {
        let r = crate::config::load_config().orchestration.refresh;
        Self {
            tab_id,
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
    /// A refresh is now running in the background for this tab.
    Started,
    /// A refresh was already in flight for this tab — coalesced, no new run.
    Coalesced,
    /// The tab is not an agent / has no PTY — nothing to refresh.
    NotEligible,
}

/// Per-tab coalescing guard. A double-trigger while a refresh is in flight is a
/// no-op — mirrors `wake::watchers` needle idempotency at the cycle level.
fn active() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Resolved tab facts captured once at start (session/cwd for the mode-B path).
struct TabCtx {
    tab_id: String,
    pty_id: String,
    agent_type: String,
    session_id: Option<String>,
}

impl ServiceContext {
    /// Kick off the verified-ordered refresh cycle for `tab_id`. Returns
    /// immediately; the S1..S5 machine runs on a background thread. Idempotent:
    /// a second call for a tab already refreshing coalesces.
    pub fn orchestrator_refresh(&self, req: RefreshRequest) -> RefreshOutcome {
        let Some(ctx) = self.resolve_refresh_tab(&req.tab_id) else {
            return RefreshOutcome::NotEligible;
        };
        if !active().lock().insert(ctx.tab_id.clone()) {
            return RefreshOutcome::Coalesced;
        }
        let svc = self.clone();
        std::thread::spawn(move || {
            run_cycle(&svc, &req, &ctx);
            active().lock().remove(&ctx.tab_id);
        });
        RefreshOutcome::Started
    }

    /// Post-reflection trigger (config-gated OFF). Fires a refresh only after a
    /// reflection block's learning is recorded — keyed on the `reflection` tag so
    /// ordinary `orchestrator learn` calls never trip it.
    pub fn maybe_refresh_after_reflection(&self, tab_id: &str, plan_id: &str, tag: Option<&str>) {
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
            tab_id.to_string(),
            plan_id.to_string(),
            RefreshTrigger::AfterReflection,
        );
        self.orchestrator_refresh(req);
    }

    fn resolve_refresh_tab(&self, tab_id: &str) -> Option<TabCtx> {
        let mux = self.mux.read();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .unwrap_or_else(|| tab_id.to_string());
        let tab = mux.get_tab(&resolved)?;
        let pty_id = tab.pty_id.clone()?;
        let agent_type = tab.agent_type.clone()?;
        Some(TabCtx {
            tab_id: resolved,
            pty_id,
            agent_type,
            session_id: tab.agent_session_id.clone(),
        })
    }

    fn tab_status(&self, pty_id: &str) -> TabStatus {
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

    fn inject(&self, tab_id: &str, body: &str, marked: bool) {
        let envelope = if marked {
            MessageEnvelope::reminder("CodeTerm Refresh", "")
        } else {
            MessageEnvelope::tab("", "")
        };
        if let Err(e) = self.gateway_deliver(
            tab_id,
            envelope,
            body,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        ) {
            log::warn!("[refresh] inject failed tab={} err={}", short(tab_id), e);
        }
    }
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn log_phase(ctx: &TabCtx, trigger: RefreshTrigger, phase: RefreshPhase) {
    // Structured ordering marker the drills grep for transcript-order proof.
    log::info!(
        "[refresh] tab={} trigger={} phase={}",
        short(&ctx.tab_id),
        trigger.tag(),
        phase.tag()
    );
}

fn run_cycle(svc: &ServiceContext, req: &RefreshRequest, ctx: &TabCtx) {
    let baseline_seq = svc.max_handoff_seq(&req.plan_id);
    let mut phase = RefreshPhase::HandoffSnapshot;
    let mut phase_started = Instant::now();
    let cycle_started = Instant::now();
    let mut renudged = false;
    let mut reorient_needle = String::new();

    // S1 entry: request the pre-compaction snapshot.
    log_phase(ctx, req.trigger, phase);
    svc.inject(
        &ctx.tab_id,
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
                    "[refresh] tab={} resumption not observed — single re-nudge",
                    short(&ctx.tab_id)
                );
                svc.inject(&ctx.tab_id, &reorient_needle, true);
                renudged = true;
                phase_started = Instant::now();
            }
            Step::Complete => {
                log::info!(
                    "[refresh] tab={} resumed — cycle complete in {:?}",
                    short(&ctx.tab_id),
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
    ctx: &TabCtx,
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
                svc.tab_status(&ctx.pty_id),
                TabStatus::Idle | TabStatus::Waiting
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
            obs.resumption_observed = matches!(svc.tab_status(&ctx.pty_id), TabStatus::Working);
        }
        RefreshPhase::Compact => {}
    }
    obs
}

fn enter_phase(
    svc: &ServiceContext,
    req: &RefreshRequest,
    ctx: &TabCtx,
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
            svc.inject(&ctx.tab_id, &body, true);
            *reorient_needle = body;
        }
        // Settle / Verify only start a timer; no entry side-effect.
        _ => {}
    }
}

/// S2 — capability-keyed compaction. Mode A: the provider's declared compact
/// command. Mode B (no compact capability): clean tab restart + session resume.
/// Neither available → skip-with-log. Never pretends.
fn send_compact(svc: &ServiceContext, _req: &RefreshRequest, ctx: &TabCtx) {
    let provider = svc.agent_registry.provider_arc(&ctx.agent_type);
    let compact = provider.as_ref().and_then(|p| p.compact_command());
    if let Some(cmd) = compact {
        svc.inject(&ctx.tab_id, cmd, false);
        log::info!(
            "[refresh] tab={} compact mode=A cmd={}",
            short(&ctx.tab_id),
            cmd
        );
        return;
    }
    let permission_bypass = match crate::service::permission_policy::configured_permission_bypass(
        &svc.agent_registry,
        &ctx.agent_type,
    ) {
        Ok(authorization) => authorization.is_some(),
        Err(error) => {
            log::warn!(
                "[refresh] tab={} cannot resolve agent access: {}",
                short(&ctx.tab_id),
                error
            );
            return;
        }
    };
    let can_resume = provider
        .as_ref()
        .and_then(|p| {
            ctx.session_id
                .as_deref()
                .and_then(|s| p.build_resume_command(s, permission_bypass))
        })
        .is_some();
    if can_resume {
        log::info!(
            "[refresh] tab={} compact mode=B (restart+resume)",
            short(&ctx.tab_id)
        );
        if let Err(e) = svc.agent_restart(&ctx.tab_id, false, 3) {
            log::warn!(
                "[refresh] tab={} mode-B restart failed: {:?} — proceeding to re-orient",
                short(&ctx.tab_id),
                e
            );
        }
        return;
    }
    log::warn!(
        "[refresh] tab={} no compact capability and no resume — skip-with-log",
        short(&ctx.tab_id)
    );
}

fn alert_stranded(svc: &ServiceContext, req: &RefreshRequest, ctx: &TabCtx, reason: &str) {
    let title = format!(
        "Refresh stranded the orchestrator on Tab {}",
        short(&ctx.tab_id)
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
        Some(&ctx.tab_id),
        None,
        &format!(
            "refresh stranded orchestrator on Tab {} ({reason}) — decide re-seed vs hand off",
            short(&ctx.tab_id)
        ),
        Some("refresh-stranded"),
        crate::delayed_send::now_ms(),
    );
}

/// Token-accounting hook — one refresh cycle vs ~10 post-drift turns. The real
/// numbers land in the drill's `token-accounting.md`; here we log the wall-clock
/// cost so the drill can correlate.
fn record_cost(ctx: &TabCtx, elapsed: Duration) {
    log::info!(
        "[refresh] tab={} cycle_cost_wall_ms={}",
        short(&ctx.tab_id),
        elapsed.as_millis()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_guard_coalesces_duplicate_tab() {
        // Cycle-level idempotency: a second trigger for an in-flight tab is a
        // no-op — mirrors wake.rs needle coalescing.
        let key = "tab-refresh-coalesce-unit-test";
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
