use super::{Billing, TimeState};
use std::{future::Future, time::Duration};

/// What the daemon owes an entitlement instant that just passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeadlineTick {
    pub refresh_due: bool,
}

impl Billing {
    /// The next instant projected access can change: refresh, lease or period end, checking bound.
    pub fn next_deadline(&self) -> Option<u64> {
        let now = self.now();
        let cache = self.cache.lock();
        let grant = cache
            .verified
            .as_ref()
            .and_then(|grant| grant.next_transition(now));
        let checking = cache.checking_until.filter(|until| *until > now);
        grant.into_iter().chain(checking).min()
    }

    pub fn refresh_due(&self) -> bool {
        let now = self.now();
        self.cache
            .lock()
            .verified
            .as_ref()
            .is_some_and(|grant| grant.verdict(now).state == TimeState::Stale)
    }

    /// Runs for the daemon's life. One timer armed for the next signed instant and re-armed when
    /// the grant changes; it never wakes between instants, so it is not a polling loop.
    pub async fn drive_deadlines<F, Fut>(&self, mut on_deadline: F)
    where
        F: FnMut(DeadlineTick) -> Fut,
        Fut: Future<Output = ()>,
    {
        loop {
            let rearm = self.rearm.notified();
            tokio::pin!(rearm);
            rearm.as_mut().enable();
            let Some(at) = self.next_deadline() else {
                rearm.await;
                continue;
            };
            let wait = Duration::from_secs(at.saturating_sub(self.now()));
            tokio::select! {
                _ = &mut rearm => continue,
                _ = tokio::time::sleep(wait) => {}
            }
            let observer = self.clone();
            if let Err(error) = tokio::task::spawn_blocking(move || observer.observe()).await {
                log::debug!("Observed-time persistence task failed: {error}");
            }
            on_deadline(DeadlineTick {
                refresh_due: self.refresh_due(),
            })
            .await;
        }
    }
}
