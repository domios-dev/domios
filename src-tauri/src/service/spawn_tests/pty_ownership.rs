use std::collections::{BTreeSet, HashMap};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::multiplexer::state::MultiplexerState;
use crate::service::pty_ownership::OwnershipEvent;
use crate::service::test_fixture::ServiceFixture;

const EVENT_DEADLINE: Duration = Duration::from_secs(30);

enum Step {
    Ownership(OwnershipEvent),
    Frontend(Result<String, String>),
    Hidden(Result<String, String>),
    Activated,
}

type Gates = Arc<Mutex<HashMap<bool, Receiver<()>>>>;

struct OwnershipRace {
    fixture: ServiceFixture,
    tab_id: String,
    steps: Receiver<Step>,
    step_tx: Sender<Step>,
    // PTY readers block until cleanup closes their sessions.
    frontend_runtimes: Arc<Mutex<Vec<tokio::runtime::Runtime>>>,
    gates: Gates,
    releases: HashMap<bool, Sender<()>>,
    actors: Vec<JoinHandle<()>>,
    outstanding: usize,
    candidates: BTreeSet<String>,
    frontend: Option<Result<String, String>>,
    hidden: Option<Result<String, String>>,
    // Serializes with tests that repoint CODETERM_INSTANCE, which moves every data_dir() lookup.
    _env: codeterm_util::test_env::EnvGuard,
}

struct Outcome {
    linked: Option<String>,
    live: BTreeSet<String>,
    run_pty: Option<String>,
    launch_claim: Option<String>,
    claim_after_cleanup: Option<String>,
}

fn actor_result(run: impl FnOnce() -> Result<String, String>) -> Result<String, String> {
    catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|_| Err("ownership actor panicked".into()))
}

fn isolated_tab_state() -> (MultiplexerState, String) {
    // Other test fixtures share the process-wide activation registry. A UUID-prefixed id
    // keeps this fixture outside the 32-bit id space used by MultiplexerState::new.
    let (mux, _) = MultiplexerState::new();
    let mut snapshot = mux.to_session_snapshot();
    let tab_id = format!("pty-ownership-{}", uuid::Uuid::new_v4());
    let workspace = snapshot.workspaces.first_mut().expect("default workspace");
    workspace.active_tab_id = Some(tab_id.clone());
    let tab = workspace.tabs.first_mut().expect("default tab");
    tab.id = tab_id.clone();
    let crate::session::store::SplitNodeSnapshot::Leaf {
        tab_id: leaf_tab_id,
        ..
    } = &mut tab.split_tree
    else {
        panic!("default tab is a leaf");
    };
    *leaf_tab_id = tab_id.clone();
    (MultiplexerState::from_session_snapshot(&snapshot), tab_id)
}

impl OwnershipRace {
    fn new() -> Self {
        let env = codeterm_util::test_env::EnvGuard::lock();
        let (mut mux, tab_id) = isolated_tab_state();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.hidden = true;
        tab.show_in_tabbar = false;
        let fixture = ServiceFixture::new(mux);
        let (step_tx, steps) = mpsc::channel();
        let gates: Gates = Arc::default();
        let observed_tab = tab_id.clone();
        let observer_steps = Mutex::new(step_tx.clone());
        let observer_gates = gates.clone();
        fixture.service.pty_ownership.set_test_observer(Arc::new(
            move |tab_id: &str, event: OwnershipEvent| {
                if tab_id != observed_tab {
                    return;
                }
                let gate = match &event {
                    OwnershipEvent::Linking { launch_owned, .. } => {
                        observer_gates.lock().remove(launch_owned)
                    }
                    OwnershipEvent::AwaitingActivation => None,
                };
                let _ = observer_steps.lock().send(Step::Ownership(event));
                // Teardown drops the release, and an expired deadline also lets the actor finish.
                if let Some(gate) = gate {
                    let _ = gate.recv_timeout(EVENT_DEADLINE);
                }
            },
        ));
        Self {
            fixture,
            tab_id,
            steps,
            step_tx,
            frontend_runtimes: Arc::default(),
            gates,
            releases: HashMap::new(),
            actors: Vec::new(),
            outstanding: 0,
            candidates: BTreeSet::new(),
            frontend: None,
            hidden: None,
            _env: env,
        }
    }

