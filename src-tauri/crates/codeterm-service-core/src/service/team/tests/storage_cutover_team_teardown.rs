use crate::service::TeamExt;
use std::sync::Arc;
use std::time::Duration;

use crate::cron::clock::MockClock;
use crate::cron::scheduler::CronState;
use crate::cron::store::{CronStorageOptions, CronStore};
use crate::service::team::storage_teardown::StorageTeardownSubmitted;
use crate::service::team::TeamMutationCaller;
use crate::service::test_fixture::ServiceFixture;
use crate::storage::{
    CronJobId, CronJobRecord, CronRunId, CronRunRecord, CronRunState, DeskAgentSessionIdentity,
    DeskLayoutNode, DeskLeafFields, DeskSnapshot, DeskTab, DeskTeam, DeskTeamLifecycle,
    DeskWorkspace, Fault, Storage, StorageConfig, WorkspaceId, WorkspaceRecord,
};
use codeterm_events::SystemEvent;
use codeterm_sentinel::store::{Create, Owner, Store};
use parking_lot::Mutex;
use tauri::Manager;

const DEADLINE: Duration = Duration::from_secs(30);

struct TeamStorageFixture {
    service: ServiceFixture,
    storage: Storage,
    cron: CronState,
    sentinel: Arc<Mutex<Store>>,
    team_id: String,
    protected_cron_id: String,
    ordinary_cron_id: String,
    sentinel_id: String,
}

fn team_storage_fixture() -> TeamStorageFixture {
    let (mux, team_id, orchestrator, _, _) = super::team_members();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let service = ServiceFixture::new(mux);
    let events: codeterm_events::EventBroadcasterState = service.service.events.clone();
    assert!(service._app.manage(events));
    let storage = Storage::open(
        service.temp.path().join("instance"),
        StorageConfig::default(),
    )
    .unwrap();
    storage
        .desk()
        .import(desk_snapshot(&team_id, &workspace_id, &orchestrator))
        .wait(DEADLINE)
        .unwrap();

    let mut protected = cron_record("protected-team-cron", &workspace_id, &team_id, true);
    let created = storage
        .crons()
        .create(protected.clone())
        .wait(DEADLINE)
        .unwrap();
    let run_id = CronRunId::new("team-cron-history").unwrap();
    protected.last_run_id = Some(run_id.clone());
    storage
        .crons()
        .start_run(
            protected,
            CronRunRecord {
                run_id,
                started_at: 10,
                finished_at: None,
                status: CronRunState::Running,
                tab_id: Some("closed-cron-tab".into()),
                session_id: None,
                cwd: None,
                summary: None,
                report_delivery: None,
            },
            created.receipt.revision,
        )
        .wait(DEADLINE)
        .unwrap();
    let ordinary_cron_id = "ordinary-team-cron".to_string();
    storage
        .crons()
        .create(cron_record(
            &ordinary_cron_id,
            &workspace_id,
            &team_id,
            false,
        ))
        .wait(DEADLINE)
        .unwrap();
    let cron = Arc::new(Mutex::new(
        CronStore::open_storage(
            &storage,
            service.temp.path().join("crons"),
            MockClock::starting_at(100),
            CronStorageOptions::default(),
            crate::config::schema::AppConfig::default()
                .general_agent
                .agent_provider,
        )
        .unwrap(),
    ));

    let sentinel_store = Store::with_storage(&storage, MockClock::starting_at(100));
    let sentinel = Arc::new(Mutex::new(sentinel_store));
    let sentinel_job = sentinel
        .lock()
        .prepare(
            Create {
                terminal: None,
                session: false,
                name: Some("Team teardown sentinel".into()),
                probe: Some("status".into()),
                observe: None,
                input: None,
                question: "Did the state change?".into(),
                buckets: vec!["waiting".into(), "passed".into(), "failed".into()],
                every: "20s".into(),
                routes: Default::default(),
                matches: [("waiting".to_string(), ".*".to_string())]
                    .into_iter()
                    .collect(),
                rearm_on_change: Default::default(),
                until: None,
                timeout: Some("1h".into()),
                paused: true,
                handlers: None,
                model_classification: false,
                max_transitions: None,
                max_actions: None,
                escalation_timeout_ms: None,
            },
            Owner {
                creator_tab_id: orchestrator.clone(),
                team_id: Some(team_id.clone()),
                workspace_id: workspace_id.clone(),
                cwd: service.temp.path().display().to_string(),
            },
            false,
        )
        .unwrap();
    let sentinel_id =
        codeterm_sentinel::storage::commit(&sentinel, |store| store.stage_insert(sentinel_job))
            .unwrap()
            .id;
    let wake = Arc::new(std::sync::Condvar::new());
    assert!(service._app.manage(storage.clone()));
    assert!(service._app.manage(cron.clone()));
    assert!(service._app.manage(wake));
    super::sentinel_fixture::manage(&service._app.handle().clone(), &sentinel);

    TeamStorageFixture {
        service,
        storage,
        cron,
        sentinel,
        team_id,
        protected_cron_id: "protected-team-cron".into(),
        ordinary_cron_id,
        sentinel_id,
    }
}

