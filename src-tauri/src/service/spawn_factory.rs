//! Single seam for constructing role-carrying spawns. Every spawn edge routes
//! its role-identity inputs and team-pairing introductions through here instead
//! of hand-building `pending_greeting` display hacks or one-directional reminders.
//!
//! The two concerns:
//! - Role identity (system prompt) — `forming_identity` mints the placeholder
//!   `ManagerBootstrapInputs` shared by the pair + standalone-planner edges.
//! - Team introductions — when a spawn completes a pairing, EACH member's first
//!   codeterm message names its counterpart (title, pane id, reach command) so a
//!   fresh member never rediscovers — and duplicate-spawns — its own pair.

use codeterm_types::pty::PaneStatus;

use crate::service::messenger::{MessageEnvelope, Messenger};
use crate::service::role_identity::ManagerBootstrapInputs;
use crate::service::ServiceContext;

/// Bounded failure deadline for delivering a freshly-spawned pane's pairing
/// intro. A recipient still mid-first-turn is not receptive; we wait for it to
/// settle rather than dropping the send to the 3s submit-verify cap.
const PAIR_INTRO_RECEPTIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// Placeholder ids carried by a forming manager before a team/plan is bound.
pub(crate) const UNBOUND_TEAM: &str = "not yet established";
pub(crate) const UNBOUND_PLAN: &str = "no plan bound yet";

/// Role-identity inputs for a manager spawned before its team/plan exist.
pub(crate) fn forming_identity(role: &str, project: String) -> ManagerBootstrapInputs {
    ManagerBootstrapInputs {
        role: role.into(),
        team_id: UNBOUND_TEAM.into(),
        team_name: UNBOUND_TEAM.into(),
        plan_id: UNBOUND_PLAN.into(),
        project,
    }
}

/// A member of a freshly-formed pairing — the minimal identity another member
/// needs to reach it: display name (tab title), pane id, and human role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Counterpart {
    pub label: String,
    pub pane_id: String,
    pub role: String,
}

impl Counterpart {
    pub(crate) fn new(role: &str, label: impl Into<String>, pane_id: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            pane_id: pane_id.into(),
            role: role.into(),
        }
    }
}

/// The exact reach command a member uses to message its counterpart. `--tab`
/// accepts the pane id (see prompts/codeterm_rule.md).
pub(crate) fn reach_command(pane_id: &str) -> String {
    format!("codeterm send \"…\" --tab {pane_id}")
}

/// The body one member reads about its counterpart: names it, its pane id, and
/// the reach command, and forbids spawning a duplicate of an already-live pair.
pub(crate) fn introduction_body(me: &Counterpart, other: &Counterpart) -> String {
    format!(
        "You are the {my_role} of this team. Your {other_role} counterpart is {other_label} (pane {other_pane}) — reach it with `{reach}`. It already exists: coordinate through it, never spawn a duplicate.",
        my_role = me.role,
        other_role = other.role,
        other_label = other.label,
        other_pane = other.pane_id,
        reach = reach_command(&other.pane_id),
    )
}

/// Outcome of a readiness/receptivity-gated introduction delivery.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IntroDelivery {
    Delivered,
    NotReady(String),
}

/// A pane accepts a follow-up message once its agent turn has settled. A pane
/// still `Working` (e.g. a freshly-spawned agent generating its identity turn)
/// would drop the send to the submit-verify cap, so we wait for it to settle.
pub(crate) fn status_is_receptive(status: PaneStatus) -> bool {
    matches!(status, PaneStatus::Idle | PaneStatus::Waiting)
}

/// Wait for the recipient to be receptive, then deliver its intro exactly once.
/// Generic over status polling, the event wait, and delivery so the gating
/// logic is unit-tested without a live PTY. A `Dead`/unresolvable recipient
/// returns `NotReady` without delivering; a recipient that never settles by the
/// deadline still gets one final attempt (its intro is never silently dropped).
pub(crate) fn deliver_when_receptive<S, W, D>(
    mut status: S,
    mut wait_change: W,
    deliver: D,
) -> IntroDelivery
where
    S: FnMut() -> Option<PaneStatus>,
    W: FnMut() -> bool,
    D: FnOnce() -> Result<(), String>,
{
    loop {
        match status() {
            Some(s) if status_is_receptive(s) => break,
            Some(PaneStatus::Dead) | None => {
                return IntroDelivery::NotReady("recipient pane is gone".into());
            }
            _ => {
                // Deadline hit while still Working — fall through to a final
                // attempt rather than dropping the intro.
                if !wait_change() {
                    break;
                }
            }
        }
    }
    match deliver() {
        Ok(()) => IntroDelivery::Delivered,
        Err(e) => IntroDelivery::NotReady(e),
    }
}

