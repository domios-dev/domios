use super::*;
use std::{cell::RefCell, io, rc::Rc};

fn surface() -> Surface {
    Surface {
        json: false,
        stdout_terminal: true,
        stdin_terminal: true,
        daemon_reached: true,
        tab_identity: true,
    }
}

#[test]
fn only_human_output_in_the_agents_own_terminal_carries_messages() {
    assert!(carries_messages(&surface()));
    for (name, excluded) in [
        (
            "--json is parsed",
            Surface {
                json: true,
                ..surface()
            },
        ),
        (
            "a pipe, capture or file on stdout may never reach the agent",
            Surface {
                stdout_terminal: false,
                ..surface()
            },
        ),
        (
            "a pipeline or script feeds stdin",
            Surface {
                stdin_terminal: false,
                ..surface()
            },
        ),
        (
            "a local command never needed the daemon",
            Surface {
                daemon_reached: false,
                ..surface()
            },
        ),
        (
            "no tab of its own",
            Surface {
                tab_identity: false,
                ..surface()
            },
        ),
    ] {
        assert!(!carries_messages(&excluded), "{name}");
    }
}

/// Records what reached the reader when each settlement was made.
#[derive(Default)]
struct Port {
    waiting: Option<&'static str>,
    claims: usize,
    settled: Vec<(String, bool, Vec<u8>)>,
    terminal: Rc<RefCell<Vec<u8>>>,
}

impl Inbound for Port {
    fn claim(&mut self) -> Option<Claim> {
        self.claims += 1;
        self.waiting.take().map(|text| Claim {
            id: "claim".into(),
            text: text.into(),
        })
    }
    fn settle(&mut self, id: &str, written: bool) {
        self.settled
            .push((id.into(), written, self.terminal.borrow().clone()));
    }
}

struct Terminal {
    bytes: Rc<RefCell<Vec<u8>>>,
    /// Bytes accepted before writes start failing; `None` never fails.
    accepts: Option<usize>,
    chunk: usize,
    interrupted_once: bool,
    flush_results: Vec<io::Result<()>>,
}

impl Terminal {
    fn new(bytes: &Rc<RefCell<Vec<u8>>>) -> Self {
        Self {
            bytes: bytes.clone(),
            accepts: None,
            chunk: usize::MAX,
            interrupted_once: false,
            flush_results: Vec::new(),
        }
    }
}

impl io::Write for Terminal {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.interrupted_once {
            self.interrupted_once = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let room = self
            .accepts
            .map_or(usize::MAX, |accepts| accepts - self.bytes.borrow().len());
        if room == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let count = buffer.len().min(room).min(self.chunk);
        self.bytes.borrow_mut().extend_from_slice(&buffer[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.flush_results.is_empty() {
            Ok(())
        } else {
            self.flush_results.remove(0)
        }
    }
}

fn port(bytes: &Rc<RefCell<Vec<u8>>>) -> Port {
    Port {
        waiting: Some("## Inbound messages opaque"),
        terminal: bytes.clone(),
        ..Default::default()
    }
}

#[test]
fn messages_follow_the_output_and_settle_only_after_every_byte_was_written() {
    let bytes = Rc::new(RefCell::new(b"opaque own output\n".to_vec()));
    let mut port = port(&bytes);
    let mut terminal = Terminal::new(&bytes);
    terminal.chunk = 5;
    terminal.interrupted_once = true;
    append_messages(&mut terminal, &mut port);
    let expected = b"opaque own output\n\n## Inbound messages opaque".to_vec();
    assert_eq!(*bytes.borrow(), expected);
    assert_eq!(port.settled, [("claim".to_owned(), true, expected)]);
}

#[test]
fn nothing_waiting_leaves_the_output_untouched() {
    let bytes = Rc::new(RefCell::new(b"opaque".to_vec()));
    let mut port = Port {
        terminal: bytes.clone(),
        ..Default::default()
    };
    append_messages(&mut Terminal::new(&bytes), &mut port);
    assert_eq!(*bytes.borrow(), b"opaque");
    assert_eq!(port.claims, 1);
    assert!(port.settled.is_empty());
}

#[test]
fn a_terminal_that_refused_the_commands_own_output_never_claims() {
    let bytes = Rc::new(RefCell::new(Vec::new()));
    let mut port = port(&bytes);
    let mut terminal = Terminal::new(&bytes);
    terminal.flush_results = vec![Err(io::ErrorKind::BrokenPipe.into())];
    append_messages(&mut terminal, &mut port);
    assert_eq!(port.claims, 0, "nothing is taken off the queue");
}

#[test]
fn a_write_that_took_no_byte_returns_the_claim() {
    let bytes = Rc::new(RefCell::new(Vec::new()));
    let mut port = port(&bytes);
    let mut terminal = Terminal::new(&bytes);
    terminal.accepts = Some(0);
    append_messages(&mut terminal, &mut port);
    assert_eq!(port.settled, [("claim".to_owned(), false, Vec::new())]);
}

#[test]
fn a_partial_write_stays_unsettled_because_returning_it_could_deliver_it_twice() {
    let bytes = Rc::new(RefCell::new(Vec::new()));
    let mut port = port(&bytes);
    let mut terminal = Terminal::new(&bytes);
    terminal.accepts = Some(4);
    append_messages(&mut terminal, &mut port);
    assert_eq!(bytes.borrow().len(), 4);
    assert!(port.settled.is_empty());
}

#[test]
fn a_failed_final_flush_stays_unsettled() {
    let bytes = Rc::new(RefCell::new(Vec::new()));
    let mut port = port(&bytes);
    let mut terminal = Terminal::new(&bytes);
    terminal.flush_results = vec![Ok(()), Err(io::ErrorKind::BrokenPipe.into())];
    append_messages(&mut terminal, &mut port);
    assert!(port.settled.is_empty());
}