fn desk_snapshot(team_id: &str, workspace_id: &str, tab_id: &str) -> DeskSnapshot {
    DeskSnapshot {
        version: "1.12.4".into(),
        timestamp: 1,
        active_workspace_id: Some(workspace_id.into()),
        workspaces: vec![DeskWorkspace {
            record: WorkspaceRecord {
                id: WorkspaceId::new(workspace_id).unwrap(),
                name: "Team workspace".into(),
                cwd: Some("C:/team".into()),
                provisioned_cwd: None,
                protected: false,
                task_key_prefix: None,
                task_key_next: 0,
            },
            sort_order: 0,
            active_tab_id: Some(tab_id.into()),
            tabs: vec![DeskTab {
                id: tab_id.into(),
                title: "Orchestrator".into(),
                description: None,
                sort_order: 0,
                layout: DeskLayoutNode::Leaf {
                    tab_id: tab_id.into(),
                    cwd: Some("C:/team".into()),
                    fields: DeskLeafFields::default(),
                },
            }],
        }],
        teams: vec![DeskTeam {
            id: team_id.into(),
            name: Some("Storage teardown".into()),
            lifecycle: DeskTeamLifecycle::Active,
            created_at: 1,
            workspace_id: workspace_id.into(),
        }],
        agent_session_identities: Vec::<DeskAgentSessionIdentity>::new(),
    }
}

fn cron_record(id: &str, workspace_id: &str, team_id: &str, protected: bool) -> CronJobRecord {
    CronJobRecord {
        id: CronJobId::new(id).unwrap(),
        name: id.into(),
        description: None,
        workspace_id: workspace_id.into(),
        team_id: Some(team_id.into()),
        schedule: "0 * * * *".into(),
        enabled: true,
        agent: "claude".into(),
        model: None,
        reasoning_effort: None,
        task: "task".into(),
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        oneshot: false,
        max_history: 10,
        created_at: 1,
        last_run_id: None,
        next_run: None,
        jitter_max_ms: None,
        tz: None,
        timeout_seconds: None,
        consecutive_failures: 0,
        protected,
    }
}

fn assert_domain_rows(fixture: &TeamStorageFixture, present: bool) {
    let desk = fixture.storage.desk().snapshot().wait(DEADLINE).unwrap();
    assert_eq!(
        desk.value
            .snapshot
            .teams
            .iter()
            .any(|team| team.id == fixture.team_id),
        present
    );
    let crons = fixture.storage.crons().snapshot().wait(DEADLINE).unwrap();
    assert_eq!(
        crons
            .value
            .states
            .iter()
            .any(|state| state.job_id.as_str() == fixture.protected_cron_id),
        present
    );
    assert_eq!(
        crons
            .value
            .states
            .iter()
            .any(|state| state.job_id.as_str() == fixture.ordinary_cron_id),
        present
    );
    let sentinels = fixture
        .storage
        .sentinels()
        .inventory()
        .wait(DEADLINE)
        .unwrap();
    assert_eq!(
        sentinels
            .value
            .iter()
            .any(|sentinel| { sentinel.record.identity.id.as_str() == fixture.sentinel_id }),
        present
    );
}

