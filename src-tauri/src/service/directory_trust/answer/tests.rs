use super::*;
use std::collections::VecDeque;

struct Script {
    frame: Frame,
    events: VecDeque<Option<Frame>>,
    writes: Vec<Vec<u8>>,
    fail_write: Option<usize>,
    deadlines: Vec<Instant>,
    clock: Instant,
}

fn prompt(generation: u64, occurrence: &str, step: &[u8]) -> Frame {
    Frame {
        generation,
        prompt: Some((
            occurrence.into(),
            DirectoryTrustPromptResponse::Answer {
                step: step.to_vec(),
            },
        )),
    }
}

fn clear(generation: u64) -> Option<Frame> {
    Some(Frame {
        generation,
        prompt: None,
    })
}

fn focused(generation: u64) -> Option<Frame> {
    Some(prompt(generation, "trust", b"\r"))
}

fn script(events: Vec<Option<Frame>>) -> Script {
    Script {
        frame: prompt(0, "trust", b"j"),
        events: events.into(),
        writes: Vec::new(),
        fail_write: None,
        deadlines: Vec::new(),
        clock: Instant::now(),
    }
}

impl AnswerIo for Script {
    fn read(&mut self) -> Result<Frame, String> {
        Ok(Frame {
            generation: self.frame.generation,
            prompt: self.frame.prompt.clone(),
        })
    }
    fn write(&mut self, occurrence: &str, step: &[u8]) -> Result<u64, String> {
        if next_step(&self.frame, occurrence)? != step {
            return Err("stale".into());
        }
        self.writes.push(step.to_vec());
        if self.fail_write == Some(self.writes.len()) {
            return Err("scripted write failure".into());
        }
        Ok(self.frame.generation)
    }
    fn now(&self) -> Instant {
        self.clock
    }
    fn wait_after(&mut self, _generation: u64, deadline: Instant) -> bool {
        self.deadlines.push(deadline);
        match self.events.pop_front().expect("unexpected event wait") {
            Some(frame) => {
                self.frame = frame;
                true
            }
            None => false,
        }
    }
}

#[test]
fn navigation_then_confirmed_focus_writes_separate_keys() {
    let mut io = script(vec![focused(1), clear(2)]);
    assert_eq!(answer(&mut io, "trust", Instant::now()), Ok(()));
    assert_eq!(io.writes, vec![b"j".to_vec(), b"\r".to_vec()]);
}
#[test]
fn stalled_focus_retries_navigation_once_without_enter() {
    let mut io = script(vec![None, None]);
    assert_eq!(
        answer(&mut io, "trust", Instant::now()),
        Err("folder trust affirmative focus was not confirmed".into())
    );
    assert_eq!(io.writes, vec![b"j".to_vec(), b"j".to_vec()]);
}
#[test]
fn navigation_write_error_propagates_without_enter() {
    let mut io = script(vec![]);
    io.fail_write = Some(1);
    assert_eq!(
        answer(&mut io, "trust", Instant::now()),
        Err("scripted write failure".into())
    );
    assert_eq!(io.writes, vec![b"j".to_vec()]);
}
#[test]
fn persistent_dialog_after_enter_is_failure() {
    let mut io = script(vec![focused(1), focused(2), None]);
    assert_eq!(
        answer(&mut io, "trust", Instant::now()),
        Err("folder trust acceptance was not confirmed".into())
    );
    assert_eq!(io.writes, vec![b"j".to_vec(), b"\r".to_vec()]);
}
#[test]
fn later_disappearance_confirms_acceptance() {
    let mut io = script(vec![clear(1)]);
    io.frame = prompt(0, "trust", b"\r");
    assert_eq!(answer(&mut io, "trust", Instant::now()), Ok(()));
    assert_eq!(io.writes, vec![b"\r".to_vec()]);
}
#[test]
fn changed_occurrence_withholds_stale_enter() {
    let mut io = script(vec![Some(prompt(1, "other", b"\r"))]);
    assert!(answer(&mut io, "trust", Instant::now()).is_err());
    assert_eq!(io.writes, vec![b"j".to_vec()]);
}
#[test]
fn retry_waits_for_confirmed_focus_before_enter() {
    let mut io = script(vec![None, focused(1), clear(2)]);
    assert_eq!(answer(&mut io, "trust", Instant::now()), Ok(()));
    assert_eq!(
        io.writes,
        vec![b"j".to_vec(), b"j".to_vec(), b"\r".to_vec()]
    );
}
#[test]
fn enter_write_error_is_not_acceptance() {
    let mut io = script(vec![focused(1)]);
    io.fail_write = Some(2);
    assert_eq!(
        answer(&mut io, "trust", Instant::now()),
        Err("scripted write failure".into())
    );
}
#[test]
fn disappearance_before_submit_is_not_acceptance() {
    let mut io = script(vec![clear(1)]);
    assert!(answer(&mut io, "trust", Instant::now()).is_err());
    assert_eq!(io.writes, vec![b"j".to_vec()]);
}

#[test]
fn failed_transaction_releases_claim_and_leaves_occurrence_for_owner() {
    let mut ledger = super::super::TrustLedger::default();
    let occurrence = frame_id("trust");
    assert!(ledger.claim_trust("tab", occurrence));
    ledger.finish_trust_answer("tab", occurrence, false);
    assert!(!ledger.tabs["tab"].trust_claimed);
    assert!(!ledger.tabs["tab"].trust_prompt_resolved);
    assert!(!ledger.claim_trust("tab", occurrence));
    assert!(ledger.claim_trust("tab", frame_id("next")));
}

#[test]
fn retry_and_acceptance_share_one_overall_deadline() {
    let mut io = script(vec![None, focused(1), clear(2)]);
    let started = Instant::now();
    assert_eq!(answer(&mut io, "trust", started), Ok(()));
    assert_eq!(
        io.deadlines,
        vec![
            started + ANSWER_DEADLINE / 2,
            started + ANSWER_DEADLINE,
            started + ANSWER_DEADLINE
        ]
    );
}

#[test]
fn exhausted_deadline_never_submits() {
    let mut io = script(vec![]);
    io.frame = prompt(0, "trust", b"\r");
    let started = Instant::now();
    io.clock = started + ANSWER_DEADLINE;
    assert!(answer(&mut io, "trust", started).is_err());
    assert!(io.writes.is_empty());
}

#[test]
fn same_generation_cannot_confirm_navigation_focus() {
    let mut io = script(vec![focused(0), None]);
    assert!(answer(&mut io, "trust", Instant::now()).is_err());
    assert_eq!(io.writes, vec![b"j".to_vec()]);
}
