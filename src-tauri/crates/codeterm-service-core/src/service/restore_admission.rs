use crate::multiplexer::commands::MuxState;
use crate::service::ServiceContext;
use parking_lot::{Condvar, Mutex};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Priority {
    Focused,
    Visible,
    Background,
}

impl Priority {
    pub fn from_visibility(
        focused: bool,
        active_workspace: bool,
        shown: bool,
        hidden: bool,
    ) -> Self {
        if focused {
            Self::Focused
        } else if active_workspace && shown && !hidden {
            Self::Visible
        } else {
            Self::Background
        }
    }
}

pub fn priority(mux: &crate::multiplexer::state::MultiplexerState, tab_id: &str) -> Priority {
    let tab = mux.get_tab(tab_id);
    Priority::from_visibility(
        mux.get_focused_tab_id() == Some(tab_id),
        mux.get_active_workspace()
            .is_some_and(|workspace| workspace.tabs.iter().any(|id| id == tab_id)),
        tab.is_some_and(|tab| tab.show_in_tabbar),
        tab.is_none_or(|tab| tab.hidden),
    )
}

impl Priority {
    fn label(self) -> &'static str {
        match self {
            Self::Focused => "focused",
            Self::Visible => "visible",
            Self::Background => "background",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionPermit {
    Reserved,
    Shared,
    Ungated,
}

impl AdmissionPermit {
    fn label(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Shared => "shared",
            Self::Ungated => "ungated",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Admission {
    pub priority: Priority,
    pub permit: AdmissionPermit,
}

impl Admission {
    pub(crate) fn ungated(service: &ServiceContext, tab_id: &str) -> Self {
        Self {
            priority: priority(&service.mux.read(), tab_id),
            permit: AdmissionPermit::Ungated,
        }
    }

    pub fn mark(self, tab_id: &str, pty_id: Option<&str>, phase: &'static str) {
        crate::startup::boot_timing::mark_admission(
            tab_id,
            pty_id,
            phase,
            self.priority.label(),
            self.permit.label(),
        );
    }
}

#[derive(Default)]
pub struct Changes {
    revision: Mutex<u64>,
    changed: Condvar,
}

impl Changes {
    pub fn revision(&self) -> u64 {
        *self.revision.lock()
    }

    pub fn notify(&self) {
        *self.revision.lock() += 1;
        self.changed.notify_all();
    }

    pub fn wait(&self, observed: u64) {
        let mut revision = self.revision.lock();
        while *revision == observed {
            self.changed.wait(&mut revision);
        }
    }
}

pub fn changes() -> &'static Changes {
    static CHANGES: OnceLock<Changes> = OnceLock::new();
    CHANGES.get_or_init(Changes::default)
}

pub fn notify_changed() {
    changes().notify();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Permit {
    Foreground,
    Shared,
}

#[derive(Default)]
struct Permits {
    foreground: bool,
    shared: usize,
    shared_domains: HashSet<ProviderDomain>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ProviderDomain {
    mux: usize,
    provider: String,
}

#[derive(Clone)]
struct Candidate {
    priority: Priority,
    domain: Option<ProviderDomain>,
}

impl Permits {
    fn available(&self, priority: Priority, domain: Option<&ProviderDomain>) -> Option<Permit> {
        if priority != Priority::Background && !self.foreground {
            Some(Permit::Foreground)
        } else if self.shared < 3
            && domain.is_none_or(|domain| !self.shared_domains.contains(domain))
        {
            Some(Permit::Shared)
        } else {
            None
        }
    }

    fn take(&mut self, priority: Priority, domain: Option<&ProviderDomain>) -> Option<Permit> {
        let permit = self.available(priority, domain)?;
        match permit {
            Permit::Foreground => self.foreground = true,
            Permit::Shared => {
                self.shared += 1;
                self.shared_domains.extend(domain.cloned());
            }
        }
        Some(permit)
    }

    fn release(&mut self, permit: Permit, domain: Option<&ProviderDomain>) {
        match permit {
            Permit::Foreground => self.foreground = false,
            Permit::Shared => {
                self.shared -= 1;
                if let Some(domain) = domain {
                    self.shared_domains.remove(domain);
                }
            }
        }
    }

    fn next(&self, candidates: &[Candidate]) -> Option<usize> {
        candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                self.available(candidate.priority, candidate.domain.as_ref())
                    .is_some()
            })
            .min_by_key(|(_, candidate)| candidate.priority)
            .map(|(index, _)| index)
    }
}

struct Waiting {
    ticket: u64,
    mux: MuxState,
    tab_id: String,
}

type PreparationKey = (usize, String);

#[derive(Default)]
struct PreparationState {
    next_ticket: u64,
    waiting: Vec<Waiting>,
    active: HashSet<PreparationKey>,
    permits: Permits,
}

impl PreparationState {
    fn candidates(&self) -> Vec<Candidate> {
        let mut candidates = vec![
            Candidate {
                priority: Priority::Background,
                domain: None
            };
            self.waiting.len()
        ];
        let mut classified = HashSet::new();
        for waiting in &self.waiting {
            let instance = Arc::as_ptr(&waiting.mux) as usize;
            if !classified.insert(instance) {
                continue;
            }
            let mux = waiting.mux.read();
            for (index, waiting) in self.waiting.iter().enumerate() {
                if Arc::as_ptr(&waiting.mux) as usize == instance {
                    candidates[index] = Candidate {
                        priority: priority(&mux, &waiting.tab_id),
                        domain: mux
                            .get_tab(&waiting.tab_id)
                            .and_then(|tab| tab.agent_type.clone())
                            .map(|provider| ProviderDomain {
                                mux: instance,
                                provider,
                            }),
                    };
                }
            }
        }
        candidates
    }
}

#[derive(Default)]
struct PreparationGate {
    state: Mutex<PreparationState>,
}

// Never enqueue or drop a PreparationTicket under a mux guard: gate state precedes mux.
struct PreparationTicket<'a> {
    gate: &'a PreparationGate,
    ticket: u64,
    key: PreparationKey,
    permit: Option<Permit>,
    domain: Option<ProviderDomain>,
}

impl PreparationGate {
    fn enqueue(&self, mux: &MuxState, tab_id: &str) -> Option<PreparationTicket<'_>> {
        let key = (Arc::as_ptr(mux) as usize, tab_id.to_owned());
        let mut state = self.state.lock();
        if !state.active.insert(key.clone()) {
            return None;
        }
        let ticket = state.next_ticket;
        state.next_ticket += 1;
        state.waiting.push(Waiting {
            ticket,
            mux: mux.clone(),
            tab_id: tab_id.to_owned(),
        });
        drop(state);
        notify_changed();
        Some(PreparationTicket {
            gate: self,
            ticket,
            key,
            permit: None,
            domain: None,
        })
    }
}

impl PreparationTicket<'_> {
    fn acquire(&mut self) -> Admission {
        loop {
            let observed = changes().revision();
            let mut state = self.gate.state.lock();
            let candidates = state.candidates();
            let selected = state
                .permits
                .next(&candidates)
                .filter(|index| state.waiting[*index].ticket == self.ticket);
            if let Some((index, permit)) = selected.and_then(|index| {
                state
                    .permits
                    .take(
                        candidates[index].priority,
                        candidates[index].domain.as_ref(),
                    )
                    .map(|permit| (index, permit))
            }) {
                self.permit = Some(permit);
                self.domain = candidates[index].domain.clone();
                state.waiting.remove(index);
                drop(state);
                notify_changed();
                return Admission {
                    priority: candidates[index].priority,
                    permit: match permit {
                        Permit::Foreground => AdmissionPermit::Reserved,
                        Permit::Shared => AdmissionPermit::Shared,
                    },
                };
            }
            drop(state);
            changes().wait(observed);
        }
    }
}

