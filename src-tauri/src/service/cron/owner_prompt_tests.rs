//! cron_owner_prompt_: owner-prompt state settles off the routing path on the Storage
//! completion's wake, in per-tab request order. Writer holds sequence it, never sleeps.

use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use codeterm_types::pty::{PendingPrompt, PromptKind, PromptOption};
use parking_lot::Mutex;
use tauri::Manager;

use super::OwnerPromptRequests;
use crate::cron::clock::system_clock;
use crate::cron::store::{
    commit, CronCommitError, CronCommitted, CronCreateParams, CronRunInfo, CronRunStatus,
    CronStorageOptions, CronStore,
};
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use crate::storage::{Fault, Hold, Storage, StorageConfig};
use crate::CronState;

/// Failure bound around event waits only; elapsed time never signals readiness.
const DEADLINE: Duration = Duration::from_secs(30);
const RUN: &str = "owner-prompt-run";

/// Storage shuts down before the temp root goes away.
struct CronFixture {
    state: CronState,
    storage: Storage,
    temp: tempfile::TempDir,
}

impl Drop for CronFixture {
    fn drop(&mut self) {
        let _ = self.storage.shutdown().wait(DEADLINE);
    }
}

impl CronFixture {
    /// One job whose running run is owned by `tab_id`.
    fn owned_by(tab_id: &str) -> CronFixture {
        let temp = tempfile::tempdir().expect("temp root");
        let storage = Storage::open(temp.path().join("instance"), StorageConfig::default())
            .expect("open storage");
        let store = CronStore::open_storage(
            &storage,
            logs_root(&temp),
            system_clock(),
            CronStorageOptions::default(),
        )
        .expect("hydrate cron storage");
        let state: CronState = Arc::new(Mutex::new(store));
        own_run(&state, tab_id);
        CronFixture {
            state,
            storage,
            temp,
        }
    }

    fn cached(&self) -> CronRunStatus {
        run_status(&self.state.lock())
    }

    fn committed(&self) -> CronRunStatus {
        let fresh = CronStore::open_storage(
            &self.storage,
            logs_root(&self.temp),
            system_clock(),
            CronStorageOptions::default(),
        )
        .expect("rehydrate cron storage");
        run_status(&fresh)
    }

    fn hold(&self) -> Hold {
        let hold = self.storage.hold_writer().expect("writer hold admitted");
        hold.wait_held(DEADLINE).expect("writer reached the hold");
        hold
    }

    /// The single writer has passed every write admitted before this call.
    fn pass_writer(&self) {
        self.hold().release();
    }
}

fn logs_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join("crons")
}

fn own_run(cron: &CronState, tab_id: &str) {
    let job_id = commit(cron, |store| store.prepare_create(params()))
        .expect("create job")
        .value
        .id;
    commit(cron, |store| {
        store.prepare_record_run(&job_id, running(tab_id))
    })
    .expect("record running run");
    cron.lock().register_tab(tab_id, &job_id, RUN);
}

fn params() -> CronCreateParams {
    serde_json::from_value(serde_json::json!({
        "name": "owner prompt",
        "workspace_id": "ws",
        "schedule": "0 * * * *",
        "task": "task",
        "agent": "claude",
        "jitter_max_ms": 0,
    }))
    .expect("create params")
}

fn running(tab_id: &str) -> CronRunInfo {
    CronRunInfo {
        run_id: RUN.to_string(),
        started_at: 1_700_000_000_000,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: Some(tab_id.to_string()),
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    }
}

fn run_status(store: &CronStore) -> CronRunStatus {
    store
        .find_run_by_id(RUN)
        .expect("owned run in history")
        .1
        .status
        .clone()
}

type Settled = (Result<bool, CronCommitError>, CronRunStatus);

/// Records each settlement with the cached run status at the moment it was reported.
fn recorder(
    cron: &CronState,
) -> (
    impl Fn(&str, Result<CronCommitted<bool>, CronCommitError>) + Clone + Send + 'static,
    mpsc::Receiver<Settled>,
) {
    let (sender, settled) = mpsc::channel();
    let cron = cron.clone();
    let report = move |_: &str, outcome: Result<CronCommitted<bool>, CronCommitError>| {
        let cached = run_status(&cron.lock());
        let _ = sender.send((outcome.map(|done| done.value), cached));
    };
    (report, settled)
}

