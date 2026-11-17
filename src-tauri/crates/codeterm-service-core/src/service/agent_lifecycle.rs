//! Agent lifecycle state and types the core names; the lifecycle itself lives in
//! `codeterm-agent-lifecycle` and the core reaches it through `ports::lifecycle`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

use crate::pty::screen_state::ScreenChangeSignal;

pub mod account_restart;
pub mod model_request;
mod model_switch;
pub mod sleep_transition;
mod staged_completion;
pub mod staged_switches;
pub mod timing;
pub mod worker_binding;
pub use account_restart::{AccountRestartBinding, AccountRestartTarget};

pub use model_switch::{
    ControlChange, ModelSwitchConfirmation, ModelSwitchError, ModelSwitchEvidence,
    ModelSwitchOutcome,
};

/// Minimum seconds between consecutive restarts for a single tab.
pub const RESTART_RATE_LIMIT_SECS: u64 = 30;

/// Default grace period before tearing down the old PTY.
pub const DEFAULT_GRACE_SECS: u64 = 10;

/// Failure deadline for the new PTY to reach AgentReady.
pub const RESTART_READY_TIMEOUT_SECS: u64 = 45;

/// Tracks restart attempts and serializes in-flight recovery per tab.
/// Lives on `ServiceContext` and survives across requests.
#[derive(Default)]
pub struct RestartTrackerState {
    sleep_transitions: HashMap<String, Arc<sleep_transition::SleepTransition>>,
    pub staged_models: staged_switches::PendingSwitches,
    pub last_restart: HashMap<String, Instant>,
    pub in_progress: HashSet<String>,
    pub command_ptys: HashMap<String, (String, String)>,
    pub switch_confirmations: HashMap<String, Arc<crate::pty::screen_state::ScreenChangeSignal>>,
    pub pending_command_observers:
        HashMap<String, (String, Arc<crate::pty::screen_state::ScreenChangeSignal>)>,
}

pub type RestartTracker = Arc<Mutex<RestartTrackerState>>;

pub fn new_restart_tracker() -> RestartTracker {
    Arc::new(Mutex::new(RestartTrackerState::default()))
}

#[derive(Debug, Clone)]
pub enum AgentRestartError {
    BackgroundWork { items: Vec<String> },
    RuntimeProviderHandoffUnsupported,
    NotFound(String),
    BadRequest(String),
    RateLimited { retry_after_secs: u64 },
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct RestartOutcome {
    pub tab_id: String,
    pub provider: String,
    pub session_id: Option<String>,
    pub session_was_stale: bool,
    pub force: bool,
    pub auto_approve: bool,
    pub restart_accepted: bool,
    pub old_pty_torn_down: bool,
    pub agent_ready: bool,
    /// The tab's plan/task caller binding was re-established against its own
    /// authoritative session. Always settled before `agent_ready` is returned.
    pub binding_restored: bool,
    /// An exited provider was relaunched in its own shell PTY; nothing was torn down.
    pub recovered_in_place: bool,
    pub old_pty_id: Option<String>,
    pub new_pty_id: Option<String>,
}

impl RestartOutcome {
    pub fn accepted(snap: &TabRestartSnapshot, session_was_stale: bool, force: bool) -> Self {
        let provider = snap.target_provider.as_deref().unwrap_or(&snap.provider);
        let provider_changed = provider != snap.provider;
        Self {
            tab_id: snap.tab_id.clone(),
            provider: provider.to_string(),
            session_id: (!provider_changed)
                .then(|| snap.session_id.clone())
                .flatten(),
            session_was_stale,
            force,
            auto_approve: snap.auto_approve,
            restart_accepted: true,
            old_pty_torn_down: false,
            agent_ready: false,
            binding_restored: false,
            recovered_in_place: false,
            old_pty_id: (!snap.pty_id.is_empty()).then(|| snap.pty_id.clone()),
            new_pty_id: None,
        }
    }

    /// Reports whether the restart resumed a session or launched without one.
    pub fn session_action(&self) -> &'static str {
        match (&self.session_id, self.session_was_stale) {
            (Some(_), false) => "resumed",
            _ => "fresh_launch",
        }
    }
}