impl Drop for PreparationTicket<'_> {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock();
        if let Some(permit) = self.permit {
            state.permits.release(permit, self.domain.as_ref());
        } else {
            state
                .waiting
                .retain(|waiting| waiting.ticket != self.ticket);
        }
        state.active.remove(&self.key);
        drop(state);
        notify_changed();
    }
}

pub fn admit_restored_pty(
    service: &ServiceContext,
    tab_id: &str,
    admission: Admission,
) -> Result<(), String> {
    let resolved = service
        .mux
        .read()
        .resolve_tab_id(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found or has no PTY"))?;
    if let Some(refusal) = service.tab_write_lock(&resolved) {
        return Err(refusal.to_string());
    }
    if service.tab_chat_backing(&resolved).is_some() {
        return Ok(());
    }
    if let Some(refusal) = service.tab_surface_input_refusal(&resolved) {
        return Err(refusal);
    }
    let pty_id = {
        crate::startup::boot_timing::mark(&resolved, None, "pty_claim_enter");
        let claim = service.pty_ownership.acquire(&resolved);
        crate::startup::boot_timing::mark(&resolved, None, "pty_claim_acquired");
        if service.direct_restored_pty_pending(&resolved) {
            None
        } else {
            service.ensure_tab_pty_linked_with_claim(&claim, Some(admission))?;
            Some(service.resolve_pty_id(&resolved)?)
        }
    };
    schedule_preparation(service, resolved, pty_id);
    Ok(())
}

fn preparation_matches(
    restored: bool,
    current_pty: Option<&str>,
    admitted_pty: Option<&str>,
) -> bool {
    restored && current_pty == admitted_pty
}

fn schedule_preparation(
    service: &ServiceContext,
    resolved: String,
    pty_id: Option<String>,
) -> Option<std::thread::JoinHandle<()>> {
    static PREPARATION: OnceLock<PreparationGate> = OnceLock::new();
    let Some(mut ticket) = PREPARATION
        .get_or_init(PreparationGate::default)
        .enqueue(&service.mux, &resolved)
    else {
        return None;
    };
    crate::startup::boot_timing::mark(&resolved, pty_id.as_deref(), "prepare_queued");
    let service = service.clone();
    Some(codeterm_util::named_thread::spawn(
        "restore-prepare",
        move || {
            let admission = ticket.acquire();
            admission.mark(&resolved, pty_id.as_deref(), "prepare_admitted");
            let claim = service.pty_ownership.acquire(&resolved);
            let matches = service.mux.read().get_tab(&resolved).is_some_and(|tab| {
                preparation_matches(
                    tab.restored_from_snapshot && !tab.agent_booting,
                    tab.pty_id.as_deref(),
                    pty_id.as_deref(),
                )
            });
            if !matches {
                admission.mark(&resolved, pty_id.as_deref(), "prepare_cancelled");
                return;
            }
            let result = service.materialize_restored_agent_with_admission(&claim, Some(admission));
            if let Err(reason) = &result {
                if let Some(tab) = service.mux.write().get_tab_mut(&resolved) {
                    tab.agent_booting = false;
                }
                service.emit_boot_stage(
                    &resolved,
                    crate::service::boot_stage::BootStage::SpawnFailed,
                    Some(reason),
                );
                log::error!("[startup-pty] tab={resolved} materialization failed: {reason}");
            }
            crate::startup::boot_timing::mark(&resolved, pty_id.as_deref(), "prepare_completed");
            crate::startup::boot_timing::mark(&resolved, pty_id.as_deref(), "materialized");
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unkeyed(priorities: &[Priority]) -> Vec<Candidate> {
        priorities
            .iter()
            .map(|priority| Candidate {
                priority: *priority,
                domain: None,
            })
            .collect()
    }

    #[test]
    fn preparation_guard_requires_the_observed_host_or_an_unhosted_restore() {
        assert!(preparation_matches(true, None, None));
        assert!(!preparation_matches(false, None, None));
        assert!(!preparation_matches(true, Some("new"), None));
        assert!(preparation_matches(true, Some("shell"), Some("shell")));
        assert!(!preparation_matches(false, Some("shell"), Some("shell")));
        assert!(!preparation_matches(true, Some("new"), Some("shell")));
        assert!(!preparation_matches(true, None, Some("shell")));
    }

    #[test]
    fn preparation_background_never_consumes_reserved_permit() {
        let mut permits = Permits::default();
        for _ in 0..3 {
            assert_eq!(
                permits.take(Priority::Background, None),
                Some(Permit::Shared)
            );
        }
        assert_eq!(permits.take(Priority::Background, None), None);
        assert_eq!(
            permits.take(Priority::Focused, None),
            Some(Permit::Foreground)
        );
        assert_eq!(permits.take(Priority::Visible, None), None);
        permits.release(Permit::Foreground, None);
        assert_eq!(
            permits.take(Priority::Visible, None),
            Some(Permit::Foreground)
        );
    }

    #[test]
    fn preparation_selects_live_focus_before_visible_and_background() {
        let permits = Permits::default();
        assert_eq!(
            permits.next(&unkeyed(&[
                Priority::Background,
                Priority::Visible,
                Priority::Focused
            ])),
            Some(2)
        );
        assert_eq!(
            permits.next(&unkeyed(&[
                Priority::Focused,
                Priority::Visible,
                Priority::Background
            ])),
            Some(0)
        );
    }

    #[test]
    fn preparation_completion_releases_only_the_owned_permit() {
        let mut permits = Permits::default();
        let shared = permits.take(Priority::Background, None).unwrap();
        let foreground = permits.take(Priority::Focused, None).unwrap();
        permits.release(shared, None);
        assert_eq!(permits.shared, 0);
        assert!(permits.foreground);
        permits.release(foreground, None);
        assert!(!permits.foreground);
    }

    struct RestoreProvider {
        inner: crate::agent::test_provider::MockProvider,
        calls: Arc<Mutex<Vec<&'static str>>>,
        direct: bool,
        spec: bool,
        resumable: bool,
    }

    impl crate::agent::provider::AgentProvider for RestoreProvider {
        fn id(&self) -> &str {
            self.inner.id()
        }
        fn display_name(&self) -> &str {
            self.inner.display_name()
        }
        fn build_launch_command(
            &self,
            params: &crate::agent::provider::AgentLaunchParams,
        ) -> String {
            self.inner.build_launch_command(params)
        }
        fn declares_launch_spec(&self) -> bool {
            self.direct
        }
        fn launch_spec(
            &self,
            _: &crate::agent::provider::LaunchSpecParams<'_>,
        ) -> Option<codeterm_types::agent::LaunchSpec> {
            self.calls.lock().push("launch_spec");
            self.spec.then(|| codeterm_types::agent::LaunchSpec {
                program: "restore-spy".into(),
                ..Default::default()
            })
        }
        fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
            self.inner.readiness_strategy()
        }
        fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
            self.inner.task_delivery()
        }
        fn detect_from_title(&self, title: &str) -> bool {
            self.inner.detect_from_title(title)
        }
        fn detect_from_output(&self, text: &str) -> bool {
            self.inner.detect_from_output(text)
        }
        fn session_file_path(&self, cwd: &str, session: &str) -> Option<std::path::PathBuf> {
            self.inner.session_file_path(cwd, session)
        }
        fn detect_session_id(
            &self,
            _: &str,
            _: &[String],
            _: Option<std::time::Duration>,
        ) -> Option<String> {
            None
        }
        fn session_exists(&self, _: &str, _: &str) -> Option<bool> {
            self.calls.lock().push("session_exists");
            Some(true)
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            self.calls.lock().push("build_resume_command");
            self.resumable
                .then(|| "restore-spy --resume own-session".into())
        }
    }

    #[test]
    fn pty_admission_defers_provider_calls_and_preparation_preserves_their_order() {
        use crate::agent::test_provider::MockProvider;
        use crate::multiplexer::state::MultiplexerState;
        use crate::service::test_fixture::ServiceFixture;

        let mut observed = Vec::new();
        for deferred in [false, true] {
            let (mut mux, tab_id) = MultiplexerState::new();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.protected = false;
            tab.agent_type = Some("restore-spy".into());
            tab.agent_session_id = Some("own-session".into());
            tab.restored_from_snapshot = true;
            tab.restorable_launch.captured = true;
            let fixture = ServiceFixture::new(mux);
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&tab_id)
                .unwrap()
                .cwd = Some(fixture.temp.path().to_string_lossy().into_owned());
            let calls = Arc::new(Mutex::new(Vec::new()));
            fixture
                .service
                .agent_registry
                .register_provider(Box::new(RestoreProvider {
                    inner: MockProvider::with_session("restore-spy", "own-session"),
                    calls: calls.clone(),
                    direct: false,
                    spec: false,
                    resumable: false,
                }));
            if deferred {
                let claim = fixture.service.pty_ownership.acquire(&tab_id);
                fixture
                    .service
                    .ensure_tab_pty_linked_with_claim(&claim, None)
                    .unwrap();
                assert!(calls.lock().is_empty());
                let pty_id = fixture.service.resolve_pty_id(&tab_id).unwrap();
                drop(claim);
                schedule_preparation(&fixture.service, tab_id.clone(), Some(pty_id))
                    .unwrap()
                    .join()
                    .unwrap();
            } else {
                assert!(fixture.service.ensure_tab_pty_for_send(&tab_id).is_err());
            }
            let pty_id = fixture.service.resolve_pty_id(&tab_id).unwrap();
            let stage = fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .boot_stage;
            fixture.service.pty.lock().kill(&pty_id).unwrap();
            observed.push(calls.lock().clone());
            assert_eq!(
                stage,
                Some(crate::service::boot_stage::BootStage::SpawnFailed)
            );
        }
        assert_eq!(observed[0], ["session_exists", "build_resume_command"]);
        assert_eq!(observed[1], observed[0]);
    }

    #[test]
    fn direct_admission_defers_provider_calls_and_missing_spec_builds_a_shell_in_preparation() {
        use crate::agent::test_provider::MockProvider;
        use crate::multiplexer::state::MultiplexerState;
        use crate::service::test_fixture::ServiceFixture;
        for spec in [false, true] {
            let (mut mux, tab_id) = MultiplexerState::new();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.protected = false;
            tab.agent_type = Some("restore-spy".into());
            tab.agent_session_id = Some("own-session".into());
            tab.restored_from_snapshot = true;
            let fixture = ServiceFixture::new(mux);
            let calls = Arc::new(Mutex::new(Vec::new()));
            fixture
                .service
                .agent_registry
                .register_provider(Box::new(RestoreProvider {
                    inner: MockProvider::with_session("restore-spy", "own-session"),
                    calls: calls.clone(),
                    direct: true,
                    spec,
                    resumable: true,
                }));
            let claim = fixture.service.pty_ownership.acquire(&tab_id);
            assert!(fixture.service.direct_restored_pty_pending(&tab_id));
            assert!(calls.lock().is_empty());
            assert!(fixture.service.resolve_pty_id(&tab_id).is_err());
            let service = fixture.service.clone();
            let id = tab_id.clone();
            let prepared = std::thread::spawn(move || {
                let input = crate::service::restored_launch::restored_fresh_launch_input(
                    &service.agent_registry,
                    crate::service::SpawnAgentParams {
                        agent: Some("restore-spy".into()),
                        resume_session_id: Some("own-session".into()),
                        ..Default::default()
                    },
                    crate::multiplexer::types::RestorableLaunchParameters {
                        captured: true,
                        ..Default::default()
                    },
                );
                let command = input.prepared_command(&service, &id).unwrap();
                service
                    .ensure_restored_launch_host_with_claim(&claim, &command, None)
                    .unwrap();
                command
            })
            .join()
            .unwrap();
            assert_eq!(prepared.direct.is_some(), spec);
            let pty = fixture.service.resolve_pty_id(&tab_id).ok();
            assert_eq!(pty.is_none(), spec);
            assert!(calls.lock().contains(&"launch_spec"));
            if spec {
                let prior_calls = calls.lock().clone();
                {
                    let mut mux = fixture.service.mux.write();
                    let tab = mux.get_tab_mut(&tab_id).unwrap();
                    tab.restored_from_snapshot = false;
                    tab.agent_booting = true;
                }
                fixture.service.ensure_tab_pty_for_send(&tab_id).unwrap();
                assert!(fixture.service.resolve_pty_id(&tab_id).is_err());
                assert_eq!(*calls.lock(), prior_calls);
            }
            if let Some(pty) = pty {
                fixture.service.pty.lock().kill(&pty).unwrap();
            }
        }
    }

    #[test]
    fn preparation_cancels_when_a_host_appears_after_direct_admission() {
        let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.restored_from_snapshot = true;
        tab.pty_id = Some("new-owner".into());
        let fixture = crate::service::test_fixture::ServiceFixture::new(state);
        schedule_preparation(&fixture.service, tab_id.clone(), None)
            .unwrap()
            .join()
            .unwrap();
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert!(tab.restored_from_snapshot);
        assert!(!tab.agent_booting);
        assert_eq!(tab.pty_id.as_deref(), Some("new-owner"));
    }

    #[test]
    fn preparation_queue_releases_cancelled_tickets_and_deduplicates_tab_work() {
        let (state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let mux = Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
            "prepare-test",
            state,
        ));
        let gate = PreparationGate::default();
        let ticket = gate.enqueue(&mux, &tab_id).unwrap();
        assert!(gate.enqueue(&mux, &tab_id).is_none());
        drop(ticket);
        assert!(gate.enqueue(&mux, &tab_id).is_some());
        assert!(gate.state.lock().active.is_empty());
    }

    #[test]
    fn preparation_reserved_permit_bypasses_waiting_background_work() {
        let (mut state, focused) = crate::multiplexer::state::MultiplexerState::new();
        let workspace = state.get_active_workspace().unwrap().id.clone();
        let background = (0..4)
            .map(|_| {
                let id = state.create_tab(&workspace).unwrap();
                state.get_tab_mut(&id).unwrap().hidden = true;
                id
            })
            .collect::<Vec<_>>();
        state.set_focused_tab(&focused).unwrap();
        let mux = Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
            "prepare-test",
            state,
        ));
        let gate = PreparationGate::default();
        let mut held = Vec::new();
        for tab_id in &background[..3] {
            let mut ticket = gate.enqueue(&mux, tab_id).unwrap();
            ticket.acquire();
            held.push(ticket);
        }
        let waiting = gate.enqueue(&mux, &background[3]).unwrap();
        let mut foreground = gate.enqueue(&mux, &focused).unwrap();
        foreground.acquire();
        assert_eq!(foreground.permit, Some(Permit::Foreground));
        assert_eq!(gate.state.lock().waiting.len(), 1);
        drop(foreground);
        drop(waiting);
        drop(held);
        let state = gate.state.lock();
        assert_eq!(state.permits.shared, 0);
        assert!(!state.permits.foreground);
        assert!(state.active.is_empty());
    }

    #[test]
    fn shared_preparation_permits_serve_distinct_vm_domains_without_queuing_on_one_vm() {
        let domains = ["first", "second", "third"].map(|provider| ProviderDomain {
            mux: 0,
            provider: provider.into(),
        });
        let mut permits = Permits::default();
        assert_eq!(
            permits.take(Priority::Background, Some(&domains[0])),
            Some(Permit::Shared)
        );
        assert_eq!(permits.take(Priority::Background, Some(&domains[0])), None);
        assert_eq!(permits.shared, 1);
        for domain in &domains[1..] {
            assert_eq!(
                permits.take(Priority::Background, Some(domain)),
                Some(Permit::Shared)
            );
        }
        assert_eq!(permits.shared, 3);
        assert_eq!(permits.shared_domains.len(), 3);
        assert_eq!(
            permits.take(Priority::Focused, Some(&domains[0])),
            Some(Permit::Foreground)
        );
        permits.release(Permit::Foreground, Some(&domains[0]));
        assert!(permits.shared_domains.contains(&domains[0]));
        permits.release(Permit::Shared, Some(&domains[0]));
        assert_eq!(
            permits.take(Priority::Background, Some(&domains[0])),
            Some(Permit::Shared)
        );
    }

    #[test]
    fn waiting_vm_does_not_block_another_vm_and_domains_are_instance_scoped() {
        let domain = ProviderDomain {
            mux: 0,
            provider: "fixture".into(),
        };
        let other = ProviderDomain {
            mux: 1,
            provider: "fixture".into(),
        };
        let mut permits = Permits::default();
        permits.take(Priority::Background, Some(&domain));
        let candidates = [
            Candidate {
                priority: Priority::Background,
                domain: Some(domain),
            },
            Candidate {
                priority: Priority::Background,
                domain: Some(other),
            },
        ];
        assert_eq!(permits.next(&candidates), Some(1));
        assert_eq!(permits.shared, 1);
    }
}
