use super::{DeferredSubmitWait, Observation, SubmitTransport, MAX_SUBMIT_KEY_WRITES};
use crate::agent::provider::{PasteCapability, VerifySubmitStrategy};
use crate::service::inject_verify::{InjectionError, InjectionResult, VerifyOpts};
use std::time::{Duration, Instant};

fn draft_left_composer<T: SubmitTransport>(
    transport: &T,
    text: &str,
    baseline_evidence: usize,
    echo_observed: bool,
    armed: u64,
) -> bool {
    echo_observed
        && transport.generation() != armed
        && transport.draft_evidence_count(text) <= baseline_evidence
}

fn submission_acknowledged_now<T: SubmitTransport>(
    transport: &T,
    text: &str,
    expected_user_turn: &str,
    baseline_evidence: usize,
    echo_observed: bool,
    turn_was_started: bool,
    armed: u64,
    strategy: &VerifySubmitStrategy,
) -> bool {
    let changed = transport.generation() != armed;
    let fresh_turn = changed && !turn_was_started && transport.turn_started();
    let drained = draft_left_composer(transport, text, baseline_evidence, echo_observed, armed);
    if matches!(strategy, VerifySubmitStrategy::TuiInputDrained { .. }) {
        return if transport.has_exact_user_turn_evidence() {
            transport.exact_user_turn_present(expected_user_turn)
        } else {
            fresh_turn
        };
    }
    drained || fresh_turn
}

#[cfg(test)]
pub(super) fn run<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
) -> Result<InjectionResult, InjectionError> {
    run_with_submit_key(transport, text, enter, b"\r", strategy, opts, |_| Ok(()))
}

#[cfg(test)]
pub(super) fn run_with_submit_key<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    submit_key: &[u8],
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    mut key_guard: impl FnMut(bool) -> Result<(), InjectionError>,
) -> Result<InjectionResult, InjectionError> {
    run_with_submit_key_and_follow_up(
        transport,
        text,
        enter,
        submit_key,
        None,
        strategy,
        opts,
        &mut key_guard,
    )
}

