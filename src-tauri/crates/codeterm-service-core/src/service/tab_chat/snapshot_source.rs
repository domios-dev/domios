use crate::{multiplexer::types::TabBacking, service::ServiceContext};

#[derive(Debug, PartialEq, Eq)]
pub enum SnapshotSource {
    Stored,
    Live,
    Waking,
}

pub struct SnapshotState {
    pub frozen: bool,
    pub runtime_requested: bool,
    pub runtime_present: bool,
    pub has_session: bool,
}

pub fn snapshot_source(state: SnapshotState) -> SnapshotSource {
    if state.frozen {
        SnapshotSource::Stored
    } else if !state.runtime_requested || state.runtime_present {
        SnapshotSource::Live
    } else if state.has_session {
        SnapshotSource::Stored
    } else {
        SnapshotSource::Waking
    }
}

pub fn for_tab(svc: &ServiceContext, tab_id: &str) -> Result<SnapshotSource, String> {
    let (frozen, has_session) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
        let session = match &tab.backing {
            TabBacking::ChatBackend { session_id, .. } => Some(session_id.as_str()),
            _ => tab.agent_session_id.as_deref(),
        };
        (tab.frozen, session.is_some_and(|id| !id.is_empty()))
    };
    Ok(snapshot_source(SnapshotState {
        frozen,
        runtime_requested: crate::service::agent_runtime::requested_for_tab(svc, tab_id),
        runtime_present: crate::service::agent_runtime::engine::for_tab(tab_id).is_some(),
        has_session,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_source_covers_sleeping_pty_runtime_and_wake_states() {
        for (frozen, runtime_requested, runtime_present, has_session, expected) in [
            (true, true, false, true, SnapshotSource::Stored),
            (true, false, false, true, SnapshotSource::Stored),
            (true, true, true, true, SnapshotSource::Stored),
            (false, true, false, true, SnapshotSource::Stored),
            (false, true, false, false, SnapshotSource::Waking),
            (false, true, true, true, SnapshotSource::Live),
            (false, false, false, true, SnapshotSource::Live),
        ] {
            assert_eq!(
                snapshot_source(SnapshotState {
                    frozen,
                    runtime_requested,
                    runtime_present,
                    has_session,
                }),
                expected
            );
        }
    }
}
