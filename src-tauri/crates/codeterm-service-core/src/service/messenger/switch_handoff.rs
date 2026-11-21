use super::AgentInput;
use crate::multiplexer::types::{Tab, TabBacking, TaskBinding};
use crate::service::{agent_runtime::model::RuntimeFailure, agent_lifecycle::RestartBindingAuthority, ServiceContext};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::{atomic::{AtomicBool, AtomicU8, Ordering}, Arc, Weak}};
const CLAIMED: u8 = 1;
const RETIRED: u8 = 2;

pub struct SwitchLaunchAuthority {
    host: usize,
    expected: Mutex<Tab>,
    revoked: AtomicBool,
}

thread_local! {
    static LAUNCH: std::cell::RefCell<Option<Arc<SwitchLaunchAuthority>>> = const { std::cell::RefCell::new(None) };
}
tokio::task_local! { static ASYNC_LAUNCH: Option<Arc<SwitchLaunchAuthority>>; }

struct LaunchScope { previous: Option<Arc<SwitchLaunchAuthority>>, authority: Arc<SwitchLaunchAuthority>, preserve: bool }
impl Drop for LaunchScope {
    fn drop(&mut self) {
        if !self.preserve { self.authority.revoked.store(true, Ordering::Release); }
        LAUNCH.with(|slot| slot.replace(self.previous.take()));
    }
}

pub fn with_switch_launch<T>(svc: &ServiceContext, expected: &Tab, launch: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let authority = Arc::new(SwitchLaunchAuthority { host: Arc::as_ptr(&svc.mux) as usize,
        expected: Mutex::new(expected.clone()), revoked: AtomicBool::new(false) });
    authority.validate(svc)?;
    let previous = LAUNCH.with(|slot| slot.replace(Some(authority.clone())));
    let mut scope = LaunchScope { previous, authority, preserve: false };
    let result = launch();
    scope.preserve = result.is_ok() || result.as_ref().err().is_some_and(|reason| reason.starts_with("provider_switch_readiness_unconfirmed:"));
    result
}

pub fn launch_authority(svc: &ServiceContext, tab: &str) -> Option<Arc<SwitchLaunchAuthority>> {
    ASYNC_LAUNCH.try_with(Clone::clone).ok().flatten()
        .or_else(|| LAUNCH.with(|slot| slot.borrow().clone()))
        .filter(|authority| authority.host == Arc::as_ptr(&svc.mux) as usize && authority.expected.lock().id == tab)
}

pub(crate) async fn scope_launch<T>(authority: Option<Arc<SwitchLaunchAuthority>>, future: impl std::future::Future<Output = T>) -> T {
    ASYNC_LAUNCH.scope(authority, future).await
}

impl SwitchLaunchAuthority {
    pub(crate) fn snapshot(&self) -> Tab { self.expected.lock().clone() }
    pub(crate) fn reserved(&self, expected: Tab) { *self.expected.lock() = expected; }

    pub fn matches(&self, current: &Tab) -> bool {
        let expected = self.expected.lock();
        let same_backing = match (&expected.backing, &current.backing) {
            (TabBacking::ChatBackend { plugin_id: old, .. }, TabBacking::ChatBackend { plugin_id: new, .. }) => old == new,
            (TabBacking::Pty, TabBacking::Pty) => expected.pty_id.as_ref().is_none_or(|pty| current.pty_id.as_ref() == Some(pty)),
            _ => false,
        };
        let valid = !self.revoked.load(Ordering::Acquire) && same_context(&expected, current)
            && current.agent_type == expected.agent_type && current.session_binding_nonce == expected.session_binding_nonce
            && current.lifecycle_epoch == expected.lifecycle_epoch && current.lifecycle_operation_id == expected.lifecycle_operation_id
            && current.restorable_launch.account_id == expected.restorable_launch.account_id
            && current.restorable_launch.account_namespace == expected.restorable_launch.account_namespace
            && current.frozen == expected.frozen
            && current.close_restore.as_ref().is_some_and(|closed| closed.stopped) == expected.close_restore.as_ref().is_some_and(|closed| closed.stopped) && same_backing
            && (current.agent_session_id == expected.agent_session_id || expected.agent_session_id.is_none() && current.launch_session_authenticated());
        if !valid { self.revoked.store(true, Ordering::Release); }
        valid
    }

    pub(crate) fn validate(&self, svc: &ServiceContext) -> Result<(), String> {
        let id = self.expected.lock().id.clone();
        if !crate::crash::shutdown::is_shutting_down() && svc.mux.read().get_tab(&id).is_some_and(|current| self.matches(current)) {
            Ok(())
        } else { Err("provider_switch_superseded: admitted target launch authority changed".into()) }
    }
}

pub fn same_context(expected: &Tab, current: &Tab) -> bool {
    current.id == expected.id && current.team_id == expected.team_id
        && current.parent_tab_id == expected.parent_tab_id && current.role == expected.role
        && current.role_profile == expected.role_profile && current.task_binding == expected.task_binding
        && current.can_spawn == expected.can_spawn && current.depth == expected.depth
        && current.report_target_tab_id == expected.report_target_tab_id
        && current.report_target_mesh_peer == expected.report_target_mesh_peer
        && current.spawned_by_tab_id == expected.spawned_by_tab_id
        && current.ownership == expected.ownership && current.neighbor_authority == expected.neighbor_authority
        && current.cwd == expected.cwd && current.protected == expected.protected
}

