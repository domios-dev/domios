pub(super) const MAX_ATTEMPTS: u8 = 10;
pub(super) const DELAY_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) enum Failure {
    Transient,
    RateLimitedUntil(u64),
}

#[derive(Default)]
pub(in crate::service::agent_runtime) struct Policy {
    pub send_id: String,
    pub turn_id: Option<String>,
    pub text: String,
    pub pending: Option<String>,
    attempts: u8,
    failure: Option<Failure>,
    reset_retry_used: bool,
    retry_claimed: bool,
    revoked_send: Option<String>,
    completed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) enum Decision {
    Ignore,
    Schedule {
        attempt: u8,
        at: u64,
        failure: Failure,
    },
    Exhausted(Failure),
}

impl Policy {
    pub fn begin(&mut self, send_id: &str, text: &str) {
        if self.send_id == send_id {
            return;
        }
        *self = Self {
            send_id: send_id.into(),
            text: text.into(),
            revoked_send: self.revoked_send.take(),
            ..Self::default()
        };
    }

    pub fn bind(&mut self, send_id: &str, turn_id: &str) {
        if self.send_id == send_id && !self.completed {
            self.turn_id = Some(turn_id.into());
        }
    }

    pub fn error(&mut self, turn_id: &str, failure: Failure) {
        if self.turn_id.as_deref() == Some(turn_id) && !self.completed {
            self.failure = Some(failure);
        }
    }

    pub fn clear_error(&mut self, turn_id: &str) {
        if self.turn_id.as_deref() == Some(turn_id) && !self.completed {
            self.failure = None;
        }
    }

    pub fn finish(&mut self, turn_id: &str, failed: bool, now: u64) -> Decision {
        if self.turn_id.as_deref() != Some(turn_id) || self.completed {
            return Decision::Ignore;
        }
        self.completed = true;
        if !failed || self.text.is_empty() {
            return Decision::Ignore;
        }
        match self.failure {
            Some(Failure::Transient) => {
                if self.attempts == MAX_ATTEMPTS {
                    return Decision::Exhausted(Failure::Transient);
                }
                self.attempts += 1;
                Decision::Schedule {
                    attempt: self.attempts,
                    at: now.saturating_add(DELAY_MS),
                    failure: Failure::Transient,
                }
            }
            Some(Failure::RateLimitedUntil(at)) if self.reset_retry_used => {
                Decision::Exhausted(Failure::RateLimitedUntil(at))
            }
            Some(Failure::RateLimitedUntil(at)) if at > now && at <= 9_007_199_254_740_991 => {
                self.reset_retry_used = true;
                Decision::Schedule {
                    attempt: 1,
                    at,
                    failure: Failure::RateLimitedUntil(at),
                }
            }
            _ => Decision::Ignore,
        }
    }

    pub fn claim(&mut self, id: &str, send_id: &str) -> Option<String> {
        if self.pending.as_deref() != Some(id) {
            return None;
        }
        self.pending = None;
        self.send_id = send_id.into();
        self.turn_id = None;
        self.failure = None;
        self.completed = false;
        self.retry_claimed = true;
        Some(self.text.clone())
    }

    pub fn cancel(&mut self) -> Option<String> {
        let pending = self.pending.take();
        let revoked_send = if self.retry_claimed {
            Some(self.send_id.clone())
        } else {
            self.revoked_send.take()
        };
        *self = Self {
            revoked_send,
            ..Self::default()
        };
        pending
    }

    pub fn permits_send(&self, send_id: &str) -> bool {
        self.revoked_send.as_deref() != Some(send_id)
    }
}

#[cfg(test)]
mod tests;
