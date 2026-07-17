use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::provider::VerifySubmitStrategy;
use crate::pty::screen_state::ScreenChangeSignal;
use crate::service::inject_verify::{
    screen_contains_echo, HelperErr, InjectionError, InjectionResult, VerifyOpts,
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
    fn generation(&self) -> u64;
    fn observe(&self) -> Result<Observation, HelperErr>;
    fn wait_after(&self, generation: u64, deadline: Instant) -> bool;
    fn write_paste(&self, text: &str) -> Result<usize, InjectionError>;
    fn write_enter(&self) -> Result<(), InjectionError>;
}

struct PtySubmitTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    signal: Arc<ScreenChangeSignal>,
}

fn bracketed_paste(text: &str) -> String {
    let leading_space = if cfg!(windows) { " " } else { "" };
    format!("\x1b[200~{leading_space}{text}\x1b[201~")
}

impl SubmitTransport for PtySubmitTransport<'_> {
    fn generation(&self) -> u64 {
        self.signal.generation()
    }

    fn observe(&self) -> Result<Observation, HelperErr> {
        let state = self.svc.screen_state.lock();
        let tail = state
            .tail_text(self.pty_id, TAIL_ROWS)
            .ok_or_else(|| HelperErr::PtyUnknown(self.pty_id.to_string()))?;
        Ok(Observation { tail })
    }

    fn wait_after(&self, generation: u64, deadline: Instant) -> bool {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| self.signal.wait_after(generation, remaining))
    }

    fn write_paste(&self, text: &str) -> Result<usize, InjectionError> {
        if text.is_empty() {
            return Ok(0);
        }
        let paste = bracketed_paste(text);
        self.svc
            .pty
            .lock()
            .write(self.pty_id, paste.as_bytes())
            .map_err(|_| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })?;
        Ok(paste.len())
    }

    fn write_enter(&self) -> Result<(), InjectionError> {
        self.svc
            .pty
            .lock()
            .write(self.pty_id, b"\r")
            .map_err(|_| InjectionError::PtyClosed {
                pty_id: self.pty_id.to_string(),
            })
    }
}

fn strategy_timeout(strategy: &VerifySubmitStrategy, fallback: Duration) -> Duration {
    match strategy {
        VerifySubmitStrategy::ShellPrompt { timeout }
        | VerifySubmitStrategy::TuiInputDrained { timeout, .. } => *timeout,
        VerifySubmitStrategy::None => fallback,
    }
}

fn wait_for_paste<T: SubmitTransport>(
    transport: &T,
    text: &str,
    mut generation: u64,
    deadline: Instant,
) -> Result<(Observation, u64), HelperErr> {
    loop {
        let observation = transport.observe()?;
        if text.is_empty() || screen_contains_echo(&observation.tail, text) {
            return Ok((observation, transport.generation()));
        }
        if !transport.wait_after(generation, deadline) {
            return Err(HelperErr::Timeout(observation.tail));
        }
        generation = transport.generation();
    }
}

fn wait_for_paste_activity<T: SubmitTransport>(
    transport: &T,
    text: &str,
    armed_generation: u64,
    deadline: Instant,
) -> Result<(Observation, u64), HelperErr> {
    if !text.is_empty()
        && transport.generation() == armed_generation
        && !transport.wait_after(armed_generation, deadline)
    {
        return Err(HelperErr::Timeout(transport.observe()?.tail));
    }
    Ok((transport.observe()?, transport.generation()))
}

