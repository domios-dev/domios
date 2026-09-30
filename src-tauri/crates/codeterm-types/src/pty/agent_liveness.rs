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

use super::{short_model_name, BootStage};

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
    /// A provider or backend surfaced a question the owner can answer.
    pub pending_prompt: bool,
}

/// Pure. The single classifier every reader goes through.
pub fn classify_agent_liveness(evidence: LivenessEvidence) -> AgentLiveness {
    if !evidence.declares_agent {
        return AgentLiveness::Absent;
    }
    if evidence.provider_exit_observed {
        return AgentLiveness::Absent;
    }
    if evidence.pending_prompt {
        match evidence.backing {
            AgentBacking::Pty {
                pty_live: Some(true),
            }
            | AgentBacking::ChatBackend { session_open: true } => return AgentLiveness::Live,
            _ => {}
        }
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
            Some(false)
                if matches!(
                    evidence.boot_stage,
                    Some(BootStage::ProviderReady | BootStage::AgentReady)
                ) =>
            {
                return AgentLiveness::Unobserved;
            }
            Some(false) => return AgentLiveness::Absent,
            Some(true) => {}
        },
    }
    if evidence.boot_in_flight {
        return AgentLiveness::Unobserved;
    }
    match evidence.boot_stage {
        // The provider's own TUI frame, and the sealed launch, are direct
        // evidence the process is running.
        Some(BootStage::ProviderReady) | Some(BootStage::AgentReady) => AgentLiveness::Live,
        Some(BootStage::SpawnFailed) => AgentLiveness::Absent,
        // A reattached PTY proves nothing about the agent inside it, and an
        // observer giving up on a launch is not evidence that the process died.
        Some(BootStage::PtyReattached)
        | Some(BootStage::LaunchDeadlineElapsed)
        | Some(BootStage::LaunchBindingUnconfirmed)
        | Some(BootStage::AwaitingOwnerPrompt) => AgentLiveness::Unobserved,
        // Everything up to and including `shell_ready` is a shell, not an
        // agent. Absent unless a launch is still running.
        None => AgentLiveness::Unobserved,
        Some(_) => AgentLiveness::Absent,
    }
}

