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
    let command = reserve_command_input(pty_id).unwrap();
    let command_owner = command.owner.clone();
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
    std::thread::scope(|scope| {
        let command = command;
        let command_cycle = command_cycle;
        let user_provider = provider.clone();
        let user = scope.spawn(move || {
            let _send = claim_submit_cycle(pty_id);
            let mut provider = user_provider.lock();
            provider.paste("opaque user payload");
            provider.enter();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut state = command_owner.state.lock();
        while state.waiting_sends == 0 {
            assert!(!command_owner
                .changed
                .wait_until(&mut state, deadline)
                .timed_out());
        }
        drop(state);
        assert!(provider.lock().submissions.is_empty());
        assert!(command_cycle.is_current());
        provider.lock().enter();
        drop(command_cycle);
        drop(command);
        user.join().unwrap();
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
    assert!(reserve_command_input(pty_id).is_err());
    let second = claim_submit_cycle(pty_id);
    assert!(!first.is_current());
    assert!(second.is_current());
    drop(first);
    assert!(reserve_command_input(pty_id).is_err());
    drop(second);
    assert!(reserve_command_input(pty_id).is_ok());
}

#[test]
fn command_release_on_error_allows_sends_and_other_ptys_stay_independent() {
    let pty_id = "model-command-failure-release";
    let command = reserve_command_input(pty_id).unwrap();
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
