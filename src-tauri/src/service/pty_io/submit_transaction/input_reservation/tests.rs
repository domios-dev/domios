use super::*;
use crate::service::pty_io::submit_transaction::{claim_command_submit_cycle, claim_submit_cycle};
use std::time::{Duration, Instant};

#[test]
fn ordering_holds_sends_only_for_commands_and_refuses_commands_during_sends() {
    assert_eq!(admission(0, false, false), Admission::Send);
    assert_eq!(admission(2, false, false), Admission::Send);
    assert_eq!(admission(0, true, false), Admission::Wait);
    assert_eq!(admission(0, false, true), Admission::Command);
    assert_eq!(admission(1, false, true), Admission::Refuse);
    assert_eq!(admission(0, true, true), Admission::Refuse);
}

#[test]
fn live_command_and_concurrent_send_reach_provider_as_separate_submissions() {
    let pty_id = "model-command-concurrent-send";
    let command = reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).unwrap();
    #[derive(Default)]
    struct Provider {
        draft: String,
        submissions: Vec<String>,
    }
    impl Provider {
        fn paste(&mut self, text: &str) {
            self.draft.push_str(text);
        }
        fn enter(&mut self) {
            self.submissions.push(std::mem::take(&mut self.draft));
        }
    }
    let provider = Arc::new(Mutex::new(Provider::default()));
    let command_cycle = claim_command_submit_cycle(pty_id);
    provider.lock().paste("/model selected");
    runtime().block_on(async {
        let user_provider = provider.clone();
        let (waiting, observed_wait) = tokio::sync::oneshot::channel();
        let user = tokio::spawn(async move {
            let blocked = claim_submit_cycle(pty_id);
            let wait = blocked.command_wait().expect("command must hold user send");
            assert!(!blocked.is_current());
            waiting.send(()).unwrap();
            assert!(wait.completed().await);
            let send = claim_submit_cycle(pty_id);
            assert!(send.command_wait().is_none());
            let mut provider = user_provider.lock();
            provider.paste("opaque user payload");
            provider.enter();
        });
        observed_wait.await.unwrap();
        assert!(provider.lock().submissions.is_empty());
        assert!(command_cycle.is_current());
        provider.lock().enter();
        drop(command_cycle);
        drop(command);
        user.await.unwrap();
    });
    assert_eq!(
        provider.lock().submissions,
        vec!["/model selected", "opaque user payload"]
    );
}

#[test]
fn send_claims_refuse_switches_without_blocking_newer_sends() {
    let pty_id = "model-refuses-active-send";
    let first = claim_submit_cycle(pty_id);
    assert!(reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).is_err());
    let second = claim_submit_cycle(pty_id);
    assert!(!first.is_current());
    assert!(second.is_current());
    drop(first);
    assert!(reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).is_err());
    drop(second);
    assert!(reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).is_ok());
}

#[test]
fn command_release_on_error_allows_sends_and_other_ptys_stay_independent() {
    let pty_id = "model-command-failure-release";
    let command = reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).unwrap();
    let unrelated = claim_submit_cycle("model-command-unrelated-pty");
    assert!(unrelated.is_current());
    let failed = (|| -> Result<(), ()> {
        let _command = command;
        Err(())
    })();
    assert!(failed.is_err());
    let send = claim_submit_cycle(pty_id);
    assert!(send.is_current());
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}

#[test]
fn command_failure_releases_an_awaiting_send() {
    let pty_id = "model-failed-command-waiter";
    let command = reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).unwrap();
    let blocked = claim_submit_cycle(pty_id);
    let wait = blocked.command_wait().unwrap();
    runtime().block_on(async {
        let (waiting, observed) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            waiting.send(()).unwrap();
            wait.completed().await
        });
        observed.await.unwrap();
        let failure = (|| -> Result<(), ()> {
            let _command = command;
            Err(())
        })();
        assert!(failure.is_err());
        assert!(task.await.unwrap());
        assert!(claim_submit_cycle(pty_id).command_wait().is_none());
    });
}

#[test]
fn pty_exit_releases_waiting_send_without_stale_guard_releasing_a_new_command() {
    let pty_id = "model-command-dead-pty";
    let command = reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).unwrap();
    let wait = claim_submit_cycle(pty_id).command_wait().unwrap();
    runtime().block_on(async {
        let (waiting, observed) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            waiting.send(()).unwrap();
            wait.completed().await
        });
        observed.await.unwrap();
        crate::service::pty_io::submit_transaction::notify_submit_cycle_provider_exit(pty_id);
        assert!(task.await.unwrap());
        let replacement =
            reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).unwrap();
        drop(command);
        assert!(claim_submit_cycle(pty_id).command_wait().is_some());
        drop(replacement);
        assert!(claim_submit_cycle(pty_id).command_wait().is_none());
    });
}

#[test]
fn completion_deadline_refuses_without_authorizing_a_write() {
    let pty_id = "model-command-wait-deadline";
    let command = reserve_command_input(pty_id, Instant::now()).unwrap();
    let blocked = claim_submit_cycle(pty_id);
    assert!(!runtime().block_on(blocked.command_wait().unwrap().completed()));
    assert!(!blocked.is_current());
    assert!(claim_submit_cycle(pty_id).command_wait().is_some());
    drop(command);
    assert!(claim_submit_cycle(pty_id).command_wait().is_none());
}

#[test]
fn panic_releases_command_input() {
    let pty_id = "model-command-panic";
    let command = reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).unwrap();
    let wait = claim_submit_cycle(pty_id).command_wait().unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _command = command;
            panic!("injected command failure");
        }))
        .is_err()
    );
    assert!(runtime().block_on(wait.completed()));
    assert!(claim_submit_cycle(pty_id).command_wait().is_none());
}

#[test]
fn command_completion_notifies_owner_even_after_wait_deadline() {
    let pty_id = "model-command-late-completion";
    let (notify, notified) = tokio::sync::oneshot::channel();
    let command = reserve_command_input(pty_id, Instant::now())
        .unwrap()
        .on_completion(move || {
            notify.send(()).unwrap();
        });
    let wait = claim_submit_cycle(pty_id).command_wait().unwrap();
    assert!(!runtime().block_on(wait.completed()));
    assert!(notified.is_empty());
    drop(command);
    runtime().block_on(notified).unwrap();
    assert!(claim_submit_cycle(pty_id).command_wait().is_none());
}

#[test]
fn each_command_schedules_one_waiter_per_user_send() {
    let pty_id = "model-command-wait-dedup";
    let command = reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).unwrap();
    let wait = claim_submit_cycle(pty_id).command_wait().unwrap();
    assert!(wait.schedule_user("send-1"));
    assert!(!wait.clone().schedule_user("send-1"));
    assert!(wait.schedule_user("send-2"));
    drop(command);
    let next = reserve_command_input(pty_id, Instant::now() + Duration::from_secs(2)).unwrap();
    assert!(claim_submit_cycle(pty_id)
        .command_wait()
        .unwrap()
        .schedule_user("send-1"));
    drop(next);
}
