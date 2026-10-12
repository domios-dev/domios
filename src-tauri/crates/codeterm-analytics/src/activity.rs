use crate::Consent;
use chrono::{Datelike, NaiveDate};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

pub(crate) struct Activity {
    enabled: AtomicBool,
    day: AtomicI32,
    pending: AtomicBool,
    machines: AtomicUsize,
}

impl Activity {
    pub fn new(consent: Consent, day: Option<NaiveDate>) -> Self {
        Self {
            enabled: AtomicBool::new(consent == Consent::On),
            day: AtomicI32::new(day.map_or(0, |day| day.num_days_from_ce())),
            pending: AtomicBool::new(false),
            machines: AtomicUsize::new(1),
        }
    }

    pub fn publish(&self, consent: Consent, day: Option<NaiveDate>) {
        self.day.store(
            day.map_or(0, |day| day.num_days_from_ce()),
            Ordering::Release,
        );
        self.enabled
            .store(consent == Consent::On, Ordering::Release);
    }

    pub fn claim(&self, day: NaiveDate) -> bool {
        if !self.enabled.load(Ordering::Acquire)
            || self.day.load(Ordering::Acquire) == day.num_days_from_ce()
            || self
                .pending
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        if !self.enabled.load(Ordering::Acquire)
            || self.day.load(Ordering::Acquire) == day.num_days_from_ce()
        {
            self.release();
            return false;
        }
        true
    }

    pub fn release(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub fn set_machines(&self, machines: usize) {
        self.machines.store(machines, Ordering::Release);
    }

    pub fn machines(&self) -> usize {
        self.machines.load(Ordering::Acquire)
    }
}