/// Deliver a still-booting recipient's introduction ONLY once its launch
/// readiness signal resolves Ok — the first-message guarantee. Nothing is sent
/// before the pane is live; a readiness failure returns `NotReady` (the caller
/// warns with pane ids) rather than dropping the intro silently.
pub(crate) async fn deliver_when_ready<F>(
    ready: tokio::sync::oneshot::Receiver<Result<(), String>>,
    deliver: F,
) -> IntroDelivery
where
    F: FnOnce() -> Result<(), String>,
{
    match ready.await {
        Ok(Ok(())) => match deliver() {
            Ok(()) => IntroDelivery::Delivered,
            Err(e) => IntroDelivery::NotReady(e),
        },
        Ok(Err(e)) => IntroDelivery::NotReady(e),
        Err(_) => IntroDelivery::NotReady("readiness channel closed before launch".into()),
    }
}

impl ServiceContext {
    /// Deliver a single introduction as a real codeterm message from CodeTerm:
    /// `to` learns about `other` (name, pane id, reach command). Returns the
    /// delivery error so a caller can gate/report it; never panics.
    pub(crate) fn try_introduce(
        &self,
        to: &Counterpart,
        other: &Counterpart,
    ) -> Result<(), String> {
        use crate::service::message_gateway::{
            DeliveryClass, MessageBody, MessageOrigin, OutboundMessage,
        };
        // Routed through the gateway so the SystemNotice class gates on the
        // recipient's receptivity — a freshly-spawned pane's intro no longer
        // races its own first turn and drops the Enter.
        self.gateway
            .submit(
                self,
                OutboundMessage {
                    target: to.pane_id.clone(),
                    body: MessageBody::Text {
                        body: introduction_body(to, other),
                        enter: true,
                    },
                    envelope: Some(MessageEnvelope::from_codeterm()),
                    class: DeliveryClass::SystemNotice,
                    origin: MessageOrigin::System,
                    idempotency_key: None,
                },
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Introduce a LIVE recipient to `other` now. A recipient that is not a live
    /// agent is logged and skipped, never fatal.
    pub(crate) fn introduce_to(&self, to: &Counterpart, other: &Counterpart) {
        if let Err(e) = self.try_introduce(to, other) {
            log::warn!(
                "[spawn-factory] introduction to {} not delivered: {e}",
                to.pane_id
            );
        }
    }

    /// Blocking. Introduce `to` to `other` once `to` is receptive — the fix for
    /// a freshly-spawned pair member whose immediate `send_text` would race its
    /// own first turn and be dropped by the submit-verify cap. A chat-backend or
    /// not-yet-linked recipient is delivered directly. A recipient that never
    /// settles by the deadline is warned with both pane ids, never silently
    /// dropped. Run from a blocking task — it waits on the screen-change signal.
    pub(crate) fn introduce_when_receptive(
        &self,
        to: &Counterpart,
        other: &Counterpart,
        timeout: std::time::Duration,
    ) {
        let (pty_id, is_chat_backend) = {
            let mux = self.mux.read();
            match mux.get_pane(&to.pane_id) {
                Some(pane) => (pane.pty_id.clone(), pane.is_chat_backend()),
                None => (None, false),
            }
        };
        let outcome = match (pty_id, is_chat_backend) {
            (Some(pty_id), false) => {
                let signal = self.screen_state.lock().arm_screen_change_signal(&pty_id);
                let deadline = std::time::Instant::now() + timeout;
                let mut generation = signal.generation();
                deliver_when_receptive(
                    || {
                        self.mux
                            .read()
                            .get_pane(&to.pane_id)
                            .map(|_| self.agent_tracker.lock().get_status(&pty_id))
                    },
                    || {
                        let Some(remaining) =
                            deadline.checked_duration_since(std::time::Instant::now())
                        else {
                            return false;
                        };
                        let woke = signal.wait_after(generation, remaining);
                        generation = signal.generation();
                        woke
                    },
                    || self.try_introduce(to, other),
                )
            }
            // Chat-backend (no submit-verify race) or not-yet-linked pane.
            _ => match self.try_introduce(to, other) {
                Ok(()) => IntroDelivery::Delivered,
                Err(e) => IntroDelivery::NotReady(e),
            },
        };
        if let IntroDelivery::NotReady(reason) = outcome {
            log::warn!(
                "[spawn-factory] introduction to {} (counterpart {}) not delivered: {reason}",
                to.pane_id,
                other.pane_id
            );
        }
    }

    /// Deliver both directions of a pairing introduction between two
    /// freshly-spawned panes, each gated on that recipient becoming receptive.
    /// Runs the two directions as concurrent detached blocking tasks so neither
    /// recipient's settle-wait delays the other. Replaces the one-directional
    /// `pending_greeting` hacks and the superseded "planner is live" reminder.
    pub(crate) fn deliver_pairing_introductions(&self, a: &Counterpart, b: &Counterpart) {
        for (to, other) in [(a.clone(), b.clone()), (b.clone(), a.clone())] {
            let svc = self.clone();
            tauri::async_runtime::spawn_blocking(move || {
                svc.introduce_when_receptive(&to, &other, PAIR_INTRO_RECEPTIVE_TIMEOUT);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orch() -> Counterpart {
        Counterpart::new("Orchestrator", "Kepler", "orch-pane-1111")
    }

    fn planner() -> Counterpart {
        Counterpart::new("Planner", "Euler", "planner-pane-2222")
    }

    // Byron regression: a formed pairing must yield BOTH directions, and each
    // body must name the counterpart's pane id AND the reach command. The
    // pre-factory shape delivered only a one-directional display greeting that
    // named the counterpart by label with no pane id — the duplicate-spawn bug.
    #[test]
    fn pair_introductions_name_both_counterpart_pane_ids_and_reach_commands() {
        let (o, p) = (orch(), planner());
        let to_orch = introduction_body(&o, &p);
        let to_planner = introduction_body(&p, &o);

        // Orchestrator learns the planner's pane id + reach command.
        assert!(to_orch.contains(&p.pane_id), "names planner pane id");
        assert!(to_orch.contains("Euler"), "names planner");
        assert!(
            to_orch.contains(&reach_command(&p.pane_id)),
            "carries the reach command for the planner"
        );

        // Planner learns the orchestrator's pane id + reach command.
        assert!(to_planner.contains(&o.pane_id), "names orch pane id");
        assert!(to_planner.contains("Kepler"), "names orchestrator");
        assert!(
            to_planner.contains(&reach_command(&o.pane_id)),
            "carries the reach command for the orchestrator"
        );

        // Both are told not to duplicate the already-live counterpart.
        for body in [&to_orch, &to_planner] {
            assert!(
                body.contains("never spawn a duplicate"),
                "each member is warned against duplicating its pair"
            );
        }
    }

    // A newly-added team member is still booting when its intro is scheduled.
    // The intro must NOT be sent before the pane's launch readiness resolves,
    // and once it flips ready it is delivered exactly once as the first message.
    // Red on best-effort code, which delivers immediately regardless of boot.
    #[tokio::test]
    async fn member_intro_waits_for_readiness_then_delivers_first_message() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let (tx, rx) = tokio::sync::oneshot::channel();
        let delivered = Arc::new(AtomicUsize::new(0));
        let seen = delivered.clone();
        let handle = tokio::spawn(deliver_when_ready(rx, move || {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));

        // Still booting: nothing delivered yet.
        tokio::task::yield_now().await;
        assert_eq!(
            delivered.load(Ordering::SeqCst),
            0,
            "intro must not reach a still-booting member"
        );

        // Readiness flips → intro delivered exactly once.
        tx.send(Ok(())).unwrap();
        assert_eq!(handle.await.unwrap(), IntroDelivery::Delivered);
        assert_eq!(delivered.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn member_intro_reports_readiness_failure_without_delivering() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let (tx, rx) = tokio::sync::oneshot::channel();
        let called = Arc::new(AtomicBool::new(false));
        let seen = called.clone();
        let handle = tokio::spawn(deliver_when_ready(rx, move || {
            seen.store(true, Ordering::SeqCst);
            Ok(())
        }));

        tx.send(Err("boot failed".into())).unwrap();
        match handle.await.unwrap() {
            IntroDelivery::NotReady(e) => assert!(e.contains("boot failed")),
            other => panic!("expected NotReady, got {other:?}"),
        }
        assert!(
            !called.load(Ordering::SeqCst),
            "a member that never became ready must not have its intro delivered"
        );
    }

    #[test]
    fn reach_command_uses_tab_flag_with_pane_id() {
        assert_eq!(
            reach_command("abc-123"),
            "codeterm send \"…\" --tab abc-123"
        );
    }

    #[test]
    fn only_settled_statuses_are_receptive() {
        assert!(status_is_receptive(PaneStatus::Idle));
        assert!(status_is_receptive(PaneStatus::Waiting));
        assert!(!status_is_receptive(PaneStatus::Working));
        assert!(!status_is_receptive(PaneStatus::Dead));
    }

    // The pair orchestrator is still Working (generating its identity turn) at
    // the instant both agents report ready; an immediate send is dropped by the
    // submit-verify cap. The intro must be WITHHELD until the pane settles, then
    // delivered exactly once. Red on the prior code, which sent immediately.
    #[test]
    fn intro_is_withheld_until_recipient_settles_then_delivered_once() {
        use std::cell::Cell;

        let poll = Cell::new(0usize);
        let waited = Cell::new(0usize);
        let delivered = Cell::new(0usize);
        let outcome = deliver_when_receptive(
            || {
                poll.set(poll.get() + 1);
                // Working for the first two polls, then settles.
                Some(if poll.get() < 3 {
                    PaneStatus::Working
                } else {
                    PaneStatus::Idle
                })
            },
            || {
                waited.set(waited.get() + 1);
                true
            },
            || {
                delivered.set(delivered.get() + 1);
                Ok(())
            },
        );

        assert_eq!(outcome, IntroDelivery::Delivered);
        assert_eq!(delivered.get(), 1, "delivered exactly once");
        assert_eq!(
            waited.get(),
            2,
            "waited out each Working poll before sending"
        );
    }

    #[test]
    fn intro_to_a_dead_recipient_is_reported_not_delivered() {
        use std::cell::Cell;
        let delivered = Cell::new(false);
        let outcome = deliver_when_receptive(
            || Some(PaneStatus::Dead),
            || panic!("must not wait on a dead pane"),
            || {
                delivered.set(true);
                Ok(())
            },
        );
        assert!(matches!(outcome, IntroDelivery::NotReady(_)));
        assert!(!delivered.get(), "a gone recipient gets no delivery");
    }

    // A recipient that never settles by the deadline still gets one final
    // attempt — the intro is never silently dropped.
    #[test]
    fn intro_gets_a_final_attempt_when_deadline_hits_while_working() {
        use std::cell::Cell;
        let delivered = Cell::new(0usize);
        let outcome = deliver_when_receptive(
            || Some(PaneStatus::Working),
            || false, // deadline
            || {
                delivered.set(delivered.get() + 1);
                Ok(())
            },
        );
        assert_eq!(outcome, IntroDelivery::Delivered);
        assert_eq!(delivered.get(), 1, "one final attempt, not a silent drop");
    }

    #[test]
    fn forming_identity_carries_placeholders_not_fabricated_ids() {
        let id = forming_identity("Planner", "CodeTerm".into());
        assert_eq!(id.role, "Planner");
        assert_eq!(id.project, "CodeTerm");
        assert_eq!(id.team_id, UNBOUND_TEAM);
        assert_eq!(id.plan_id, UNBOUND_PLAN);
        let rendered = id.compose();
        assert!(rendered.contains(UNBOUND_TEAM));
        assert!(rendered.contains(UNBOUND_PLAN));
        assert!(!rendered.contains("team_"));
        assert!(!rendered.contains("pl_"));
    }
}
