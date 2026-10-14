use super::approval::{admit, scope_key, scope_keys, ReminderOutcome};
use super::tests::{plan, task};
use super::*;
use crate::service::supporter::registry::SupporterRegistry;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{OrchestratorRef, TaskExecutionAttempt};
use crate::service::hierarchy::heartbeat_gate::{Telemetry, Worker};
use crate::service::hierarchy::stalled_team::{maybe_nudge_stalled_team, StallEpisodeTracker};
use crate::service::supporter::{SupporterRuntime, TokenSnapshot};
use codeterm_decision::{
    DecisionAnswer, DecisionAnswers, DecisionError, DecisionModel, DecisionModelInfo,
    DecisionPrimitives, DecisionQuestionsRequest, DecisionRequest, DecisionResponse, TokenMode,
};
use codeterm_types::config::{SupporterConfig, SupporterConnection};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

const KINDS: [ReminderKind; 5] = [
    ReminderKind::Owner,
    ReminderKind::ModeChange,
    ReminderKind::StalledTeam,
    ReminderKind::CompleteWrapup,
    ReminderKind::Worker,
];
/// Kinds whose only path is Supporter approval; the Owner floor is pinned in floor.rs.
const SUPPORTER_ONLY: [ReminderKind; 4] = [
    ReminderKind::ModeChange,
    ReminderKind::StalledTeam,
    ReminderKind::CompleteWrapup,
    ReminderKind::Worker,
];
const PLAN: &str = "plan-a";
const DAY: u64 = 24 * 60 * 60;
const SECRETS: [&str; 3] = ["owner-nonce", "worker-nonce", "view-secret-nonce"];

