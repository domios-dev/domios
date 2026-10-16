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
}

impl Permits {
    fn available(&self, priority: Priority) -> Option<Permit> {
        if priority != Priority::Background && !self.foreground {
            Some(Permit::Foreground)
        } else if self.shared < 3 {
            Some(Permit::Shared)
        } else {
            None
        }
    }

    fn take(&mut self, priority: Priority) -> Option<Permit> {
        let permit = self.available(priority)?;
        match permit {
            Permit::Foreground => self.foreground = true,
            Permit::Shared => self.shared += 1,
        }
        Some(permit)
    }

    fn release(&mut self, permit: Permit) {
        match permit {
            Permit::Foreground => self.foreground = false,
            Permit::Shared => self.shared -= 1,
        }
    }

    fn next(&self, priorities: &[Priority]) -> Option<usize> {
        priorities
            .iter()
            .enumerate()
            .filter(|(_, priority)| self.available(**priority).is_some())
            .min_by_key(|(_, priority)| **priority)
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
    fn priorities(&self) -> Vec<Priority> {
        let mut priorities = vec![Priority::Background; self.waiting.len()];
        let mut classified = HashSet::new();
        for waiting in &self.waiting {
            if !classified.insert(Arc::as_ptr(&waiting.mux) as usize) {
                continue;
            }
            let mux = waiting.mux.read();
            for (index, candidate) in self.waiting.iter().enumerate() {
                if Arc::ptr_eq(&candidate.mux, &waiting.mux) {
                    priorities[index] = priority(&mux, &candidate.tab_id);
                }
            }
        }
        priorities
    }
}

#[derive(Default)]
struct PreparationGate {
    state: Mutex<PreparationState>,
}

struct PreparationTicket<'a> {
    gate: &'a PreparationGate,
    ticket: u64,
    key: PreparationKey,
    permit: Option<Permit>,
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
        })
    }
}

impl PreparationTicket<'_> {
    fn acquire(&mut self) {
        loop {
            let observed = changes().revision();
            let mut state = self.gate.state.lock();
            let priorities = state.priorities();
            if let Some(index) = state.permits.next(&priorities) {
                if state.waiting[index].ticket == self.ticket {
                    self.permit = state.permits.take(priorities[index]);
                    state.waiting.remove(index);
                    drop(state);
                    notify_changed();
                    return;
                }
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
            state.permits.release(permit);
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

pub fn admit_restored_pty(service: &ServiceContext, tab_id: &str) -> Result<(), String> {
    let resolved = service
        .mux
        .read()
        .resolve_tab_id(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found or has no PTY"))?;
    if service.tab_chat_backing(&resolved).is_some() {
        return Ok(());
    }
    if let Some(refusal) = service.tab_surface_input_refusal(&resolved) {
        return Err(refusal);
    }
    let pty_id = {
        let claim = service.pty_ownership.acquire(&resolved);
        service.ensure_tab_pty_linked_with_claim(&claim)?;
        service.resolve_pty_id(&resolved)?
    };
    schedule_preparation(service, resolved, pty_id);
    Ok(())
}

fn schedule_preparation(
    service: &ServiceContext,
    resolved: String,
    pty_id: String,
) -> Option<std::thread::JoinHandle<()>> {
    static PREPARATION: OnceLock<PreparationGate> = OnceLock::new();
    let Some(mut ticket) = PREPARATION
        .get_or_init(PreparationGate::default)
        .enqueue(&service.mux, &resolved)
    else {
        return None;
    };
    crate::startup::boot_timing::mark(&resolved, Some(&pty_id), "prepare_queued");
    let service = service.clone();
    Some(codeterm_util::named_thread::spawn(
        "restore-prepare",
        move || {
            ticket.acquire();
            crate::startup::boot_timing::mark(&resolved, Some(&pty_id), "prepare_admitted");
            let claim = service.pty_ownership.acquire(&resolved);
            if service.resolve_pty_id(&resolved).as_deref() != Ok(pty_id.as_str()) {
                crate::startup::boot_timing::mark(&resolved, Some(&pty_id), "prepare_cancelled");
                return;
            }
            let result = service.materialize_restored_agent_with_claim(&claim);
            if let Err(reason) = &result {
                if let Some(tab) = service.mux.write().get_tab_mut(&resolved) {
                    tab.agent_booting = false;
                }
                service.emit_boot_stage(
                    &resolved,
                    crate::service::boot_stage::BootStage::SpawnFailed,
                    Some(reason),
                );
                service.emit_state_changed();
                log::error!("[startup-pty] tab={resolved} materialization failed: {reason}");
            }
            crate::startup::boot_timing::mark(&resolved, Some(&pty_id), "prepare_completed");
            crate::startup::boot_timing::mark(&resolved, Some(&pty_id), "materialized");
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_background_never_consumes_reserved_permit() {
        let mut permits = Permits::default();
        for _ in 0..3 {
            assert_eq!(permits.take(Priority::Background), Some(Permit::Shared));
        }
        assert_eq!(permits.take(Priority::Background), None);
        assert_eq!(permits.take(Priority::Focused), Some(Permit::Foreground));
        assert_eq!(permits.take(Priority::Visible), None);
        permits.release(Permit::Foreground);
        assert_eq!(permits.take(Priority::Visible), Some(Permit::Foreground));
    }

    #[test]
    fn preparation_selects_live_focus_before_visible_and_background() {
        let permits = Permits::default();
        assert_eq!(
            permits.next(&[Priority::Background, Priority::Visible, Priority::Focused]),
            Some(2)
        );
        assert_eq!(
            permits.next(&[Priority::Focused, Priority::Visible, Priority::Background]),
            Some(0)
        );
    }

    #[test]
    fn preparation_completion_releases_only_the_owned_permit() {
        let mut permits = Permits::default();
        let shared = permits.take(Priority::Background).unwrap();
        let foreground = permits.take(Priority::Focused).unwrap();
        permits.release(shared);
        assert_eq!(permits.shared, 0);
        assert!(permits.foreground);
        permits.release(foreground);
        assert!(!permits.foreground);
    }

    struct RestoreProvider {
        inner: crate::agent::test_provider::MockProvider,
        calls: Arc<Mutex<Vec<&'static str>>>,
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
            None
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
                }));
            if deferred {
                let claim = fixture.service.pty_ownership.acquire(&tab_id);
                fixture
                    .service
                    .ensure_tab_pty_linked_with_claim(&claim)
                    .unwrap();
                assert!(calls.lock().is_empty());
                let pty_id = fixture.service.resolve_pty_id(&tab_id).unwrap();
                drop(claim);
                schedule_preparation(&fixture.service, tab_id.clone(), pty_id)
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
}