pub fn same_binding(expected: &Tab, current: &Tab) -> bool {
    same_context(expected, current) && current.agent_type == expected.agent_type
        && current.agent_session_id == expected.agent_session_id
        && current.session_binding_nonce == expected.session_binding_nonce
        && current.reported_session_binding_nonce == expected.reported_session_binding_nonce
        && current.lifecycle_epoch == expected.lifecycle_epoch
        && current.lifecycle_operation_id == expected.lifecycle_operation_id
        && current.restorable_launch.account_id == expected.restorable_launch.account_id
        && current.restorable_launch.account_namespace == expected.restorable_launch.account_namespace
        && current.frozen == expected.frozen
        && current.close_restore.as_ref().is_some_and(|closed| closed.stopped) == expected.close_restore.as_ref().is_some_and(|closed| closed.stopped)
        && current.backing == expected.backing && current.pty_id == expected.pty_id
}

#[derive(Clone)]
pub struct SwitchSourceAuthority(Tab);

impl SwitchSourceAuthority {
    pub fn admitted(source: &Tab) -> Self { Self(source.clone()) }
    pub fn snapshot(&self) -> &Tab { &self.0 }
    pub fn matches(&self, current: &Tab) -> bool { same_binding(&self.0, current) }

    pub fn matches_retired(&self, current: &Tab) -> bool {
        let source = &self.0;
        same_context(source, current)
            && current.lifecycle_epoch == source.lifecycle_epoch && current.lifecycle_operation_id == source.lifecycle_operation_id
            && current.restorable_launch.account_id == source.restorable_launch.account_id
            && current.restorable_launch.account_namespace == source.restorable_launch.account_namespace
            && current.frozen == source.frozen
            && current.close_restore.as_ref().is_some_and(|closed| closed.stopped) == source.close_restore.as_ref().is_some_and(|closed| closed.stopped)
            && current.backing == source.backing
            && current.pty_id.as_ref().is_none_or(|pty| Some(pty) == source.pty_id.as_ref())
            && current.agent_type.as_ref().is_none_or(|provider| Some(provider) == source.agent_type.as_ref())
            && current.agent_session_id.as_ref().is_none_or(|session| Some(session) == source.agent_session_id.as_ref())
            && (current.session_binding_nonce == source.session_binding_nonce
                || current.session_binding_nonce.is_none() && source.session_binding_nonce.as_ref().is_some_and(|nonce| {
                    current.prior_session_binding_nonces.iter().any(|prior| &prior.nonce == nonce
                        && prior.session_id == source.agent_session_id)
                }))
            && current.reported_session_binding_nonce.as_ref().is_none_or(|nonce| Some(nonce) == source.session_binding_nonce.as_ref())
    }

    pub fn ready_target(&self, target: &Tab) -> Result<Arc<SwitchHandoffAuthority>, String> {
        if !same_context(&self.0, target) || !target.launch_session_authenticated()
            || target.boot_stage != Some(codeterm_types::pty::BootStage::AgentReady) || target.agent_booting
            || target.agent_session_id.is_none() || target.frozen {
            return Err("provider_switch_superseded: exact ready target authority unavailable".into());
        }
        Ok(Arc::new(SwitchHandoffAuthority {
            source: self.clone(), target: target.clone(), started: AtomicBool::new(false), claim_state: AtomicU8::new(0),
        }))
    }
}

// Native lifecycle authority is never deserialized or supplied by an installed plugin.
pub struct SwitchHandoffAuthority {
    source: SwitchSourceAuthority,
    target: Tab,
    started: AtomicBool,
    claim_state: AtomicU8,
}

impl SwitchHandoffAuthority {
    pub(crate) fn target(&self) -> &Tab { &self.target }
    pub fn was_claimed(&self) -> bool { self.claim_state.load(Ordering::Acquire) & CLAIMED != 0 }
    pub(crate) fn retire(&self) { self.claim_state.fetch_or(RETIRED, Ordering::AcqRel); }

    pub fn validate(&self, svc: &ServiceContext) -> Result<(), String> {
        let mux = svc.mux.read();
        let valid = self.claim_state.load(Ordering::Acquire) & RETIRED == 0
            && mux.get_tab(&self.target.id).is_some_and(|tab| self.matches_current(tab));
        if valid { Ok(()) } else { Err("provider_switch_superseded: handoff authority revoked before write claim".into()) }
    }

    fn matches_current(&self, tab: &Tab) -> bool {
        same_context(self.source.snapshot(), tab) && same_binding(&self.target, tab)
                && tab.launch_session_authenticated() && !tab.frozen && !tab.agent_booting
                && tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady)
    }

    pub(crate) fn claim(&self, svc: &ServiceContext) -> Result<(), RuntimeFailure> {
        let mux = svc.mux.read();
        if !mux.get_tab(&self.target.id).is_some_and(|tab| self.matches_current(tab)) {
            return Err(if self.was_claimed() { RuntimeFailure::WrittenAwaitingEvidence } else { RuntimeFailure::Superseded });
        }
        self.claim_state.fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| (state & RETIRED == 0).then_some(state | CLAIMED))
            .map_err(|state| if state & CLAIMED != 0 { RuntimeFailure::WrittenAwaitingEvidence } else { RuntimeFailure::Superseded })?;
        Ok(())
    }
}

