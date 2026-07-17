use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::provider::VerifySubmitStrategy;
use crate::service::inject_verify::{
    screen_contains_echo, InjectionError, InjectionResult, VerifyOpts,
};
use crate::service::ServiceContext;

const TAIL_ROWS: usize = 8;

fn transaction_locks() -> &'static Mutex<HashMap<String, Weak<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn transaction_lock(pty_id: &str) -> Arc<Mutex<()>> {
    let mut locks = transaction_locks().lock();
    if let Some(lock) = locks.get(pty_id).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(pty_id.to_string(), Arc::downgrade(&lock));
    lock
}

fn retire_transaction_lock(pty_id: &str, lock: &Arc<Mutex<()>>) {
    let mut locks = transaction_locks().lock();
    let current_is_lock = locks
        .get(pty_id)
        .and_then(Weak::upgrade)
        .is_some_and(|current| Arc::ptr_eq(&current, lock));
    if current_is_lock && Arc::strong_count(lock) == 1 {
        locks.remove(pty_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PtySubmitEntry {
    Messenger,
    Report,
    Planner,
    Direct,
}

pub(crate) fn submit_entry<T, E>(
    _entry: PtySubmitEntry,
    text: &str,
    submit: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, E> {
    submit(text)
}

#[derive(Clone)]
struct Observation {
    tail: String,
}

trait SubmitTransport {
    fn observe(&self) -> Option<Observation>;
    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError>;
    fn write_enter(&self, deadline: Instant) -> Result<(), InjectionError>;
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
}

fn bracketed_paste(text: &str) -> String {
    let leading_space = if cfg!(windows) { " " } else { "" };
    format!("\x1b[200~{leading_space}{text}\x1b[201~")
}

impl SubmitTransport for PtySubmitTransport<'_> {
    fn observe(&self) -> Option<Observation> {
        let state = self.svc.screen_state.lock();
        state
            .tail_text(self.pty_id, TAIL_ROWS)
            .map(|tail| Observation { tail })
    }

    fn write_paste(&self, text: &str, deadline: Instant) -> Result<usize, InjectionError> {
        if text.is_empty() {
            return Ok(0);
        }
        let paste = bracketed_paste(text);
        self.svc
            .pty
            .lock()
            .write_and_flush(
                self.pty_id,
                paste.as_bytes(),
                deadline.saturating_duration_since(Instant::now()),
            )
            .map_err(|_| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })?;
        Ok(paste.len())
    }

    fn write_enter(&self, deadline: Instant) -> Result<(), InjectionError> {
        self.svc
            .pty
            .lock()
            .write_and_flush(
                self.pty_id,
                b"\r",
                deadline.saturating_duration_since(Instant::now()),
            )
            .map_err(|_| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })
    }
}

fn run<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    _strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    let started = Instant::now();
    let deadline = started + opts.echo_timeout.saturating_add(opts.submit_timeout);
    let bytes_written = transport.write_paste(text, deadline)?;
    let echo_observed = transport.observe().is_some_and(|observation| {
        text.is_empty() || screen_contains_echo(&observation.tail, text)
    });

    if enter {
        transport.write_enter(deadline)?;
        return Ok(InjectionResult {
            bytes_written,
            echo_observed,
            submit_observed: true,
            elapsed: started.elapsed(),
        });
    }

    Ok(InjectionResult {
        bytes_written,
        echo_observed,
        submit_observed: false,
        elapsed: started.elapsed(),
    })
}

pub(crate) fn submit_text(
    svc: &ServiceContext,
    pty_id: &str,
    text: &str,
    enter: bool,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    let lock = transaction_lock(pty_id);
    let transaction = lock.lock();
    svc.feed_input_parser(pty_id, text, enter);
    let transport = PtySubmitTransport { svc, pty_id };
    let required = enter && opts.verify_submit;
    let result = run(&transport, text, enter, strategy, opts)
        .and_then(|result| result.require_submission(required, Duration::ZERO, String::new()));
    drop(transaction);
    retire_transaction_lock(pty_id, &lock);
    result
}