#[test]
fn storage_cutover_team_teardown_commits_all_domains_and_publishes_after_commit() {
    let fixture = team_storage_fixture();
    assert_eq!(
        fixture
            .cron
            .lock()
            .get_history(&fixture.protected_cron_id)
            .len(),
        1
    );
    let mut events = fixture.service.service.events.subscribe();
    let (submitted, settled) = std::sync::mpsc::channel();
    assert!(fixture
        .service
        ._app
        .manage(StorageTeardownSubmitted(submitted)));
    let hold = fixture.storage.hold_writer().unwrap();
    hold.wait_held(DEADLINE).unwrap();
    let committed_before = fixture.storage.metrics().committed_operations;
    let service = fixture.service.service.clone();
    let team_id = fixture.team_id.clone();
    let close = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(service.close_team(&team_id, TeamMutationCaller::Human, true))
    });

    settled
        .recv_timeout(DEADLINE)
        .expect("teardown was submitted");
    fixture
        .storage
        .workspaces()
        .list()
        .wait(DEADLINE)
        .expect("an unrelated read is served while the writer is held");
    assert_domain_rows(&fixture, true);
    assert!(fixture
        .cron
        .lock()
        .get(&fixture.protected_cron_id)
        .is_some());
    assert!(fixture.sentinel.lock().get(&fixture.sentinel_id).is_some());
    assert!(fixture
        .service
        .service
        .mux
        .read()
        .get_team(&fixture.team_id)
        .is_some());
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    hold.release();
    close.join().unwrap().unwrap();
    assert_eq!(
        fixture.storage.metrics().committed_operations,
        committed_before + 1
    );
    assert_domain_rows(&fixture, false);
    assert!(fixture
        .cron
        .lock()
        .get(&fixture.protected_cron_id)
        .is_none());
    assert!(fixture
        .cron
        .lock()
        .get_history(&fixture.protected_cron_id)
        .is_empty());
    assert!(fixture.sentinel.lock().get(&fixture.sentinel_id).is_none());
    assert!(fixture
        .service
        .service
        .mux
        .read()
        .get_team(&fixture.team_id)
        .is_none());
    let mut saw_mux_publication = false;
    while let Ok(event) = events.try_recv() {
        saw_mux_publication |= matches!(event.event, SystemEvent::MuxStateChanged);
    }
    assert!(saw_mux_publication);
    fixture.storage.shutdown().wait(DEADLINE).unwrap();
}

#[tokio::test]
async fn storage_cutover_team_teardown_failure_preserves_domains_and_raises_one_incident() {
    let fixture = team_storage_fixture();
    let mut events = fixture.service.service.events.subscribe();
    fixture
        .storage
        .inject_fault(Fault::FailNextCommit)
        .wait(DEADLINE)
        .unwrap();
    let committed_before = fixture.storage.metrics().committed_operations;
    let error = fixture
        .service
        .service
        .close_team(&fixture.team_id, TeamMutationCaller::Human, true)
        .await
        .unwrap_err();
    assert!(error.contains("Team storage teardown failed"));
    assert_eq!(
        fixture.storage.metrics().committed_operations,
        committed_before
    );
    assert_domain_rows(&fixture, true);
    assert!(fixture
        .cron
        .lock()
        .get(&fixture.protected_cron_id)
        .is_some());
    assert_eq!(
        fixture
            .cron
            .lock()
            .get_history(&fixture.protected_cron_id)
            .len(),
        1
    );
    assert!(fixture.sentinel.lock().get(&fixture.sentinel_id).is_some());
    assert!(fixture
        .service
        .service
        .mux
        .read()
        .get_team(&fixture.team_id)
        .is_some());
    let incidents: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
        .filter(|event| {
            matches!(
                &event.event,
                SystemEvent::Generic { event_type, .. }
                    if event_type == "coordination-incident"
            )
        })
        .collect();
    assert_eq!(incidents.len(), 1);
    fixture.storage.shutdown().wait(DEADLINE).unwrap();
}
