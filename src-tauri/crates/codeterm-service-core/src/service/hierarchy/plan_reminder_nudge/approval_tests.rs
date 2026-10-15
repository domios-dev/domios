use super::approval::{admit, scope_key, scope_keys, ReminderOutcome};
use super::tests::{plan, task};
use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{OrchestratorRef, TaskExecutionAttempt};
use crate::service::hierarchy::heartbeat_gate::{Telemetry, Worker};
use crate::service::supporter::SupporterRuntime;
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;

const KINDS: [ReminderKind; 5] = [
    ReminderKind::Owner,
    ReminderKind::ModeChange,
    ReminderKind::StalledTeam,
    ReminderKind::CompleteWrapup,
    ReminderKind::Worker,
];
const PLAN: &str = "plan-a";

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

    fn journal_runtime(&self) -> Arc<SupporterRuntime> {
        self.runtime.clone()
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
    owner: String,
    worker: String,
}

fn host(plans: Vec<Plan>, mux: MultiplexerState) -> FakeHost {
    let dir = tempfile::tempdir().unwrap();
    let host = FakeHost {
        gate: Gate::default(),
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
    host
}

fn fixture(kind: ReminderKind) -> Fixture {
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
    let host = host(plans, mux);
    Fixture {
        host,
        owner,
        worker,
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

#[test]
fn plan_reminder_rule_delivers_owner_without_supporter_and_journals_rule() {
    let f = fixture(ReminderKind::Owner);
    assert_eq!(f.admit(ReminderKind::Owner, 100), ReminderOutcome::Accepted);
    assert_eq!(
        f.admit(ReminderKind::Owner, 10000),
        ReminderOutcome::Suppressed("same_snapshot")
    );
    assert_eq!(f.emits(), 1);
    let decisions = f.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0]["decided_by"], "rule");
    assert!(decisions[0]["model_id"].is_null());
    assert_eq!(f.telemetry(ReminderKind::Owner), (1, 1, 1, 0));
}

#[test]
fn plan_reminder_rule_worker_needs_new_task_evidence() {
    let f = fixture(ReminderKind::Worker);
    assert_eq!(
        f.admit(ReminderKind::Worker, 100),
        ReminderOutcome::Suppressed("healthy_snapshot")
    );
    f.change(ReminderKind::Worker, "changed");
    assert_eq!(
        f.admit(ReminderKind::Worker, 101),
        ReminderOutcome::Accepted
    );
    assert_eq!(
        f.admit(ReminderKind::Worker, 10000),
        ReminderOutcome::Suppressed("same_snapshot")
    );
    assert_eq!(f.emits(), 1);
}

#[test]
fn plan_reminder_rule_scope_is_revalidated_at_handoff() {
    let f = fixture(ReminderKind::Owner);
    *f.host.compose_hook.lock() = Some(|host| host.plans.lock()[0].mode = PlanMode::Paused);
    assert_eq!(
        f.admit(ReminderKind::Owner, 100),
        ReminderOutcome::Suppressed("stale_scope")
    );
    assert_eq!(f.emits(), 0);
}

#[test]
fn plan_reminder_rule_failed_delivery_is_not_retried_for_same_snapshot() {
    let f = fixture(ReminderKind::Owner);
    *f.host.emit_error.lock() = Some("opaque failure".into());
    assert_eq!(
        f.admit(ReminderKind::Owner, 100),
        ReminderOutcome::Failed("opaque failure".into())
    );
    *f.host.emit_error.lock() = None;
    assert_eq!(
        f.admit(ReminderKind::Owner, 10000),
        ReminderOutcome::Suppressed("same_snapshot")
    );
    assert_eq!(f.telemetry(ReminderKind::Owner), (1, 1, 0, 1));
}

#[test]
fn plan_reminder_rule_capacity_is_bounded_and_cleanup_releases_it() {
    let f = fixture(ReminderKind::Owner);
    for index in 0..256 {
        drop(f.host.gate.claim(&format!("filler:{index}")).unwrap());
    }
    assert_eq!(
        f.admit(ReminderKind::Owner, 100),
        ReminderOutcome::Suppressed("scope_capacity")
    );
    f.host.gate.retain(&scope_keys(&f.host.plans.lock()));
    assert_eq!(f.host.gate.scopes(), 0);
    assert_eq!(f.admit(ReminderKind::Owner, 101), ReminderOutcome::Accepted);
}

#[test]
fn plan_reminder_rule_preserves_all_owner_producer_kinds() {
    for kind in KINDS
        .into_iter()
        .filter(|kind| *kind != ReminderKind::Worker)
    {
        let f = fixture(kind);
        assert_eq!(f.admit(kind, 100), ReminderOutcome::Accepted, "{kind:?}");
        assert_eq!(
            f.admit(kind, 10000),
            ReminderOutcome::Suppressed("same_snapshot")
        );
        assert_eq!(f.emits(), 1);
    }
}

#[test]
fn plan_reminder_rule_rejects_unbound_worker_and_blocked_task() {
    let f = fixture(ReminderKind::Worker);
    f.edit_plan(|plan| plan.tasks[0].status = TaskStatus::Blocked);
    assert_eq!(
        f.admit(ReminderKind::Worker, 100),
        ReminderOutcome::Suppressed("recipient_not_actionable")
    );
    f.edit_tab(&f.worker, |tab| tab.reported_session_binding_nonce = None);
    assert_eq!(
        f.admit(ReminderKind::Worker, 101),
        ReminderOutcome::Suppressed("binding_missing")
    );
    assert_eq!(f.emits(), 0);
}
