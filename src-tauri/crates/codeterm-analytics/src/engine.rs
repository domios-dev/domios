use crate::{
    state::{Queued, State, BATCH_CAP, QUEUE_CAP, RETRY_DELAYS_MS},
    AnalyticsSink, Config, Consent, Counters, DeliveryEvent, DeliveryResult, Event, Mode,
};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub consent: Consent,
    pub config: Config,
    pub queued: usize,
    pub has_device_id: bool,
    pub has_user_id: bool,
    pub counters: Counters,
}

pub(crate) struct Engine {
    pub state: State,
    path: PathBuf,
    pub sink: Box<dyn AnalyticsSink>,
}

impl Engine {
    pub fn open(path: PathBuf, config: Config) -> Result<Self, String> {
        let mut state = State::read(&path)?;
        state.user_id = None;
        if state.consent == Consent::Off {
            state.clear();
        }
        if state.config.project != config.project
            || state.config.zone != config.zone
            || config.mode == Mode::Off
        {
            state.counters.dropped_config += state.queue.len() as u64;
            state.queue.clear();
        }
        while state.queue.len() > QUEUE_CAP {
            state.queue.pop_front();
            state.counters.dropped_oldest += 1;
        }
        state.config = config;
        state.save(&path)?;
        let sink = crate::sinks::from_config(
            config,
            Arc::new(|_| Err("analytics credential unavailable".into())),
        )?;
        Ok(Self { state, path, sink })
    }

    pub fn status(&self) -> Status {
        Status {
            consent: self.state.consent,
            config: self.state.config,
            queued: self.state.queue.len(),
            has_device_id: self.state.device_id.is_some(),
            has_user_id: self.state.user_id.is_some(),
            counters: self.state.counters.clone(),
        }
    }

    fn commit(&mut self, next: State) -> Result<(), String> {
        if let Err(error) = next.save(&self.path) {
            self.state.clear();
            return Err(error);
        }
        self.state = next;
        Ok(())
    }

    pub fn consent(&mut self, consent: Consent) -> Result<Status, String> {
        let mut next = self.state.clone();
        match consent {
            Consent::On => {
                next.consent = Consent::On;
                if next.device_id.is_none() {
                    next.device_id = Some(Uuid::new_v4());
                }
            }
            Consent::Off => next.clear(),
        }
        self.commit(next)?;
        Ok(self.status())
    }

    pub fn identity(&mut self, uid: Option<String>) -> Result<(), String> {
        if self.state.consent == Consent::Off {
            return Ok(());
        }
        if uid
            .as_deref()
            .is_some_and(|uid| !crate::state::valid_uid(uid))
        {
            return Err("invalid account uid".into());
        }
        let mut next = self.state.clone();
        next.user_id = uid;
        self.commit(next)
    }

    pub fn enqueue(&mut self, event: Event, now: u64) -> Result<bool, String> {
        self.enqueue_on_day(event, now, chrono::Local::now().date_naive())
    }
    pub fn enqueue_on_day(
        &mut self,
        event: Event,
        now: u64,
        day: chrono::NaiveDate,
    ) -> Result<bool, String> {
        if self.state.consent != Consent::On || !self.sink.enabled() {
            return Ok(false);
        }
        match &event {
            Event::ActiveDay { .. } if self.state.last_active_day == Some(day) => return Ok(false),
            Event::FeatureUsed { feature }
                if self.state.feature_days.get(feature) == Some(&day) =>
            {
                return Ok(false)
            }
            _ => {}
        }
        let mut next = self.state.clone();
        match &event {
            Event::ActiveDay { .. } => next.last_active_day = Some(day),
            Event::FeatureUsed { feature } => {
                next.feature_days.insert(*feature, day);
            }
            _ => {}
        }
        next.queue.push_back(Queued {
            insert_id: Uuid::new_v4(),
            time: now,
            event,
            user_id: next.user_id.clone(),
            failures: 0,
            retry_at: now,
        });
        if next.queue.len() > QUEUE_CAP {
            next.queue.pop_front();
            next.counters.dropped_oldest += 1;
        }
        self.commit(next)?;
        Ok(true)
    }

    pub fn deadline(&self) -> Option<u64> {
        if self.state.consent != Consent::On || !self.sink.enabled() {
            return None;
        }
        self.state.queue.front().map(|event| event.retry_at)
    }

    pub fn deliver(&mut self, now: u64) -> Result<(), String> {
        if self.deadline().is_none_or(|deadline| deadline > now) {
            return Ok(());
        }
        let batch: Vec<_> = self
            .state
            .queue
            .iter()
            .take(BATCH_CAP)
            .take_while(|event| event.retry_at <= now)
            .cloned()
            .collect();
        let device_id = self
            .state
            .device_id
            .ok_or("analytics device identity missing")?;
        let events: Vec<_> = batch
            .iter()
            .map(|event| DeliveryEvent {
                insert_id: event.insert_id,
                device_id,
                user_id: event.user_id.clone(),
                time: event.time,
                event: event.event.clone(),
            })
            .collect();
        let result = self.sink.deliver(&events);
        if result == DeliveryResult::Unavailable {
            return Err("analytics sink unavailable".into());
        }
        let mut next = self.state.clone();
        if matches!(
            result,
            DeliveryResult::Delivered | DeliveryResult::InvalidPayload
        ) {
            for _ in &batch {
                next.queue.pop_front();
            }
            if result == DeliveryResult::Delivered {
                next.counters.delivered += batch.len() as u64;
            } else {
                next.counters.dropped_payload += batch.len() as u64;
            }
        } else {
            let mut exhausted = 0;
            for event in next.queue.iter_mut().take(batch.len()) {
                if let Some(delay) = RETRY_DELAYS_MS.get(event.failures) {
                    event.retry_at = now.saturating_add(*delay);
                    event.failures += 1;
                } else {
                    exhausted += 1;
                }
            }
            for _ in 0..exhausted {
                next.queue.pop_front();
            }
            next.counters.retry_exhausted += exhausted;
        }
        self.commit(next)
    }
}
