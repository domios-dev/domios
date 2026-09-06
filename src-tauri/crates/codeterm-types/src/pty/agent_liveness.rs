//! The one observation of whether an agent PROCESS is live in a tab, and the
//! one label rule every surface renders it with.
//!
//! Every other agent-state claim in the product (restore reporting, record
//! reconciliation, lifecycle teardown, CLI/UI/sidebar labels) is downstream of
//! [`classify_agent_liveness`]. The inputs are facts a caller can point at — a
//! PTY session that exists, a boot stage emitted at a real launch transition, a
//! shell that reported its foreground command finished, an open plugin session.
//! Elapsed time is not an input and never will be: a tab that has not yet
//! produced any of those facts is [`AgentLiveness::Unobserved`], which is a
//! legitimate answer, and the reason this module exists.

use serde::{Deserialize, Serialize};

use super::BootStage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLiveness {
    /// An agent process was observed running behind this tab.
    Live,
    /// Observed absent: the tab declares an agent, and the process is gone
    /// (PTY died, provider exited to the shell, spawn failed, or the tab came
    /// back from restore as a bare shell).
    Absent,
    /// Nothing has been observed yet — a launch is in flight, or the tab was
    /// rebuilt from a snapshot and no PTY has materialized. NOT a guess in
    /// either direction.
    Unobserved,
}

impl AgentLiveness {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentLiveness::Live => "live",
            AgentLiveness::Absent => "absent",
            AgentLiveness::Unobserved => "unobserved",
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self, AgentLiveness::Live)
    }
}

/// How the tab is backed, because the observable fact differs per transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentBacking {
    /// PTY-transport provider. `None` means the tab names no PTY yet, so
    /// nothing has been observed; `Some` is the PtyManager session lookup.
    Pty { pty_live: Option<bool> },
    /// chatBackend plugin. `session_open` is the plugin session lookup.
    ChatBackend { session_open: bool },
}

/// Only observed facts. No timestamps, no durations, no counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LivenessEvidence {
    /// The tab record claims an agent (`agent_type` is set). A record alone is
    /// never evidence of a process — it only makes the question meaningful.
    pub declares_agent: bool,
    pub backing: AgentBacking,
    pub boot_stage: Option<BootStage>,
    /// A launch owns this tab right now (`agent_booting`).
    pub boot_in_flight: bool,
    /// The shell reported its foreground command finished (OSC 633 `D`) after
    /// the agent was proven up. The agent process exited; the shell survived.
    pub provider_exit_observed: bool,
}

/// Pure. The single classifier every reader goes through.
pub fn classify_agent_liveness(evidence: LivenessEvidence) -> AgentLiveness {
    if !evidence.declares_agent {
        return AgentLiveness::Absent;
    }
    match evidence.backing {
        AgentBacking::ChatBackend { session_open } => {
            return if session_open {
                AgentLiveness::Live
            } else {
                AgentLiveness::Absent
            };
        }
        // The process cannot outlive the PTY that hosts it, but a tab that has
        // no PTY yet has not been observed either way.
        AgentBacking::Pty { pty_live } => match pty_live {
            None => return AgentLiveness::Unobserved,
            Some(false) => return AgentLiveness::Absent,
            Some(true) => {}
        },
    }
    if evidence.provider_exit_observed {
        return AgentLiveness::Absent;
    }
    match evidence.boot_stage {
        // The provider's own TUI frame, and the sealed launch, are direct
        // evidence the process is running.
        Some(BootStage::ProviderReady) | Some(BootStage::AgentReady) => AgentLiveness::Live,
        Some(BootStage::SpawnFailed) => AgentLiveness::Absent,
        // A restored tab that proved only its PTY is live has proved nothing
        // about the agent inside it.
        Some(BootStage::PtyReattached) => AgentLiveness::Unobserved,
        // Everything up to and including `shell_ready` is a shell, not an
        // agent. Absent unless a launch is still running.
        _ if evidence.boot_in_flight => AgentLiveness::Unobserved,
        None => AgentLiveness::Unobserved,
        Some(_) => AgentLiveness::Absent,
    }
}