/// Tab state captured before any restart leg mutates it.
#[derive(Debug, Clone)]
pub struct TabRestartSnapshot {
    pub tab_id: String,
    pub pty_id: String,
    pub provider: String,
    pub target_provider: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub auto_approve: bool,
    pub role: Option<String>,
    pub role_profile: Option<String>,
    pub handoff_history: Option<String>,
}

#[derive(Debug)]
pub enum RestartAttemptError {
    Request(AgentRestartError),
    Failed {
        outcome: RestartOutcome,
        reason: String,
        timed_out: bool,
    },
}

impl From<AgentRestartError> for RestartAttemptError {
    fn from(error: AgentRestartError) -> Self {
        Self::Request(error)
    }
}

/// Build the shell command that launches the agent inside the replacement PTY.
/// If the recorded session id is still valid, prefer the provider's resume
/// command. Otherwise fall back to a fresh launch — the spec calls for graceful
/// degradation on stale ids, not failure.

pub fn screen_shows_agent_tui(
    registry: &crate::agent::registry::AgentRegistry,
    screen_text: &str,
) -> bool {
    crate::pty::screen_state::screen_has_agent_tui(screen_text)
        || registry.any_provider_screen_has_tui(screen_text)
}

#[derive(Default)]
pub struct RestartOptions {
    pub account_restart: Option<AccountRestartTarget>,
    pub force: bool,
    pub grace_secs: u64,
    pub target_provider: Option<String>,
    pub handoff_history: bool,
    pub fresh: bool,
    /// An explicit request relaunches a runtime whose last start the provider refused;
    /// automatic restarts leave that refusal blocking.
    pub retry_refused_start: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerInterruptionEvent {
    ProviderExhausted,
    ProcessExited,
    ForcedContextReset,
}

impl WorkerInterruptionEvent {
    pub fn reason(self) -> &'static str {
        match self {
            Self::ProviderExhausted => "provider_exhausted",
            Self::ProcessExited => "process_exited",
            Self::ForcedContextReset => "forced_context_reset",
        }
    }
}

impl crate::service::ServiceContext {
    /// The installed agent lifecycle crate; see `ports::lifecycle`.
    pub fn lifecycle_port(&self) -> &Arc<dyn crate::ports::lifecycle::AgentLifecyclePort> {
        crate::ports::lifecycle::installed(&self.extensions)
    }

    pub fn model_command_owns_prompt(&self, tab_id: &str) -> bool {
        let owner = self
            .ext::<RestartTracker>()
            .lock()
            .command_ptys
            .get(tab_id)
            .cloned();
        self.mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| command_prompt_owner_matches(owner.as_ref(), tab))
    }
    pub fn replace_switch_confirmation(&self, pty: &str, signal: Option<Arc<ScreenChangeSignal>>) {
        let mut tracker = self.ext::<RestartTracker>().lock();
        if let Some(old) = tracker.switch_confirmations.remove(pty) {
            old.mark_changed();
        }
        if let Some(signal) = signal {
            tracker.switch_confirmations.insert(pty.to_owned(), signal);
        }
    }

    pub fn staged_model_turn_started(&self, tab_id: &str) {
        self.ext::<RestartTracker>()
            .lock()
            .staged_models
            .signal_started(tab_id);
    }
    pub fn apply_staged_model_switch_on_idle(&self, tab_id: &str) {
        self.ext::<RestartTracker>()
            .lock()
            .staged_models
            .signal_idle(tab_id);
    }
}

pub fn command_prompt_owner_matches(
    owner: Option<&(String, String)>,
    tab: &crate::multiplexer::types::Tab,
) -> bool {
    owner.is_some_and(|(pty_id, nonce)| {
        tab.pty_id.as_deref() == Some(pty_id)
            && tab.session_binding_nonce.as_deref() == Some(nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
    })
}

/// Only a plan task binding carries an attempt across restart; a launch-only spawn handle does not.
pub fn restart_carries_task_attempt(
    binding: Option<&crate::multiplexer::types::TaskBinding>,
) -> bool {
    binding.is_some_and(|binding| binding.task_id.is_some())
}
