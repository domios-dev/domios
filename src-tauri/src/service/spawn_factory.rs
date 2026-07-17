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

use crate::service::messenger::{MessageEnvelope, Messenger};
use crate::service::role_identity::ManagerBootstrapInputs;
use crate::service::ServiceContext;

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

/// Outcome of a readiness-gated introduction delivery.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IntroDelivery {
    Delivered,
    NotReady(String),
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
        Messenger::deliver(
            self,
            &to.pane_id,
            MessageEnvelope::from_codeterm(),
            &introduction_body(to, other),
        )
        .map(|_| ())
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

    /// Deliver both directions of a pairing introduction between two LIVE panes,
    /// replacing the one-directional `pending_greeting` hacks and the superseded
    /// "planner is live" reminder.
    pub(crate) fn deliver_pairing_introductions(&self, a: &Counterpart, b: &Counterpart) {
        self.introduce_to(a, b);
        self.introduce_to(b, a);
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
