use super::*;
use tokio::sync::watch;

enum Admission {
    Cached(Snapshot),
    Flight(watch::Receiver<Option<Snapshot>>),
}

impl Billing {
    #[cfg(test)]
    pub(super) fn admit_test_refresh(
        &self,
        session: &codeterm_account::Session,
        event: RefreshEvent,
    ) -> watch::Receiver<Option<Snapshot>> {
        match self.admit_refresh(Some(session.clone()), event, None) {
            Admission::Flight(flight) => flight,
            Admission::Cached(_) => panic!("Expected an active refresh flight"),
        }
    }

    pub async fn refresh(
        &self,
        session: Option<&codeterm_account::Session>,
        event: RefreshEvent,
        _now: Instant,
    ) -> Snapshot {
        self.refresh_bound(session, event, None).await
    }

    pub(super) async fn refresh_bound(
        &self,
        session: Option<&codeterm_account::Session>,
        event: RefreshEvent,
        expected_generation: Option<u64>,
    ) -> Snapshot {
        let worker = self.clone();
        let owned_session = session.cloned();
        let admission = tokio::task::spawn_blocking(move || {
            worker.admit_refresh(owned_session, event, expected_generation)
        })
        .await;
        let Ok(Admission::Flight(mut flight)) = admission else {
            return match admission {
                Ok(Admission::Cached(snapshot)) => snapshot,
                _ => self.snapshot(session),
            };
        };
        loop {
            if let Some(snapshot) = flight.borrow().clone() {
                return snapshot;
            }
            if flight.changed().await.is_err() {
                return self.snapshot(session);
            }
        }
    }

    fn admit_refresh(
        &self,
        session: Option<codeterm_account::Session>,
        event: RefreshEvent,
        expected_generation: Option<u64>,
    ) -> Admission {
        let mut cache = self.cache.lock();
        if expected_generation.is_some_and(|generation| cache.generation != generation) {
            return Admission::Cached(Snapshot::default());
        }
        if !self.bind(&mut cache, session.as_ref()) {
            return Admission::Cached(Snapshot::default());
        }
        if session.is_none() || event == RefreshEvent::Focus {
            return Admission::Cached(self.recompute(&cache));
        }
        let forced = matches!(
            event,
            RefreshEvent::CheckoutReturned | RefreshEvent::PlanChanged
        ) || (event == RefreshEvent::Explicit && cache.checkout_pending);
        if forced {
            cache.reads[1].invalidate();
        }
        let returned = cache.checkout_event_pending
            && matches!(
                event,
                RefreshEvent::CheckoutReturned | RefreshEvent::Explicit
            );
        if forced {
            cache.checkout_pending = false;
            cache.checkout_event_pending = false;
        }
        if let Some(flight) = cache.refresh_flight.clone() {
            cache.refresh_queued |= forced;
            drop(cache);
            if returned {
                if let Some(observer) = &self.observer {
                    observer(ProductEvent::CheckoutReturned);
                }
            }
            return Admission::Flight(flight);
        }
        let now = self.now();
        if !forced
            && (now < cache.retry_after
                || cache
                    .refresh_attempted_at
                    .is_some_and(|at| now < at.saturating_add(60))
                || (event == RefreshEvent::SignIn
                    && !cache.refresh_queued
                    && cache
                        .verified
                        .as_ref()
                        .is_some_and(|grant| grant.verdict(now).state == TimeState::Fresh)))
        {
            return Admission::Cached(self.recompute(&cache));
        }
        cache.checkout_pending = false;
        cache.checkout_event_pending = false;
        cache.refresh_attempted_at = Some(now);
        cache.refresh_queued = false;
        cache.refresh_followup = false;
        let generation = cache.generation;
        let (sender, receiver) = watch::channel(None);
        cache.refresh_flight = Some(receiver.clone());
        let worker = self.clone();
        tokio::spawn(async move {
            if returned {
                if let Some(observer) = &worker.observer {
                    observer(ProductEvent::CheckoutReturned);
                }
            }
            loop {
                let mut snapshot = worker.fetch_refresh(session.as_ref(), generation).await;
                let mut cache = worker.cache.lock();
                if cache.generation != generation {
                    let _ = sender.send_replace(Some(Snapshot::default()));
                    break;
                }
                if cache.refresh_queued && !cache.refresh_followup {
                    cache.refresh_followup = true;
                    cache.refresh_queued = false;
                    cache.refresh_attempted_at = Some(worker.now());
                    continue;
                }
                if cache.refresh_queued {
                    cache.snapshot.stale = true;
                    snapshot.stale = true;
                }
                cache.refresh_flight = None;
                let _ = sender.send_replace(Some(snapshot));
                break;
            }
        });
        Admission::Flight(receiver)
    }
}
