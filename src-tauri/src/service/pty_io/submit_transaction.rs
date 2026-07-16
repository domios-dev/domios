use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::agent::provider::VerifySubmitStrategy;
use crate::pty::screen_state::{screen_has_shell_prompt, ScreenChangeSignal};
use crate::service::inject_verify::{
    screen_contains_echo, HelperErr, InjectionError, InjectionResult, VerifyOpts,
};
use crate::service::ServiceContext;

const TAIL_ROWS: usize = 8;
const MAX_ENTER_ATTEMPTS: usize = 2;

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
    hash: [u8; 32],
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

impl SubmitTransport for PtySubmitTransport<'_> {
    fn generation(&self) -> u64 {
        self.signal.generation()
    }

    fn observe(&self) -> Result<Observation, HelperErr> {
        let state = self.svc.screen_state.lock();
        let tail = state
            .tail_text(self.pty_id, TAIL_ROWS)
            .ok_or_else(|| HelperErr::PtyUnknown(self.pty_id.to_string()))?;
        let hash = state
            .get_content_hash(self.pty_id, 0)
            .ok_or_else(|| HelperErr::PtyUnknown(self.pty_id.to_string()))?;
        Ok(Observation { tail, hash })
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
        let paste = if cfg!(windows) {
            format!("\x1b[200~ {}\x1b[201~", text)
        } else {
            format!("\x1b[200~{}\x1b[201~", text)
        };
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

fn submitted(
    strategy: &VerifySubmitStrategy,
    observation: &Observation,
    hash_before: [u8; 32],
    text: &str,
    enter_generation: u64,
    current_generation: u64,
) -> bool {
    match strategy {
        VerifySubmitStrategy::ShellPrompt { .. } => {
            observation.hash != hash_before && screen_has_shell_prompt(&observation.tail)
        }
        VerifySubmitStrategy::TuiInputDrained { .. } => {
            if text.is_empty() {
                current_generation != enter_generation
            } else {
                false
            }
        }
        VerifySubmitStrategy::None => current_generation != enter_generation,
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
    let initial = transport.observe().map_err(helper_to_closed)?;
    let bytes_written = transport.write_paste(text)?;
    let (paste_observation, mut generation) = if opts.verify_echo {
        wait_for_paste(transport, text, armed_generation, deadline)
            .map_err(|error| helper_to_injection(error, started, text, true))?
    } else {
        wait_for_paste_activity(transport, text, armed_generation, deadline)
            .map_err(|error| helper_to_injection(error, started, text, true))?
    };
    let echo_observed = text.is_empty() || screen_contains_echo(&paste_observation.tail, text);

    if !enter {
        return Ok(InjectionResult {
            bytes_written,
            echo_observed,
            submit_observed: false,
            elapsed: started.elapsed(),
        });
    }

    for attempt in 0..MAX_ENTER_ATTEMPTS {
        let enter_generation = generation;
        let enter_hash = transport.observe().map_err(helper_to_closed)?.hash;
        let mut absent_candidate = None;
        let mut body_reappeared_after_candidate = false;
        transport.write_enter()?;
        if !opts.verify_submit {
            return Ok(InjectionResult {
                bytes_written,
                echo_observed,
                submit_observed: false,
                elapsed: started.elapsed(),
            });
        }
        loop {
            let observation = transport.observe().map_err(helper_to_closed)?;
            let current_generation = transport.generation();
            let body_present = screen_contains_echo(&observation.tail, text);
            let tui_confirmed = matches!(&strategy, VerifySubmitStrategy::TuiInputDrained { .. })
                && !text.is_empty()
                && !body_present
                && current_generation != enter_generation
                && (body_reappeared_after_candidate
                    || absent_candidate.is_some_and(|candidate| current_generation != candidate));
            if tui_confirmed
                || submitted(
                    &strategy,
                    &observation,
                    initial.hash,
                    text,
                    enter_generation,
                    current_generation,
                )
            {
                return Ok(InjectionResult {
                    bytes_written,
                    echo_observed,
                    submit_observed: true,
                    elapsed: started.elapsed(),
                });
            }
            if matches!(&strategy, VerifySubmitStrategy::TuiInputDrained { .. })
                && !text.is_empty()
                && current_generation != enter_generation
            {
                if body_present {
                    if absent_candidate.take().is_some() {
                        body_reappeared_after_candidate = true;
                    }
                } else if absent_candidate.is_none() {
                    absent_candidate = Some(current_generation);
                }
            }
            if current_generation != generation {
                generation = current_generation;
                if attempt + 1 < MAX_ENTER_ATTEMPTS
                    && observation.hash != enter_hash
                    && body_present
                    && !body_reappeared_after_candidate
                {
                    break;
                }
            }
            if !transport.wait_after(generation, deadline) {
                return Err(InjectionError::SubmitNeverConfirmed {
                    waited: started.elapsed(),
                    screen_tail: observation.tail,
                });
            }
        }
    }

    let tail = transport
        .observe()
        .map(|observation| observation.tail)
        .unwrap_or_default();
    Err(InjectionError::SubmitNeverConfirmed {
        waited: started.elapsed(),
        screen_tail: tail,
    })
}

fn helper_to_closed(error: HelperErr) -> InjectionError {
    match error {
        HelperErr::PtyUnknown(pty_id) => InjectionError::PtyClosed { pty_id },
        HelperErr::Timeout(screen_tail) => InjectionError::SubmitNeverConfirmed {
            waited: Duration::ZERO,
            screen_tail,
        },
    }
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
        hash: u8,
    }

    struct ControlledTransport {
        generation: Cell<u64>,
        frame: RefCell<Frame>,
        after_paste: RefCell<VecDeque<Frame>>,
        waits: RefCell<VecDeque<Option<Frame>>>,
        consumed_frames: Cell<usize>,
        current_body: RefCell<String>,
        writes: RefCell<Vec<String>>,
    }

    impl ControlledTransport {
        fn new(after_paste: Option<Frame>, waits: Vec<Option<Frame>>) -> Self {
            Self::sequence(after_paste.into_iter().collect(), waits)
        }

        fn sequence(after_paste: Vec<Frame>, waits: Vec<Option<Frame>>) -> Self {
            Self {
                generation: Cell::new(0),
                frame: RefCell::new(Frame {
                    tail: "> ",
                    hash: 0,
                }),
                after_paste: RefCell::new(after_paste.into()),
                waits: RefCell::new(waits.into()),
                consumed_frames: Cell::new(0),
                current_body: RefCell::new(String::new()),
                writes: RefCell::new(Vec::new()),
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
            let mut hash = [0; 32];
            hash[0] = frame.hash;
            Ok(Observation {
                tail: frame.tail.to_string(),
                hash,
            })
        }

        fn wait_after(&self, generation: u64, _deadline: Instant) -> bool {
            if self.generation.get() != generation {
                return true;
            }
            match self.waits.borrow_mut().pop_front().flatten() {
                Some(frame) => {
                    self.consumed_frames.set(self.consumed_frames.get() + 1);
                    self.advance(frame);
                    true
                }
                None => false,
            }
        }

        fn write_paste(&self, text: &str) -> Result<usize, InjectionError> {
            *self.current_body.borrow_mut() = text.to_string();
            self.writes.borrow_mut().push(format!("paste:{text}"));
            if let Some(frame) = self.after_paste.borrow_mut().pop_front() {
                self.advance(frame);
            }
            Ok(4)
        }

        fn write_enter(&self) -> Result<(), InjectionError> {
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
    fn submission_ack_harness_text_present_enter_absent_is_error() {
        let io = ControlledTransport::new(
            Some(Frame {
                tail: "> hello",
                hash: 1,
            }),
            vec![None],
        );
        let result = run(&io, "hello", true, tui(), opts());
        assert!(matches!(
            result,
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
        assert_eq!(&*io.writes.borrow(), &["paste:hello", "enter:hello"]);
    }

    #[test]
    fn long_prompt_with_truncated_echo_still_reaches_acknowledged_enter() {
        let body = "continuation ".repeat(200);
        let io = ControlledTransport::new(
            Some(Frame {
                tail: "> continuation tail",
                hash: 1,
            }),
            vec![
                Some(Frame {
                    tail: "working",
                    hash: 2,
                }),
                Some(Frame {
                    tail: "working stable",
                    hash: 3,
                }),
            ],
        );
        let mut no_echo = opts();
        no_echo.verify_echo = false;

        let result = run(&io, &body, true, tui(), no_echo).unwrap();

        assert!(!result.echo_observed);
        assert!(result.submit_observed);
        assert_eq!(
            &*io.writes.borrow(),
            &[format!("paste:{body}"), format!("enter:{body}")]
        );
    }

    #[test]
    fn submit_transaction_arms_generation_before_paste() {
        let io = ControlledTransport::new(
            Some(Frame {
                tail: "> hello",
                hash: 1,
            }),
            vec![
                Some(Frame {
                    tail: "working",
                    hash: 2,
                }),
                Some(Frame {
                    tail: "working stable",
                    hash: 3,
                }),
            ],
        );
        let result = run(&io, "hello", true, tui(), opts()).unwrap();
        assert!(result.echo_observed);
        assert!(result.submit_observed);
    }

    #[test]
    fn submit_transaction_retries_only_on_new_generation() {
        let stale = ControlledTransport::new(
            Some(Frame {
                tail: "> hello",
                hash: 1,
            }),
            vec![None],
        );
        assert!(run(&stale, "hello", true, tui(), opts()).is_err());
        assert_eq!(&*stale.writes.borrow(), &["paste:hello", "enter:hello"]);

        let progressed = ControlledTransport::new(
            Some(Frame {
                tail: "> hello",
                hash: 1,
            }),
            vec![
                Some(Frame {
                    tail: "> hello",
                    hash: 2,
                }),
                Some(Frame {
                    tail: "working",
                    hash: 3,
                }),
                Some(Frame {
                    tail: "working stable",
                    hash: 4,
                }),
            ],
        );
        assert!(run(&progressed, "hello", true, tui(), opts()).is_ok());
        assert_eq!(
            &*progressed.writes.borrow(),
            &["paste:hello", "enter:hello", "enter:hello"]
        );
    }

    #[test]
    fn submit_transaction_deadline_fails_without_submission() {
        let io = ControlledTransport::new(
            Some(Frame {
                tail: "> hello",
                hash: 1,
            }),
            vec![None],
        );
        assert!(matches!(
            run(&io, "hello", true, tui(), opts()),
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
    }

    #[test]
    fn submission_ack_harness_exactly_once_on_submission_evidence() {
        let io = ControlledTransport::sequence(
            vec![
                Frame {
                    tail: "> body-a",
                    hash: 1,
                },
                Frame {
                    tail: "> body-b",
                    hash: 4,
                },
            ],
            vec![
                Some(Frame {
                    tail: "a submitted",
                    hash: 2,
                }),
                Some(Frame {
                    tail: "a stable",
                    hash: 3,
                }),
                Some(Frame {
                    tail: "stale a submitted",
                    hash: 2,
                }),
                Some(Frame {
                    tail: "> body-b",
                    hash: 4,
                }),
                Some(Frame {
                    tail: "b submitted",
                    hash: 5,
                }),
            ],
        );
        assert!(run(&io, "body-a", true, tui(), opts()).is_ok());
        assert!(run(&io, "body-b", true, tui(), opts()).is_ok());

        let writes = io.writes.borrow();
        let replacement = writes
            .iter()
            .position(|write| write == "paste:body-b")
            .unwrap();
        assert!(!writes[replacement..]
            .iter()
            .any(|write| write == "enter:body-a"));
        assert_eq!(
            writes
                .iter()
                .filter(|write| *write == "enter:body-b")
                .count(),
            1
        );
        assert_eq!(writes.last().unwrap(), "enter:body-b");
        assert_eq!(io.consumed_frames.get(), 5);
        assert_eq!(io.generation.get(), 7);
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
    fn transaction_lock_registry_retires_closed_ptys() {
        let lock = transaction_lock("closed-pty");
        assert!(transaction_locks().lock().contains_key("closed-pty"));
        retire_transaction_lock("closed-pty", &lock);
        assert!(!transaction_locks().lock().contains_key("closed-pty"));
    }
}
