//! Plugin call deadlines, declared in one place: every plugin call and its host I/O
//! jobs share the standard bound unless its capability declares a longer one here.

use std::time::{Duration, Instant};

use crate::converter::ConversionCancellation;

/// Plugin calls are asynchronous over host I/O, so a minute bounds only a wedged call.
pub(crate) const STANDARD: Duration = Duration::from_secs(60);
/// Transcription is long by nature: local engines and remote APIs scale with the note.
const TRANSCRIPTION: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LongCall {
    Transcription,
}

impl LongCall {
    pub(crate) fn bound(self) -> Duration {
        match self {
            Self::Transcription => TRANSCRIPTION,
        }
    }

    /// A fresh cancellable operation interrupt ending at this call's deadline from `now`.
    pub(crate) fn interrupt_from(self, now: Instant) -> (ConversionCancellation, Instant) {
        (ConversionCancellation::default(), now + self.bound())
    }

    pub(crate) fn interrupt(self) -> (ConversionCancellation, Instant) {
        self.interrupt_from(Instant::now())
    }
}

#[cfg(test)]
mod tests;