/// THE bracket-label rule, stated once and shared by every renderer.
///
/// The bracket is the tab's agent IDENTITY — `provider:model` — never its role;
/// role has its own field on every surface. A component that was not observed
/// is omitted rather than guessed, and the liveness of the process is carried
/// by a suffix so a label can never claim a running agent that isn't there:
///
/// * live         -> `provider:model`
/// * unobserved   -> `provider:model?`
/// * absent       -> `provider:model!`
///
/// A tab with no declared provider has no bracket at all.
pub fn agent_bracket_label(
    provider: Option<&str>,
    model: Option<&str>,
    liveness: AgentLiveness,
) -> Option<String> {
    let provider = provider.map(str::trim).filter(|value| !value.is_empty())?;
    let model = model.map(str::trim).filter(|value| !value.is_empty());
    let identity = match model {
        Some(model) => format!("{provider}:{model}"),
        None => provider.to_string(),
    };
    Some(match liveness {
        AgentLiveness::Live => identity,
        AgentLiveness::Unobserved => format!("{identity}?"),
        AgentLiveness::Absent => format!("{identity}!"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pty(pty_live: bool) -> AgentBacking {
        AgentBacking::Pty {
            pty_live: Some(pty_live),
        }
    }

    fn evidence(boot_stage: Option<BootStage>) -> LivenessEvidence {
        LivenessEvidence {
            declares_agent: true,
            backing: pty(true),
            boot_stage,
            boot_in_flight: false,
            provider_exit_observed: false,
        }
    }

    #[test]
    fn a_record_without_a_process_is_never_live() {
        // The live reproduction: role intact, nonce matched, agent_state idle,
        // boot_stage shell_ready, no agent process.
        assert_eq!(
            classify_agent_liveness(evidence(Some(BootStage::ShellReady))),
            AgentLiveness::Absent
        );
    }

    #[test]
    fn a_launch_in_flight_is_unobserved_not_absent() {
        let mut e = evidence(Some(BootStage::AgentLaunching));
        e.boot_in_flight = true;
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Unobserved);
    }

    #[test]
    fn a_snapshot_rebuilt_tab_with_no_pty_yet_is_unobserved() {
        // Restore rebuilds the record with `pty_id: None`; nothing about the
        // agent has been observed, in either direction.
        let mut e = evidence(Some(BootStage::ShellReady));
        e.backing = AgentBacking::Pty { pty_live: None };
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Unobserved);
    }

    #[test]
    fn a_reattached_pty_proves_the_pty_not_the_agent() {
        assert_eq!(
            classify_agent_liveness(evidence(Some(BootStage::PtyReattached))),
            AgentLiveness::Unobserved
        );
    }

    #[test]
    fn proven_launch_stages_are_live() {
        for stage in [BootStage::ProviderReady, BootStage::AgentReady] {
            assert_eq!(
                classify_agent_liveness(evidence(Some(stage))),
                AgentLiveness::Live,
                "{stage:?}"
            );
        }
    }

    #[test]
    fn provider_exit_in_a_surviving_shell_is_absent() {
        let mut e = evidence(Some(BootStage::AgentReady));
        e.provider_exit_observed = true;
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Absent);
    }

    #[test]
    fn a_dead_pty_is_absent_whatever_the_record_says() {
        let mut e = evidence(Some(BootStage::AgentReady));
        e.backing = pty(false);
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Absent);
    }

    #[test]
    fn spawn_failure_is_absent_not_unobserved() {
        assert_eq!(
            classify_agent_liveness(evidence(Some(BootStage::SpawnFailed))),
            AgentLiveness::Absent
        );
    }

    #[test]
    fn a_tab_that_declares_no_agent_has_none() {
        let mut e = evidence(Some(BootStage::AgentReady));
        e.declares_agent = false;
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Absent);
    }

    #[test]
    fn chat_backend_liveness_is_the_plugin_session() {
        let mut e = evidence(None);
        e.backing = AgentBacking::ChatBackend { session_open: true };
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Live);
        e.backing = AgentBacking::ChatBackend {
            session_open: false,
        };
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Absent);
    }

    #[test]
    fn bracket_label_follows_one_rule_for_every_row() {
        let rows: [(Option<&str>, Option<&str>, AgentLiveness, Option<&str>); 8] = [
            // fresh spawn, everything observed
            (
                Some("claude"),
                Some("opus"),
                AgentLiveness::Live,
                Some("claude:opus"),
            ),
            // launch in flight
            (
                Some("claude"),
                Some("opus"),
                AgentLiveness::Unobserved,
                Some("claude:opus?"),
            ),
            // restored as a bare shell
            (
                Some("claude"),
                Some("opus"),
                AgentLiveness::Absent,
                Some("claude:opus!"),
            ),
            // model not observed yet
            (Some("codex"), None, AgentLiveness::Live, Some("codex")),
            (Some("codex"), None, AgentLiveness::Absent, Some("codex!")),
            // empty strings are not observations
            (
                Some("claude"),
                Some("  "),
                AgentLiveness::Live,
                Some("claude"),
            ),
            (Some(""), Some("opus"), AgentLiveness::Live, None),
            // a plain terminal has no bracket
            (None, None, AgentLiveness::Absent, None),
        ];
        for (provider, model, liveness, expected) in rows {
            assert_eq!(
                agent_bracket_label(provider, model, liveness).as_deref(),
                expected,
                "provider={provider:?} model={model:?} liveness={liveness:?}"
            );
        }
    }

    #[test]
    fn the_bracket_never_carries_a_role() {
        // Role is a separate axis on every surface; the General Agent, an
        // orchestrator and a worker on the same provider/model render alike.
        let label = agent_bracket_label(Some("claude"), Some("opus"), AgentLiveness::Live);
        assert_eq!(label.as_deref(), Some("claude:opus"));
        for role in [
            "manager",
            "worker",
            "general_agent",
            "orchestrator",
            "planner",
        ] {
            assert!(
                !label.as_deref().unwrap().contains(role),
                "role {role} must not reach the bracket"
            );
        }
    }
}
