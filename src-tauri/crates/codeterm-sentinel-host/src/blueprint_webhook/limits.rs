use super::Refusal;
use std::collections::{HashMap, VecDeque};

const MINUTE: u64 = 60_000;
const DEDUP_WINDOW: u64 = 10 * MINUTE;

#[derive(Default)]
pub(super) struct Limits {
    attempts: VecDeque<u64>,
    graphs: HashMap<String, VecDeque<u64>>,
    deliveries: VecDeque<(String, String, String, u64)>,
}

fn expire(queue: &mut VecDeque<u64>, now: u64) {
    while queue
        .front()
        .is_some_and(|at| now.saturating_sub(*at) >= MINUTE)
    {
        queue.pop_front();
    }
}

impl Limits {
    pub fn attempt(&mut self, now: u64) -> Result<(), Refusal> {
        expire(&mut self.attempts, now);
        if self.attempts.len() >= 120 {
            return Err(Refusal::RateLimited);
        }
        self.attempts.push_back(now);
        Ok(())
    }

    pub fn admit(&mut self, graph: &str, now: u64) -> Result<(), Refusal> {
        self.graphs.retain(|_, times| {
            expire(times, now);
            !times.is_empty()
        });
        if !self.graphs.contains_key(graph) && self.graphs.len() >= 1024 {
            return Err(Refusal::RateLimited);
        }
        let times = self.graphs.entry(graph.into()).or_default();
        if times.len() >= 30 {
            return Err(Refusal::RateLimited);
        }
        times.push_back(now);
        Ok(())
    }

    pub fn duplicate(
        &mut self,
        graph: &str,
        delivery: Option<&str>,
        now: u64,
    ) -> Result<Option<String>, Refusal> {
        while self
            .deliveries
            .front()
            .is_some_and(|(_, _, _, at)| now.saturating_sub(*at) >= DEDUP_WINDOW)
        {
            self.deliveries.pop_front();
        }
        let Some(delivery) = delivery else {
            return Ok(None);
        };
        if delivery.is_empty()
            || delivery.len() > 128
            || !delivery.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Refusal::InvalidPayload);
        }
        if let Some((_, _, run, _)) = self
            .deliveries
            .iter()
            .find(|(g, d, _, _)| g == graph && d == delivery)
        {
            return Ok(Some(run.clone()));
        }
        if self.deliveries.len() >= 4096
            || self
                .deliveries
                .iter()
                .filter(|(g, _, _, _)| g == graph)
                .count()
                >= 256
        {
            return Err(Refusal::RateLimited);
        }
        Ok(None)
    }

    pub fn remember(&mut self, graph: &str, delivery: Option<String>, run: &str, now: u64) {
        if let Some(delivery) = delivery {
            self.deliveries
                .push_back((graph.into(), delivery, run.into(), now));
        }
    }
}
