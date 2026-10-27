use super::state::AccountMeshRefusal;

#[derive(Default)]
pub(super) struct Retry {
    failures: u32,
    pub deadline: Option<u64>,
    pub refusal: Option<AccountMeshRefusal>,
}

impl Retry {
    pub fn exhausted(&self) -> bool {
        self.failures > 4
    }

    pub fn resume(&mut self) {
        if self.exhausted() {
            *self = Self::default();
        }
    }

    pub fn pending(&self, now: u64) -> Option<AccountMeshRefusal> {
        self.refusal
            .filter(|_| self.exhausted() || self.deadline.is_some_and(|at| now < at))
    }

    pub fn settle(&mut self, now: u64, result: Result<(), AccountMeshRefusal>) {
        let delay = match result {
            Err(AccountMeshRefusal::RateLimited {
                retry_after_seconds,
            }) => retry_after_seconds.max(30),
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
