use crate::{AnalyticsSink, DeliveryEvent, DeliveryResult};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct Recording {
    pub batches: Vec<Vec<DeliveryEvent>>,
    pub results: VecDeque<DeliveryResult>,
}

pub struct Fake {
    pub recording: Arc<Mutex<Recording>>,
}

impl AnalyticsSink for Fake {
    fn deliver(&mut self, events: &[DeliveryEvent]) -> DeliveryResult {
        let Ok(mut recording) = self.recording.lock() else {
            return DeliveryResult::Unavailable;
        };
        recording.batches.push(events.to_vec());
        recording
            .results
            .pop_front()
            .unwrap_or(DeliveryResult::Delivered)
    }
}