/// THE bracket-label rule, stated once and shared by every renderer.
///
/// The bracket is the tab's agent IDENTITY — never its role, which has its own
/// field on every surface. The model REPLACES the provider, it never joins it,
/// and exactly three cases exist:
///
/// * no usable provider              -> no bracket at all
/// * provider, model not recognised  -> the provider name
/// * model recognised                -> the short model name, alone
///
/// Falling back to the provider is a display decision about an id no catalogue
/// claims; it says nothing about the stored model, which this rule never reads
/// back to its caller or writes.
///
/// The liveness of the process rides as a suffix, so a label can never claim a
/// running agent that isn't there: live -> `opus`, unobserved -> `opus?`,
/// absent -> `opus!`.
pub fn agent_bracket_label(
    provider: Option<&str>,
    model: Option<&str>,
    liveness: AgentLiveness,
) -> Option<String> {
    let provider = provider.map(str::trim).filter(|value| !value.is_empty())?;
    let identity = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(short_model_name)
        .unwrap_or_else(|| provider.to_string());
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
            pending_prompt: false,
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
    fn a_live_pty_with_a_failure_stage_stays_unobserved_during_launch() {
        let mut e = evidence(Some(BootStage::SpawnFailed));
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

    /// The deadline that latched issue 477: an observer gave up, which says
    /// nothing about whether the process it was watching is running.
    #[test]
    fn a_launch_deadline_is_unobserved_not_absent() {
        assert_eq!(
            classify_agent_liveness(evidence(Some(BootStage::LaunchDeadlineElapsed))),
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
    fn proven_launch_stages_are_unobserved_while_launch_is_in_flight() {
        for stage in [BootStage::ProviderReady, BootStage::AgentReady] {
            let mut e = evidence(Some(stage));
            e.boot_in_flight = true;
            assert_eq!(
                classify_agent_liveness(e),
                AgentLiveness::Unobserved,
                "{stage:?}"
            );
        }
    }

    #[test]
    fn a_live_pty_with_a_pending_prompt_is_live_without_a_session_binding() {
        let mut e = evidence(Some(BootStage::AwaitingOwnerPrompt));
        e.boot_in_flight = true;
        e.pending_prompt = true;
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Live);
    }

    #[test]
    fn a_pending_prompt_needs_a_live_backing() {
        let mut e = evidence(Some(BootStage::AwaitingOwnerPrompt));
        e.pending_prompt = true;
        e.backing = AgentBacking::Pty { pty_live: None };
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Unobserved);
        e.backing = pty(false);
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Absent);
    }

    #[test]
    fn provider_exit_in_a_surviving_shell_is_absent() {
        let mut e = evidence(Some(BootStage::AgentReady));
        e.provider_exit_observed = true;
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Absent);
    }

    #[test]
    fn a_dead_pty_is_absent_whatever_the_record_says() {
        let mut e = evidence(Some(BootStage::ShellReady));
        e.backing = pty(false);
        assert_eq!(classify_agent_liveness(e), AgentLiveness::Absent);
    }

    #[test]
    fn conflicting_ready_stage_and_dead_pty_wait_for_lifecycle_reconciliation() {
        assert_eq!(
            classify_agent_liveness(evidence(Some(BootStage::AgentReady))),
            AgentLiveness::Live
        );
        let mut contradictory = evidence(Some(BootStage::AgentReady));
        contradictory.backing = pty(false);
        assert_eq!(
            classify_agent_liveness(contradictory),
            AgentLiveness::Unobserved
        );
        let mut exited = evidence(Some(BootStage::AgentReady));
        exited.backing = pty(false);
        exited.provider_exit_observed = true;
        assert_eq!(classify_agent_liveness(exited), AgentLiveness::Absent);
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
        let rows: [(Option<&str>, Option<&str>, AgentLiveness, Option<&str>); 14] = [
            // recognised model: the model alone, never the provider with it
            (
                Some("claude"),
                Some("opus"),
                AgentLiveness::Live,
                Some("opus"),
            ),
            (
                Some("claude"),
                Some("opus"),
                AgentLiveness::Unobserved,
                Some("opus?"),
            ),
            (
                Some("claude"),
                Some("opus"),
                AgentLiveness::Absent,
                Some("opus!"),
            ),
            (
                Some("codex"),
                Some("gpt-5.6-sol"),
                AgentLiveness::Live,
                Some("5.6-sol"),
            ),
            (
                Some("codex"),
                Some("gpt-5.6-luna"),
                AgentLiveness::Unobserved,
                Some("5.6-luna?"),
            ),
            (
                Some("codex"),
                Some("gpt-5.3-codex-spark"),
                AgentLiveness::Absent,
                Some("5.3-spark!"),
            ),
            (
                Some("codex"),
                Some("gpt-5.5"),
                AgentLiveness::Live,
                Some("gpt 5.5"),
            ),
            // model missing: the provider stands in, across every liveness
            (Some("codex"), None, AgentLiveness::Live, Some("codex")),
            (
                Some("codex"),
                None,
                AgentLiveness::Unobserved,
                Some("codex?"),
            ),
            (Some("codex"), None, AgentLiveness::Absent, Some("codex!")),
            // empty strings are not observations
            (
                Some("claude"),
                Some("  "),
                AgentLiveness::Live,
                Some("claude"),
            ),
            // model present but no catalogue claims it: the provider stands in
            (
                Some("lmstudio"),
                Some("nvidia/nemotron-3-nano"),
                AgentLiveness::Live,
                Some("lmstudio"),
            ),
            // no usable provider: no bracket, whatever the model says
            (Some(""), Some("opus"), AgentLiveness::Live, None),
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

    /// The owner's rule has exactly three cases; a provider-qualified model is
    /// not one of them, at any liveness.
    #[test]
    fn a_recognised_model_never_carries_its_provider() {
        for liveness in [
            AgentLiveness::Live,
            AgentLiveness::Unobserved,
            AgentLiveness::Absent,
        ] {
            for (provider, model) in [("claude", "claude-opus-5"), ("codex", "gpt-5.6-terra")] {
                let label = agent_bracket_label(Some(provider), Some(model), liveness)
                    .expect("a declared provider brackets");
                assert!(
                    !label.contains(':') && !label.contains(provider),
                    "{provider}/{model} at {liveness:?} rendered {label}"
                );
            }
        }
    }

    /// A long vendor id must not become the label; it resolves to its family or
    /// it is not shown at all.
    #[test]
    fn a_vendor_id_never_reaches_the_bracket_verbatim() {
        for (provider, model) in [
            ("claude", "claude-sonnet-4-5-20250929"),
            ("codex", "gpt-5.6-sol"),
            ("lmstudio", "google/gemma-4-26b-a4b-qat"),
        ] {
            let label = agent_bracket_label(Some(provider), Some(model), AgentLiveness::Live)
                .expect("a declared provider brackets");
            assert!(!label.contains(model), "{model} leaked as {label}");
        }
    }

    /// Choosing the provider is a rendering decision about an unclaimed id. The
    /// model the caller holds is untouched by it.
    #[test]
    fn the_provider_fallback_does_not_erase_the_stored_model() {
        let stored = String::from("mystery-local-build");
        let label = agent_bracket_label(Some("lmstudio"), Some(&stored), AgentLiveness::Live);
        assert_eq!(label.as_deref(), Some("lmstudio"));
        assert_eq!(stored, "mystery-local-build");
    }

    #[test]
    fn the_bracket_never_carries_a_role() {
        // Role is a separate axis on every surface; the General Agent, an
        // orchestrator and a worker on the same provider/model render alike.
        let label = agent_bracket_label(Some("claude"), Some("opus"), AgentLiveness::Live);
        assert_eq!(label.as_deref(), Some("opus"));
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
