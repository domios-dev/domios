use super::*;

#[derive(Debug, PartialEq, Eq)]
pub struct ArmedLaunchSnapshot {
    pub attempt: LaunchAttempt,
    pub origin: Option<crate::service::AgentLaunchOrigin>,
    pub registrations: usize,
    pub acknowledged: bool,
}

pub fn snapshot(tab_id: &str) -> Option<ArmedLaunchSnapshot> {
    let state = OBSERVATIONS.state.lock();
    let armed = state.armed.get(tab_id)?;
    Some(ArmedLaunchSnapshot {
        attempt: armed.attempt.clone(),
        origin: armed.origin,
        registrations: armed.registrations,
        acknowledged: matches!(armed.evidence, Some(LaunchEvidence::Acknowledged)),
    })
}

pub fn acknowledge(attempt: &LaunchAttempt) -> bool {
    note_acknowledged(&attempt.tab_id, &attempt.pty_id, &attempt.binding_nonce)
}

pub fn consume_acknowledgement(attempt: &LaunchAttempt) -> bool {
    matches!(
        observe(attempt, Instant::now()),
        LaunchObservation::Evidence(LaunchEvidence::Acknowledged)
    )
}
