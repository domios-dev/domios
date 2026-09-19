use serde::Serialize;

use crate::multiplexer::types::{Tab, TabBacking};
use crate::pty::screen_state::{screen_has_agent_tui, screen_has_bare_shell_prompt};
use crate::service::boot_stage::apply_boot_stage;
use crate::service::hierarchy::report_plan_ctx::tab_holds;
use crate::service::tab_repair::{RepairField, RepairFieldStatus};
use crate::session::lifecycle::AgentTypeWriteOrigin;
use codeterm_types::agent::role_kind;
use codeterm_types::pty::BootStage;

use super::provider_identity::{AgentTypeWriteOutcome, TabAgentTypeState};
use super::{AuthenticatedPlanCaller, ServiceContext};

pub(crate) const TAB_ADOPT_NOT_AUTHORIZED: &str = "tab_adopt_not_authorized";
pub(crate) const TAB_ADOPT_LIVE_PTY_REQUIRED: &str = "tab_adopt_live_pty_required";
pub(crate) const TAB_ADOPT_AGENT_TUI_REQUIRED: &str = "tab_adopt_agent_tui_required";
pub(crate) const TAB_ADOPT_UNKNOWN_PROVIDER: &str = "tab_adopt_unknown_provider";
pub(crate) const TAB_ADOPT_PROVIDER_REQUIRED: &str = "tab_adopt_provider_required";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabAdoptErrorKind {
    NotFound,
    NotAuthorized,
    LivePtyRequired,
    AgentTuiRequired,
    UnknownProvider,
    ProviderRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabAdoptError {
    pub(crate) kind: TabAdoptErrorKind,
    pub(crate) message: String,
}

impl TabAdoptError {
    pub(crate) fn new(kind: TabAdoptErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        match self.kind {
            TabAdoptErrorKind::NotFound => "tab_adopt_target_not_found",
            TabAdoptErrorKind::NotAuthorized => TAB_ADOPT_NOT_AUTHORIZED,
            TabAdoptErrorKind::LivePtyRequired => TAB_ADOPT_LIVE_PTY_REQUIRED,
            TabAdoptErrorKind::AgentTuiRequired => TAB_ADOPT_AGENT_TUI_REQUIRED,
            TabAdoptErrorKind::UnknownProvider => TAB_ADOPT_UNKNOWN_PROVIDER,
            TabAdoptErrorKind::ProviderRequired => TAB_ADOPT_PROVIDER_REQUIRED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TabAdoptResponse {
    pub(crate) ok: bool,
    pub(crate) tab_id: String,
    pub(crate) pty_id: String,
    pub(crate) provider: RepairField<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) caller_tab_id: Option<String>,
    pub(crate) force: bool,
}

pub(crate) fn screen_allows_adopt(
    screen: &str,
    force: bool,
    any_provider_tui: bool,
) -> Result<(), TabAdoptError> {
    if force {
        return Ok(());
    }
    let has_tui = screen_has_agent_tui(screen) || any_provider_tui;
    if has_tui && !screen_has_bare_shell_prompt(screen) {
        return Ok(());
    }
    Err(TabAdoptError::new(
        TabAdoptErrorKind::AgentTuiRequired,
        "refusing to mark a login shell as an agent; pass --force to adopt this live PTY anyway",
    ))
}

fn live_pty_id(tab: &Tab) -> Result<String, TabAdoptError> {
    if !matches!(&tab.backing, TabBacking::Pty) {
        return Err(TabAdoptError::new(
            TabAdoptErrorKind::LivePtyRequired,
            format!("Tab {} is not a live PTY tab", tab.id),
        ));
    }
    tab.pty_id.clone().ok_or_else(|| {
        TabAdoptError::new(
            TabAdoptErrorKind::LivePtyRequired,
            format!("Tab {} is not a live PTY tab", tab.id),
        )
    })
}

fn caller_proves_own_tab(tab: &Tab, caller: &AuthenticatedPlanCaller) -> bool {
    caller.tab_id.as_deref() == Some(tab.id.as_str())
        && caller.binding_nonce.is_some()
        && caller.binding_nonce == tab.session_binding_nonce
        && caller.binding_nonce == tab.reported_session_binding_nonce
}

fn live_task_plan(svc: &ServiceContext, tab: &Tab) -> Option<String> {
    let binding = tab.task_binding.as_ref()?;
    let plan_id = binding.plan_id.as_deref()?;
    let task_id = binding.task_id.as_deref()?;
    let plan = svc.plan_store.get(plan_id)?;
    let task = crate::plan::task_key::find_task(&plan.tasks, task_id)?;
    tab_holds(task, &tab.id).then(|| plan_id.to_string())
}

fn identity_states(
    outcome: AgentTypeWriteOutcome,
) -> Result<(TabAgentTypeState, TabAgentTypeState), TabAdoptError> {
    match outcome {
        AgentTypeWriteOutcome::Applied { previous, current } => Ok((previous, current)),
        AgentTypeWriteOutcome::Kept { current } => Ok((current.clone(), current)),
        AgentTypeWriteOutcome::EvidenceChanged { .. }
        | AgentTypeWriteOutcome::RejectedClaimedSession { .. } => Err(TabAdoptError::new(
            TabAdoptErrorKind::NotAuthorized,
            "the identity boundary refused the requested provider",
        )),
    }
}

impl ServiceContext {
    pub(crate) fn authorize_tab_adopt(
        &self,
        tab_id: &str,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<String, TabAdoptError> {
        let (target, caller_tab) = {
            let mux = self.mux.read();
            let target = mux.get_tab(tab_id).cloned().ok_or_else(|| {
                TabAdoptError::new(
                    TabAdoptErrorKind::NotFound,
                    format!("Tab {tab_id} was not found"),
                )
            })?;
            let caller_tab = caller
                .tab_id
                .as_deref()
                .and_then(|id| mux.get_tab(id).cloned());
            (target, caller_tab)
        };
        let pty_id = live_pty_id(&target)?;
        if caller.tab_id.is_none() {
            return Ok(pty_id);
        }
        let Some(caller_tab) = caller_tab else {
            return Err(not_authorized(tab_id));
        };
        if !caller_proves_own_tab(&caller_tab, caller) {
            return Err(not_authorized(tab_id));
        }
        if caller_tab.id == target.id {
            return Ok(pty_id);
        }
        if role_kind::anchors_team(caller_tab.role, caller_tab.role_profile.as_deref()) {
            return Ok(pty_id);
        }
        let Some(plan_id) = live_task_plan(self, &target) else {
            return Err(not_authorized(tab_id));
        };
        self.authorize_plan_orchestrator(&plan_id, caller)
            .map_err(|_| not_authorized(tab_id))?;
        Ok(pty_id)
    }

    pub(crate) fn adopt_live_pty_as_agent(
        &self,
        tab_id: &str,
        caller: &AuthenticatedPlanCaller,
        provider: &str,
        force: bool,
    ) -> Result<TabAdoptResponse, TabAdoptError> {
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(TabAdoptError::new(
                TabAdoptErrorKind::ProviderRequired,
                "provider is required; adopt never infers agent_type",
            ));
        }
        if self
            .agent_registry
            .with_provider(provider, |_| ())
            .is_none()
        {
            return Err(TabAdoptError::new(
                TabAdoptErrorKind::UnknownProvider,
                format!("provider '{provider}' is not a registered PTY agent"),
            ));
        }
        let pty_id = self.authorize_tab_adopt(tab_id, caller)?;
        let screen = self
            .screen_state
            .lock()
            .get_screen_text(&pty_id)
            .unwrap_or_default();
        let any_provider_tui = self.agent_registry.any_provider_screen_has_tui(&screen);
        screen_allows_adopt(&screen, force, any_provider_tui)?;
        let pty_before = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| tab.pty_id.clone())
                .ok_or_else(|| {
                    TabAdoptError::new(
                        TabAdoptErrorKind::LivePtyRequired,
                        format!("Tab {tab_id} is not a live PTY tab"),
                    )
                })?
        };
        if pty_before != pty_id {
            return Err(TabAdoptError::new(
                TabAdoptErrorKind::LivePtyRequired,
                format!("Tab {tab_id} is not a live PTY tab"),
            ));
        }
        let outcome = self
            .write_tab_agent_type_with_origin(
                tab_id,
                provider.to_string(),
                true,
                AgentTypeWriteOrigin::Adopt,
                caller.tab_id.as_deref(),
            )
            .ok_or_else(|| {
                TabAdoptError::new(
                    TabAdoptErrorKind::NotFound,
                    format!("Tab {tab_id} was not found"),
                )
            })?;
        {
            let mut mux = self.mux.write();
            let tab = mux.get_tab_mut(tab_id).ok_or_else(|| {
                TabAdoptError::new(
                    TabAdoptErrorKind::NotFound,
                    format!("Tab {tab_id} was not found"),
                )
            })?;
            if tab.pty_id.as_deref() != Some(pty_id.as_str()) {
                return Err(TabAdoptError::new(
                    TabAdoptErrorKind::LivePtyRequired,
                    format!("Tab {tab_id} is not a live PTY tab"),
                ));
            }
            apply_boot_stage(tab, BootStage::AgentReady, None);
        }
        let (previous, current) = identity_states(outcome)?;
        let status = if previous.provider == current.provider {
            RepairFieldStatus::Kept
        } else {
            RepairFieldStatus::Corrected
        };
        self.emit_state_changed();
        Ok(TabAdoptResponse {
            ok: true,
            tab_id: tab_id.to_string(),
            pty_id,
            provider: RepairField {
                status,
                previous: previous.provider,
                current: current.provider,
            },
            caller_tab_id: caller.tab_id.clone(),
            force,
        })
    }
}

fn not_authorized(tab_id: &str) -> TabAdoptError {
    TabAdoptError::new(
        TabAdoptErrorKind::NotAuthorized,
        format!("not authorized to adopt tab {tab_id}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::test_provider::MockProvider;
    use crate::api::tab_routes::{should_prepend_source, TabInputTransport};
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;

    fn tui_screen() -> &'static str {
        "╭─ agent ─╮\n│ ready   │\n╰─────────╯"
    }

    fn shell_screen() -> &'static str {
        "PS C:\\Users\\User> "
    }

    #[test]
    fn login_shell_is_refused_without_force() {
        let error = screen_allows_adopt(shell_screen(), false, false).unwrap_err();
        assert_eq!(error.code(), TAB_ADOPT_AGENT_TUI_REQUIRED);
    }

    #[test]
    fn force_adopts_a_raw_shell_screen() {
        assert!(screen_allows_adopt(shell_screen(), true, false).is_ok());
    }

    #[test]
    fn agent_tui_screen_is_accepted_without_force() {
        assert!(screen_allows_adopt(tui_screen(), false, false).is_ok());
    }

    #[test]
    fn empty_screen_is_refused_without_force() {
        let error = screen_allows_adopt("", false, false).unwrap_err();
        assert_eq!(error.code(), TAB_ADOPT_AGENT_TUI_REQUIRED);
    }

    fn live_shell_tab(mux: &mut MultiplexerState, tab_id: &str) {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.pty_id = Some("pty-live".into());
        tab.backing = TabBacking::Pty;
        tab.boot_stage = Some(BootStage::SpawnFailed);
        tab.agent_type = None;
    }

    fn proven_caller(tab_id: &str, nonce: &str) -> AuthenticatedPlanCaller {
        AuthenticatedPlanCaller {
            tab_id: Some(tab_id.into()),
            session_id: Some(format!("session-{tab_id}")),
            binding_nonce: Some(nonce.into()),
        }
    }

    fn prove_tab(mux: &mut MultiplexerState, tab_id: &str, nonce: &str) {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
        tab.agent_session_id = Some(format!("session-{tab_id}"));
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn owner_adopt_sets_identity_keeps_pty_and_unlocks_send() {
        let (mut mux, owner_id) = MultiplexerState::new();
        mux.set_orchestrator_role(&owner_id).unwrap();
        prove_tab(&mut mux, &owner_id, "owner-nonce");
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let target_id = mux.create_tab(&workspace_id).unwrap();
        live_shell_tab(&mut mux, &target_id);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(MockProvider::new("mock-pty")));
        fixture
            .service
            .screen_state
            .lock()
            .process("pty-live", tui_screen().as_bytes());
        let caller = proven_caller(&owner_id, "owner-nonce");

        let response = fixture
            .service
            .adopt_live_pty_as_agent(&target_id, &caller, "mock-pty", false)
            .unwrap();

        assert_eq!(response.pty_id, "pty-live");
        assert_eq!(response.provider.current.as_deref(), Some("mock-pty"));
        assert_eq!(response.provider.previous, None);
        assert_eq!(response.caller_tab_id.as_deref(), Some(owner_id.as_str()));
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&target_id).unwrap();
        assert_eq!(tab.pty_id.as_deref(), Some("pty-live"));
        assert_eq!(tab.agent_type.as_deref(), Some("mock-pty"));
        assert!(tab.agent_type_locked);
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
        drop(mux);
        assert!(fixture.service.tab_has_agent(&target_id));
        assert!(should_prepend_source(
            true,
            fixture.service.tab_has_agent(&target_id),
            TabInputTransport::Pty,
        ));
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn force_adopts_login_shell_and_human_owner_is_enough() {
        let (mut mux, tab_id) = MultiplexerState::new();
        live_shell_tab(&mut mux, &tab_id);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(MockProvider::new("mock-pty")));
        fixture
            .service
            .screen_state
            .lock()
            .process("pty-live", shell_screen().as_bytes());
        let human = AuthenticatedPlanCaller::default();

        let refused = fixture
            .service
            .adopt_live_pty_as_agent(&tab_id, &human, "mock-pty", false)
            .unwrap_err();
        assert_eq!(refused.code(), TAB_ADOPT_AGENT_TUI_REQUIRED);

        let adopted = fixture
            .service
            .adopt_live_pty_as_agent(&tab_id, &human, "mock-pty", true)
            .unwrap();
        assert!(adopted.force);
        assert_eq!(adopted.pty_id, "pty-live");
        assert!(fixture.service.tab_has_agent(&tab_id));
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn worker_cannot_adopt_and_unknown_provider_is_refused() {
        let (mut mux, owner_id) = MultiplexerState::new();
        mux.set_orchestrator_role(&owner_id).unwrap();
        prove_tab(&mut mux, &owner_id, "owner-nonce");
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let worker_id = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&worker_id, AgentRole::Worker, Some(&owner_id))
            .unwrap();
        prove_tab(&mut mux, &worker_id, "worker-nonce");
        let target_id = mux.create_tab(&workspace_id).unwrap();
        live_shell_tab(&mut mux, &target_id);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(MockProvider::new("mock-pty")));
        fixture
            .service
            .screen_state
            .lock()
            .process("pty-live", tui_screen().as_bytes());

        let worker = proven_caller(&worker_id, "worker-nonce");
        let denied = fixture
            .service
            .adopt_live_pty_as_agent(&target_id, &worker, "mock-pty", false)
            .unwrap_err();
        assert_eq!(denied.code(), TAB_ADOPT_NOT_AUTHORIZED);

        let human = AuthenticatedPlanCaller::default();
        let unknown = fixture
            .service
            .adopt_live_pty_as_agent(&target_id, &human, "not-a-provider", false)
            .unwrap_err();
        assert_eq!(unknown.code(), TAB_ADOPT_UNKNOWN_PROVIDER);
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&target_id)
            .unwrap()
            .agent_type
            .is_none());
    }
}