    fn pause_linking(&mut self, launch_owned: bool) {
        let (release, gate) = mpsc::channel();
        self.gates.lock().insert(launch_owned, gate);
        self.releases.insert(launch_owned, release);
    }

    fn release_linking(&mut self, launch_owned: bool) {
        let release = self
            .releases
            .remove(&launch_owned)
            .expect("paused linking gate");
        let _ = release.send(());
    }

    fn spawn_actor(&mut self, actor: impl FnOnce(Sender<Step>) + Send + 'static) {
        let done = self.step_tx.clone();
        self.outstanding += 1;
        self.actors.push(std::thread::spawn(move || actor(done)));
    }

    fn start_frontend(&mut self) {
        self.frontend = None;
        let service = Arc::new(self.fixture.service.clone());
        let tab_id = self.tab_id.clone();
        let frontend_runtimes = self.frontend_runtimes.clone();
        self.spawn_actor(move |done| {
            let attached = actor_result(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("frontend runtime: {error}"))?;
                let attached = runtime.block_on(crate::commands::spawn_pty_core(
                    &service,
                    None,
                    None,
                    80,
                    24,
                    None,
                    Some(tab_id),
                ));
                frontend_runtimes.lock().push(runtime);
                attached
            });
            let _ = done.send(Step::Frontend(attached));
        });
    }

    fn start_hidden(&mut self) {
        self.hidden = None;
        let service = self.fixture.service.clone();
        let tab_id = self.tab_id.clone();
        self.spawn_actor(move |done| {
            let launched =
                actor_result(|| service.link_hidden_agent_pty(&tab_id, None, HashMap::new()));
            let _ = done.send(Step::Hidden(launched));
        });
    }

    fn record(&mut self, step: Step) {
        match step {
            Step::Ownership(OwnershipEvent::Linking { candidate, .. }) => {
                self.candidates.insert(candidate);
            }
            Step::Ownership(OwnershipEvent::AwaitingActivation) => {}
            Step::Frontend(result) => {
                self.outstanding = self.outstanding.saturating_sub(1);
                self.candidates.extend(result.iter().cloned());
                self.frontend = Some(result);
            }
            Step::Hidden(result) => {
                self.outstanding = self.outstanding.saturating_sub(1);
                self.candidates.extend(result.iter().cloned());
                self.hidden = Some(result);
            }
            Step::Activated => self.outstanding = self.outstanding.saturating_sub(1),
        }
    }

    fn await_step(&mut self, waiting_for: &str, mut reached: impl FnMut(&Step) -> bool) {
        loop {
            let step = self
                .steps
                .recv_timeout(EVENT_DEADLINE)
                .unwrap_or_else(|_| panic!("ownership race deadline waiting for {waiting_for}"));
            let done = reached(&step);
            self.record(step);
            if done {
                return;
            }
        }
    }

    fn await_linking(&mut self, launch_owned: bool) -> String {
        let mut linking = None;
        self.await_step("a linking candidate", |step| match step {
            Step::Ownership(OwnershipEvent::Linking {
                candidate,
                launch_owned: owned,
            }) if *owned == launch_owned => {
                linking = Some(candidate.clone());
                true
            }
            _ => false,
        });
        linking.expect("linking candidate")
    }

    fn await_frontend(&mut self) -> Result<String, String> {
        if self.frontend.is_none() {
            self.await_step("the frontend attach", |step| {
                matches!(step, Step::Frontend(_))
            });
        }
        self.frontend.take().expect("frontend result")
    }

    fn await_hidden(&mut self) -> Result<String, String> {
        if self.hidden.is_none() {
            self.await_step("the hidden launch", |step| matches!(step, Step::Hidden(_)));
        }
        self.hidden.take().expect("hidden result")
    }

    fn remount(&mut self) -> Result<String, String> {
        self.start_frontend();
        self.await_frontend()
    }

    fn is_live(&self, pty_id: &str) -> bool {
        self.fixture
            .service
            .pty
            .lock()
            .get_session(pty_id)
            .is_some()
    }

    fn settle_and_cleanup(&self) -> Outcome {
        let service = &self.fixture.service;
        let linked = service
            .mux
            .read()
            .get_tab(&self.tab_id)
            .and_then(|tab| tab.pty_id.clone());
        let live: BTreeSet<String> = self
            .candidates
            .iter()
            .filter(|pty_id| self.is_live(pty_id))
            .cloned()
            .collect();
        let run_pty = service.agent_run_pty_map.lock().get(&self.tab_id).cloned();
        let launch_claim = service.pty_ownership.launch_claim(&self.tab_id);
        self.release_owned_ptys();
        Outcome {
            linked,
            live,
            run_pty,
            launch_claim,
            claim_after_cleanup: service.pty_ownership.launch_claim(&self.tab_id),
        }
    }

    fn release_owned_ptys(&self) {
        let owned: Vec<String> = self.candidates.iter().cloned().collect();
        let live: Vec<String> = owned
            .iter()
            .filter(|pty_id| self.is_live(pty_id))
            .cloned()
            .collect();
        self.fixture.service.kill_ptys(&live);
        self.fixture.service.pty_ownership.release_ptys(&owned);
    }

    fn assert_activation_released(&mut self) {
        let ownership = self.fixture.service.pty_ownership.clone();
        let tab_id = self.tab_id.clone();
        self.spawn_actor(move |done| {
            drop(ownership.acquire(&tab_id));
            let _ = done.send(Step::Activated);
        });
        self.await_step("the tab activation permit", |step| {
            matches!(step, Step::Activated)
        });
    }
}

