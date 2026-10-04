use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use parking_lot::Mutex;

use super::*;
use crate::cron::store::{write_log, CronCreateParams, CronRunInfo};

const TAB: &str = "cron-tab";
const RUN: &str = "run-1";
const TERMINAL: &str = "dying-terminal-token";
const CLEAN_LOG: &[u8] = b"--- SUMMARY ---\nclean-summary-token\n";

struct Owned {
    _dir: tempfile::TempDir,
    json: PathBuf,
    cron: CronState,
    job_id: String,
}

fn owned_run() -> Owned {
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("crons.json");
    let mut store = CronStore::new_with_path(json.clone());
    let job_id = store
        .create(CronCreateParams {
            name: "fallback".into(),
            workspace_id: "ws".into(),
            team_id: None,
            detach_from_team: false,
            schedule: "0 * * * *".into(),
            task: "run".into(),
            description: None,
            agent: Some("claude".into()),
            model: None,
            reasoning_effort: None,
            cwd: None,
            role: None,
            skill: None,
            creator_tab_id: None,
            report_target_tab_id: None,
            enabled: None,
            max_history: None,
            oneshot: None,
            jitter_max_ms: Some(0),
            tz: None,
            timeout_seconds: None,
        })
        .unwrap()
        .id;
    store
        .record_run(
            &job_id,
            CronRunInfo {
                run_id: RUN.into(),
                started_at: 1,
                finished_at: None,
                status: CronRunStatus::Running,
                tab_id: Some(TAB.into()),
                session_id: None,
                cwd: None,
                summary: None,
                report_delivery: None,
            },
        )
        .unwrap();
    store.register_tab(TAB, &job_id, RUN);
    Owned {
        _dir: dir,
        json,
        cron: Arc::new(Mutex::new(store)),
        job_id,
    }
}

impl Owned {
    fn log_path(&self) -> PathBuf {
        self.cron.lock().log_file(&self.job_id, RUN).unwrap()
    }

    fn status(&self) -> CronRunStatus {
        self.cron.lock().get_history(&self.job_id)[0].status.clone()
    }

    fn capture_unlocked(&self) -> impl FnOnce() -> String + '_ {
        move || {
            assert!(
                self.cron.try_lock().is_some(),
                "capture runs under the cron lock"
            );
            TERMINAL.to_string()
        }
    }
}

#[test]
fn cron_pty_death_fallback_fails_run_with_terminal_log() {
    let run = owned_run();
    let notified = Mutex::new(Vec::new());

    let settled = settle(&run.cron, TAB, run.capture_unlocked(), |job| {
        notified.lock().push(job.to_string())
    });

    assert_eq!(settled, Some((run.job_id.clone(), RUN.to_string())));
    assert_eq!(
        std::fs::read_to_string(run.log_path()).unwrap(),
        crate::pty::ansi::clean_terminal_output(TERMINAL)
    );
    assert_eq!(run.status(), CronRunStatus::Failed);
    assert!(run.cron.lock().get_tab_owner(TAB).is_none());
    assert_eq!(*notified.lock(), vec![run.job_id.clone()]);
    let reloaded = CronStore::new_with_path(run.json.clone());
    assert_eq!(
        reloaded.get_history(&run.job_id)[0].status,
        CronRunStatus::Failed
    );
}

#[test]
fn cron_pty_death_fallback_keeps_a_concurrent_clean_log() {
    let run = owned_run();
    let reached = Arc::new(Barrier::new(2));
    let completed = Arc::new(Barrier::new(2));
    let completer = {
        let (cron, job_id) = (run.cron.clone(), run.job_id.clone());
        let (reached, completed) = (reached.clone(), completed.clone());
        std::thread::spawn(move || {
            reached.wait();
            let path = cron.lock().log_file(&job_id, RUN).unwrap();
            let written = write_log(&path, std::str::from_utf8(CLEAN_LOG).unwrap());
            let finished = finish_run(&cron, &job_id, RUN, CronRunStatus::Success, None);
            completed.wait();
            (written, finished)
        })
    };
    let notified = Mutex::new(0);

    let capture = run.capture_unlocked();
    let settled = settle(
        &run.cron,
        TAB,
        || {
            let text = capture();
            reached.wait();
            completed.wait();
            text
        },
        |_| *notified.lock() += 1,
    );
    let (written, finished) = completer.join().unwrap();

    written.unwrap();
    assert!(
        finished.unwrap(),
        "the completion path owns the fresh transition"
    );
    assert!(settled.is_some());
    assert_eq!(std::fs::read(run.log_path()).unwrap(), CLEAN_LOG);
    assert_eq!(run.status(), CronRunStatus::Success);
    assert!(run.cron.lock().get_tab_owner(TAB).is_none());
    assert_eq!(*notified.lock(), 0);
}

#[test]
fn cron_pty_death_fallback_notifies_only_on_first_terminal_transition() {
    let run = owned_run();
    assert!(finish_run(&run.cron, &run.job_id, RUN, CronRunStatus::Failed, None).unwrap());
    let notified = Mutex::new(0);

    let settled = settle(&run.cron, TAB, run.capture_unlocked(), |_| {
        *notified.lock() += 1
    });

    assert!(settled.is_some());
    assert!(run.log_path().exists());
    assert_eq!(run.status(), CronRunStatus::Failed);
    assert_eq!(*notified.lock(), 0);
}

#[test]
fn cron_pty_death_fallback_ignores_a_tab_without_a_cron_run() {
    let run = owned_run();
    let notified = Mutex::new(0);

    let settled = settle(
        &run.cron,
        "unowned-tab",
        || panic!("an unowned tab captures no terminal text"),
        |_| *notified.lock() += 1,
    );

    assert!(settled.is_none());
    assert!(!run.log_path().exists());
    assert_eq!(run.status(), CronRunStatus::Running);
    assert!(run.cron.lock().get_tab_owner(TAB).is_some());
    assert_eq!(*notified.lock(), 0);
}

#[test]
fn cron_pty_death_fallback_create_log_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("job").join("run.log");

    assert!(CronStore::create_log(&path, "first").unwrap());
    std::fs::write(&path, CLEAN_LOG).unwrap();
    assert!(!CronStore::create_log(&path, "second").unwrap());

    assert_eq!(std::fs::read(&path).unwrap(), CLEAN_LOG);
    let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, vec![std::ffi::OsString::from("run.log")]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn cron_settle_pty_death_fallback_finishes_once_off_runtime() {
    let run = owned_run();
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let writer = run.cron.lock();

    let settlement = fail_run_on_pty_death(&fixture.service, &run.cron, TAB, "pty-1")
        .expect("owned run starts settlement");
    drop(writer);
    settlement.join().unwrap();

    assert_eq!(run.status(), CronRunStatus::Failed);
    assert!(run.log_path().exists());
    assert!(run.cron.lock().get_tab_owner(TAB).is_none());
    let reloaded = CronStore::new_with_path(run.json.clone());
    assert_eq!(reloaded.get_history(&run.job_id).len(), 1);
    assert_eq!(
        reloaded.get_history(&run.job_id)[0].status,
        CronRunStatus::Failed
    );
}