#[derive(Default)]
struct Registry(Mutex<HashMap<String, Weak<SwitchHandoffAuthority>>>);
struct HandoffLifetime(Arc<SwitchHandoffAuthority>);
impl Drop for HandoffLifetime { fn drop(&mut self) { self.0.retire(); } }
struct Registration<'a> { service: &'a ServiceContext, send: String, authority: Arc<SwitchHandoffAuthority> }
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.authority.retire();
        let _ = self.service.ext_or_init::<Registry>(Registry::default).0.lock().remove(&self.send);
    }
}

pub(crate) fn for_send(svc: &ServiceContext, send: &str) -> Option<Arc<SwitchHandoffAuthority>> {
    svc.ext_or_init::<Registry>(Registry::default).0.lock().get(send).and_then(Weak::upgrade)
}

impl ServiceContext {
    fn send_switch_pty_handoff(
        &self,
        authority: std::sync::Arc<crate::service::messenger::switch_handoff::SwitchHandoffAuthority>,
        frame: crate::service::messenger::FramedDelivery,
    ) -> Result<(), String> {
        let target = authority.target();
        let _claim = self.pty_ownership.acquire(&target.id)?;
        authority.validate(self)?;
        let mut delivery = self.authenticated_pty_delivery(
            &target.id, target.pty_id.as_deref(),
            target.agent_session_id.as_deref().ok_or("provider_switch_handoff_invalid: session missing")?,
            target.session_binding_nonce.as_deref().ok_or("provider_switch_handoff_invalid: nonce missing")?,
        )?;
        let service = self.clone();
        delivery.permit = delivery.permit.with_prewrite_claim(std::sync::Arc::new(move || authority.claim(&service).is_ok()));
        self.send_authenticated_delivery_with_routed_frame(delivery, frame)
    }


    pub fn authorize_provider_switch_binding_from_source(
        &self,
        admitted: &crate::service::messenger::switch_handoff::SwitchSourceAuthority,
        target: &str,
    ) -> Result<Option<RestartBindingAuthority>, String> {
        let source = admitted.snapshot();
        let validate = |binding: Option<&TaskBinding>| {
            let mut expected = source.clone();
            if let Some(binding) = binding { expected.task_binding = Some(binding.clone()); }
            if self.mux.read().get_tab(&source.id).is_some_and(|current| {
                crate::service::messenger::switch_handoff::same_binding(&expected, current)
            }) { Ok(()) } else { Err("provider_switch_superseded: source authority changed during task carry".to_owned()) }
        };
        validate(None)?;
        let authority = self.authorize_provider_switch_binding_inner(
            &source.id, source.agent_type.as_deref().ok_or("provider_switch_binding_missing: provider unavailable")?,
            target, source.session_binding_nonce.as_deref().ok_or("provider_switch_binding_missing: source binding unavailable")?, Some(admitted))?;
        validate(authority.as_ref().map(RestartBindingAuthority::authorized_task_binding))?;
        Ok(authority)
    }


    pub fn send_provider_switch_handoff(&self, authority: Arc<SwitchHandoffAuthority>, input: &AgentInput) -> Result<(), String> {
        if authority.started.swap(true, Ordering::AcqRel) {
            return Err("provider_switch_handoff_claimed: payload cannot be replayed".into());
        }
        let _lifetime = HandoffLifetime(authority.clone());
        authority.validate(self)?;
        let AgentInput::Framed(frame) = input else { return Err("provider_switch_handoff_invalid: host frame required".into()) };
        if matches!(authority.target.backing, TabBacking::Pty) {
            return self.send_switch_pty_handoff(authority, frame.clone());
        }
        let send = {
            let mut registry = self.ext_or_init::<Registry>(Registry::default).0.lock();
            registry.retain(|_, authority| authority.strong_count() > 0);
            if registry.len() >= 128 { return Err("provider_switch_handoff_capacity: active authority limit".into()); }
            let pending = self.chat_send_ledger.try_start_scoped_with_normalization(
                &authority.target.id, &input.text(), None, None, Default::default())?;
            let _ = self.chat_send_ledger.attach_system_send(&pending.send_id, &input.text(), chrono::Utc::now().timestamp_millis().max(0) as u64);
            registry.insert(pending.send_id.clone(), Arc::downgrade(&authority));
            pending.send_id
        };
        let _registration = Registration { service: self, send: send.clone(), authority: authority.clone() };
        let result = crate::service::agent_runtime::transport::send(self, &authority.target.id, input, Some(&send));
        if let Err(error) = &result {
            let _ = self.chat_send_ledger.finish(&send, Err(error.ledger_message()));
        }
        result.map_err(|error| error.ledger_message())
    }
}