impl Drop for OwnershipRace {
    fn drop(&mut self) {
        self.gates.lock().clear();
        self.releases.clear();
        let deadline = Instant::now() + EVENT_DEADLINE;
        while self.outstanding > 0 {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match self.steps.recv_timeout(remaining) {
                Ok(step) => self.record(step),
                Err(_) => break,
            }
        }
        if self.outstanding == 0 {
            for actor in self.actors.drain(..) {
                let _ = actor.join();
            }
        }
        self.release_owned_ptys();
        let frontend_runtimes = std::mem::take(&mut *self.frontend_runtimes.lock());
        for runtime in frontend_runtimes {
            runtime.shutdown_background();
        }
    }
}

fn assert_launch_pty_conserved(outcome: &Outcome, launch_pty: &str, frontend_pty: &str) {
    assert!(
        outcome.live.contains(launch_pty),
        "frontend PTY {frontend_pty} displaced the live launch-owned PTY {launch_pty}"
    );
    assert_eq!(outcome.linked.as_deref(), Some(launch_pty));
    assert_eq!(outcome.live, BTreeSet::from([launch_pty.to_string()]));
    assert_eq!(outcome.run_pty.as_deref(), Some(launch_pty));
    assert_eq!(outcome.launch_claim.as_deref(), Some(launch_pty));
    assert_eq!(outcome.claim_after_cleanup, None);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires a desktop runtime"
)]
fn cron_pty_ownership_frontend_first_shell_does_not_displace_launch_pty() {
    let mut race = OwnershipRace::new();
    race.pause_linking(false);
    race.start_frontend();
    let frontend_candidate = race.await_linking(false);
    race.start_hidden();
    race.await_step("the hidden launch to link or wait", |step| {
        matches!(
            step,
            Step::Hidden(_) | Step::Ownership(OwnershipEvent::AwaitingActivation)
        )
    });
    race.release_linking(false);

    let frontend = race.await_frontend();
    let launch_pty = race.await_hidden().expect("hidden launch links its PTY");
    let candidates_before_remount = race.candidates.clone();
    let remounted = race.remount();
    let remount_candidates = race.candidates.clone();
    let outcome = race.settle_and_cleanup();

    assert_eq!(frontend.as_deref(), Ok(frontend_candidate.as_str()));
    assert_ne!(frontend_candidate, launch_pty);
    assert_launch_pty_conserved(&outcome, &launch_pty, &frontend_candidate);
    assert_eq!(remounted.as_deref(), Ok(launch_pty.as_str()));
    assert_eq!(remount_candidates, candidates_before_remount);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires a desktop runtime"
)]
fn cron_pty_ownership_hidden_first_launch_pty_survives_frontend_attach() {
    let mut race = OwnershipRace::new();
    race.pause_linking(true);
    race.pause_linking(false);
    race.start_hidden();
    let launch_candidate = race.await_linking(true);
    race.start_frontend();
    race.await_step("the frontend to link or wait", |step| {
        matches!(
            step,
            Step::Ownership(OwnershipEvent::Linking {
                launch_owned: false,
                ..
            }) | Step::Ownership(OwnershipEvent::AwaitingActivation)
        )
    });
    race.release_linking(true);
    let launch_pty = race.await_hidden().expect("hidden launch links its PTY");
    race.release_linking(false);

    let frontend = race.await_frontend().expect("frontend attach succeeds");
    let outcome = race.settle_and_cleanup();

    assert_eq!(launch_pty, launch_candidate);
    assert_launch_pty_conserved(&outcome, &launch_pty, &frontend);
    assert_eq!(frontend, launch_pty);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires a desktop runtime"
)]
fn cron_pty_ownership_remount_reuses_live_launch_pty() {
    let mut race = OwnershipRace::new();
    race.start_hidden();
    let launch_pty = race.await_hidden().expect("hidden launch links its PTY");
    let launch_candidates = race.candidates.clone();

    let first = race.remount();
    let second = race.remount();
    let outcome = race.settle_and_cleanup();

    assert_eq!(first.as_deref(), Ok(launch_pty.as_str()));
    assert_eq!(second.as_deref(), Ok(launch_pty.as_str()));
    assert_eq!(race.candidates, launch_candidates);
    assert_eq!(outcome.linked.as_deref(), Some(launch_pty.as_str()));
    assert_eq!(outcome.live, BTreeSet::from([launch_pty.clone()]));
    assert_eq!(outcome.run_pty.as_deref(), Some(launch_pty.as_str()));
    assert_eq!(outcome.claim_after_cleanup, None);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires a desktop runtime"
)]
fn cron_pty_ownership_closed_tab_reaps_launch_candidate() {
    let mut race = OwnershipRace::new();
    race.fixture.service.mux.write().remove_tab(&race.tab_id);

    race.start_hidden();
    let launched = race.await_hidden();
    let outcome = race.settle_and_cleanup();

    assert_eq!(race.candidates.len(), 1);
    assert!(outcome.live.is_empty(), "closed-tab launch PTY leaked");
    assert!(launched.is_err());
    assert_eq!(outcome.run_pty, None);
    assert_eq!(outcome.launch_claim, None);
    race.assert_activation_released();
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires a desktop runtime"
)]
fn cron_pty_ownership_spawn_failure_holds_no_ownership() {
    let mut race = OwnershipRace::new();
    race.fixture.service.pty.lock().spawn_gate().quiesce();

    race.start_hidden();
    let launched = race.await_hidden();
    let outcome = race.settle_and_cleanup();

    assert!(launched.is_err());
    assert!(race.candidates.is_empty());
    assert_eq!(outcome.linked, None);
    assert_eq!(outcome.run_pty, None);
    assert_eq!(outcome.launch_claim, None);
    race.assert_activation_released();
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires a desktop runtime"
)]
fn cron_pty_ownership_dead_launch_claim_yields_to_frontend_shell() {
    let mut race = OwnershipRace::new();
    race.start_hidden();
    let exited = race.await_hidden().expect("hidden launch links its PTY");
    let reaper = race
        .fixture
        .service
        .pty
        .lock()
        .begin_kill(&exited)
        .expect("provider PTY termination starts");
    reaper
        .wait_bounded(EVENT_DEADLINE)
        .expect("provider PTY exits before frontend attaches");
    assert!(!race.is_live(&exited));
    assert_eq!(
        race.fixture
            .service
            .pty_ownership
            .launch_claim(&race.tab_id)
            .as_deref(),
        Some(exited.as_str())
    );

    let shell = race.remount().expect("frontend attaches a fresh shell");
    let outcome = race.settle_and_cleanup();

    assert_ne!(shell, exited);
    assert_eq!(outcome.linked.as_deref(), Some(shell.as_str()));
    assert_eq!(outcome.live, BTreeSet::from([shell]));
    assert_eq!(outcome.launch_claim, None);
}