pub(crate) fn write_terminal_reply(
    svc: &ServiceContext,
    pty_id: &str,
    reply: &[u8],
) -> Result<(), String> {
    let lock = transaction_lock(pty_id);
    let transaction = lock.lock();
    let result = svc.pty.lock().write(pty_id, reply);
    drop(transaction);
    retire_transaction_lock(pty_id, &lock);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Clone)]
    struct Frame {
        tail: &'static str,
    }

    struct ControlledTransport {
        frame: RefCell<Frame>,
        current_body: RefCell<String>,
        writes: RefCell<Vec<String>>,
    }

    impl ControlledTransport {
        fn new(tail: &'static str) -> Self {
            Self {
                frame: RefCell::new(Frame { tail }),
                current_body: RefCell::new(String::new()),
                writes: RefCell::new(Vec::new()),
            }
        }
    }

    impl SubmitTransport for ControlledTransport {
        fn observe(&self) -> Option<Observation> {
            let frame = self.frame.borrow();
            Some(Observation {
                tail: frame.tail.to_string(),
            })
        }

        fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
            *self.current_body.borrow_mut() = text.to_string();
            self.writes.borrow_mut().push(format!("paste:{text}"));
            Ok(4)
        }

        fn write_enter(&self, _deadline: Instant) -> Result<(), InjectionError> {
            self.writes
                .borrow_mut()
                .push(format!("enter:{}", self.current_body.borrow()));
            Ok(())
        }
    }

    fn opts() -> VerifyOpts {
        VerifyOpts {
            require_prompt_first: false,
            verify_echo: true,
            verify_submit: true,
            echo_timeout: Duration::ZERO,
            submit_timeout: Duration::ZERO,
            prompt_timeout: Duration::ZERO,
        }
    }

    fn tui() -> VerifySubmitStrategy {
        VerifySubmitStrategy::TuiInputDrained {
            timeout: Duration::ZERO,
            idle_after_drain: Duration::ZERO,
        }
    }

    #[test]
    fn every_pty_provider_strategy_preserves_paste_then_enter_order() {
        let strategies = [
            VerifySubmitStrategy::TuiInputDrained {
                timeout: Duration::ZERO,
                idle_after_drain: Duration::ZERO,
            },
            VerifySubmitStrategy::ShellPrompt {
                timeout: Duration::ZERO,
            },
            VerifySubmitStrategy::None,
        ];

        for strategy in strategies {
            let io = ControlledTransport::new("> provider-neutral prompt");
            let result = run(&io, "provider-neutral prompt", true, strategy, opts()).unwrap();
            assert!(result.submit_observed);
            assert_eq!(
                &*io.writes.borrow(),
                &[
                    "paste:provider-neutral prompt",
                    "enter:provider-neutral prompt"
                ]
            );
        }
    }

    #[test]
    fn long_wrapped_chat_message_enters_after_paste_placeholder_echo() {
        let body = "continuation ".repeat(200);
        let io = ControlledTransport::new("> [Pasted Content 2600 chars]");
        let result = run(&io, &body, true, tui(), opts()).unwrap();

        assert!(result.submit_observed);
        assert!(result.echo_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &[format!("paste:{body}"), format!("enter:{body}")]
        );
    }

    #[test]
    fn complete_echo_is_diagnostic_but_not_required_for_enter() {
        let io = ControlledTransport::new("> hello");
        let result = run(&io, "hello", true, tui(), opts()).unwrap();
        assert!(result.echo_observed);
        assert!(result.submit_observed);
    }

    #[test]
    fn unrelated_screen_churn_is_diagnostic_only() {
        let io = ControlledTransport::new("loading provider");
        let result = run(&io, "hello", true, tui(), opts()).unwrap();

        assert!(!result.echo_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &["paste:hello", "enter:hello"],
            "screen churn cannot block the physical submit transaction"
        );
    }

    #[test]
    fn spawn_introduction_uses_the_same_paste_then_enter_contract() {
        let intro = "-=-codeterm:pane label=CodeTerm pane=system-=-You are the Orchestrator of this team. Your Planner counterpart is Rodin.";
        let io = ControlledTransport::new("> ");

        let result = submit_entry(PtySubmitEntry::Messenger, intro, |body| {
            run(&io, body, true, tui(), opts())
        })
        .unwrap();

        assert!(result.submit_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &[format!("paste:{intro}"), format!("enter:{intro}")]
        );
    }

    #[test]
    fn no_render_event_still_sends_enter() {
        let io = ControlledTransport::new("> ");
        let result = run(&io, "hello", true, tui(), opts()).unwrap();
        assert!(!result.echo_observed);
        assert!(result.submit_observed);
        assert_eq!(&*io.writes.borrow(), &["paste:hello", "enter:hello"]);
    }

    #[test]
    fn each_submission_sends_exactly_one_enter() {
        let io = ControlledTransport::new("> ");
        assert!(run(&io, "body-a", true, tui(), opts()).is_ok());
        assert!(run(&io, "body-b", true, tui(), opts()).is_ok());

        let writes = io.writes.borrow();
        assert_eq!(
            writes
                .iter()
                .filter(|write| *write == "enter:body-b")
                .count(),
            1
        );
        assert_eq!(
            writes.as_slice(),
            [
                "paste:body-a",
                "enter:body-a",
                "paste:body-b",
                "enter:body-b"
            ]
        );
    }

    #[test]
    fn messenger_report_planner_send_share_submit_transaction() {
        for entry in [
            PtySubmitEntry::Messenger,
            PtySubmitEntry::Report,
            PtySubmitEntry::Planner,
            PtySubmitEntry::Direct,
        ] {
            let mut submit_calls = 0;
            let result = submit_entry(entry, "shared body", |text| {
                submit_calls += 1;
                assert_eq!(text, "shared body");
                Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                    waited: Duration::ZERO,
                    screen_tail: "> shared body".into(),
                })
            });
            assert!(matches!(
                result,
                Err(InjectionError::SubmitNeverConfirmed { .. })
            ));
            assert_eq!(submit_calls, 1);
        }
    }

    #[test]
    fn screen_churn_cannot_mint_extra_enters() {
        let io = ControlledTransport::new("> hello");
        assert!(
            run(&io, "hello", true, tui(), opts())
                .unwrap()
                .submit_observed
        );
        let writes = io.writes.borrow();
        assert_eq!(
            writes
                .iter()
                .filter(|write| *write == "enter:hello")
                .count(),
            1,
            "screen churn must not mint additional Enter writes: {writes:?}"
        );
    }

    #[test]
    fn transaction_lock_registry_retires_closed_ptys() {
        let lock = transaction_lock("closed-pty");
        assert!(transaction_locks().lock().contains_key("closed-pty"));
        retire_transaction_lock("closed-pty", &lock);
        assert!(!transaction_locks().lock().contains_key("closed-pty"));
    }

    #[test]
    fn terminal_replies_wait_for_an_active_submit_transaction() {
        let lock = transaction_lock("reply-lock");
        let submit = lock.lock();
        let (entered, receiver) = std::sync::mpsc::sync_channel(1);
        let reply_lock = lock.clone();
        let worker = std::thread::spawn(move || {
            let _reply = reply_lock.lock();
            let _ = entered.send(());
        });

        assert!(receiver.recv_timeout(Duration::from_millis(1)).is_err());
        drop(submit);
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("terminal reply must run after submit completion");
        worker.join().unwrap();
        retire_transaction_lock("reply-lock", &lock);
    }
}
