use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
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

fn transaction_locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn transaction_lock(pty_id: &str) -> Arc<Mutex<()>> {
    transaction_locks()
        .lock()
        .entry(pty_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
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
            !screen_contains_echo(&observation.tail, text)
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
    let (paste_observation, mut generation) = if enter || opts.verify_echo {
        wait_for_paste(transport, text, armed_generation, deadline)
            .map_err(|error| helper_to_injection(error, started, text, true))?
    } else {
        (initial.clone(), armed_generation)
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
            if submitted(
                &strategy,
                &observation,
                initial.hash,
                text,
                enter_generation,
                current_generation,
            ) {
                return Ok(InjectionResult {
                    bytes_written,
                    echo_observed,
                    submit_observed: true,
                    elapsed: started.elapsed(),
                });
            }
            if current_generation != generation {
                generation = current_generation;
                if attempt + 1 < MAX_ENTER_ATTEMPTS && screen_contains_echo(&observation.tail, text)
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
    let _transaction = lock.lock();
    let signal = svc.screen_state.lock().arm_screen_change_signal(pty_id);
    svc.feed_input_parser(pty_id, text, enter);
    let transport = PtySubmitTransport {
        svc,
        pty_id,
        signal,
    };
    run(&transport, text, enter, strategy, opts)
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
        after_paste: Option<Frame>,
        waits: RefCell<VecDeque<Option<Frame>>>,
        writes: RefCell<Vec<&'static str>>,
    }

    impl ControlledTransport {
        fn new(after_paste: Option<Frame>, waits: Vec<Option<Frame>>) -> Self {
            Self {
                generation: Cell::new(0),
                frame: RefCell::new(Frame {
                    tail: "> ",
                    hash: 0,
                }),
                after_paste,
                waits: RefCell::new(waits.into()),
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
                    self.advance(frame);
                    true
                }
                None => false,
            }
        }

        fn write_paste(&self, _text: &str) -> Result<usize, InjectionError> {
            self.writes.borrow_mut().push("paste");
            if let Some(frame) = self.after_paste.clone() {
                self.advance(frame);
            }
            Ok(4)
        }

        fn write_enter(&self) -> Result<(), InjectionError> {
            self.writes.borrow_mut().push("enter");
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
    fn submit_transaction_text_present_enter_absent_red() {
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
        assert_eq!(&*io.writes.borrow(), &["paste", "enter"]);
    }

    #[test]
    fn submit_transaction_arms_generation_before_paste() {
        let io = ControlledTransport::new(
            Some(Frame {
                tail: "> hello",
                hash: 1,
            }),
            vec![Some(Frame {
                tail: "working",
                hash: 2,
            })],
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
        assert_eq!(&*stale.writes.borrow(), &["paste", "enter"]);

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
            ],
        );
        assert!(run(&progressed, "hello", true, tui(), opts()).is_ok());
        assert_eq!(&*progressed.writes.borrow(), &["paste", "enter", "enter"]);
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
    fn wake_safety_event_driven_no_double_submit() {
        let io = ControlledTransport::new(
            Some(Frame {
                tail: "> body-b",
                hash: 2,
            }),
            vec![Some(Frame {
                tail: "working",
                hash: 3,
            })],
        );
        assert!(run(&io, "body-b", true, tui(), opts()).is_ok());
        assert_eq!(
            io.writes
                .borrow()
                .iter()
                .filter(|write| **write == "enter")
                .count(),
            1
        );
    }

    #[test]
    fn messenger_report_planner_send_share_submit_transaction() {
        let inject = include_str!("inject.rs");
        let send = include_str!("send.rs");
        let messenger = include_str!("../messenger.rs");
        let hierarchy = include_str!("../hierarchy.rs");

        assert!(inject.contains("submit_transaction::submit_text"));
        assert!(send.matches("send_text_verified(").count() >= 3);
        assert!(messenger.contains("svc.send_text(to_pane, &text, true)?"));
        assert!(messenger.contains("svc.send_to_authenticated_agent_session("));
        assert!(hierarchy.contains("Messenger::deliver("));

        for entry_path in ["messenger", "report", "planner", "send"] {
            let io = ControlledTransport::new(
                Some(Frame {
                    tail: "> shared body",
                    hash: 1,
                }),
                vec![None],
            );
            assert!(
                matches!(
                    run(&io, "shared body", true, tui(), opts()),
                    Err(InjectionError::SubmitNeverConfirmed { .. })
                ),
                "{entry_path} must not erase missing submission evidence"
            );
            assert_eq!(&*io.writes.borrow(), &["paste", "enter"]);
        }
    }
}
