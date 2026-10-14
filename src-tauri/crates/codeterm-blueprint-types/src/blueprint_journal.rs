use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalMarker {
    Step(u16),
    Result,
    Run,
}

impl JournalMarker {
    pub fn index(self) -> i64 {
        match self {
            Self::Step(index) => i64::from(index),
            Self::Result | Self::Run => -1,
        }
    }

    pub fn from_columns(index: i64, name: &str) -> Result<Self, String> {
        match (index, name) {
            (-1, "$run") => Ok(Self::Run),
            (-1, "$result") => Ok(Self::Result),
            (0..=63, _) => Ok(Self::Step(index as u16)),
            _ => Err("invalid journal marker".into()),
        }
    }

    pub fn name(self, step_name: &str) -> &str {
        match self {
            Self::Step(_) => step_name,
            Self::Result => "$result",
            Self::Run => "$run",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalState {
    Running,
    Done,
    Failed,
    Skipped,
    Held,
}

impl JournalState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Held => "held",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "running" => Ok(Self::Running),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            "held" => Ok(Self::Held),
            _ => Err("invalid journal state".into()),
        }
    }
}

impl From<&str> for JournalState {
    fn from(value: &str) -> Self {
        Self::parse(value).expect("runner states are declared")
    }
}

impl PartialEq<&str> for JournalState {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum JournalOutcome {
    Done,
    Failed,
    Skipped,
    Held,
    HandlerResult,
    HandlerFailed,
    HandlerUnconfirmed,
    Classified,
    LowConfidence,
    ProbeError,
    FilterMatch,
    FilterNoMatch,
    Unchanged,
    Skip(String),
    Bucket(String),
}

impl JournalOutcome {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Held => "held",
            Self::HandlerResult => "handler_result",
            Self::HandlerFailed => "handler_failed",
            Self::HandlerUnconfirmed => "handler_unconfirmed",
            Self::Classified => "classified",
            Self::LowConfidence => "low_confidence",
            Self::ProbeError => "probe_error",
            Self::FilterMatch => "filter_match",
            Self::FilterNoMatch => "filter_no_match",
            Self::Unchanged => "unchanged",
            Self::Skip(value) | Self::Bucket(value) => value,
        }
    }

    pub fn is_skip(&self) -> bool {
        matches!(self, Self::Skip(_) | Self::Skipped)
    }
}

impl From<String> for JournalOutcome {
    fn from(value: String) -> Self {
        match value.as_str() {
            "done" => Self::Done,
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            "held" => Self::Held,
            "handler_result" => Self::HandlerResult,
            "handler_failed" => Self::HandlerFailed,
            "handler_unconfirmed" => Self::HandlerUnconfirmed,
            "classified" => Self::Classified,
            "low_confidence" => Self::LowConfidence,
            "probe_error" => Self::ProbeError,
            "filter_match" => Self::FilterMatch,
            "filter_no_match" => Self::FilterNoMatch,
            "unchanged" => Self::Unchanged,
            _ if value.starts_with("skip:") => Self::Skip(value),
            _ => Self::Bucket(value),
        }
    }
}

impl From<&str> for JournalOutcome {
    fn from(value: &str) -> Self {
        value.to_string().into()
    }
}

impl From<JournalOutcome> for String {
    fn from(value: JournalOutcome) -> Self {
        value.as_str().into()
    }
}

impl std::ops::Deref for JournalOutcome {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<&str> for JournalOutcome {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum JournalReason {
    UncertainClaim,
    UncertainEffect,
    UncertainResult,
    JournalWriteFailed,
    Other(String),
}

impl JournalReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::UncertainClaim => "uncertain_pipeline_claim",
            Self::UncertainEffect => "uncertain_pipeline_effect",
            Self::UncertainResult => "uncertain_pipeline_result",
            Self::JournalWriteFailed => "journal_write_failed",
            Self::Other(reason) => reason,
        }
    }
}

impl From<String> for JournalReason {
    fn from(value: String) -> Self {
        match value.as_str() {
            "uncertain_pipeline_claim" => Self::UncertainClaim,
            "uncertain_pipeline_effect" => Self::UncertainEffect,
            "uncertain_pipeline_result" => Self::UncertainResult,
            "journal_write_failed" => Self::JournalWriteFailed,
            _ => Self::Other(value),
        }
    }
}

impl From<&str> for JournalReason {
    fn from(value: &str) -> Self {
        value.to_string().into()
    }
}

impl From<JournalReason> for String {
    fn from(value: JournalReason) -> Self {
        value.as_str().into()
    }
}

impl std::ops::Deref for JournalReason {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blueprint_journal_markers_and_outcomes_round_trip_without_string_heuristics() {
        for marker in [
            JournalMarker::Step(0),
            JournalMarker::Step(63),
            JournalMarker::Result,
            JournalMarker::Run,
        ] {
            let name = marker.name("step");
            assert_eq!(
                JournalMarker::from_columns(marker.index(), name).unwrap(),
                marker
            );
            assert_eq!(
                serde_json::from_value::<JournalMarker>(serde_json::to_value(marker).unwrap())
                    .unwrap(),
                marker
            );
        }
        for outcome in [
            JournalOutcome::Done,
            JournalOutcome::Failed,
            JournalOutcome::Held,
            JournalOutcome::HandlerResult,
            JournalOutcome::Skip("skip:branch".into()),
            JournalOutcome::Bucket("ready".into()),
        ] {
            assert_eq!(
                serde_json::from_value::<JournalOutcome>(serde_json::to_value(&outcome).unwrap())
                    .unwrap(),
                outcome
            );
        }
        let reason = JournalReason::UncertainEffect;
        assert_eq!(
            serde_json::from_value::<JournalReason>(serde_json::to_value(&reason).unwrap())
                .unwrap(),
            reason
        );
        assert!(JournalMarker::from_columns(-1, "step").is_err());
        assert!(JournalMarker::from_columns(64, "step").is_err());
        assert!(JournalState::parse("invalid").is_err());
    }
}
