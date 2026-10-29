use super::state::AccountMeshRefusal;

#[derive(Default)]
pub(super) struct Retry {
    failures: u32,
    permanent_failures: u32,
    budget_retry_used: bool,
    pub deadline: Option<u64>,
    pub refusal: Option<AccountMeshRefusal>,
}

impl Retry {
    pub fn exhausted(&self) -> bool {
        self.failures > 4
    }

    pub fn keep_recovering(&mut self, now: u64) {
        if self.exhausted() && self.deadline.is_none() {
            self.deadline = Some(now.saturating_add(300));
        }
    }

    pub fn resume(&mut self) {
        if matches!(
            self.refusal,
            Some(AccountMeshRefusal::EnrollmentBudget { .. })
        ) {
            return;
        }
        if self.exhausted() || self.permanent_failures > 0 {
            let budget_retry_used = self.budget_retry_used;
            *self = Self {
                budget_retry_used,
                ..Self::default()
            };
        }
    }

    pub fn pending(&self, now: u64) -> Option<AccountMeshRefusal> {
        self.refusal.filter(|_| {
            (self.exhausted() && self.deadline.is_none())
                || (self.budget_retry_used && self.deadline.is_none())
                || self.deadline.is_some_and(|at| now < at)
        })
    }

    pub fn settle(&mut self, now: u64, result: Result<(), AccountMeshRefusal>) {
        if let Err(AccountMeshRefusal::EnrollmentBudget {
            retry_after_seconds,
        }) = result
        {
            self.failures = 0;
            self.permanent_failures = 0;
            self.deadline = (!self.budget_retry_used)
                .then(|| now.saturating_add(retry_after_seconds.unwrap_or(900)));
            self.budget_retry_used = true;
            self.refusal = result.err();
            return;
        }
        if matches!(
            result,
            Err(AccountMeshRefusal::Invalid
                | AccountMeshRefusal::Forbidden
                | AccountMeshRefusal::Conflict
                | AccountMeshRefusal::KeyConflict
                | AccountMeshRefusal::GenerationMismatch
                | AccountMeshRefusal::ChallengeInvalid
                | AccountMeshRefusal::ProofInvalid
                | AccountMeshRefusal::Revoked)
        ) {
            let delay = (300u64 << self.permanent_failures.min(6)).min(14400);
            self.permanent_failures = self.permanent_failures.saturating_add(1);
            self.failures = 0;
            self.deadline = Some(now.saturating_add(delay));
            self.refusal = result.err();
            return;
        }
        self.permanent_failures = 0;
        let delay = match result {
            Err(AccountMeshRefusal::RateLimited {
                retry_after_seconds,
            }) => retry_after_seconds.max((30u64 << self.failures.min(4)).min(300)),
            Err(
                AccountMeshRefusal::Unavailable
                | AccountMeshRefusal::Offline
                | AccountMeshRefusal::AuthorityUnavailable,
            ) => (30u64 << self.failures.min(4)).min(300),
            _ => {
                *self = Self::default();
                return;
            }
        };
        self.failures = self.failures.saturating_add(1);
        self.deadline = (!self.exhausted()).then(|| now.saturating_add(delay));
        self.refusal = result.err();
    }
}

#[cfg(test)]
mod tests;