fn run<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    let started = Instant::now();
    let deadline = started
        + opts
            .echo_timeout
            .saturating_add(strategy_timeout(&strategy, opts.submit_timeout));
    let armed_generation = transport.generation();
    let bytes_written = transport.write_paste(text)?;
    let paste_wait = if enter {
        wait_for_paste_activity(transport, text, armed_generation, deadline)
    } else if opts.verify_echo {
        wait_for_paste(transport, text, armed_generation, deadline)
    } else {
        wait_for_paste_activity(transport, text, armed_generation, deadline)
    };
    let (paste_observation, _) = match paste_wait {
        Ok(observed) => observed,
        Err(HelperErr::PtyUnknown(pty_id)) => return Err(InjectionError::PtyClosed { pty_id }),
        Err(error) => return Err(helper_to_injection(error, started, text, true)),
    };
    let echo_observed = text.is_empty() || screen_contains_echo(&paste_observation.tail, text);

    if enter {
        transport.write_enter()?;
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

fn helper_to_injection(
    error: HelperErr,
    started: Instant,
    text: &str,
    paste_phase: bool,
) -> InjectionError {
    match error {
        HelperErr::PtyUnknown(pty_id) => InjectionError::PtyClosed { pty_id },
        HelperErr::Timeout(screen_tail) if paste_phase => InjectionError::EchoMismatch {
            expected_substr: text.to_string(),
            screen_tail,
        },
        HelperErr::Timeout(screen_tail) => InjectionError::SubmitNeverConfirmed {
            waited: started.elapsed(),
            screen_tail,
        },
    }
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
    let signal = svc.screen_state.lock().arm_screen_change_signal(pty_id);
    svc.feed_input_parser(pty_id, text, enter);
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signal,
    };
    let required = enter && opts.verify_submit;
    let result = run(&transport, text, enter, strategy, opts)
        .and_then(|result| result.require_submission(required, Duration::ZERO, String::new()));
    drop(transaction);
    retire_transaction_lock(pty_id, &lock);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    #[derive(Clone)]
    struct Frame {
        tail: &'static str,
    }

    struct ControlledTransport {
        generation: Cell<u64>,
        frame: RefCell<Frame>,
        after_paste: RefCell<VecDeque<Frame>>,
        waits: RefCell<VecDeque<Option<Frame>>>,
        current_body: RefCell<String>,
        writes: RefCell<Vec<String>>,
        enter_before_render: Cell<bool>,
    }

    impl ControlledTransport {
        fn new(after_paste: Option<Frame>, waits: Vec<Option<Frame>>) -> Self {
            Self::sequence(after_paste.into_iter().collect(), waits)
        }

        fn sequence(after_paste: Vec<Frame>, waits: Vec<Option<Frame>>) -> Self {
            Self {
                generation: Cell::new(0),
                frame: RefCell::new(Frame { tail: "> " }),
                after_paste: RefCell::new(after_paste.into()),
                waits: RefCell::new(waits.into()),
                current_body: RefCell::new(String::new()),
                writes: RefCell::new(Vec::new()),
                enter_before_render: Cell::new(false),
            }
        }

        fn advance(&self, frame: Frame) {
            *self.frame.borrow_mut() = frame;
            self.generation.set(self.generation.get() + 1);
        }
    }

    impl SubmitTransport for ControlledTransport {
        fn generation(&self) -> u64 {
            self.generation.get()
        }

        fn observe(&self) -> Result<Observation, HelperErr> {
            let frame = self.frame.borrow();
            Ok(Observation {
                tail: frame.tail.to_string(),
            })
        }

        fn wait_after(&self, generation: u64, _deadline: Instant) -> bool {
            if self.generation.get() != generation {
                return true;
            }
            if let Some(frame) = self.after_paste.borrow_mut().pop_front() {
                self.advance(frame);
                return true;
            }
            match self.waits.borrow_mut().pop_front().flatten() {
                Some(frame) => {
                    self.advance(frame);
                    true
                }
                None => false,
            }
        }

        fn write_paste(&self, text: &str) -> Result<usize, InjectionError> {
            *self.current_body.borrow_mut() = text.to_string();
            self.writes.borrow_mut().push(format!("paste:{text}"));
            Ok(4)
        }

        fn write_enter(&self) -> Result<(), InjectionError> {
            if self.generation.get() == 0 {
                self.enter_before_render.set(true);
            }
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
    fn every_pty_provider_strategy_waits_for_paste_render_then_enters() {
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
            let io = ControlledTransport::new(Some(Frame { tail: "> rendered" }), vec![]);
            let result = run(&io, "provider-neutral prompt", true, strategy, opts()).unwrap();
            assert!(result.submit_observed);
            assert!(!io.enter_before_render.get());
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
    fn long_wrapped_chat_message_enters_after_render_without_full_echo() {
        let body = "continuation ".repeat(200);
        let io = ControlledTransport::new(
            Some(Frame {
                tail: "> final wrapped line",
            }),
            vec![],
        );
        let result = run(&io, &body, true, tui(), opts()).unwrap();

        assert!(result.submit_observed);
        assert!(!result.echo_observed);
        assert!(!io.enter_before_render.get());
        assert_eq!(
            &*io.writes.borrow(),
            &[format!("paste:{body}"), format!("enter:{body}")]
        );
    }

    #[test]
    fn complete_echo_is_diagnostic_but_not_required_for_enter() {
        let io = ControlledTransport::new(Some(Frame { tail: "> hello" }), vec![]);
        let result = run(&io, "hello", true, tui(), opts()).unwrap();
        assert!(result.echo_observed);
        assert!(result.submit_observed);
        assert!(!io.enter_before_render.get());
    }

    #[test]
    fn spawn_introduction_uses_the_same_render_then_enter_contract() {
        let intro = "-=-codeterm:pane label=CodeTerm pane=system-=-You are the Orchestrator of this team. Your Planner counterpart is Rodin.";
        let io = ControlledTransport::new(
            Some(Frame {
                tail: "> wrapped introduction",
            }),
            vec![],
        );

        let result = submit_entry(PtySubmitEntry::Messenger, intro, |body| {
            run(&io, body, true, tui(), opts())
        })
        .unwrap();

        assert!(result.submit_observed);
        assert!(!io.enter_before_render.get());
        assert_eq!(
            &*io.writes.borrow(),
            &[format!("paste:{intro}"), format!("enter:{intro}")]
        );
    }

    #[test]
    fn no_render_event_never_sends_enter() {
        let io = ControlledTransport::new(None, vec![None]);
        let error = run(&io, "hello", true, tui(), opts()).unwrap_err();
        assert!(matches!(error, InjectionError::EchoMismatch { .. }));
        assert_eq!(&*io.writes.borrow(), &["paste:hello"]);
    }

    #[test]
    fn each_submission_sends_exactly_one_enter_after_its_render_event() {
        let io = ControlledTransport::sequence(
            vec![Frame { tail: "> body-a" }, Frame { tail: "> body-b" }],
            vec![],
        );
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
        let io = ControlledTransport::new(Some(Frame { tail: "> hello" }), vec![]);
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
}
