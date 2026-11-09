use super::{RuntimeFailure, RuntimeFailureCode, MAX_RUNTIME_BATCH};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeStreamWindow {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_sequences: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_requests: Vec<RuntimeTurnRequestWitness>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeTurnRequestWitness {
    pub send_id: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub written: bool,
}
fn is_false(value: &bool) -> bool {
    !*value
}

pub(super) fn validate(
    window: Option<&RuntimeStreamWindow>,
    identifier: impl Fn(&str) -> bool,
) -> Result<(), RuntimeFailure> {
    let Some(window) = window else {
        return Ok(());
    };
    let mut sends = std::collections::HashSet::new();
    if window.frame_sequences.len() > MAX_RUNTIME_BATCH
        || window.turn_requests.len() > 32
        || window
            .frame_sequences
            .iter()
            .any(|sequence| *sequence == 0 || *sequence > 9_007_199_254_740_991)
        || window
            .frame_sequences
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || window.turn_requests.iter().any(|request| {
            !identifier(&request.send_id)
                || !identifier(&request.turn_id)
                || !sends.insert(&request.send_id)
                || request
                    .after_sequence
                    .is_some_and(|sequence| sequence > 9_007_199_254_740_991)
                || request.written && request.after_sequence.is_none()
        })
    {
        return Err(RuntimeFailure::new(
            RuntimeFailureCode::InvalidSchema,
            "Invalid runtime stream window",
        ));
    }
    Ok(())
}
