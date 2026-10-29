use super::*;
use tokio::sync::watch;

type ReadResult = Result<serde_json::Value>;

#[derive(Default)]
pub(super) struct PlanRead {
    value: Option<ReadResult>,
    attempted_at: Option<u64>,
    revision: u64,
    deferred_until: u64,
    flight: Option<watch::Receiver<Option<ReadResult>>>,
}

impl PlanRead {
    pub(super) fn recovered(&mut self) {
        self.deferred_until = 0;
    }

    pub(super) fn invalidate(&mut self) {
        self.value = None;
        self.attempted_at = None;
        self.revision += 1;
    }
}

impl Billing {
    pub async fn plan_operation(
        &self,
        session: &codeterm_account::Session,
        operation: PlanOperation,
    ) -> ReadResult {
        let (index, force) = match operation {
            PlanOperation::Catalog {} => (0, false),
            PlanOperation::Subscription {} => (1, false),
            PlanOperation::RefreshCatalog {} => (0, true),
            PlanOperation::RefreshSubscription {} => (1, true),
            PlanOperation::Change { .. } => {
                let worker = self.clone();
                let owned_session = session.clone();
                let generation = tokio::task::spawn_blocking(move || {
                    let mut cache = worker.cache.lock();
                    if !worker.bind(&mut cache, Some(&owned_session)) {
                        return Err("Account changed while managing billing".to_string());
                    }
                    Ok(cache.generation)
                })
                .await
                .map_err(|_| "Billing cache unavailable".to_string())??;
                let result = self.provider.plan_operation(session, operation).await;
                // An uncertain mutation may already have reached the billing server.
                self.refresh_bound(Some(session), RefreshEvent::PlanChanged, Some(generation)).await;
                return if self.read_session_matches(session) { result } else {
                    Err("Account changed while managing billing".into())
                };
            }
        };
        let worker = self.clone();
        let owned_session = session.clone();
        let mut flight = tokio::task::spawn_blocking(move || {
            worker.admit_read(owned_session, index, force)
        })
        .await
        .map_err(|_| "Billing cache unavailable".to_string())??;
        loop {
            if let Some(result) = flight.borrow().clone() {
                return if self.read_session_matches(session) { result } else {
                    Err("Account changed while reading billing".into())
                };
            }
            if flight.changed().await.is_err() {
                return Err("Billing cache unavailable".into());
            }
        }
    }

    #[cfg(test)]
    pub(super) fn admit_test_read(
        &self,
        session: codeterm_account::Session,
    ) -> watch::Receiver<Option<ReadResult>> {
        self.admit_read(session, 1, false).unwrap()
    }

    fn read_session_matches(&self, session: &codeterm_account::Session) -> bool {
        let cache = self.cache.lock();
        cache.uid.as_ref() == Some(&session.identity.uid)
            && cache.login_generation == Some(session.login_generation)
    }

    fn admit_read(
        &self,
        session: codeterm_account::Session,
        index: usize,
        force: bool,
    ) -> Result<watch::Receiver<Option<ReadResult>>> {
        let mut cache = self.cache.lock();
        if !self.bind(&mut cache, Some(&session)) {
            return Err("Account changed while reading billing".into());
        }
        let generation = cache.generation;
        let now = self.now();
        let cooling = now < cache.retry_after;
        let read = &mut cache.reads[index];
        if let Some(flight) = &read.flight {
            return Ok(flight.clone());
        }
        if let Some(value) = &read.value {
            let recent = read.attempted_at.is_some_and(|at| now < at.saturating_add(60));
            if cooling || recent || (!force && value.is_ok()) {
                let (_, receiver) = watch::channel(Some(value.clone()));
                return Ok(receiver);
            }
        }
        if cooling || now < read.deferred_until {
            return Err("Billing refresh cooling down. Try again shortly.".into());
        }
        let (sender, receiver) = watch::channel(None);
        read.flight = Some(receiver.clone());
        let worker = self.clone();
        tokio::spawn(async move {
            let mut followed_up = false;
            loop {
                let (revision, error_revision) = {
                    let mut cache = worker.cache.lock();
                    if cache.generation != generation {
                        let _ = sender.send_replace(Some(Err("Account changed while reading billing".into())));
                        break;
                    }
                    cache.reads[index].attempted_at = Some(worker.now());
                    (cache.reads[index].revision, cache.error_revision)
                };
                let operation = if index == 0 {
                    PlanOperation::Catalog {}
                } else {
                    PlanOperation::Subscription {}
                };
                let result = worker.provider.plan_operation(&session, operation).await;
                let mut cache = worker.cache.lock();
                if cache.generation != generation {
                    let _ = sender.send_replace(Some(Err("Account changed while reading billing".into())));
                    break;
                }
                if result.is_err() {
                    cache.error_revision += 1;
                    cache.retry_after = worker.now().saturating_add(60);
                } else if cache.error_revision == error_revision {
                    cache.retry_after = 0;
                }
                let cooling = worker.now() < cache.retry_after;
                let read = &mut cache.reads[index];
                if read.revision != revision {
                    if !followed_up && !cooling && result.is_ok() {
                        followed_up = true;
                        continue;
                    }
                    read.flight = None;
                    read.deferred_until = worker.now().saturating_add(60);
                    let failure = result.and_then(|_| Err("Billing changed during the read. Refresh shortly.".into()));
                    let _ = sender.send_replace(Some(failure));
                    break;
                }
                read.deferred_until = 0;
                read.value = Some(result.clone());
                read.flight = None;
                let _ = sender.send_replace(Some(result));
                break;
            }
        });
        Ok(receiver)
    }
}