fn prompt() -> PendingPrompt {
    PendingPrompt {
        request_id: None,
        question: "opaque question".into(),
        options: vec![PromptOption {
            label: "opaque".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

#[test]
fn cron_owner_prompt_request_returns_while_the_writer_is_held() {
    let cron = CronFixture::owned_by("held-tab");
    let (report, settled) = recorder(&cron.state);
    let requests = OwnerPromptRequests::default();
    let hold = cron.hold();
    let (returned_tx, returned) = mpsc::channel();
    let (state, queue) = (cron.state.clone(), requests.clone());
    std::thread::spawn(move || {
        queue.request(&state, "held-tab", true, report);
        let _ = returned_tx.send(());
    });

    returned
        .recv_timeout(DEADLINE)
        .expect("the request returns while COMMIT is held");
    assert_eq!(
        cron.cached(),
        CronRunStatus::Running,
        "nothing is published before COMMIT"
    );
    assert!(
        settled.try_recv().is_err(),
        "nothing is reported before COMMIT"
    );

    hold.release();
    assert_eq!(
        settled
            .recv_timeout(DEADLINE)
            .expect("settled on the completion's wake"),
        (Ok(true), CronRunStatus::AwaitingOwnerPrompt),
        "the cache is published before the change is reported"
    );
    assert_eq!(cron.committed(), CronRunStatus::AwaitingOwnerPrompt);
}

#[test]
fn cron_owner_prompt_true_then_false_commits_false_last() {
    let cron = CronFixture::owned_by("ordered-tab");
    let (report, settled) = recorder(&cron.state);
    let requests = OwnerPromptRequests::default();
    let hold = cron.hold();

    requests.request(&cron.state, "ordered-tab", true, report.clone());
    requests.request(&cron.state, "ordered-tab", false, report);
    assert!(
        settled.try_recv().is_err(),
        "both requests return with the first still uncommitted"
    );

    hold.release();
    assert_eq!(
        settled.recv_timeout(DEADLINE).expect("true settles"),
        (Ok(true), CronRunStatus::AwaitingOwnerPrompt)
    );
    assert_eq!(
        settled
            .recv_timeout(DEADLINE)
            .expect("false is admitted only after true settled"),
        (Ok(true), CronRunStatus::Running)
    );
    assert_eq!(
        cron.committed(),
        CronRunStatus::Running,
        "the later request commits last"
    );
}

#[test]
fn cron_owner_prompt_commit_failure_is_reported_unapplied_and_unpublished() {
    let cron = CronFixture::owned_by("failing-tab");
    let (report, settled) = recorder(&cron.state);
    let requests = OwnerPromptRequests::default();
    cron.storage
        .inject_fault(Fault::FailNextCommit)
        .wait(DEADLINE)
        .expect("arm commit fault");

    requests.request(&cron.state, "failing-tab", true, report.clone());
    let (outcome, cached) = settled.recv_timeout(DEADLINE).expect("failure settles");
    let error = outcome.expect_err("an injected commit failure");
    assert!(
        matches!(error, CronCommitError::NotApplied(_)) && !error.applied(),
        "a failed commit is never claimed as applied: {error:?}"
    );
    assert_eq!(cached, CronRunStatus::Running, "nothing is published");
    assert_eq!(cron.committed(), CronRunStatus::Running);

    requests.request(&cron.state, "failing-tab", true, report);
    assert_eq!(
        settled
            .recv_timeout(DEADLINE)
            .expect("a later request still settles"),
        (Ok(true), CronRunStatus::AwaitingOwnerPrompt)
    );
}

#[test]
fn cron_owner_prompt_held_writer_does_not_block_route_pending_prompt() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let cron = CronFixture::owned_by(&tab_id);
    fixture.service.app_handle.manage(cron.state.clone());
    let asked = prompt();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).expect("tab");
        tab.pending_prompt = Some(asked.clone());
        tab.pending_prompt_kind = Some(PromptKind::Provider);
    }
    let hold = cron.hold();
    let (returned_tx, returned) = mpsc::channel();
    let (service, routed) = (fixture.service.clone(), tab_id.clone());
    std::thread::spawn(move || {
        let _ =
            returned_tx.send(service.route_pending_prompt(&routed, &asked, PromptKind::Provider));
    });

    returned
        .recv_timeout(DEADLINE)
        .expect("prompt routing returns while COMMIT is held");
    assert_eq!(cron.cached(), CronRunStatus::Running);

    hold.release();
    cron.pass_writer();
    assert_eq!(
        cron.committed(),
        CronRunStatus::AwaitingOwnerPrompt,
        "the staged change commits once the writer moves"
    );
}

#[test]
fn cron_owner_prompt_json_backend_commits_before_returning() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let path = fixture.temp.path().join("crons.json");
    let cron: CronState = Arc::new(Mutex::new(CronStore::new_with_path(path.clone())));
    own_run(&cron, &tab_id);
    fixture.service.app_handle.manage(cron.clone());

    fixture.service.set_cron_owner_prompt_waiting(&tab_id, true);

    assert_eq!(
        run_status(&cron.lock()),
        CronRunStatus::AwaitingOwnerPrompt,
        "the JSON cache holds the change on return"
    );
    assert_eq!(
        run_status(&CronStore::new_with_path(path)),
        CronRunStatus::AwaitingOwnerPrompt,
        "and the file was saved before return"
    );
    assert!(
        fixture
            .service
            .app_handle
            .try_state::<OwnerPromptRequests>()
            .is_none(),
        "the JSON backend never stages"
    );
}
