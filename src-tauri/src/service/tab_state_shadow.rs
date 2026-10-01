//! Observe-only tab-state verdict beside the structural startup nudge: one log
//! line per spawn-health event, labelled as `evals/tab-state-jev` measures it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::startup_nudge::StartupNudge;
use super::tab_transport::TabTransport;
use super::ServiceContext;
use crate::api::mem_routes::decision::DecisionRegistry;
use crate::mem::decision::{DecisionAnswer, DecisionModel, DecisionQuestion, DecisionRequest};
use codeterm_types::config::TabStateShadowConfig;

pub(crate) const SCREEN_LINES: usize = 40;
pub(crate) const DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const CONFIDENCE_BAR: f64 = 0.8;
pub(crate) const INSTRUCTIONS: &str =
    "Classify the current agent tab state from its visible terminal screen. Choose the single best label.";

pub(crate) const READY_IDLE: &str = "ready_idle";
pub(crate) const WORKING_NORMALLY: &str = "working_normally";
pub(crate) const PROVIDER_UPDATE_PROMPT: &str = "provider_update_prompt";
pub(crate) const TRUST_OR_PERMISSION_PROMPT: &str = "trust_or_permission_prompt";
pub(crate) const LOGIN_OR_AUTH_REQUIRED: &str = "login_or_auth_required";
pub(crate) const MODEL_UNAVAILABLE_OR_QUOTA: &str = "model_unavailable_or_quota";
pub(crate) const CRASHED_TO_SHELL: &str = "crashed_to_shell";
pub(crate) const ERROR_LOOP: &str = "error_loop";
pub(crate) const STILL_STARTING: &str = "still_starting";

/// Kept byte-identical to `CRITERIA` in `evals/tab-state-jev/run.py`, which measured them.
pub(crate) const CRITERIA: [(&str, &str); 9] = [
    (READY_IDLE, "The agent TUI is ready, and its composer is waiting for user input."),
    (WORKING_NORMALLY, "The agent TUI is making ordinary progress, such as thinking or running tools."),
    (PROVIDER_UPDATE_PROMPT, "The provider is blocked on an update available dialog or update choice."),
    (TRUST_OR_PERMISSION_PROMPT, "The provider is blocked on a trust, folder access, command approval, or permission choice."),
    (LOGIN_OR_AUTH_REQUIRED, "The provider requires sign-in, authentication, or an authorization step."),
    (MODEL_UNAVAILABLE_OR_QUOTA, "The selected model is unavailable or the provider reached a usage quota or rate limit."),
    (CRASHED_TO_SHELL, "The provider TUI is gone and the terminal is showing a shell prompt."),
    (ERROR_LOOP, "The provider repeatedly shows the same runtime or tool error and makes no progress."),
    (STILL_STARTING, "The provider is initializing and has not reached a usable agent TUI state."),
];

const PROMPT_LABELS: [&str; 4] = [
    TRUST_OR_PERMISSION_PROMPT,
    PROVIDER_UPDATE_PROMPT,
    LOGIN_OR_AUTH_REQUIRED,
    MODEL_UNAVAILABLE_OR_QUOTA,
];

/// Screen labels consistent with each structural verdict, for the agree/disagree tally.
pub(crate) fn agreeing_labels(structural: StartupNudge) -> &'static [&'static str] {
    match structural {
        StartupNudge::Deliver => &[READY_IDLE, WORKING_NORMALLY],
        StartupNudge::AnsweredTrustDialog => &[TRUST_OR_PERMISSION_PROMPT],
        StartupNudge::BlockedOnPrompt | StartupNudge::WaitingForOwnerPrompt => &PROMPT_LABELS,
        StartupNudge::UnboundSession => &[STILL_STARTING, LOGIN_OR_AUTH_REQUIRED],
        StartupNudge::NoAgent => &[CRASHED_TO_SHELL],
    }
}

/// Counts confident shadow verdicts against the structural one; below the bar is `uncertain`.
#[derive(Default)]
pub(crate) struct ShadowTally {
    agree: AtomicU64,
    disagree: AtomicU64,
    uncertain: AtomicU64,
}

impl ShadowTally {
    fn count(&self, verdict: &ShadowVerdict) -> (u64, u64, u64) {
        let counter = if verdict.confidence < CONFIDENCE_BAR {
            &self.uncertain
        } else if verdict.agrees {
            &self.agree
        } else {
            &self.disagree
        };
        counter.fetch_add(1, Ordering::Relaxed);
        (
            self.agree.load(Ordering::Relaxed),
            self.disagree.load(Ordering::Relaxed),
            self.uncertain.load(Ordering::Relaxed),
        )
    }
}

