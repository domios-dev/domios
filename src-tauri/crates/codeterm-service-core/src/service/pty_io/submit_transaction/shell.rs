use super::cycle::{CycleWait, SubmitCycleSignal};
use super::payload_write::{MessagePayload, PayloadWriteFailure};
use super::*;
use crate::service::shell_exec::ExecWaiter;

pub(crate) fn shell_dispatch(bytes: &[u8]) -> (Vec<u8>, String) {
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    let line = String::from_utf8_lossy(bytes);
    let expected = line
        .strip_prefix(paste_encoding::BRACKETED_PASTE_START)
        .and_then(|line| line.strip_suffix(paste_encoding::BRACKETED_PASTE_END))
        .unwrap_or(&line)
        .to_string();
    (bytes.to_vec(), expected)
}

struct ShellTransport<'a> {
    svc: &'a ServiceContext,
    pty_id: &'a str,
    payload: &'a [u8],
    expected: &'a str,
    claim: SubmitCycleClaim,
    wake_tx: SyncSender<SubmitCycleSignal>,
    wake_rx: Receiver<SubmitCycleSignal>,
    waiter: RefCell<Option<ExecWaiter>>,
    queued: Option<&'a dyn Fn(usize)>,
}

impl SubmitTransport for ShellTransport<'_> {
    fn observe(&self) -> Option<Observation> {
        None
    }
    fn draft_evidence_count(&self, _: &str) -> usize {
        0
    }
    fn generation(&self) -> u64 {
        0
    }
    fn wait_change(&self, _: u64, _: Instant) -> bool {
        false
    }
    fn turn_started(&self) -> bool {
        false
    }
    fn exact_user_turn_present(&self, _: &str) -> bool {
        self.waiter
            .borrow()
            .as_ref()
            .is_some_and(ExecWaiter::started)
    }
    fn retry_active(&self) -> bool {
        self.claim.is_current()
    }
    fn max_submit_keys(&self) -> usize {
        4
    }
    fn prewrite_guard(&self) -> Result<(), InjectionError> {
        pty_owner_guard(self.svc, self.pty_id)?;
        let mux = self.svc.mux.read();
        if mux
            .tab_id_for_pty(self.pty_id)
            .and_then(|id| mux.get_tab(&id))
            .is_some_and(|tab| tab.agent_type.is_some() || tab.agent_booting)
            || self.claim.command_wait().is_some()
        {
            return Err(InjectionError::AgentInputUnavailable {
                pty_id: self.pty_id.into(),
            });
        }
        Ok(())
    }
    fn wait_retry_deadline_for_turn(&self, deadline: Instant, text: &str) -> bool {
        while self.retry_active() {
            if self.exact_user_turn_present(text) {
                return true;
            }
            match self.claim.cycle.wait_for_wake_until(
                self.claim.generation,
                deadline,
                &self.wake_rx,
            ) {
                CycleWait::Event => {}
                CycleWait::Deadline | CycleWait::Inactive => break,
            }
        }
        self.exact_user_turn_present(text)
    }
    fn wait_for_deferred_submit_ready(&self, deadline: Instant) -> DeferredSubmitWait {
        match self
            .claim
            .cycle
            .wait_for_wake_until(self.claim.generation, deadline, &self.wake_rx)
        {
            CycleWait::Event => DeferredSubmitWait::Woken,
            CycleWait::Deadline => DeferredSubmitWait::Deadline,
            CycleWait::Inactive => DeferredSubmitWait::Inactive,
        }
    }
    fn write_paste(&self, _: &str, deadline: Instant) -> Result<usize, InjectionError> {
        let session = self
            .svc
            .pty
            .lock()
            .get_session(self.pty_id)
            .ok_or_else(|| InjectionError::PtyClosed {
                pty_id: self.pty_id.into(),
            })?;
        MessagePayload::new(
            self.payload.to_vec(),
            self.claim.clone(),
            self.wake_tx.clone(),
        )
        .enqueue(
            &session.write_queue,
            None,
            deadline.saturating_duration_since(Instant::now()),
            || {
                *self.waiter.borrow_mut() =
                    Some(ExecWaiter::for_command(self.pty_id, self.expected));
                if let Some(queued) = &self.queued {
                    queued(self.payload.len());
                }
            },
        )
        .map(|queued| queued.len())
        .map_err(|failure| match failure {
            PayloadWriteFailure::NotQueued(error) => injection_write_error(self.pty_id, error),
            PayloadWriteFailure::Queued(_) => InjectionError::SubmitNeverConfirmed {
                waited: Duration::ZERO,
                screen_tail: String::new(),
            },
        })
    }
    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError> {
        self.prewrite_guard()?;
        if !self.retry_active() {
            return Err(InjectionError::PtyClosed {
                pty_id: self.pty_id.into(),
            });
        }
        let session = self
            .svc
            .pty
            .lock()
            .get_session(self.pty_id)
            .ok_or_else(|| InjectionError::PtyClosed {
                pty_id: self.pty_id.into(),
            })?;
        session
            .write_and_flush_with_enqueued(
                key.to_vec(),
                deadline.saturating_duration_since(Instant::now()),
                || {},
            )
            .map_err(|error| injection_write_error(self.pty_id, error))
    }
}

pub(crate) fn submit_shell_dispatch(
    svc: &ServiceContext,
    pty_id: &str,
    payload: &[u8],
    expected: &str,
    claim: SubmitCycleClaim,
    queued: Option<&dyn Fn(usize)>,
) -> Result<(InjectionResult, ExecWaiter), InjectionError> {
    let (wake_tx, wake_rx) = mpsc::sync_channel(1);
    let transport = ShellTransport {
        svc,
        pty_id,
        payload,
        expected,
        claim,
        wake_tx,
        wake_rx,
        waiter: RefCell::new(None),
        queued,
    };
    let result = run_shell(&transport, expected)?;
    let elapsed = result.elapsed;
    let result = result.require_submission(true, elapsed, String::new())?;
    let waiter =
        transport
            .waiter
            .into_inner()
            .ok_or_else(|| InjectionError::SubmitNeverConfirmed {
                waited: elapsed,
                screen_tail: String::new(),
            })?;
    Ok((result, waiter))
}

fn run_shell<T: SubmitTransport>(
    transport: &T,
    expected: &str,
) -> Result<InjectionResult, InjectionError> {
    runner::run_with_submit_key_and_payload(
        transport,
        expected,
        true,
        b"\r",
        expected,
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
            retry_offsets: DEFAULT_SUBMIT_RETRY_OFFSETS,
        },
        VerifyOpts {
            require_prompt_first: false,
            verify_echo: false,
            verify_submit: true,
            submit_timeout: Duration::from_secs(2),
            ..Default::default()
        },
        |_, _| {
            transport.prewrite_guard()?;
            if transport.retry_active() {
                Ok(())
            } else {
                Err(InjectionError::SubmitNeverConfirmed {
                    waited: Duration::ZERO,
                    screen_tail: String::new(),
                })
            }
        },
    )
}

#[cfg(test)]
mod tests;
