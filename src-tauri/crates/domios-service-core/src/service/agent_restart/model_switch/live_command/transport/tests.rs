use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use super::super::{CommandPrompt, CommandTarget};
use super::*;
use crate::pty::write_queue::GuardedInputGate;
use crate::service::pty_io::submit_transaction::live_command_input;

#[derive(Default)]
struct Terminal {
    screen: String,
    writes: Vec<Vec<u8>>,
    user_turns: Vec<String>,
}

struct Writer(Arc<Mutex<Terminal>>);

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut terminal = self.0.lock().unwrap();
        terminal.writes.push(bytes.to_vec());
        match bytes {
            b"/choose one\r" => terminal.screen = "confirm one".into(),
            b"/choose two\r" => terminal.screen = "confirm two".into(),
            b"y\r" | b"\x1b" => terminal.screen.clear(),
            _ if terminal.screen.is_empty() => terminal
                .user_turns
                .push(String::from_utf8_lossy(bytes).into_owned()),
            _ => {}
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn prompt(model: &str) -> CommandPrompt {
    CommandPrompt::new(
        &crate::agent::provider::ModelSwitchPrompt {
            screen_regex: "^confirm {model}$".into(),
            response_keys: vec!["y\r".into()],
            cancel_keys: vec!["\u{1b}".into()],
        },
        &CommandTarget {
            tab_id: "tab".into(),
            pty_id: "pty".into(),
            provider: "provider".into(),
            session_id: "session".into(),
            binding_nonce: "nonce".into(),
            cwd: None,
            model: model.into(),
            readiness: None,
            display_name: model.into(),
        },
        None,
    )
    .unwrap()
}

#[test]
fn two_consecutive_switches_preserve_permits_and_allow_a_following_send() {
    let terminal = Arc::new(Mutex::new(Terminal::default()));
    let queue = WriteQueue::new(Box::new(Writer(terminal.clone())));
    let gate = GuardedInputGate::default();
    gate.rearm();
    let original_permit = gate.issue().unwrap();
    for model in ["one", "two"] {
        let injected = Cell::new(false);
        enqueue_command(
            &queue,
            gate.issue().unwrap(),
            live_command_input(&format!("/choose {model}"), Some(false)),
            Duration::from_secs(1),
            &injected,
        )
        .unwrap();
        assert!(injected.get());
        let mut prompt = prompt(model);
        prompt.injected();
        let screen = terminal.lock().unwrap().screen.clone();
        prompt
            .respond(&screen, |keys| {
                queue.send_and_wait_guarded(
                    keys.to_vec(),
                    gate.issue().unwrap(),
                    Duration::from_secs(1),
                )
            })
            .unwrap();
        prompt
            .respond(&screen, |_| panic!("duplicate response"))
            .unwrap();
        assert!(terminal.lock().unwrap().screen.is_empty());
        let rearms = Cell::new(0);
        finish_command(
            Ok(()),
            injected.get(),
            || panic!("successful command cleanup"),
            || {
                rearms.set(rearms.get() + 1);
                gate.rearm();
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(rearms.get(), 1);
    }
    queue
        .send_and_wait_guarded(
            b"marker\r".to_vec(),
            original_permit,
            Duration::from_secs(1),
        )
        .unwrap();
    let terminal = terminal.lock().unwrap();
    assert_eq!(
        terminal.writes,
        [
            b"/choose one\r".to_vec(),
            b"y\r".to_vec(),
            b"/choose two\r".to_vec(),
            b"y\r".to_vec(),
            b"marker\r".to_vec()
        ]
    );
    assert_eq!(terminal.user_turns, ["marker\r"]);
}

#[test]
fn send_after_failed_switch_reaches_a_clean_composer_without_payload_replay() {
    let terminal = Arc::new(Mutex::new(Terminal::default()));
    let queue = WriteQueue::new(Box::new(Writer(terminal.clone())));
    let gate = GuardedInputGate::default();
    gate.rearm();
    let injected = Cell::new(false);
    enqueue_command(
        &queue,
        gate.issue().unwrap(),
        live_command_input("/choose one", Some(false)),
        Duration::from_secs(1),
        &injected,
    )
    .unwrap();
    let mut prompt = prompt("one");
    prompt.injected();
    let screen = terminal.lock().unwrap().screen.clone();
    let failure = finish_command(
        Err("confirmation timeout".into()),
        injected.get(),
        || {
            assert!(!prompt
                .cancel(&screen, |keys| queue.send_and_wait_guarded(
                    keys.to_vec(),
                    gate.issue().unwrap(),
                    Duration::from_secs(1)
                ))
                .unwrap());
            let screen = terminal.lock().unwrap().screen.clone();
            assert!(prompt
                .cancel(&screen, |_| panic!("duplicate cancellation"))
                .unwrap());
            Ok(())
        },
        || {
            gate.rearm();
            Ok(())
        },
    );
    assert_eq!(failure.unwrap_err(), "confirmation timeout");
    queue
        .send_and_wait_guarded(
            b"marker\r".to_vec(),
            gate.issue().unwrap(),
            Duration::from_secs(1),
        )
        .unwrap();
    let terminal = terminal.lock().unwrap();
    assert_eq!(
        terminal.writes,
        [
            b"/choose one\r".to_vec(),
            b"\x1b".to_vec(),
            b"marker\r".to_vec()
        ]
    );
    assert_eq!(terminal.user_turns, ["marker\r"]);
}

#[test]
fn revoked_permit_never_authorizes_a_dialog_response() {
    let queue = WriteQueue::new(Box::new(io::sink()));
    let gate = GuardedInputGate::default();
    gate.rearm();
    let permit = gate.issue().unwrap();
    gate.invalidate();
    let injected = Cell::new(false);
    assert!(enqueue_command(
        &queue,
        permit,
        b"/choose one\r".to_vec(),
        Duration::from_secs(1),
        &injected
    )
    .is_err());
    assert!(!injected.get());
}

#[test]
fn command_frame_contains_exactly_one_submit_key_outside_the_paste() {
    assert_eq!(
        live_command_input("/choose one", Some(true)),
        b"\x1b[200~/choose one\x1b[201~\r"
    );
    assert_eq!(
        live_command_input("/choose one", Some(false)),
        b"/choose one\r"
    );
}

#[test]
fn failed_cleanup_is_explicit_and_does_not_claim_input_restored() {
    let error = finish_command(
        Err("timeout".into()),
        true,
        || Err("dialog still open".into()),
        || panic!("unsafe input rearm"),
    )
    .unwrap_err();
    assert!(error.contains("dialog still open"));
    assert!(error.contains("restart and resume"));
}

#[test]
fn prewrite_refusal_neither_cleans_a_foreign_dialog_nor_rearms_input() {
    assert_eq!(
        finish_command(
            Err("refused".into()),
            false,
            || panic!("foreign cleanup"),
            || panic!("unowned rearm")
        ),
        Err("refused".into())
    );
}