static TALLY: ShadowTally = ShadowTally {
    agree: AtomicU64::new(0),
    disagree: AtomicU64::new(0),
    uncertain: AtomicU64::new(0),
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShadowVerdict {
    pub(crate) label: String,
    pub(crate) confidence: f64,
    pub(crate) agrees: bool,
}

/// What the observer thread needs from the daemon; a test supplies a stub.
pub(crate) trait ShadowHost: Clone + Send + 'static {
    fn transport(&self, tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String>;
    fn decision_models(&self) -> Arc<DecisionRegistry>;
    fn tally(&self) -> &ShadowTally;
    fn record(&self, line: &str);
}

impl ShadowHost for ServiceContext {
    fn transport(&self, tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String> {
        self.tab_transport(tab_id)
    }

    fn decision_models(&self) -> Arc<DecisionRegistry> {
        self.decision_models.clone()
    }

    fn tally(&self) -> &ShadowTally {
        &TALLY
    }

    fn record(&self, line: &str) {
        log::info!("{line}");
    }
}

impl ServiceContext {
    /// Shadow hook for the spawn-health event. Returns at once and changes nothing.
    pub(crate) fn shadow_tab_state(&self, tab_id: &str, structural: StartupNudge) {
        let config = crate::config::load_config().agent.tab_state_shadow;
        drop(spawn_observer(self, &config, tab_id, structural));
    }
}

/// Starts the detached observer when enabled. The handle exists only so tests can join it.
pub(crate) fn spawn_observer<H: ShadowHost>(
    host: &H,
    config: &TabStateShadowConfig,
    tab_id: &str,
    structural: StartupNudge,
) -> Option<JoinHandle<Option<ShadowVerdict>>> {
    if !config.enabled {
        return None;
    }
    let host = host.clone();
    let tab = tab_id.to_owned();
    let spawned = std::thread::Builder::new()
        .name("tab-state-shadow".into())
        .spawn(move || observe(&host, &tab, structural, Instant::now() + DEADLINE));
    match spawned {
        Ok(handle) => Some(handle),
        Err(error) => {
            log::debug!("[tab-state.shadow] tab={tab_id} observer thread failed: {error}");
            None
        }
    }
}

pub(crate) fn observe<H: ShadowHost>(
    host: &H,
    tab_id: &str,
    structural: StartupNudge,
    deadline: Instant,
) -> Option<ShadowVerdict> {
    match classify(host, tab_id, structural, deadline) {
        Ok(verdict) => {
            let (agree, disagree, uncertain) = host.tally().count(&verdict);
            host.record(&format!(
                "[tab-state.shadow] tab={tab_id} structural={:?} model={} confidence={:.3} agree={agree} disagree={disagree} uncertain={uncertain}",
                structural.reason(),
                verdict.label,
                verdict.confidence,
            ));
            Some(verdict)
        }
        Err(error) => {
            log::debug!("[tab-state.shadow] tab={tab_id} no verdict: {error}");
            None
        }
    }
}

fn classify<H: ShadowHost>(
    host: &H,
    tab_id: &str,
    structural: StartupNudge,
    deadline: Instant,
) -> Result<ShadowVerdict, String> {
    let model = choice_model(&host.decision_models())
        .ok_or("no registered decision model has the choice primitive")?;
    let screen = {
        let transport = host.transport(tab_id)?;
        transport.read_screen(false)?
    };
    let text = screen
        .get("text")
        .and_then(serde_json::Value::as_str)
        .ok_or("screen read returned no text")?;
    let request = DecisionRequest {
        state: serde_json::Value::String(screen_tail(text)),
        instructions: INSTRUCTIONS.to_owned(),
        question: DecisionQuestion::Choice {
            options: CRITERIA
                .iter()
                .map(|(label, criterion)| ((*label).to_owned(), (*criterion).to_owned()))
                .collect(),
        },
    };
    let answer = model
        .decide(&request, deadline)
        .map_err(|error| error.to_string())?;
    let (label, confidence) = chosen_label(answer)?;
    Ok(ShadowVerdict {
        agrees: agreeing_labels(structural).contains(&label.as_str()),
        label,
        confidence,
    })
}

/// The first registered model, by sorted id, that declares the `choice` primitive.
pub(crate) fn choice_model(models: &DecisionRegistry) -> Option<Arc<dyn DecisionModel>> {
    let mut ids = models.ids();
    ids.sort();
    ids.iter()
        .filter_map(|id| models.get(id))
        .find(|model| model.primitives().choice)
}

pub(crate) fn screen_tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    lines[lines.len().saturating_sub(SCREEN_LINES)..].join("\n")
}

fn chosen_label(answer: DecisionAnswer) -> Result<(String, f64), String> {
    let DecisionAnswer::Choice {
        choice,
        probabilities,
        confidence,
    } = answer
    else {
        return Err("decision model answered a non-choice question".into());
    };
    if !CRITERIA.iter().any(|(label, _)| *label == choice) {
        return Err("decision model chose an unknown label".into());
    }
    let confidence = confidence
        .or_else(|| probabilities.get(&choice).copied())
        .filter(|c| c.is_finite() && (0.0..=1.0).contains(c))
        .ok_or("decision model gave no usable confidence")?;
    Ok((choice, confidence))
}

#[cfg(test)]
#[path = "tab_state_shadow/tests.rs"]
mod tests;