#[derive(Clone, Debug)]
pub(super) enum Reply {
    Choice(&'static str, f64),
    Undeclared,
    WrongType,
    MissingTick,
    Error(DecisionError),
}

pub(super) fn approve() -> Reply {
    Reply::Choice("action_needed", 0.99)
}

type Barrier = (mpsc::Sender<()>, mpsc::Receiver<()>);

/// A Choice adapter whose answers, capability and token mode the test sets.
pub(super) struct FakeChoice {
    calls: AtomicUsize,
    reply: Mutex<Reply>,
    choice: AtomicBool,
    token_required: AtomicBool,
    barrier: Mutex<Option<Barrier>>,
    last_state: Mutex<Value>,
}

impl FakeChoice {
    pub(super) fn new(reply: Reply) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            reply: Mutex::new(reply),
            choice: AtomicBool::new(true),
            token_required: AtomicBool::new(false),
            barrier: Mutex::new(None),
            last_state: Mutex::new(Value::Null),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl DecisionModel for FakeChoice {
    fn id(&self) -> &str {
        "fake-choice"
    }
    fn token_mode(&self) -> TokenMode {
        if self.token_required.load(Ordering::SeqCst) {
            TokenMode::Required
        } else {
            TokenMode::None
        }
    }
    fn model_id(&self) -> Option<String> {
        Some("fake-model".into())
    }
    fn models(&self) -> Vec<DecisionModelInfo> {
        vec![DecisionModelInfo {
            id: "fake-model".into(),
            display_name: "fake-model".into(),
        }]
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: self.choice.load(Ordering::SeqCst),
            ..Default::default()
        }
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        Err(DecisionError::Unsupported)
    }
    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        _: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        codeterm_decision::physical::before_request()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_state.lock() = request.state.clone();
        let barrier = self.barrier.lock().take();
        if let Some((entered, release)) = barrier {
            entered.send(()).ok();
            release.recv().ok();
        }
        let reply = self.reply.lock().clone();
        let (tick, p) = match &reply {
            Reply::Error(error) => return Err(error.clone()),
            Reply::Choice(tick, p) => (*tick, *p),
            Reply::Undeclared => ("wake_now", 0.99),
            Reply::WrongType | Reply::MissingTick => ("action_needed", 0.99),
        };
        let mut answers: DecisionAnswers = request
            .questions
            .keys()
            .map(|key| {
                let label = match key.as_str() {
                    "tick" => tick,
                    key if key.starts_with("learning:") => "irrelevant",
                    _ => "on_track",
                };
                let answer = DecisionAnswer::Choice {
                    choice: label.into(),
                    probabilities: BTreeMap::from([(label.into(), p)]),
                    confidence: Some(p),
                };
                (key.clone(), answer)
            })
            .collect();
        match reply {
            Reply::WrongType => {
                answers.insert("tick".into(), DecisionAnswer::Noul { p: 0.99 });
            }
            Reply::MissingTick => {
                answers.remove("tick");
            }
            _ => {}
        }
        Ok(DecisionResponse {
            answers,
            usage: None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Emitted {
    plan_id: String,
    scope: Scope,
    emission: Emission,
}

#[derive(Clone, Debug, PartialEq)]
struct NotifiedBlocker {
    owner_tab: String,
    notice: BlockedNotice,
}

/// Real plans and multiplexer tabs; owner observations and composed text are
/// opaque test inputs, and the emitter only counts what it is handed.
struct FakeHost {
    gate: Gate,
    registry: SupporterRegistry,
    cfg: Mutex<SupporterConfig>,
    token_usable: AtomicBool,
    runtime: Arc<SupporterRuntime>,
    plans: Mutex<Vec<Plan>>,
    mux: Mutex<MultiplexerState>,
    owner_view: Mutex<Snapshot>,
    emits: Mutex<Vec<Emitted>>,
    notices: Mutex<Vec<NotifiedBlocker>>,
    emit_error: Mutex<Option<String>>,
    compose_hook: Mutex<Option<fn(&FakeHost)>>,
    dir: tempfile::TempDir,
}

impl ReminderHost for FakeHost {
    fn gate(&self) -> &Gate {
        &self.gate
    }

    fn supporter(&self) -> Supporter<'_> {
        Supporter {
            epoch: self.runtime.connection_epoch(),
            cfg: self.cfg.lock().clone(),
            token: TokenSnapshot {
                usable: self.token_usable.load(Ordering::SeqCst),
                acl_invalid: false,
            },
            registry: &self.registry,
            runtime: self.runtime.clone(),
        }
    }

    fn interval(&self) -> u64 {
        1800
    }

    fn plan(&self, plan_id: &str) -> Option<Plan> {
        self.plans
            .lock()
            .iter()
            .find(|plan| plan.id == plan_id)
            .cloned()
    }

    fn with_plans<R>(&self, read: impl FnOnce(&[Plan]) -> R) -> R {
        read(&self.plans.lock())
    }

    fn recipient(&self, tab_id: &str) -> Option<Recipient> {
        let mux = self.mux.lock();
        let resolved = mux.resolve_tab_id(tab_id)?;
        Some(Recipient {
            tab: mux.get_tab(&resolved)?.clone(),
            workspace_id: mux.find_workspace_for_tab(&resolved),
        })
    }

    fn observe_owner(
        &self,
        plan: &Plan,
        _: &str,
        previous: &Observation,
        _: u64,
    ) -> (Snapshot, Observation) {
        let mut snapshot = self.owner_view.lock().clone();
        snapshot.plan_id = plan.id.clone();
        let observed = Observation {
            projection: Some(snapshot.projection.clone()),
            ..previous.clone()
        };
        (snapshot, observed)
    }

    fn observe_worker(
        &self,
        plan: &Plan,
        scope: &Scope,
        previous: &crate::service::hierarchy::heartbeat_gate::Observation,
        _: u64,
    ) -> (
        Snapshot,
        crate::service::hierarchy::heartbeat_gate::Observation,
    ) {
        crate::service::hierarchy::heartbeat_gate::collect_worker_scope(plan, scope, previous)
    }

    fn compose(&self, plan: &Plan, scope: &Scope, _: &Snapshot, decision: &Decision) -> Emission {
        let hook = self.compose_hook.lock().take();
        if let Some(hook) = hook {
            hook(self);
        }
        let transport = match scope.kind {
            ReminderKind::Worker | ReminderKind::CompleteWrapup => Transport::Automated,
            _ => Transport::Heartbeat {
                learning_ids: decision.learning_ids.clone(),
                plain: false,
                mode_transition: false,
            },
        };
        Emission {
            text: opaque(scope.kind, &plan.id),
            transport,
        }
    }

    fn emit(
        &self,
        plan_id: &str,
        scope: &Scope,
        emission: &Emission,
        _observer: Option<crate::service::messenger::DeliveryWriteObserver>,
    ) -> Result<bool, String> {
        assert!(self.gate.unlocked(), "gate lock held across the emitter");
        self.emits.lock().push(Emitted {
            plan_id: plan_id.into(),
            scope: scope.clone(),
            emission: emission.clone(),
        });
        self.emit_error.lock().clone().map_or(Ok(true), Err)
    }

    fn notify_blocked(&self, owner_tab: &str, notice: &BlockedNotice) {
        self.notices.lock().push(NotifiedBlocker {
            owner_tab: owner_tab.into(),
            notice: notice.clone(),
        });
    }
}

fn opaque(kind: ReminderKind, plan_id: &str) -> String {
    format!("opaque-{}-{plan_id}", kind.name())
}

fn bind(tab: &mut crate::multiplexer::types::Tab, session: &str, nonce: &str) {
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some(session.into());
    tab.session_binding_nonce = Some(nonce.into());
    tab.reported_session_binding_nonce = Some(nonce.into());
    tab.pty_id = Some("fixture-pty".into());
    tab.chat_view = true;
}

fn attempt(id: &str, tab: &str) -> TaskExecutionAttempt {
    TaskExecutionAttempt {
        id: id.into(),
        executor_tab_id: tab.into(),
        session_id: Some("worker-session".into()),
        session_binding_nonce: Some("worker-nonce".into()),
        ..Default::default()
    }
}

fn kind_plan(kind: ReminderKind, id: &str, owner: &str, worker: &str) -> Plan {
    let mut work = task("task-a", TaskStatus::InProgress, Some(worker));
    work.execution_attempts.push(attempt("attempt-a", worker));
    let mut plan = plan(id, owner, PlanMode::Execution, vec![work]);
    match kind {
        ReminderKind::ModeChange => plan.mode = PlanMode::Planning,
        ReminderKind::CompleteWrapup => plan.tasks[0].status = TaskStatus::Done,
        _ => {}
    }
    plan
}

fn every_hard_wake() -> Vec<HardWake> {
    vec![
        HardWake::UndeliveredReport("r".into()),
        HardWake::NewReport("r".into()),
        HardWake::TaskState("task-a".into()),
        HardWake::FailedGate("g".into()),
        HardWake::OwnerMessage("m".into()),
        HardWake::ChoiceAnswer("c".into()),
        HardWake::Incident("i".into()),
        HardWake::ModeChange,
    ]
}

fn owner_view() -> Snapshot {
    let mut view = Snapshot::default();
    view.projection.insert("mode".into(), "opaque-mode".into());
    view.delta.insert("report:r".into(), "opaque-digest".into());
    view.hard.push(HardWake::NewReport("r".into()));
    view.workers.push(Worker {
        task_id: "task-a".into(),
        tab_id: "view-worker".into(),
        ack_received: true,
        session_key: Some("view-session:view-secret-nonce".into()),
        process: "present".into(),
        activity: "working".into(),
        ..Default::default()
    });
    view
}

struct Fixture {
    host: FakeHost,
    model: Arc<FakeChoice>,
    owner: String,
    worker: String,
    spare: String,
}

fn host(plans: Vec<Plan>, mux: MultiplexerState, reply: Reply) -> (FakeHost, Arc<FakeChoice>) {
    let model = FakeChoice::new(reply);
    let registry = SupporterRegistry::new();
    registry.register("fake", model.clone() as Arc<dyn DecisionModel>);
    let dir = tempfile::tempdir().unwrap();
    let host = FakeHost {
        gate: Gate::default(),
        registry,
        cfg: Mutex::new(SupporterConfig {
            adapter: Some("fake".into()),
            deadline_ms: 5_000,
            ..Default::default()
        }),
        token_usable: AtomicBool::new(true),
        runtime: Arc::new(SupporterRuntime::new(dir.path().into())),
        plans: Mutex::new(plans),
        mux: Mutex::new(mux),
        owner_view: Mutex::new(owner_view()),
        emits: Mutex::default(),
        notices: Mutex::default(),
        emit_error: Mutex::default(),
        compose_hook: Mutex::default(),
        dir,
    };
    (host, model)
}

fn fixture(kind: ReminderKind, reply: Reply) -> Fixture {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let worker = mux.create_tab(&workspace).unwrap();
    let spare = mux.create_tab(&workspace).unwrap();
    for (tab, nonce) in [(&owner, "owner"), (&worker, "worker"), (&spare, "spare")] {
        bind(
            mux.get_tab_mut(tab).unwrap(),
            &format!("{nonce}-session"),
            &format!("{nonce}-nonce"),
        );
    }
    let plans = vec![kind_plan(kind, PLAN, &owner, &worker)];
    let (host, model) = host(plans, mux, reply);
    Fixture {
        host,
        model,
        owner,
        worker,
        spare,
    }
}

impl Fixture {
    fn recipient(&self, kind: ReminderKind) -> &str {
        if kind == ReminderKind::Worker {
            &self.worker
        } else {
            &self.owner
        }
    }

    fn key(&self, kind: ReminderKind) -> String {
        scope_key(PLAN, self.recipient(kind))
    }

    fn admit(&self, kind: ReminderKind, now: u64) -> ReminderOutcome {
        let request = match kind {
            ReminderKind::Worker => ReminderRequest::worker(&self.worker),
            kind => ReminderRequest::owner(PLAN, &self.owner, kind),
        };
        admit(&self.host, &request, now)
    }

    fn emits(&self) -> usize {
        self.host.emits.lock().len()
    }

    fn telemetry(&self, kind: ReminderKind) -> (u64, u64, u64, u64) {
        let Telemetry {
            suppressed,
            attempted,
            accepted,
            failed,
            ..
        } = self.host.gate.telemetry(&self.key(kind));
        (suppressed, attempted, accepted, failed)
    }

    fn journal(&self) -> String {
        std::fs::read_to_string(self.host.dir.path().join("journal.jsonl")).unwrap_or_default()
    }

    fn decisions(&self) -> Vec<Value> {
        self.journal()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|row| row["kind"] == "heartbeat" && row.get("observation_id").is_some())
            .collect()
    }

    fn edit_plan(&self, edit: impl FnOnce(&mut Plan)) {
        edit(&mut self.host.plans.lock()[0]);
    }

    fn edit_tab(&self, tab: &str, edit: impl FnOnce(&mut crate::multiplexer::types::Tab)) {
        edit(self.host.mux.lock().get_tab_mut(tab).unwrap());
    }

    /// A meaningful change for the kind's own observation.
    fn change(&self, kind: ReminderKind, token: &str) {
        if kind == ReminderKind::Worker {
            self.edit_plan(|plan| plan.tasks[0].status_label = Some(token.into()));
        } else {
            let mut view = self.host.owner_view.lock();
            view.delta.insert(format!("stride:{token}"), token.into());
        }
    }
}

/// Runs one admission and pauses it inside the model call; `between` runs on
/// this thread while the admission waits, then the call completes.
fn during_model_call(
    f: &Fixture,
    kind: ReminderKind,
    now: u64,
    between: impl FnOnce(),
) -> ReminderOutcome {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *f.model.barrier.lock() = Some((entered_tx, release_rx));
    std::thread::scope(move |scope| {
        let first = scope.spawn(move || f.admit(kind, now));
        entered_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the admission never reached the model");
        assert!(f.host.gate.unlocked(), "gate lock held across the model");
        assert!(f.host.mux.try_lock().is_some(), "mux held across the model");
        assert!(
            f.host.plans.try_lock().is_some(),
            "plans held across the model"
        );
        between();
        release_tx.send(()).unwrap();
        first.join().unwrap()
    })
}

#[test]
fn plan_reminder_choice_unavailable_zero_call_zero_emit() {
    let variants: [(&str, fn(&Fixture)); 6] = [
        ("stopped", |f| f.host.cfg.lock().adapter = None),
        ("unregistered", |f| {
            f.host.cfg.lock().adapter = Some("absent".into());
        }),
        ("disconnected", |f| f.host.registry.unregister("fake")),
        ("choice_unsupported", |f| {
            f.model.choice.store(false, Ordering::SeqCst);
        }),
        ("token_unusable", |f| {
            f.model.token_required.store(true, Ordering::SeqCst);
            f.host.token_usable.store(false, Ordering::SeqCst);
        }),
        ("saved_model_differs", |f| {
            let connection = SupporterConnection {
                model: Some("unselected-model".into()),
                ..Default::default()
            };
            f.host
                .cfg
                .lock()
                .connections
                .insert("fake".into(), connection);
        }),
    ];
    for kind in SUPPORTER_ONLY {
        for (name, disable) in variants {
            let f = fixture(kind, approve());
            disable(&f);
            for now in [100, 100 + DAY] {
                let outcome = f.admit(kind, now);
                assert_eq!(
                    outcome,
                    ReminderOutcome::Suppressed("no_adapter"),
                    "{kind:?}/{name}"
                );
            }
            assert_eq!(f.model.calls(), 0, "{kind:?}/{name}");
            assert_eq!(f.emits(), 0, "{kind:?}/{name}");
            assert!(f.host.notices.lock().is_empty(), "{kind:?}/{name}");
            assert!(f.host.gate.observation(&f.key(kind)).projection.is_none());
            assert_eq!(f.telemetry(kind), (2, 0, 0, 0), "{kind:?}/{name}");
            assert!(f.decisions().is_empty(), "{kind:?}/{name}");
        }
    }
}

#[test]
fn plan_reminder_choice_invalid_or_unavailable_zero_emit() {
    let cases = [
        (Reply::Choice("other", 0.99), "low_confidence"),
        (Reply::Choice("action_needed", 0.79), "low_confidence"),
        (Reply::Choice("action_needed", f64::NAN), "invalid_answer"),
        (Reply::Choice("action_needed", 1.5), "invalid_answer"),
        (Reply::Undeclared, "invalid_answer"),
        (Reply::WrongType, "invalid_answer"),
        (Reply::MissingTick, "invalid_answer"),
        (Reply::Error(DecisionError::Deadline), "model_deadline"),
        (
            Reply::Error(DecisionError::Transport("opaque".into())),
            "model_failed",
        ),
        (
            Reply::Error(DecisionError::Unavailable("opaque".into())),
            "adapter_disconnected",
        ),
        (Reply::Error(DecisionError::Http(402)), "credit_exhausted"),
    ];
    for kind in SUPPORTER_ONLY {
        for (reply, reason) in cases.clone() {
            let f = fixture(kind, reply.clone());
            let case = format!("{kind:?}/{reply:?}");
            assert_eq!(
                f.admit(kind, 100),
                ReminderOutcome::Suppressed(reason),
                "{case}"
            );
            assert_eq!(
                f.admit(kind, 100 + DAY),
                ReminderOutcome::Suppressed("same_snapshot"),
                "{case}"
            );
            assert_eq!(f.model.calls(), 1, "{case}");
            assert_eq!(f.emits(), 0, "{case}");
            assert!(f.host.notices.lock().is_empty(), "{case}");
            assert_eq!(f.telemetry(kind), (2, 0, 0, 0), "{case}");
            assert!(
                f.decisions()
                    .iter()
                    .any(|row| row["action"] == "suppress" && row["reason"] == reason),
                "{case}"
            );
        }
        let f = fixture(kind, approve());
        f.host.cfg.lock().calls_per_hour = Some(1);
        assert_eq!(f.admit(kind, 100), ReminderOutcome::Accepted);
        f.change(kind, "after-budget");
        for (now, reason) in [
            (100 + DAY, "budget_exhausted"),
            (101 + DAY, "same_snapshot"),
        ] {
            assert_eq!(
                f.admit(kind, now),
                ReminderOutcome::Suppressed(reason),
                "{kind:?}"
            );
        }
        assert_eq!((f.model.calls(), f.emits()), (1, 1), "{kind:?}");
    }
}

#[test]
fn plan_reminder_choice_deny_beats_hard_and_silence() {
    for denial in ["nothing_changed", "status_only"] {
        for kind in SUPPORTER_ONLY {
            let f = fixture(kind, Reply::Choice(denial, 0.99));
            f.host.owner_view.lock().hard = every_hard_wake();
            f.host.owner_view.lock().workers[0].prompt = Some("opaque-prompt".into());
            let ticks = [
                (100, "model_answer"),
                (100 + DAY, "same_snapshot"),
                (100 + 7 * DAY, "same_snapshot"),
            ];
            for (now, reason) in ticks {
                let outcome = f.admit(kind, now);
                assert_eq!(
                    outcome,
                    ReminderOutcome::Suppressed(reason),
                    "{denial}/{kind:?}"
                );
            }
            assert_eq!((f.model.calls(), f.emits()), (1, 0), "{denial}/{kind:?}");
            let state = f.model.last_state.lock().clone();
            assert_eq!(state["reminder"]["kind"], kind.name());
            let hard = if kind == ReminderKind::Worker {
                Vec::new()
            } else {
                every_hard_wake()
            };
            assert_eq!(state["hard"], serde_json::to_value(hard).unwrap());
        }
    }
}

#[test]
fn plan_reminder_choice_approve_exact_scope_once() {
    for kind in KINDS {
        let f = fixture(kind, approve());
        assert_eq!(f.admit(kind, 100), ReminderOutcome::Accepted, "{kind:?}");
        let emits = f.host.emits.lock().clone();
        assert_eq!(emits.len(), 1);
        assert_eq!(emits[0].plan_id, PLAN);
        assert_eq!(emits[0].scope.tab_id, f.recipient(kind));
        assert_eq!(emits[0].scope.kind, kind);
        let workspace = f.host.mux.lock().find_workspace_for_tab(f.recipient(kind));
        assert!(workspace.is_some());
        assert_eq!(emits[0].scope.recipient_workspace, workspace);
        assert_eq!(emits[0].emission.text, opaque(kind, PLAN));
        let (role, task, attempt) = if kind == ReminderKind::Worker {
            ("worker", Some("task-a"), Some("attempt-a"))
        } else {
            ("orchestrator", None, None)
        };
        assert_eq!(emits[0].scope.role, role);
        assert_eq!(emits[0].scope.task_id.as_deref(), task);
        assert_eq!(emits[0].scope.attempt_id.as_deref(), attempt);
        assert_eq!(f.telemetry(kind), (0, 1, 1, 0));
        let decisions = f.decisions();
        let wake = decisions
            .iter()
            .find(|row| row["action"] == "wake")
            .unwrap();
        assert_eq!(wake["reason"], "model_answer");
        assert_eq!(wake["reminder"], kind.name());
        assert_eq!(wake["adapter"], "fake-choice");
        assert_eq!(wake["model_id"], "fake-model");
        assert!(wake.get("observation").is_none() && wake.get("next_steps").is_none());
        let state = f.model.last_state.lock().to_string();
        for secret in SECRETS {
            assert!(
                !f.journal().contains(secret),
                "{kind:?} journal leaks {secret}"
            );
            assert!(
                !state.contains(secret),
                "{kind:?} model state leaks {secret}"
            );
        }
    }
    let scopes: [(&str, ReminderKind, fn(&Fixture), &str); 8] = [
        (
            "attempt_missing",
            ReminderKind::Worker,
            |f| f.edit_plan(|plan| plan.tasks[0].execution_attempts.clear()),
            "binding_missing",
        ),
        (
            "two_open_attempts",
            ReminderKind::Worker,
            |f| {
                let extra = attempt("attempt-b", &f.worker);
                f.edit_plan(|plan| plan.tasks[0].execution_attempts.push(extra));
            },
            "binding_ambiguous",
        ),
        (
            "two_assigned_tasks",
            ReminderKind::Worker,
            |f| {
                let extra = task("task-b", TaskStatus::Pending, Some(&f.worker));
                f.edit_plan(|plan| plan.tasks.push(extra));
            },
            "binding_ambiguous",
        ),
        (
            "foreign_attempt_session",
            ReminderKind::Worker,
            |f| {
                f.edit_plan(|plan| {
                    plan.tasks[0].execution_attempts[0].session_binding_nonce =
                        Some("foreign-nonce".into());
                });
            },
            "binding_missing",
        ),
        (
            "recipient_unconfirmed",
            ReminderKind::Worker,
            |f| {
                f.edit_tab(&f.worker, |tab| {
                    tab.reported_session_binding_nonce = Some("unconfirmed".into());
                });
            },
            "recipient_unavailable",
        ),
        (
            "owner_mismatch",
            ReminderKind::Owner,
            |f| {
                let spare = f.spare.clone();
                f.edit_plan(|plan| plan.owner = OrchestratorRef::from_tab(&spare));
            },
            "ineligible",
        ),
        (
            "recipient_outside_plan_team",
            ReminderKind::Owner,
            |f| {
                f.edit_plan(|plan| plan.team_id = Some("plan-team".into()));
                f.edit_tab(&f.owner, |tab| tab.team_id = Some("other-team".into()));
            },
            "recipient_scope_mismatch",
        ),
        (
            "teamless_recipient_for_team_plan",
            ReminderKind::Worker,
            |f| f.edit_plan(|plan| plan.team_id = Some("plan-team".into())),
            "recipient_scope_mismatch",
        ),
    ];
    for (name, kind, mutate, reason) in scopes {
        let f = fixture(kind, approve());
        mutate(&f);
        assert_eq!(
            f.admit(kind, 100),
            ReminderOutcome::Suppressed(reason),
            "{name}"
        );
        assert_eq!((f.model.calls(), f.emits()), (0, 0), "{name}");
    }
    let f = fixture(ReminderKind::Worker, approve());
    f.edit_plan(|plan| plan.team_id = Some("plan-team".into()));
    f.edit_tab(&f.worker, |tab| tab.team_id = Some("plan-team".into()));
    assert_eq!(
        f.admit(ReminderKind::Worker, 100),
        ReminderOutcome::Accepted
    );
}

#[test]
fn plan_reminder_choice_unchanged_observation_no_repeat() {
    for kind in SUPPORTER_ONLY {
        let f = fixture(kind, approve());
        assert_eq!(f.admit(kind, 100), ReminderOutcome::Accepted);
        f.edit_plan(|plan| {
            plan.updated_at = 999;
            plan.tasks[0].updated_at = 999;
            plan.tasks[0].status_changed_at = 999;
        });
        for now in [101, 100 + DAY, 100 + 30 * DAY] {
            let outcome = f.admit(kind, now);
            assert_eq!(
                outcome,
                ReminderOutcome::Suppressed("same_snapshot"),
                "{kind:?}"
            );
        }
        assert_eq!((f.model.calls(), f.emits()), (1, 1), "{kind:?}");
        f.change(kind, "meaningful");
        assert_eq!(f.admit(kind, 100 + 31 * DAY), ReminderOutcome::Accepted);
        assert_eq!(
            f.admit(kind, 100 + 32 * DAY),
            ReminderOutcome::Suppressed("same_snapshot")
        );
        assert_eq!((f.model.calls(), f.emits()), (2, 2), "{kind:?}");
        assert_eq!(f.telemetry(kind), (4, 2, 2, 0), "{kind:?}");
    }
}

#[test]
fn plan_reminder_choice_stale_scope_and_disconnect_refuse() {
    let cases: [(&str, ReminderKind, fn(&Fixture)); 17] = [
        ("owner_rebound", ReminderKind::Owner, |f| {
            f.edit_tab(&f.owner, |tab| bind(tab, "owner-session", "rotated"));
        }),
        ("owner_reassigned", ReminderKind::StalledTeam, |f| {
            let spare = f.spare.clone();
            f.edit_plan(|plan| plan.owner = OrchestratorRef::from_tab(&spare));
        }),
        ("plan_paused", ReminderKind::Owner, |f| {
            f.edit_plan(|plan| plan.mode = PlanMode::Paused);
        }),
        ("plan_reopened", ReminderKind::CompleteWrapup, |f| {
            f.edit_plan(|plan| plan.tasks[0].status = TaskStatus::InProgress);
        }),
        ("plan_resumed", ReminderKind::ModeChange, |f| {
            f.edit_plan(|plan| plan.mode = PlanMode::Execution);
        }),
        ("attempt_closed", ReminderKind::Worker, |f| {
            f.edit_plan(|plan| plan.tasks[0].execution_attempts[0].closed_at = Some(1));
        }),
        ("worker_rebound", ReminderKind::Worker, |f| {
            f.edit_tab(&f.worker, |tab| bind(tab, "worker-session", "rotated"));
        }),
        ("adapter_disconnected", ReminderKind::Owner, |f| {
            f.host.registry.unregister("fake");
        }),
        ("adapter_reconfigured", ReminderKind::Worker, |f| {
            f.host.token_usable.store(false, Ordering::SeqCst);
        }),
        ("scope_cleaned_up", ReminderKind::StalledTeam, |f| {
            f.host.gate.retain(&HashSet::new());
        }),
        ("owner_moved_workspace", ReminderKind::Owner, |f| {
            let mut mux = f.host.mux.lock();
            let from = mux.find_workspace_for_tab(&f.owner).unwrap();
            let to = mux.create_workspace("moved".into(), None).id;
            mux.move_tab(&f.owner, &from, &to).unwrap();
        }),
        ("owner_moved_team", ReminderKind::Owner, |f| {
            f.edit_tab(&f.owner, |tab| tab.team_id = Some("moved-team".into()));
        }),
        ("worker_re_roled", ReminderKind::Worker, |f| {
            f.edit_tab(&f.worker, |tab| {
                tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
            });
        }),
        (
            "adapter_reconnected_same_instance",
            ReminderKind::Owner,
            |f| {
                f.host.registry.unregister("fake");
                f.host.runtime.connection_changed();
                let same = f.model.clone() as Arc<dyn DecisionModel>;
                f.host.registry.register("fake", same);
                f.host.runtime.connection_changed();
            },
        ),
        (
            "adapter_replaced_same_id",
            ReminderKind::CompleteWrapup,
            |f| {
                let replacement = FakeChoice::new(approve()) as Arc<dyn DecisionModel>;
                f.host.registry.register("fake", replacement);
            },
        ),
        ("owner_state_changed", ReminderKind::StalledTeam, |f| {
            let mut view = f.host.owner_view.lock();
            view.delta.insert("task:task-a".into(), "changed".into());
        }),
        ("worker_task_changed", ReminderKind::Worker, |f| {
            f.edit_plan(|plan| plan.tasks[0].status_label = Some("changed".into()));
        }),
    ];
    for (name, kind, mutate) in cases {
        let f = fixture(kind, approve());
        let outcome = during_model_call(&f, kind, 100, || mutate(&f));
        let reason = if name.ends_with("_changed") {
            "stale_observation"
        } else {
            "stale_scope"
        };
        assert_eq!(outcome, ReminderOutcome::Suppressed(reason), "{name}");
        assert_eq!((f.model.calls(), f.emits()), (1, 0), "{name}");
        let (_, attempted, accepted, _) = f.telemetry(kind);
        assert_eq!((attempted, accepted), (0, 0), "{name}");
    }
    let during_compose: [(&str, ReminderKind, fn(&FakeHost)); 3] = [
        ("compose_disconnect", ReminderKind::Worker, |host| {
            host.registry.unregister("fake");
            host.runtime.connection_changed();
        }),
        (
            "compose_reconnect_same_instance",
            ReminderKind::Owner,
            |host| {
                host.runtime.connection_changed();
            },
        ),
        ("compose_plan_paused", ReminderKind::StalledTeam, |host| {
            host.plans.lock()[0].mode = PlanMode::Paused;
        }),
    ];
    for (name, kind, mutate) in during_compose {
        let f = fixture(kind, approve());
        *f.host.compose_hook.lock() = Some(mutate);
        let outcome = f.admit(kind, 100);
        assert_eq!(
            outcome,
            ReminderOutcome::Suppressed("stale_scope"),
            "{name}"
        );
        assert!(
            f.host.compose_hook.lock().is_none(),
            "{name} never composed"
        );
        assert_eq!((f.model.calls(), f.emits()), (1, 0), "{name}");
        let (_, attempted, accepted, _) = f.telemetry(kind);
        assert_eq!((attempted, accepted), (0, 0), "{name}");
    }
}

#[test]
fn plan_reminder_choice_producer_paths_and_excluded_notices() {
    for (reply, emitted) in [
        (Reply::Choice("nothing_changed", 0.99), false),
        (approve(), true),
    ] {
        let (mut mux, owner) = MultiplexerState::new();
        let workspace = mux.list_workspaces()[0].id.clone();
        let tabs: Vec<String> = (0..5)
            .map(|_| mux.create_tab(&workspace).unwrap())
            .collect();
        let (worker, mode_owner, wrap_owner) = (&tabs[0], &tabs[1], &tabs[2]);
        let (paused_owner, archived_owner) = (&tabs[3], &tabs[4]);
        bind(
            mux.get_tab_mut(&owner).unwrap(),
            "owner-session",
            "owner-nonce",
        );
        bind(
            mux.get_tab_mut(worker).unwrap(),
            "worker-session",
            "worker-nonce",
        );
        bind(
            mux.get_tab_mut(mode_owner).unwrap(),
            "mode-session",
            "mode-n",
        );
        bind(
            mux.get_tab_mut(wrap_owner).unwrap(),
            "wrap-session",
            "wrap-n",
        );
        bind(
            mux.get_tab_mut(paused_owner).unwrap(),
            "paused-session",
            "paused-n",
        );
        bind(
            mux.get_tab_mut(archived_owner).unwrap(),
            "archived-session",
            "archived-n",
        );
        let mut paused = kind_plan(ReminderKind::CompleteWrapup, "plan-p", paused_owner, "x");
        paused.mode = PlanMode::Paused;
        let mut archived = kind_plan(ReminderKind::CompleteWrapup, "plan-x", archived_owner, "x");
        archived.status = PlanStatus::Archived;
        let plans = vec![
            kind_plan(ReminderKind::Owner, PLAN, &owner, worker),
            kind_plan(ReminderKind::ModeChange, "plan-m", mode_owner, "nobody"),
            kind_plan(ReminderKind::CompleteWrapup, "plan-w", wrap_owner, "nobody"),
            paused,
            archived,
        ];
        let (host, model) = host(plans.clone(), mux, reply);
        let baseline = BTreeMap::from([("mode".to_string(), "Execution:Active".to_string())]);
        host.gate.observed(
            &scope_key("plan-m", mode_owner),
            Observation {
                projection: Some(baseline),
                ..Default::default()
            },
        );
        let counter = TurnCounter::new();
        let retained = deadline_tabs(&plans, &counter);
        assert!(retained.contains(wrap_owner.as_str()));
        assert!(!retained.contains(paused_owner.as_str()));
        assert!(!retained.contains(archived_owner.as_str()));
        for now in [0, 1800, 3600] {
            run_reminder_tick(&host, &plans, &counter, now, 1800);
        }
        let tracker = StallEpisodeTracker::default();
        let stalled = PlanReminderTarget::Owner {
            plan_id: PLAN.into(),
        };
        let nudges: Vec<_> = (0..2)
            .map(|_| {
                maybe_nudge_stalled_team(&plans[0], &HashSet::new(), true, &tracker, |_| {
                    deliver_target(
                        &host,
                        &counter,
                        &owner,
                        &stalled,
                        NudgeKind::StalledTeam,
                        3600,
                    )
                })
            })
            .collect();
        assert_eq!(nudges, vec![Ok(emitted), Ok(false)]);
        run_reminder_tick(&host, &plans, &counter, 5400, 1800);
        let mut kinds: Vec<_> = host
            .emits
            .lock()
            .iter()
            .map(|emit| (emit.scope.kind, emit.scope.tab_id.clone()))
            .collect();
        kinds.sort();
        // The Owner floor fires at 1800, 3600 and 5400 whatever the verdict.
        let mut expected = vec![(ReminderKind::Owner, owner.clone()); 3];
        if emitted {
            expected.extend([
                (ReminderKind::Owner, owner.clone()),
                (ReminderKind::ModeChange, mode_owner.clone()),
                (ReminderKind::StalledTeam, owner.clone()),
                (ReminderKind::CompleteWrapup, wrap_owner.clone()),
                (ReminderKind::Worker, worker.clone()),
            ]);
        }
        expected.sort();
        assert_eq!(kinds, expected);
        assert_eq!(model.calls(), 5, "one verdict per producer kind");
        assert!(counter
            .time_reminder_telemetry(&owner)
            .last_delivery
            .is_some());
        assert_eq!(
            counter
                .time_reminder_telemetry(worker)
                .last_delivery
                .is_some(),
            emitted
        );
        assert_eq!(counter.complete_wrapup_due("plan-w"), !emitted);
        assert!(counter.complete_wrapup_due("plan-p") && counter.complete_wrapup_due("plan-x"));
    }
    let excluded = [
        ("planner gate delivery", include_str!("../gate_delivery.rs")),
        (
            "report and planner routing",
            include_str!("../../hierarchy.rs"),
        ),
        (
            "orchestrator notices",
            include_str!("../../orchestrator_digests.rs"),
        ),
        ("lane watchdog", include_str!("../../plan_reconcile.rs")),
    ];
    for (name, source) in excluded {
        for symbol in [
            "approval::admit",
            "ReminderRequest",
            "deliver_plan_reminder_target",
        ] {
            assert!(
                !source.contains(symbol),
                "{name} must stay outside reminder approval"
            );
        }
    }
    let producers = include_str!("../plan_reminder_nudge.rs");
    assert_eq!(producers.matches("approval::admit(").count(), 1);
}

#[test]
fn plan_reminder_choice_bounded_claim_cleanup_and_failure() {
    for kind in KINDS {
        let f = fixture(kind, approve());
        let mut duplicate = None;
        let first = during_model_call(&f, kind, 100, || duplicate = Some(f.admit(kind, 100)));
        assert_eq!(duplicate, Some(ReminderOutcome::Suppressed("in_flight")));
        assert_eq!(first, ReminderOutcome::Accepted, "{kind:?}");
        assert_eq!(
            f.admit(kind, 101),
            ReminderOutcome::Suppressed("same_snapshot")
        );
        assert_eq!((f.model.calls(), f.emits()), (1, 1), "{kind:?}");
        assert_eq!(f.telemetry(kind), (2, 1, 1, 0), "{kind:?}");
    }
    let f = fixture(ReminderKind::Owner, approve());
    for index in 0..256 {
        drop(f.host.gate.claim(&format!("filler:{index}")).unwrap());
    }
    assert_eq!(f.host.gate.scopes(), 256);
    assert_eq!(
        f.admit(ReminderKind::Owner, 100),
        ReminderOutcome::Suppressed("scope_capacity")
    );
    assert_eq!((f.model.calls(), f.emits()), (0, 0));
    f.host.gate.retain(&scope_keys(&f.host.plans.lock()));
    assert_eq!(f.host.gate.scopes(), 0);
    assert_eq!(f.admit(ReminderKind::Owner, 100), ReminderOutcome::Accepted);
    assert_eq!(f.host.gate.scopes(), 1);
    for kind in SUPPORTER_ONLY {
        let f = fixture(kind, approve());
        *f.host.emit_error.lock() = Some("opaque transport failure".into());
        assert_eq!(
            f.admit(kind, 100),
            ReminderOutcome::Failed("opaque transport failure".into())
        );
        *f.host.emit_error.lock() = None;
        assert_eq!(
            f.admit(kind, 100 + DAY),
            ReminderOutcome::Suppressed("same_snapshot")
        );
        assert_eq!((f.model.calls(), f.emits()), (1, 1), "{kind:?}");
        assert_eq!(f.telemetry(kind), (1, 1, 0, 1), "{kind:?}");
    }
}

mod blocked;
mod cadence;
mod floor;