pub(super) fn run_with_submit_key_and_follow_up<T: SubmitTransport>(
    transport: &T,
    text: &str,
    enter: bool,
    submit_key: &[u8],
    typed_follow_up: Option<&str>,
    strategy: VerifySubmitStrategy,
    opts: VerifyOpts,
    mut key_guard: impl FnMut(bool) -> Result<(), InjectionError>,
) -> Result<InjectionResult, InjectionError> {
    if let Err(error) = transport.dialog_guard() {
        return Err(error);
    }
    transport.prepare_paste(text, enter);
    let started = Instant::now();
    let turn_deadline = started + opts.submit_timeout;
    let two_phase = enter && opts.verify_submit;
    let typed_follow_up = typed_follow_up
        .filter(|follow_up| PasteCapability::is_valid_typed_follow_up(follow_up))
        .filter(|_| enter && !text.is_empty());
    let expected_user_turn = if enter && !text.is_empty() {
        typed_follow_up.map_or_else(
            || text.to_string(),
            |follow_up| format!("{text}{follow_up}"),
        )
    } else {
        text.to_string()
    };
    let exact_turn_evidence = matches!(&strategy, VerifySubmitStrategy::TuiInputDrained { .. })
        && transport.has_exact_user_turn_evidence();
    let initial_key_delay = if enter && !text.is_empty() {
        strategy.initial_key_delay()
    } else {
        Duration::ZERO
    };

    let baseline_evidence = transport.draft_evidence_count(text);
    let turn_was_started = enter.then(|| transport.turn_started());
    let bytes_written = transport.write_paste(text, turn_deadline)?;
    let mut submit_key_writes = 0;
    let mut typed_follow_up_written = false;
    let mut paste_observed =
        text.is_empty() || transport.draft_evidence_count(text) > baseline_evidence;

    if !enter {
        return Ok(InjectionResult {
            bytes_written,
            echo_observed: paste_observed,
            submit_observed: false,
            submit_anomaly: false,
            submit_key_writes: 0,
            elapsed: started.elapsed(),
        });
    }

    let turn_was_started = turn_was_started.unwrap_or(false);
    let submit_started = Instant::now();
    let mut first_submit_key_at = (submit_key_writes > 0).then_some(submit_started);
    let mut submit_observed = !two_phase;
    let mut dialog_blocked = false;
    while submit_key_writes < MAX_SUBMIT_KEY_WRITES {
        if (submit_key_writes > 0 && submit_observed)
            || (exact_turn_evidence && transport.exact_user_turn_present(&expected_user_turn))
        {
            submit_observed = true;
            break;
        }
        if submit_key_writes == 0 && !initial_key_delay.is_zero() {
            let first_key_deadline = submit_started + initial_key_delay;
            if transport.wait_retry_deadline_for_turn(
                first_key_deadline.min(turn_deadline),
                &expected_user_turn,
            ) {
                submit_observed = true;
                break;
            }
            if !transport.retry_active()
                || (submit_key_writes > 0 && Instant::now() >= turn_deadline)
            {
                break;
            }
        } else if submit_key_writes > 0 {
            if !transport.retry_active() {
                break;
            }
            let retry_offsets = strategy.retry_offsets();
            let offset = retry_offsets
                .get(submit_key_writes - 1)
                .copied()
                .unwrap_or(*retry_offsets.last().expect("non-empty"));
            let retry_deadline = first_submit_key_at.unwrap_or(submit_started) + offset;
            if transport.wait_retry_deadline_for_turn(
                retry_deadline.min(turn_deadline),
                &expected_user_turn,
            ) {
                submit_observed = true;
                break;
            }
            if !transport.retry_active() || Instant::now() >= turn_deadline {
                break;
            }
        }
        if exact_turn_evidence && transport.exact_user_turn_present(&expected_user_turn) {
            submit_observed = true;
            break;
        }
        let allow_initial_key = submit_key_writes == 0;
        let mut key_error = None;
        let key_ready = loop {
            match key_guard(allow_initial_key) {
                Ok(()) => break true,
                Err(error @ InjectionError::AgentTuiPresent { .. }) if bytes_written > 0 => {
                    dialog_blocked = true;
                    key_error = Some(error);
                    break false;
                }
                Err(error @ InjectionError::AgentInputUnavailable { .. }) if bytes_written > 0 => {
                    if transport.wait_for_submit_event(turn_deadline) {
                        continue;
                    }
                    key_error = Some(error);
                    break false;
                }
                Err(error) => {
                    key_error = Some(error);
                    break false;
                }
            }
        };
        if !key_ready {
            if submit_key_writes > 0 {
                break;
            }
            if bytes_written == 0 {
                return Err(key_error.expect("failed key guard retains its error"));
            }
            let screen_tail = transport
                .observe()
                .map(|state| state.tail)
                .unwrap_or_default();
            return Err(InjectionError::SubmitNeverConfirmed {
                waited: started.elapsed(),
                screen_tail,
            });
        }
        if exact_turn_evidence && transport.exact_user_turn_present(&expected_user_turn) {
            submit_observed = true;
            break;
        }
        let key_deadline = if Instant::now() < turn_deadline {
            turn_deadline
        } else {
            Instant::now() + opts.submit_timeout
        };
        if !typed_follow_up_written {
            if let Some(follow_up) = typed_follow_up {
                if transport
                    .write_typed_follow_up(follow_up, key_deadline)
                    .is_err()
                {
                    let screen_tail = transport
                        .observe()
                        .map(|state| state.tail)
                        .unwrap_or_default();
                    return Err(InjectionError::SubmitNeverConfirmed {
                        waited: started.elapsed(),
                        screen_tail,
                    });
                }
                typed_follow_up_written = true;
            }
        }
        let enter_armed = transport.generation();
        // A prior payload or submit key makes this failure submission uncertainty.
        if let Err(error) = transport.write_submit_key(submit_key, key_deadline) {
            if text.is_empty() && submit_key_writes == 0 {
                return Err(error);
            }
            log::warn!(
                "[submit-transaction] post-paste submit-key write failed ({error}); classified unconfirmed"
            );
            let screen_tail = transport
                .observe()
                .map(|state| state.tail)
                .unwrap_or_default();
            return Err(InjectionError::SubmitNeverConfirmed {
                waited: started.elapsed(),
                screen_tail,
            });
        }
        submit_key_writes += 1;
        first_submit_key_at.get_or_insert_with(Instant::now);
        if !two_phase {
            break;
        }
        paste_observed |= transport.draft_evidence_count(text) > baseline_evidence;
        submit_observed |= submission_acknowledged_now(
            transport,
            text,
            &expected_user_turn,
            baseline_evidence,
            paste_observed,
            turn_was_started,
            enter_armed,
            &strategy,
        );
        submit_observed |=
            exact_turn_evidence && transport.exact_user_turn_present(&expected_user_turn);
        if submit_observed {
            break;
        }
    }

    if two_phase
        && !submit_observed
        && bytes_written > 0
        && submit_key_writes > 0
        && !dialog_blocked
        && transport.retry_active()
    {
        let deferred_deadline = Instant::now() + opts.submit_timeout;
        loop {
            if exact_turn_evidence && transport.exact_user_turn_present(&expected_user_turn) {
                submit_observed = true;
                break;
            }
            match transport.wait_for_deferred_submit_ready(deferred_deadline) {
                DeferredSubmitWait::Ready => {}
                DeferredSubmitWait::Woken => {
                    if exact_turn_evidence && transport.exact_user_turn_present(&expected_user_turn)
                    {
                        submit_observed = true;
                        break;
                    }
                    continue;
                }
                DeferredSubmitWait::Deadline | DeferredSubmitWait::Inactive => break,
            }
            if exact_turn_evidence && transport.exact_user_turn_present(&expected_user_turn) {
                submit_observed = true;
                break;
            }
            match key_guard(false) {
                Ok(()) => {}
                Err(InjectionError::AgentTuiPresent { .. }) => break,
                Err(InjectionError::AgentInputUnavailable { .. }) => {
                    if transport.wait_for_submit_event(deferred_deadline) {
                        continue;
                    }
                    break;
                }
                Err(_) => break,
            }
            let enter_armed = transport.generation();
            let key_deadline = Instant::now() + opts.submit_timeout;
            if let Err(error) = transport.write_submit_key(submit_key, key_deadline) {
                log::warn!(
                    "[submit-transaction] deferred submit-key write failed ({error}); classified unconfirmed"
                );
                let screen_tail = transport
                    .observe()
                    .map(|state| state.tail)
                    .unwrap_or_default();
                return Err(InjectionError::SubmitNeverConfirmed {
                    waited: started.elapsed(),
                    screen_tail,
                });
            }
            submit_key_writes += 1;
            if exact_turn_evidence {
                while !transport.exact_user_turn_present(&expected_user_turn) {
                    if !transport.wait_for_submit_event(deferred_deadline) {
                        break;
                    }
                }
                submit_observed = transport.exact_user_turn_present(&expected_user_turn);
            } else {
                submit_observed = submission_acknowledged_now(
                    transport,
                    text,
                    &expected_user_turn,
                    baseline_evidence,
                    paste_observed,
                    turn_was_started,
                    enter_armed,
                    &strategy,
                );
            }
            break;
        }
    }

    Ok(InjectionResult {
        bytes_written,
        echo_observed: paste_observed,
        submit_observed,
        submit_anomaly: two_phase && !submit_observed,
        submit_key_writes,
        elapsed: started.elapsed(),
    })
}
